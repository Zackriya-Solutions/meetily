/// GitHub Copilot CLI provider - generates summaries by invoking the `copilot`
/// binary in non-interactive prompt mode (`copilot -p ... --output-format json`).
///
/// Authentication rides on the user's existing Copilot credentials: either a
/// prior `copilot login`, or a GitHub token supplied in the provider config
/// (passed to the child process as COPILOT_GITHUB_TOKEN). No API key is sent
/// over HTTP by this app; the CLI handles all network access.
///
/// The prompt (which embeds the full transcript) can exceed OS argument-length
/// limits - notably the ~32KB command-line cap on Windows - so it is written to
/// temp files that the CLI reads with its built-in file tool. The system temp
/// directory is readable by the CLI without extra permission flags.
///
/// The CLI's `view` tool refuses a file over 20 KB, so the agent pages through
/// a long file and may stop early, summarizing only the start of a meeting.
/// The prompt is therefore split into parts that each fit in one read, and a
/// run whose tool results do not show every part is rejected.
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use log::{info, warn};

const COPILOT_TIMEOUT: Duration = Duration::from_secs(300);

/// Copilot CLI 1.0.109 reports `large_output_threshold_bytes: 20480` and its
/// `view` tool answers "File too large to read at once" above that. 16 KiB
/// leaves room for the end marker.
const PROMPT_PART_MAX_BYTES: usize = 16 * 1024;

/// A run that skipped part of the prompt is retried once before failing
const MAX_READ_ATTEMPTS: usize = 2;

/// Retired Copilot model IDs per GitHub's model retirement history. Must
/// match `RETIRED_COPILOT_CLI_MODELS` in frontend/src/lib/copilot-cli-models.ts;
/// a test below enforces that. Never remapped: the user picks a replacement.
pub(crate) const RETIRED_COPILOT_CLI_MODELS: &[&str] = &[
    "claude-sonnet-4.5",
    "claude-sonnet-4",
    "gpt-5",
    "gpt-4.1",
    "gemini-2.5-pro",
    "gemini-3.1-pro",
    "gemini-3-flash",
    "claude-opus-4.5",
    "claude-opus-4.6",
    "claude-sonnet-4.6",
    "raptor-mini",
    "mai-code-1-flash",
    "gemini-3.5-flash",
    "gemini-3.6-flash",
    "kimi-k2.7-code",
    "claude-opus-4.7",
    "gpt-5-mini",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.5",
    "gemini-3.7-flash",
];

/// Reject a retired model with an actionable message; anything else
/// (including 'auto' and unknown custom IDs) is allowed through.
pub(crate) fn validate_copilot_model(model: &str) -> Result<(), String> {
    let model = model.trim();
    if RETIRED_COPILOT_CLI_MODELS
        .iter()
        .any(|retired| retired.eq_ignore_ascii_case(model))
    {
        return Err(format!(
            "GitHub has retired the Copilot model \"{}\". Choose another model in Settings (Auto works on every plan).",
            model
        ));
    }
    Ok(())
}

/// Resolves the Copilot CLI binary, preferring an explicitly configured path.
///
/// GUI apps on macOS launch with a minimal PATH, so after a plain PATH lookup
/// fails we probe the common install locations (npm global, Homebrew, and the
/// `gh copilot` download directory).
fn resolve_copilot_binary(configured_path: Option<&str>) -> Result<PathBuf, String> {
    if let Some(configured) = configured_path.map(str::trim).filter(|p| !p.is_empty()) {
        let path = PathBuf::from(configured);
        if path.is_file() {
            // Absolutize so the path survives the child's working directory
            // being set to the temp dir
            return Ok(std::path::absolute(&path).unwrap_or(path));
        }
        return Err(format!(
            "Configured GitHub Copilot CLI binary not found at: {}",
            path.display()
        ));
    }

    let binary_name = if cfg!(target_os = "windows") {
        "copilot.exe"
    } else {
        "copilot"
    };

    if let Ok(found) = which::which(binary_name) {
        return Ok(found);
    }
    // npm installs a `copilot.cmd` shim on Windows
    if cfg!(target_os = "windows") {
        if let Ok(found) = which::which("copilot.cmd") {
            return Ok(found);
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        // `gh copilot` downloads the CLI here
        candidates.push(home.join(".local/share/gh/copilot").join(binary_name));
        candidates.push(home.join(".npm-global/bin").join(binary_name));
        if cfg!(target_os = "windows") {
            candidates.push(home.join("AppData/Roaming/npm/copilot.cmd"));
        }
    }
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/opt/homebrew/bin/copilot"));
        candidates.push(PathBuf::from("/usr/local/bin/copilot"));
    }
    if cfg!(target_os = "linux") {
        candidates.push(PathBuf::from("/usr/local/bin/copilot"));
        candidates.push(PathBuf::from("/usr/bin/copilot"));
    }

    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        "GitHub Copilot CLI not found. Install it with `npm install -g @github/copilot` \
             (or run `gh copilot` once), then sign in with `copilot login`. If it is installed \
             in a non-standard location, set the binary path in Settings."
            .to_string()
    })
}

/// Extracts the final assistant reply from Copilot CLI JSONL output
/// (`--output-format json`).
///
/// The stream contains ephemeral delta events plus complete `assistant.message`
/// events; intermediate messages narrate tool use (they carry `toolRequests`),
/// so the answer is the last complete message without pending tool requests.
fn extract_final_message(jsonl: &str) -> Option<String> {
    let mut last: Option<String> = None;
    for line in jsonl.lines() {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if event
            .get("ephemeral")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        if event.get("type").and_then(|v| v.as_str()) != Some("assistant.message") {
            continue;
        }
        let Some(data) = event.get("data") else {
            continue;
        };
        let has_tool_requests = data
            .get("toolRequests")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        if has_tool_requests {
            continue;
        }
        if let Some(content) = data.get("content").and_then(|v| v.as_str()) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                last = Some(trimmed.to_string());
            }
        }
    }
    last
}

/// Numbers (1-based) of the prompt parts whose end marker appears in no tool
/// result, meaning the agent never read that part to its end
fn unread_prompt_parts(jsonl: &str, end_markers: &[String]) -> Vec<usize> {
    let mut read = vec![false; end_markers.len()];
    for line in jsonl.lines() {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if event.get("type").and_then(|v| v.as_str()) != Some("tool.execution_complete") {
            continue;
        }
        let Some(result) = event.get("data").and_then(|data| data.get("result")) else {
            continue;
        };
        let result = result.to_string();
        for (marker, read) in end_markers.iter().zip(read.iter_mut()) {
            *read |= result.contains(marker.as_str());
        }
    }
    read.iter()
        .enumerate()
        .filter(|(_, read)| !**read)
        .map(|(index, _)| index + 1)
        .collect()
}

/// Splits the prompt at line ends into parts of at most `max_bytes`, cutting
/// mid-line only for a line longer than that
fn split_prompt(body: &str, max_bytes: usize) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = body;
    while rest.len() > max_bytes {
        let mut window = max_bytes;
        while !rest.is_char_boundary(window) {
            window -= 1;
        }
        let cut = rest[..window].rfind('\n').map_or(window, |newline| newline + 1);
        parts.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    parts.push(rest);
    parts
}

/// The prompt as written to temp files, one per part, each ending in a marker
/// line unique to this run
struct PromptFiles {
    paths: Vec<PathBuf>,
    end_markers: Vec<String>,
}

impl PromptFiles {
    fn write(dir: &Path, body: &str) -> std::io::Result<Self> {
        let run_id = uuid::Uuid::new_v4();
        let parts = split_prompt(body, PROMPT_PART_MAX_BYTES);
        let count = parts.len();
        let mut files = PromptFiles {
            paths: Vec::with_capacity(count),
            end_markers: Vec::with_capacity(count),
        };
        for (index, part) in parts.into_iter().enumerate() {
            let number = index + 1;
            let path = dir.join(format!("meetily-copilot-{run_id}-part{number}of{count}.md"));
            let end_marker = format!("[end of prompt part {number} of {count}, {run_id}]");
            if let Err(error) = write_prompt_file(&path, &format!("{part}\n{end_marker}\n")) {
                files.remove();
                return Err(error);
            }
            files.paths.push(path);
            files.end_markers.push(end_marker);
        }
        Ok(files)
    }

    /// The `-p` instruction that has the agent read every part, in order
    fn instruction(&self) -> String {
        let reply = "Reply with ONLY the requested output - no preamble and no commentary about reading files.";
        if let [path] = self.paths.as_slice() {
            return format!(
                "Read the file at {} and follow the instructions it contains. {reply}",
                path.display()
            );
        }
        let list: String = self
            .paths
            .iter()
            .enumerate()
            .map(|(index, path)| format!("{}. {}\n", index + 1, path.display()))
            .collect();
        format!(
            "Your instructions and the full meeting transcript are split across {} files. \
             Use the view tool to read every one of these files completely, in this order, \
             before writing anything:\n{list}Then follow the instructions in those files. {reply}",
            self.paths.len()
        )
    }

    fn remove(&self) {
        for path in &self.paths {
            if let Err(e) = std::fs::remove_file(path) {
                warn!("Failed to remove Copilot CLI prompt file {}: {}", path.display(), e);
            }
        }
    }
}

/// Writes the prompt file restricted to the current user (0600 on Unix) since
/// it contains the meeting transcript and lives briefly in the shared temp dir
fn write_prompt_file(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    use std::io::Write;
    file.write_all(contents.as_bytes())
}

/// Trims process output to a readable snippet for error messages
fn output_snippet(text: &str) -> String {
    let trimmed = text.trim();
    let mut snippet: String = trimmed.chars().take(500).collect();
    if snippet.len() < trimmed.len() {
        snippet.push_str("...");
    }
    snippet
}

/// Prefer the CLI's structured failure over earlier warnings in its JSONL stream.
/// In particular, an MCP policy warning can precede an unrelated quota error.
fn extract_cli_error(jsonl: &str) -> Option<String> {
    let mut model_failure = None;
    for line in jsonl.lines() {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(data) = event.get("data") else {
            continue;
        };
        match event.get("type").and_then(|value| value.as_str()) {
            Some("session.error") => {
                if data.get("errorCode").and_then(|value| value.as_str()) == Some("quota_exceeded")
                    || data.get("errorType").and_then(|value| value.as_str()) == Some("quota")
                {
                    return Some(
                        "GitHub Copilot monthly quota exceeded. Check your Copilot usage or choose a model available under your plan."
                            .to_string(),
                    );
                }
                if let Some(message) = data.get("message").and_then(|value| value.as_str()) {
                    return Some(output_snippet(message));
                }
            }
            Some("model.call_failure") => {
                if let Some(message) = data.get("errorMessage").and_then(|value| value.as_str()) {
                    let parsed = serde_json::from_str::<serde_json::Value>(message).ok();
                    let code = parsed
                        .as_ref()
                        .and_then(|value| value.get("code"))
                        .and_then(|value| value.as_str());
                    if code == Some("quota_exceeded") {
                        model_failure = Some(
                            "GitHub Copilot monthly quota exceeded. Check your Copilot usage or choose a model available under your plan."
                                .to_string(),
                        );
                    } else {
                        let detail = parsed
                            .as_ref()
                            .and_then(|value| value.get("message"))
                            .and_then(|value| value.as_str())
                            .unwrap_or(message);
                        model_failure = Some(output_snippet(detail));
                    }
                }
            }
            _ => {}
        }
    }
    model_failure
}

/// Generates a summary by running the GitHub Copilot CLI as a subprocess
///
/// # Arguments
/// * `binary_path` - Optional explicit path to the `copilot` binary
/// * `model_name` - Copilot model id (e.g. "auto", "claude-haiku-4.5"); empty or "auto" lets Copilot pick
/// * `github_token` - Optional GitHub token passed as COPILOT_GITHUB_TOKEN (otherwise `copilot login` credentials are used)
/// * `system_prompt` / `user_prompt` - Prompts, written to temp files the CLI reads
/// * `cancellation_token` - Optional token to abort (kills the child process)
pub async fn generate_with_copilot_cli(
    binary_path: Option<&str>,
    model_name: &str,
    github_token: Option<&str>,
    system_prompt: &str,
    user_prompt: &str,
    cancellation_token: Option<&CancellationToken>,
) -> Result<String, String> {
    validate_copilot_model(model_name)?;
    let binary = resolve_copilot_binary(binary_path)?;

    let prompt_body = format!("{}\n\n{}", system_prompt, user_prompt);
    let prompt_files = PromptFiles::write(&crate::storage::tmp_dir(), &prompt_body)
        .map_err(|e| format!("Failed to write Copilot CLI prompt file: {}", e))?;

    let result = run_until_prompt_read(
        &binary,
        &prompt_files,
        model_name,
        github_token,
        cancellation_token,
    )
    .await;

    prompt_files.remove();
    result
}

/// Runs the CLI until a run reads every prompt part, and returns its reply
async fn run_until_prompt_read(
    binary: &PathBuf,
    prompt_files: &PromptFiles,
    model_name: &str,
    github_token: Option<&str>,
    cancellation_token: Option<&CancellationToken>,
) -> Result<String, String> {
    let instruction = prompt_files.instruction();
    let part_count = prompt_files.end_markers.len();
    let mut unread = Vec::new();
    for attempt in 1..=MAX_READ_ATTEMPTS {
        let output = run_copilot(
            binary,
            &instruction,
            model_name,
            github_token,
            cancellation_token,
        )
        .await?;
        unread = unread_prompt_parts(&output.stdout, &prompt_files.end_markers);
        if unread.is_empty() {
            return final_reply(&output);
        }
        warn!(
            "Copilot CLI attempt {}/{} did not read prompt parts {:?} of {}",
            attempt, MAX_READ_ATTEMPTS, unread, part_count
        );
    }
    Err(format!(
        "GitHub Copilot CLI read only {} of {} parts of the transcript, so its summary would leave out part of the meeting. Please retry, or choose another Copilot model.",
        part_count - unread.len(),
        part_count
    ))
}

/// Output of a CLI run that exited successfully
struct CopilotOutput {
    stdout: String,
    stderr: String,
}

fn final_reply(output: &CopilotOutput) -> Result<String, String> {
    extract_final_message(&output.stdout).ok_or_else(|| {
        extract_cli_error(&output.stdout)
            .or_else(|| extract_cli_error(&output.stderr))
            .map(|detail| format!("GitHub Copilot CLI failed: {}", detail))
            .unwrap_or_else(|| {
                format!(
                    "GitHub Copilot CLI returned no assistant response. Output: {}",
                    output_snippet(&output.stdout)
                )
            })
    })
}

async fn run_copilot(
    binary: &PathBuf,
    instruction: &str,
    model_name: &str,
    github_token: Option<&str>,
    cancellation_token: Option<&CancellationToken>,
) -> Result<CopilotOutput, String> {
    let mut command = Command::new(binary);
    command
        .arg("-p")
        .arg(instruction)
        .arg("--output-format")
        .arg("json")
        .arg("--no-color")
        .arg("--no-auto-update")
        .arg("--no-ask-user")
        .arg("--no-custom-instructions")
        .arg("--disable-builtin-mcps")
        .arg("--log-level")
        .arg("none")
        .current_dir(crate::storage::tmp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let model = model_name.trim();
    if !model.is_empty() {
        command.arg("--model").arg(model);
    }

    if let Some(token) = github_token.map(str::trim).filter(|t| !t.is_empty()) {
        command.env("COPILOT_GITHUB_TOKEN", token);
    }

    // Hide console window on Windows to prevent CMD popup
    // (creation_flags is an inherent method on tokio's Command)
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    info!(
        "🤖 Copilot CLI request: binary={}, model={}",
        binary.display(),
        if model.is_empty() { "auto" } else { model }
    );

    let output_future = tokio::time::timeout(COPILOT_TIMEOUT, command.output());

    // kill_on_drop terminates the child if cancellation wins the select
    let timed_output = if let Some(token) = cancellation_token {
        tokio::select! {
            result = output_future => result,
            _ = token.cancelled() => {
                return Err("Summary generation was cancelled".to_string());
            }
        }
    } else {
        output_future.await
    };

    let output = match timed_output {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(format!(
                "Failed to run GitHub Copilot CLI ({}): {}",
                binary.display(),
                e
            ))
        }
        Err(_) => {
            return Err(format!(
                "GitHub Copilot CLI timed out after {} seconds",
                COPILOT_TIMEOUT.as_secs()
            ))
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        if let Some(detail) = extract_cli_error(&stdout).or_else(|| extract_cli_error(&stderr)) {
            return Err(format!("GitHub Copilot CLI failed: {}", detail));
        }
        let detail = if !stderr.trim().is_empty() {
            output_snippet(&stderr)
        } else {
            output_snippet(&stdout)
        };
        return Err(format!(
            "GitHub Copilot CLI failed ({}): {}",
            output.status, detail
        ));
    }

    Ok(CopilotOutput {
        stdout: stdout.into_owned(),
        stderr: stderr.into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_last_complete_message_without_tool_requests() {
        let jsonl = concat!(
            r#"{"type":"session.tools_updated","data":{"model":"gpt-5-mini"},"ephemeral":true}"#,
            "\n",
            r#"{"type":"assistant.message_delta","data":{"deltaContent":"Read"},"ephemeral":true}"#,
            "\n",
            r#"{"type":"assistant.message","data":{"content":"Reading the transcript file.","toolRequests":[{"name":"view"}]}}"#,
            "\n",
            r#"{"type":"assistant.message","data":{"content":"The team approved the Q3 budget.","toolRequests":[]}}"#,
            "\n",
            r#"{"type":"result","data":{}}"#,
        );

        assert_eq!(
            extract_final_message(jsonl),
            Some("The team approved the Q3 budget.".to_string())
        );
    }

    #[test]
    fn ignores_ephemeral_and_malformed_lines() {
        let jsonl = concat!(
            "not json at all\n",
            r#"{"type":"assistant.message","data":{"content":"ephemeral echo","toolRequests":[]},"ephemeral":true}"#,
            "\n",
            r#"{"type":"assistant.message","data":{"content":"  final answer  ","toolRequests":[]}}"#,
        );

        assert_eq!(
            extract_final_message(jsonl),
            Some("final answer".to_string())
        );
    }

    #[test]
    fn returns_none_when_no_final_message() {
        let jsonl = r#"{"type":"assistant.message","data":{"content":"narration","toolRequests":[{"name":"view"}]}}"#;
        assert_eq!(extract_final_message(jsonl), None);
    }

    #[test]
    fn retired_models_are_rejected_before_running_the_cli() {
        for model in ["gpt-5-mini", "GPT-5.5", " claude-opus-4.7 ", "claude-sonnet-4.5"] {
            assert!(validate_copilot_model(model).unwrap_err().contains("retired"));
        }
        for model in ["", "auto", "claude-haiku-4.5", "my-org-model"] {
            assert!(validate_copilot_model(model).is_ok(), "{model}");
        }
    }

    #[test]
    fn retired_list_matches_the_frontend_catalog() {
        let ts = include_str!("../../../src/lib/copilot-cli-models.ts");
        let start = ts.find("RETIRED_COPILOT_CLI_MODELS: string[] = [").unwrap();
        let end = start + ts[start..].find("];").unwrap();
        let frontend: Vec<&str> = ts[start..end]
            .lines()
            .filter_map(|line| line.trim().strip_prefix('\''))
            .filter_map(|rest| rest.split('\'').next())
            .collect();
        let mut rust: Vec<&str> = RETIRED_COPILOT_CLI_MODELS.to_vec();
        let mut frontend_sorted = frontend.clone();
        rust.sort();
        frontend_sorted.sort();
        assert_eq!(rust, frontend_sorted);

        let offered_start = ts.find("COPILOT_CLI_MODELS: string[] = [").unwrap();
        let offered = &ts[offered_start..offered_start + ts[offered_start..].find("];").unwrap()];
        for retired in RETIRED_COPILOT_CLI_MODELS {
            assert!(!offered.contains(&format!("'{retired}'")), "{retired} is offered");
        }
    }

    #[test]
    fn configured_binary_path_must_exist() {
        let err = resolve_copilot_binary(Some("/nonexistent/path/to/copilot")).unwrap_err();
        assert!(err.contains("not found at"));
    }

    #[test]
    fn reports_quota_error_after_mcp_policy_warning() {
        let jsonl = concat!(
            r#"{"type":"session.warning","data":{"message":"Third-party MCP servers are disabled by your organization's Copilot policy. Only built-in servers are available."},"ephemeral":true}"#,
            "\n",
            r#"{"type":"model.call_failure","data":{"errorMessage":"{\"message\":\"You have exceeded your monthly quota\",\"code\":\"quota_exceeded\"}"}}"#,
            "\n",
            r#"{"type":"session.error","data":{"errorType":"quota","message":"You have exceeded your monthly quota (Request ID: example)","errorCode":"quota_exceeded"}}"#,
        );

        let detail = extract_cli_error(jsonl).unwrap();
        assert!(detail.contains("monthly quota exceeded"));
        assert!(!detail.contains("MCP"));
        assert!(!detail.contains("Request ID"));
    }

    #[test]
    fn ignores_warning_without_a_structured_failure() {
        let jsonl = r#"{"type":"session.warning","data":{"message":"Third-party MCP servers are disabled"}}"#;
        assert_eq!(extract_cli_error(jsonl), None);
    }

    #[test]
    fn prompt_parts_fit_one_read_and_keep_every_byte() {
        let body = format!("{}\n{}", "línea de transcripción\n".repeat(50), "ñ".repeat(300));
        let parts = split_prompt(&body, 64);
        assert!(parts.iter().all(|part| part.len() <= 64 && !part.is_empty()));
        assert_eq!(parts.concat(), body);
        // 25-byte lines, two per part, unless a single line is over the limit
        assert_eq!(parts[0], "línea de transcripción\n".repeat(2));
        assert!(parts[..25].iter().all(|part| part.ends_with('\n')));
    }

    /// A three-hour meeting's worth of transcript, about 120 KB
    #[cfg(unix)]
    fn long_transcript() -> String {
        (0..400)
            .map(|line| {
                format!(
                    "[{:02}:{:02}] Speaker {} discussed settlement, accounting and booking details for this workstream item.\n",
                    line * 27 / 60,
                    line * 27 % 60,
                    line % 4,
                ) + &"Further context on the same topic. ".repeat(5)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Writes an executable stand-in for the `copilot` binary. It finds the
    /// prompt files named in the `-p` instruction, emits a `view` tool result
    /// per read in the CLI's JSONL format, then a final answer.
    #[cfg(unix)]
    fn fake_copilot(dir: &std::path::Path, read_files: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = format!(
            r#"#!/bin/sh
emit() {{
  content=$(sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' | awk '{{printf "%s\\n", $0}}')
  printf '{{"type":"tool.execution_complete","data":{{"toolName":"view","success":true,"result":{{"content":"%s"}}}}}}\n' "$content"
}}
printf '%s\n' "$2" | grep -oE '(/[^/]+)*/meetily-copilot-[^/ ]*\.md' > "$0.files"
{read_files}
printf '{{"type":"assistant.message","data":{{"content":"Summary of the meeting.","toolRequests":[]}}}}\n'
"#
        );
        let path = dir.join("copilot");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    async fn summarize_with(binary: &std::path::Path, transcript: &str) -> Result<String, String> {
        generate_with_copilot_cli(
            Some(binary.to_str().unwrap()),
            "auto",
            None,
            "You are an expert meeting summarizer.",
            &format!("<transcript_chunks>\n{transcript}\n</transcript_chunks>"),
            None,
        )
        .await
    }

    /// The real CLI's `view` tool refuses files over 20 KB, so the agent pages
    /// through a long prompt and can stop early: a client workshop summary
    /// covered only the first half of a three-hour transcript. A run that read
    /// only the start must fail instead of returning a partial summary.
    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_a_summary_written_after_reading_only_the_start_of_the_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_copilot(
            dir.path(),
            r#"head -n 1 "$0.files" | while IFS= read -r f; do head -c 16000 "$f" | emit; done"#,
        );

        let error = summarize_with(&binary, &long_transcript())
            .await
            .expect_err("a summary of the first 16 KB of a 120 KB transcript must not pass");
        assert!(error.contains("read only"), "{error}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn accepts_a_summary_written_after_reading_the_whole_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let binary = fake_copilot(
            dir.path(),
            r#"while IFS= read -r f; do emit < "$f"; done < "$0.files""#,
        );

        let summary = summarize_with(&binary, &long_transcript()).await.unwrap();
        assert_eq!(summary, "Summary of the meeting.");
    }
}
