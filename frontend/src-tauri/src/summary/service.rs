use crate::database::repositories::{
    meeting::MeetingsRepository, meeting_calendar::MeetingCalendarRepository,
    setting::SettingsRepository, summary::SummaryProcessesRepository,
};
use crate::summary::calendar_context::build_calendar_context;
use crate::summary::llm_client::LLMProvider;
use crate::summary::language_detection::detect_summary_language;
use crate::summary::metadata::read_detected_summary_language_from_metadata;
use crate::summary::processor::{
    clean_llm_markdown_detailed, extract_meeting_name_from_markdown, generate_meeting_summary,
    language_name_from_code, require_visible_markdown,
};
use crate::summary::templates::{self, Template};
use crate::ollama::metadata::ModelMetadataCache;
use crate::utils::url_origin_for_log;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;
use log::{error, info, warn};

/// Stored when a summary attempt dies without reaching a terminal state.
const SUMMARY_TASK_CRASHED: &str =
    "Summary generation stopped unexpectedly (internal error). Please try again.";

static METADATA_CACHE: LazyLock<ModelMetadataCache> =
    LazyLock::new(|| ModelMetadataCache::new(Duration::from_secs(300)));

#[derive(Clone)]
struct RegisteredCancellation {
    started_at: DateTime<Utc>,
    token: CancellationToken,
}

static CANCELLATION_REGISTRY: LazyLock<Arc<Mutex<HashMap<String, RegisteredCancellation>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

/// Strips the first `#` heading line; returns "" if no `#` is found.
fn strip_leading_title(markdown: &str) -> String {
    if let Some(hash_pos) = markdown.find('#') {
        let body_start = markdown[hash_pos..]
            .find('\n')
            .map_or(markdown.len(), |line_end| hash_pos + line_end);
        markdown[body_start..].trim_start().to_string()
    } else {
        String::new()
    }
}

/// Strips the leading H1 (`# Title\n...`) only when the markdown starts with one.
/// No-op on already-stripped values, values starting with `## Subheading`, or values
/// without any heading. Avoids the silent-empty-return case where `strip_leading_title`
/// returns "" for input lacking a leading `#`.
fn strip_title_if_present(markdown: &str) -> String {
    if markdown.trim_start().starts_with("# ") {
        strip_leading_title(markdown)
    } else {
        markdown.to_string()
    }
}

const ENGLISH_CACHE_FIELD: &str = "english_cache";
pub(crate) const PROVENANCE_FIELD: &str = "provenance";

/// Fields of `summary_processes.result` that are owned by the generation
/// pipeline rather than by the summary editor. A manual save sends only
/// `{markdown, summary_json}`, so these have to be carried forward explicitly
/// or editing a summary would erase them.
pub(crate) const PRESERVED_RESULT_FIELDS: &[&str] = &[ENGLISH_CACHE_FIELD, PROVENANCE_FIELD];

/// Records which model, template and language actually produced a summary.
///
/// Stamped once, when generation completes, because the alternative — reading
/// the user's current model settings at render time — relabels old summaries
/// with a model that never touched them as soon as the settings change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryProvenance {
    pub provider: String,
    pub model: String,
    pub template_id: String,
    /// The requested output language, absent when the summary followed the
    /// transcript's own language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_language: Option<String>,
    pub generated_at: String,
    pub app_version: String,
}

impl SummaryProvenance {
    fn new(
        provider: &str,
        model: &str,
        template_id: &str,
        summary_language: Option<&str>,
    ) -> Self {
        Self {
            provider: provider.to_string(),
            model: model.to_string(),
            template_id: template_id.to_string(),
            summary_language: summary_language.map(str::to_string),
            generated_at: chrono::Utc::now().to_rfc3339(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Adds the provenance stamp to a freshly built result blob.
///
/// Kept separate from [`build_summary_result_json`] so the English-cache shape
/// and the provenance stamp stay independently testable.
fn attach_summary_provenance(result: &mut serde_json::Value, provenance: &SummaryProvenance) {
    let Some(object) = result.as_object_mut() else {
        return;
    };
    match serde_json::to_value(provenance) {
        Ok(value) => {
            object.insert(PROVENANCE_FIELD.to_string(), value);
        }
        Err(e) => warn!("Failed to serialize summary provenance: {}", e),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SummaryCacheSource {
    transcript_fingerprint: String,
    custom_prompt_fingerprint: String,
    template_id: String,
    template_fingerprint: String,
    token_threshold: usize,
    model_provider: String,
    model_name: String,
    ollama_endpoint: Option<String>,
    custom_openai_endpoint: Option<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct EnglishSummaryCache {
    markdown: String,
    source: SummaryCacheSource,
    output_language: Option<String>,
}

fn stable_text_fingerprint(text: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:016x}:{}", hash, text.len())
}

#[allow(clippy::too_many_arguments)]
fn build_summary_cache_source(
    text: &str,
    custom_prompt: &str,
    template_id: &str,
    template_fingerprint: &str,
    token_threshold: usize,
    model_provider: &str,
    model_name: &str,
    ollama_endpoint: Option<&str>,
    custom_openai_endpoint: Option<&str>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
) -> SummaryCacheSource {
    SummaryCacheSource {
        transcript_fingerprint: stable_text_fingerprint(text),
        custom_prompt_fingerprint: stable_text_fingerprint(custom_prompt),
        template_id: template_id.to_string(),
        template_fingerprint: template_fingerprint.to_string(),
        token_threshold,
        model_provider: model_provider.to_string(),
        model_name: model_name.to_string(),
        ollama_endpoint: ollama_endpoint.map(str::to_string),
        custom_openai_endpoint: custom_openai_endpoint.map(str::to_string),
        max_tokens,
        temperature,
        top_p,
    }
}

fn template_cache_fingerprint(template: &Template) -> String {
    let rendered_template = format!(
        "{}\n---SECTION-INSTRUCTIONS---\n{}",
        template.to_markdown_structure(),
        template.to_section_instructions()
    );
    stable_text_fingerprint(&rendered_template)
}

fn normalise_summary_language_for_cache(summary_language: Option<&str>) -> Option<String> {
    language_name_from_code(summary_language?.trim()).map(str::to_string)
}

fn build_summary_result_json(
    final_markdown: &str,
    english_markdown: &str,
    source: SummaryCacheSource,
    output_language: Option<&str>,
    reasoning_stripped: bool,
    normalization_fallback: bool,
) -> Result<serde_json::Value, String> {
    let cleaned_final = clean_llm_markdown_detailed(final_markdown);
    require_visible_markdown("Final summary", &cleaned_final)?;
    let markdown = strip_title_if_present(&cleaned_final.markdown);
    if markdown.trim().is_empty() {
        return Err("Final summary contains no visible content after title removal".to_string());
    }

    let cleaned_english = clean_llm_markdown_detailed(english_markdown);
    require_visible_markdown("English summary", &cleaned_english)?;

    Ok(serde_json::json!({
        "markdown": markdown,
        ENGLISH_CACHE_FIELD: EnglishSummaryCache {
            markdown: cleaned_english.markdown,
            source,
            output_language: normalise_summary_language_for_cache(output_language),
        },
        "reasoning_stripped": reasoning_stripped
            || cleaned_final.reasoning_stripped
            || cleaned_english.reasoning_stripped,
        "normalization_fallback": normalization_fallback,
    }))
}

/// Parses a `summary_processes.result` JSON blob and extracts a cached English
/// summary only when it was produced from exactly the same source inputs and
/// the user is switching to a different non-English target language.
fn extract_cached_english_markdown(
    raw: &str,
    expected_source: &SummaryCacheSource,
    requested_language: Option<&str>,
) -> Result<Option<String>, serde_json::Error> {
    let requested_language = match normalise_summary_language_for_cache(requested_language) {
        Some(language) if language != "English" => language,
        _ => return Ok(None),
    };

    let value: serde_json::Value = serde_json::from_str(raw)?;
    let Some(cache_value) = value.get(ENGLISH_CACHE_FIELD) else {
        return Ok(None);
    };

    let cache: EnglishSummaryCache = match serde_json::from_value(cache_value.clone()) {
        Ok(cache) => cache,
        Err(_) => return Ok(None),
    };

    if cache.source != *expected_source {
        return Ok(None);
    }

    if cache.output_language.as_deref() == Some(requested_language.as_str()) {
        return Ok(None);
    }

    let markdown = cache.markdown.trim();
    if markdown.is_empty() {
        Ok(None)
    } else {
        Ok(Some(cache.markdown))
    }
}

/// Summary service - handles all summary generation logic
pub struct SummaryService;

impl SummaryService {
    /// Registers a new cancellation token for a meeting.
    pub(crate) fn register_cancellation_token(
        meeting_id: &str,
        started_at: DateTime<Utc>,
    ) -> CancellationToken {
        let token = CancellationToken::new();
        let mut registry = CANCELLATION_REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(previous) = registry.insert(
            meeting_id.to_string(),
            RegisteredCancellation {
                started_at,
                token: token.clone(),
            },
        ) {
            previous.token.cancel();
        }
        info!("Registered cancellation token for meeting: {}", meeting_id);
        token
    }

    /// Cancels only the active generation identified by `started_at`.
    pub fn cancel_summary(meeting_id: &str, started_at: DateTime<Utc>) -> bool {
        let registry = CANCELLATION_REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = registry.get(meeting_id).filter(|entry| entry.started_at == started_at)
        {
            info!("Cancelling summary generation for meeting: {}", meeting_id);
            entry.token.cancel();
            true
        } else {
            warn!("No active summary generation found for meeting: {}", meeting_id);
            false
        }
    }

    /// Cancels whichever generation is running for the meeting. For a caller
    /// that knows only the meeting, such as the pipeline's stop.
    pub fn cancel_active_summary(meeting_id: &str) -> bool {
        let registry = CANCELLATION_REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match registry.get(meeting_id) {
            Some(entry) => {
                info!("Cancelling summary generation for meeting: {}", meeting_id);
                entry.token.cancel();
                true
            }
            None => false,
        }
    }

    /// Cleans up only the matching generation token after processing completes.
    /// Run one summary attempt so it always ends in a terminal state. A Rust
    /// panic (unwind) inside the attempt marks *that* attempt failed via the
    /// compare-and-set on (meeting_id, start_time, pending): a completion that
    /// was already persisted, or a newer attempt, is never overwritten. The
    /// cancellation token is released on every exit. Native aborts cannot be
    /// caught here; startup reconciliation covers those.
    pub(crate) async fn supervise_summary_attempt<F>(
        pool: SqlitePool,
        meeting_id: String,
        started_at: DateTime<Utc>,
        attempt: F,
    ) where
        F: std::future::Future<Output = ()>,
    {
        use futures_util::FutureExt;
        if std::panic::AssertUnwindSafe(attempt).catch_unwind().await.is_err() {
            // Never log the panic payload: it can quote meeting content.
            error!("Summary task for meeting {} panicked", meeting_id);
            Self::update_process_failed(&pool, &meeting_id, started_at, SUMMARY_TASK_CRASHED).await;
        }
        Self::cleanup_cancellation_token(&meeting_id, started_at);
    }

    fn cleanup_cancellation_token(meeting_id: &str, started_at: DateTime<Utc>) {
        let mut registry = CANCELLATION_REGISTRY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if registry
            .get(meeting_id)
            .is_some_and(|entry| entry.started_at == started_at)
        {
            registry.remove(meeting_id);
            info!("Cleaned up cancellation token for meeting: {}", meeting_id);
        }
    }

    pub(crate) async fn read_detected_summary_language(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Option<String> {
        let meeting = match MeetingsRepository::get_meeting_metadata(pool, meeting_id).await {
            Ok(Some(meeting)) => meeting,
            Ok(None) => {
                warn!("Meeting not found while reading detected summary language: {}", meeting_id);
                return None;
            }
            Err(e) => {
                warn!(
                    "Failed to read meeting metadata for detected summary language (meeting_id={}): {}",
                    meeting_id, e
                );
                return None;
            }
        };

        let Some(folder_path) = meeting.folder_path.filter(|p| !p.trim().is_empty()) else {
            return None;
        };

        match read_detected_summary_language_from_metadata(Path::new(&folder_path)) {
            Ok(language) => language,
            Err(e) => {
                warn!(
                    "Failed to read detected summary language metadata for meeting_id={}: {}",
                    meeting_id, e
                );
                None
            }
        }
    }

    /// The `<calendar_context>` block for a meeting recorded against an
    /// Outlook event; `None` for every other meeting or on any read failure.
    async fn read_calendar_context(pool: &SqlitePool, meeting_id: &str) -> Option<String> {
        let calendar = match MeetingCalendarRepository::get(pool, meeting_id).await {
            Ok(calendar) => calendar?,
            Err(e) => {
                warn!("Calendar details unavailable for meeting {}: {}", meeting_id, e);
                return None;
            }
        };
        let include_body = match crate::calendar::load_settings().await {
            Ok(settings) => settings.include_body_in_summary,
            Err(e) => {
                warn!("Calendar settings unreadable, leaving the invitation body out: {}", e);
                false
            }
        };
        let title = MeetingsRepository::get_meeting_metadata(pool, meeting_id)
            .await
            .ok()
            .flatten()
            .map(|meeting| meeting.title)
            .unwrap_or_default();
        build_calendar_context(&title, &calendar, include_body)
    }

    pub(crate) fn detect_summary_language_from_text(text: &str) -> Option<String> {
        let transcript_texts = [text.to_string()];
        let detection = detect_summary_language(&transcript_texts);
        match &detection.language {
            Some(language) => {
                info!("Detected transcript summary language for normalization: {}", language);
            }
            None => {
                info!(
                    "Transcript summary language unknown for normalization: {:?}",
                    detection.reason
                );
            }
        }
        detection.language
    }

    /// Processes transcript in the background and generates summary
    ///
    /// This function is designed to be spawned as an async task and does not block
    /// the main thread. It updates the database with progress and results.
    ///
    /// # Arguments
    /// * `_app` - Tauri app handle (for future use)
    /// * `pool` - SQLx connection pool
    /// * `meeting_id` - Unique identifier for the meeting
    /// * `text` - Full transcript text
    /// * `model_provider` - LLM provider name (e.g., "ollama", "openai")
    /// * `model_name` - Specific model (e.g., "gpt-4", "llama3.2:latest")
    /// * `custom_prompt` - Optional user-provided context
    /// * `template_id` - Template identifier (e.g., "daily_standup", "standard_meeting")
    pub async fn process_transcript_background<R: tauri::Runtime>(
        app: AppHandle<R>,
        pool: SqlitePool,
        meeting_id: String,
        started_at: DateTime<Utc>,
        cancellation_token: CancellationToken,
        text: String,
        model_provider: String,
        model_name: String,
        custom_prompt: String,
        template_id: String,
        summary_language: Option<String>,
    ) {
        let start_time = Instant::now();
        info!(
            "Starting background processing for meeting_id: {}",
            meeting_id
        );

        // Parse provider
        let provider = match LLMProvider::from_str(&model_provider) {
            Ok(p) => p,
            Err(e) => {
                Self::fail_and_cleanup(&pool, &meeting_id, started_at, &e).await;
                return;
            }
        };

        // Validate and setup api_key, Flexible for Ollama, BuiltInAI, CustomOpenAI, and CopilotCli
        let api_key = if provider == LLMProvider::Ollama
            || provider == LLMProvider::BuiltInAI
            || provider == LLMProvider::CustomOpenAI
            || provider == LLMProvider::CopilotCli
        {
            // These providers don't require API keys from the standard database column
            String::new()
        } else {
            match SettingsRepository::get_api_key(&pool, &model_provider).await {
                Ok(Some(key)) if !key.is_empty() => key,
                Ok(None) | Ok(Some(_)) => {
                    let err_msg = format!("API key not found for {}", &model_provider);
                    Self::fail_and_cleanup(&pool, &meeting_id, started_at, &err_msg).await;
                    return;
                }
                Err(e) => {
                    let err_msg = format!("Failed to retrieve API key for {}: {}", &model_provider, e);
                    Self::fail_and_cleanup(&pool, &meeting_id, started_at, &err_msg).await;
                    return;
                }
            }
        };

        // Get Ollama endpoint if provider is Ollama
        let ollama_endpoint = if provider == LLMProvider::Ollama {
            match SettingsRepository::get_model_config(&pool).await {
                Ok(Some(config)) => config.ollama_endpoint,
                Ok(None) => None,
                Err(e) => {
                    info!("Failed to retrieve Ollama endpoint: {}, using default", e);
                    None
                }
            }
        } else {
            None
        };

        // Get CustomOpenAI config if provider is CustomOpenAI
        let (custom_openai_endpoint, custom_openai_api_key, custom_openai_max_tokens, custom_openai_temperature, custom_openai_top_p) =
            if provider == LLMProvider::CustomOpenAI {
                match SettingsRepository::get_custom_openai_config(&pool).await {
                    Ok(Some(config)) => {
                        info!(
                            "✓ Using custom OpenAI endpoint_origin={}",
                            url_origin_for_log(&config.endpoint)
                        );
                        (
                            Some(config.endpoint),
                            config.api_key,
                            config.max_tokens.map(|t| t as u32),
                            config.temperature,
                            config.top_p,
                        )
                    }
                    Ok(None) => {
                        let err_msg = "Custom OpenAI provider selected but no configuration found";
                        Self::fail_and_cleanup(&pool, &meeting_id, started_at, err_msg).await;
                        return;
                    }
                    Err(e) => {
                        let err_msg = format!("Failed to retrieve custom OpenAI config: {}", e);
                        Self::fail_and_cleanup(&pool, &meeting_id, started_at, &err_msg).await;
                        return;
                    }
                }
            } else {
                (None, None, None, None, None)
            };

        // For CustomOpenAI, use its API key (if any) instead of the empty string
        let final_api_key = if provider == LLMProvider::CustomOpenAI {
            custom_openai_api_key.unwrap_or_default()
        } else {
            api_key
        };

        // Get Copilot CLI config if provider is CopilotCli (missing config is fine:
        // the binary is auto-detected and auth falls back to `copilot login` credentials)
        let copilot_cli_config = if provider == LLMProvider::CopilotCli {
            match SettingsRepository::get_copilot_cli_config(&pool).await {
                Ok(config) => {
                    if config.is_some() {
                        info!("✓ Using stored GitHub Copilot CLI config");
                    }
                    config
                }
                Err(e) => {
                    warn!("Failed to retrieve Copilot CLI config: {}, using defaults", e);
                    None
                }
            }
        } else {
            None
        };

        // Dynamically fetch context size based on provider and model
        let token_threshold = if provider == LLMProvider::Ollama {
            match METADATA_CACHE.get_or_fetch(&model_name, ollama_endpoint.as_deref()).await {
                Ok(metadata) => {
                    // Reserve 300 tokens for prompt overhead
                    let optimal = metadata.context_size.saturating_sub(300);
                    info!(
                        "✓ Using dynamic context for {}: {} tokens (chunk size: {})",
                        model_name, metadata.context_size, optimal
                    );
                    optimal
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch context for {}: {}. Using default 4000",
                        model_name, e
                    );
                    4000  // Fallback to safe default
                }
            }
        } else if provider == LLMProvider::BuiltInAI {
            // Get model's context size from registry
            use crate::summary::summary_engine::models;
            let model = models::get_model_by_name(&model_name)
                .ok_or_else(|| format!("Unknown model: {}", model_name));

            match model {
                Ok(model_def) => {
                    // Reserve 300 tokens for prompt overhead
                    let optimal = model_def.context_size.saturating_sub(300) as usize;
                    info!(
                        "✓ Using BuiltInAI context size: {} tokens (chunk size: {})",
                        model_def.context_size, optimal
                    );
                    optimal
                }
                Err(e) => {
                    warn!("{}, using default 2048", e);
                    1748  // 2048 - 300 for overhead
                }
            }
        } else {
            // Cloud providers (OpenAI, Claude, Groq, CustomOpenAI, CopilotCli) handle large contexts automatically
            100000  // Effectively unlimited for single-pass processing
        };

        // Data root for BuiltInAI provider model resolution
        let app_data_dir = Some(crate::storage::root());

        if let Some(code) = &summary_language {
            info!("📝 Summary language preference: {}", code);
        }

        let detected_summary_language =
            Self::read_detected_summary_language(&pool, &meeting_id)
                .await
                .or_else(|| Self::detect_summary_language_from_text(&text));

        if let Some(code) = &detected_summary_language {
            info!("📝 Detected transcript summary language: {}", code);
        }

        let template = match templates::get_template(&template_id) {
            Ok(template) => template,
            Err(e) => {
                let err_msg = format!("Failed to load template '{}': {}", template_id, e);
                Self::fail_and_cleanup(&pool, &meeting_id, started_at, &err_msg).await;
                return;
            }
        };
        let template_fingerprint = template_cache_fingerprint(&template);

        // Read at summary time so turning the body toggle off later stops the
        // invitation text from being sent.
        let calendar_context = Self::read_calendar_context(&pool, &meeting_id).await;
        if calendar_context.is_some() {
            info!("📅 Adding calendar context to the summary prompt for meeting_id: {}", meeting_id);
        }
        // Read at summary time so glossary edits apply to the next summary.
        let glossary = crate::summary::glossary::load();
        // The calendar block and the glossary change the prompt, so they are
        // part of the cache key.
        let prompt_context_for_cache = format!(
            "{}{}{}",
            custom_prompt,
            calendar_context.as_deref().unwrap_or_default(),
            glossary.as_deref().unwrap_or_default()
        );

        let cache_source = build_summary_cache_source(
            &text,
            &prompt_context_for_cache,
            &template_id,
            &template_fingerprint,
            token_threshold,
            &model_provider,
            &model_name,
            ollama_endpoint.as_deref(),
            custom_openai_endpoint.as_deref(),
            custom_openai_max_tokens,
            custom_openai_temperature,
            custom_openai_top_p,
        );

        let cached_english = match SummaryProcessesRepository::get_summary_data(&pool, &meeting_id).await {
            Err(e) => {
                warn!(
                    "Failed to load prior summary row for cache lookup (meeting_id={}): {}. Falling back to full pass-1 generation.",
                    meeting_id, e
                );
                None
            }
            Ok(None) => None,
            Ok(Some(process)) => process.result.and_then(|raw| {
                match extract_cached_english_markdown(
                    &raw,
                    &cache_source,
                    summary_language.as_deref(),
                ) {
                    Ok(opt) => opt,
                    Err(e) => {
                        warn!(
                            "Cached summary result for meeting_id={} is not valid JSON ({}); ignoring cache.",
                            meeting_id, e
                        );
                        None
                    }
                }
            }),
        };

        let client = reqwest::Client::new();
        let result = generate_meeting_summary(
            &client,
            &provider,
            &model_name,
            &final_api_key,
            &text,
            &custom_prompt,
            calendar_context.as_deref(),
            glossary.as_deref(),
            &template_id,
            &template,
            token_threshold,
            ollama_endpoint.as_deref(),
            custom_openai_endpoint.as_deref(),
            custom_openai_max_tokens,
            custom_openai_temperature,
            custom_openai_top_p,
            app_data_dir.as_ref(),
            copilot_cli_config.as_ref(),
            Some(&cancellation_token),
            summary_language.as_deref(),
            detected_summary_language.as_deref(),
            cached_english.as_deref(),
        )
        .await;

        let duration = start_time.elapsed().as_secs_f64();

        match result {
            Ok(generated) => {
                info!(
                    "✓ Successfully processed {} chunks for meeting_id: {}. Duration: {:.2}s",
                    generated.successful_chunk_count, meeting_id, duration
                );
                let mut result_json = match build_summary_result_json(
                    &generated.final_markdown,
                    &generated.english_markdown,
                    cache_source,
                    summary_language.as_deref(),
                    generated.reasoning_stripped,
                    generated.normalization_fallback,
                ) {
                    Ok(result) => result,
                    Err(error) => {
                        Self::update_process_failed(&pool, &meeting_id, started_at, &error).await;
                        Self::cleanup_cancellation_token(&meeting_id, started_at);
                        return;
                    }
                };
                attach_summary_provenance(
                    &mut result_json,
                    &SummaryProvenance::new(
                        &model_provider,
                        &model_name,
                        &template_id,
                        summary_language.as_deref(),
                    ),
                );

                match SummaryProcessesRepository::update_process_completed(
                    &pool,
                    &meeting_id,
                    started_at,
                    result_json,
                    generated.successful_chunk_count,
                    duration,
                )
                .await
                {
                    Ok(true) => {
                        if let Some(name) =
                            extract_meeting_name_from_markdown(&generated.final_markdown)
                                .filter(|name| !name.is_empty())
                        {
                            if let Err(error) =
                                MeetingsRepository::update_meeting_name(&pool, &meeting_id, &name)
                                    .await
                            {
                                error!("Failed to update meeting name for {}: {}", meeting_id, error);
                            }
                        }
                        info!("Summary saved successfully for meeting_id: {}", meeting_id);
                        Self::auto_export_to_obsidian(
                            &app,
                            &pool,
                            &meeting_id,
                            &generated.final_markdown,
                        )
                        .await;
                    }
                    Ok(false) => warn!("Skipped stale summary completion for meeting_id: {}", meeting_id),
                    Err(error) => error!(
                        "Failed to save completed process for {}: {}",
                        meeting_id, error
                    ),
                }
            }
            Err(error) if cancellation_token.is_cancelled() => {
                match SummaryProcessesRepository::update_process_cancelled(
                    &pool,
                    &meeting_id,
                    started_at,
                )
                .await
                {
                    Ok(false) => warn!("Skipped stale summary cancellation for meeting_id: {}", meeting_id),
                    Ok(true) => info!("Summary generation was cancelled for meeting_id: {}", meeting_id),
                    Err(db_error) => error!(
                        "Failed to update DB status to cancelled for {}: {}",
                        meeting_id, db_error
                    ),
                }
            }
            Err(error) => {
                Self::update_process_failed(&pool, &meeting_id, started_at, &error).await;
            }
        }
        Self::cleanup_cancellation_token(&meeting_id, started_at);
    }

    async fn auto_export_to_obsidian<R: tauri::Runtime>(
        app: &AppHandle<R>,
        pool: &SqlitePool,
        meeting_id: &str,
        final_markdown: &str,
    ) {
        let settings = match crate::obsidian::load_obsidian_settings(app).await {
            Ok(settings) => settings,
            Err(error) => {
                warn!("Obsidian auto-export: failed to load settings: {}", error);
                return;
            }
        };
        if !settings.auto_export || settings.vault_path.is_none() {
            return;
        }

        let meeting = match MeetingsRepository::get_meeting_metadata(pool, meeting_id).await {
            Ok(Some(meeting)) => meeting,
            Ok(None) => {
                warn!("Obsidian auto-export: meeting {} not found", meeting_id);
                return;
            }
            Err(error) => {
                warn!("Obsidian auto-export: failed to load meeting {}: {}", meeting_id, error);
                return;
            }
        };
        let transcript_markdown = match MeetingsRepository::get_meeting_transcripts_paginated(
            pool, meeting_id, i64::MAX, 0,
        )
        .await
        {
            Ok((segments, _)) => crate::obsidian::format_transcript_markdown(&segments),
            Err(error) => {
                warn!("Obsidian auto-export: failed to load transcripts for {}: {}", meeting_id, error);
                return;
            }
        };

        match crate::obsidian::export_meeting_note(
            app,
            meeting_id,
            &meeting.title,
            &meeting.created_at.0.to_rfc3339(),
            &strip_title_if_present(final_markdown),
            &transcript_markdown,
        )
        .await
        {
            Ok(result) => {
                info!("Obsidian auto-export: wrote {} for meeting {}", result.file_path, meeting_id);
                let _ = app.emit(
                    "obsidian-export-complete",
                    serde_json::json!({
                        "meeting_id": meeting_id,
                        "relative_path": result.relative_path,
                    }),
                );
            }
            Err(error) => warn!("Obsidian auto-export failed for meeting {}: {}", meeting_id, error),
        }
    }

    /// Updates the summary process status to failed with error message
    async fn fail_and_cleanup(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        error_msg: &str,
    ) {
        Self::update_process_failed(pool, meeting_id, started_at, error_msg).await;
        Self::cleanup_cancellation_token(meeting_id, started_at);
    }

    async fn update_process_failed(
        pool: &SqlitePool,
        meeting_id: &str,
        started_at: DateTime<Utc>,
        error_msg: &str,
    ) {
        error!("Summary generation failed for meeting_id {}", meeting_id);
        match SummaryProcessesRepository::update_process_failed(
            pool,
            meeting_id,
            started_at,
            error_msg,
        )
        .await
        {
            Ok(false) => warn!("Skipped stale summary failure for meeting_id: {}", meeting_id),
            Ok(true) => {}
            Err(e) => error!(
                "Failed to update DB status to failed for {}: {}",
                meeting_id, e
            ),
        }
    }


}

#[cfg(test)]
mod tests {
    use super::*;

    async fn supervisor_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO meetings (id, title, created_at, updated_at) VALUES ('m', 'M', '2026-10-04T00:00:00Z', '2026-10-04T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn status_of(pool: &SqlitePool) -> (String, Option<String>) {
        sqlx::query_as("SELECT status, error FROM summary_processes WHERE meeting_id = 'm'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn panic_before_persistence_fails_only_that_attempt_and_cleans_up() {
        let pool = supervisor_pool().await;
        let started_at = Utc::now();
        SummaryProcessesRepository::create_or_reset_process(&pool, "m", started_at).await.unwrap();
        let token = SummaryService::register_cancellation_token("m", started_at);

        SummaryService::supervise_summary_attempt(pool.clone(), "m".into(), started_at, async {
            panic!("bug in a provider adapter");
        })
        .await;

        let (status, error) = status_of(&pool).await;
        assert_eq!(status, "failed");
        assert_eq!(error.as_deref(), Some(SUMMARY_TASK_CRASHED));
        assert!(!SummaryService::cancel_summary("m", started_at), "token must be released");
        assert!(!token.is_cancelled());
    }

    #[tokio::test]
    async fn panic_after_persistence_preserves_the_completed_summary() {
        let pool = supervisor_pool().await;
        let started_at = Utc::now();
        SummaryProcessesRepository::create_or_reset_process(&pool, "m", started_at).await.unwrap();
        let inner = pool.clone();
        SummaryService::supervise_summary_attempt(pool.clone(), "m".into(), started_at, async move {
            SummaryProcessesRepository::update_process_completed(
                &inner, "m", started_at, serde_json::json!({"markdown": "done"}), 1, 1.0,
            )
            .await
            .unwrap();
            panic!("export failed after save");
        })
        .await;
        assert_eq!(status_of(&pool).await.0, "completed");
    }

    #[tokio::test]
    async fn stale_attempt_panic_cannot_change_a_newer_attempt() {
        let pool = supervisor_pool().await;
        let old_start = Utc::now();
        let new_start = old_start + chrono::Duration::seconds(1);
        SummaryProcessesRepository::create_or_reset_process(&pool, "m", new_start).await.unwrap();
        SummaryService::supervise_summary_attempt(pool.clone(), "m".into(), old_start, async {
            panic!("old attempt");
        })
        .await;
        assert_eq!(status_of(&pool).await.0.to_lowercase(), "pending");
    }

    #[tokio::test]
    async fn restart_reconciliation_fails_orphaned_attempts_and_restores_previous_summary() {
        let pool = supervisor_pool().await;
        let first = Utc::now();
        SummaryProcessesRepository::create_or_reset_process(&pool, "m", first).await.unwrap();
        SummaryProcessesRepository::update_process_completed(
            &pool, "m", first, serde_json::json!({"markdown": "previous"}), 1, 1.0,
        )
        .await
        .unwrap();
        // Regeneration started, then the app died natively.
        SummaryProcessesRepository::create_or_reset_process(&pool, "m", first + chrono::Duration::seconds(5))
            .await
            .unwrap();

        assert_eq!(SummaryProcessesRepository::fail_interrupted_processes(&pool).await.unwrap(), 1);
        let (status, error) = status_of(&pool).await;
        assert_eq!(status, "failed");
        assert!(error.unwrap().contains("interrupted"));
        let result: Option<String> = sqlx::query_scalar("SELECT result FROM summary_processes WHERE meeting_id = 'm'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(result.unwrap().contains("previous"));
        // Completed rows are left alone on later startups.
        assert_eq!(SummaryProcessesRepository::fail_interrupted_processes(&pool).await.unwrap(), 0);
    }

    #[test]
    fn stale_cleanup_keeps_the_replacement_cancellation_token() {
        let meeting_id = format!("summary-test-{}", uuid::Uuid::new_v4());
        let first_started_at = Utc::now();
        let second_started_at = first_started_at + chrono::Duration::nanoseconds(1);
        let first = SummaryService::register_cancellation_token(&meeting_id, first_started_at);
        let second = SummaryService::register_cancellation_token(&meeting_id, second_started_at);

        assert!(first.is_cancelled());
        SummaryService::cleanup_cancellation_token(&meeting_id, first_started_at);
        assert!(SummaryService::cancel_summary(&meeting_id, second_started_at));
        assert!(second.is_cancelled());
        SummaryService::cleanup_cancellation_token(&meeting_id, second_started_at);
    }

    #[test]
    fn test_strip_leading_title_with_body() {
        let input = "# Meeting Title\nThis is the body.\nMore content.";
        let result = strip_leading_title(input);
        assert_eq!(result, "This is the body.\nMore content.");
    }

    #[test]
    fn test_strip_leading_title_only() {
        let input = "# Meeting Title";
        let result = strip_leading_title(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_leading_title_no_heading() {
        let input = "No heading here.\nJust body.";
        let result = strip_leading_title(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_leading_title_multiline_body() {
        let input = "# Title\n## Subheading\nParagraph 1\n\nParagraph 2";
        let result = strip_leading_title(input);
        assert_eq!(result, "## Subheading\nParagraph 1\n\nParagraph 2");
    }

    #[test]
    fn test_strip_leading_title_empty_after_heading() {
        let input = "# Title\n";
        let result = strip_leading_title(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_leading_title_whitespace_after_heading() {
        let input = "# Title\n   \n Body with leading spaces";
        let result = strip_leading_title(input);
        assert_eq!(result, "Body with leading spaces");
    }

    #[test]
    fn test_strip_title_if_present_preserves_already_stripped() {
        assert_eq!(strip_title_if_present("## Action Items\nfoo"), "## Action Items\nfoo");
    }

    #[test]
    fn test_strip_title_if_present_strips_leading_h1() {
        assert_eq!(strip_title_if_present("# Meeting Title\n## Action Items\nfoo"), "## Action Items\nfoo");
    }

    #[test]
    fn test_strip_title_if_present_no_heading_preserved() {
        // Distinct from strip_leading_title which returns "" — this preserves input.
        assert_eq!(strip_title_if_present("Just body text"), "Just body text");
    }

    #[test]
    fn test_strip_title_if_present_hash_no_space_preserved() {
        // `#NoSpace` is not a markdown H1 — preserve.
        assert_eq!(strip_title_if_present("#NoSpace\nbody"), "#NoSpace\nbody");
    }

    #[test]
    fn test_strip_title_if_present_mid_document_h1_preserved() {
        // H1 after body content must NOT be stripped — guards the asymmetry where
        // extract_meeting_name_from_markdown scans every line for "# ".
        let input = "Some paragraph\n\n# H1 on line 3\n## Section\nbody";
        assert_eq!(strip_title_if_present(input), input);
    }

    #[test]
    fn test_strip_title_if_present_leading_whitespace_h1_stripped() {
        assert_eq!(
            strip_title_if_present("  # Title\n## Section\nbody"),
            "## Section\nbody"
        );
    }

    #[test]
    fn provenance_stamp_records_the_model_that_produced_the_summary() {
        let mut result = serde_json::json!({ "markdown": "body" });
        attach_summary_provenance(
            &mut result,
            &SummaryProvenance::new("anthropic", "claude-opus-5", "daily_standup", Some("fr")),
        );

        assert_eq!(result["provenance"]["provider"], "anthropic");
        assert_eq!(result["provenance"]["model"], "claude-opus-5");
        assert_eq!(result["provenance"]["template_id"], "daily_standup");
        assert_eq!(result["provenance"]["summary_language"], "fr");
        assert_eq!(result["provenance"]["app_version"], env!("CARGO_PKG_VERSION"));
        // Existing content is preserved alongside the stamp.
        assert_eq!(result["markdown"], "body");
    }

    #[test]
    fn provenance_omits_summary_language_when_following_the_transcript() {
        let mut result = serde_json::json!({});
        attach_summary_provenance(
            &mut result,
            &SummaryProvenance::new("ollama", "llama3", "daily_standup", None),
        );

        assert!(result["provenance"].get("summary_language").is_none());
    }

    #[test]
    fn provenance_generated_at_is_parseable_rfc3339() {
        let provenance = SummaryProvenance::new("groq", "llama-3.3", "daily_standup", None);
        assert!(chrono::DateTime::parse_from_rfc3339(&provenance.generated_at).is_ok());
    }

    fn sample_cache_source() -> SummaryCacheSource {
        let template_fingerprint = stable_text_fingerprint("standard template prompt");
        build_summary_cache_source(
            "transcript body",
            "custom prompt",
            "standard_meeting",
            &template_fingerprint,
            3700,
            "ollama",
            "gemma3:1b",
            Some("http://localhost:11434"),
            None,
            None,
            None,
            None,
        )
    }

    fn test_template(section_title: &str) -> Template {
        Template {
            name: "Test".to_string(),
            description: "Test template".to_string(),
            sections: vec![crate::summary::templates::TemplateSection {
                title: section_title.to_string(),
                instruction: "Summarize this section".to_string(),
                format: "paragraph".to_string(),
                item_format: None,
                example_item_format: None,
            }],
        }
    }

    #[test]
    fn test_template_cache_fingerprint_changes_with_rendered_template() {
        assert_ne!(
            template_cache_fingerprint(&test_template("Summary")),
            template_cache_fingerprint(&test_template("Decisions"))
        );
    }

    #[test]
    fn test_legacy_english_markdown_field_is_cache_miss() {
        let raw = serde_json::json!({
            "markdown": "translated",
            "english_markdown": "# Old English\nBody"
        })
        .to_string();

        assert_eq!(
            extract_cached_english_markdown(&raw, &sample_cache_source(), Some("de")).unwrap(),
            None
        );
    }

    #[test]
    fn test_matching_source_changed_translation_target_reuses_cache() {
        let source = sample_cache_source();
        let raw = build_summary_result_json(
            "# Reunion\n## Points\nBonjour",
            "# Meeting\n## Points\nHello",
            source.clone(),
            Some("fr"),
            false,
            false,
        )
        .unwrap()
        .to_string();

        assert_eq!(
            extract_cached_english_markdown(&raw, &source, Some("de")).unwrap(),
            Some("# Meeting\n## Points\nHello".to_string())
        );
    }

    #[test]
    fn test_same_language_regeneration_rejects_cache() {
        let source = sample_cache_source();
        let raw = build_summary_result_json(
            "# Reunion\n## Points\nBonjour",
            "# Meeting\n## Points\nHello",
            source.clone(),
            Some("fr"),
            false,
            false,
        )
        .unwrap()
        .to_string();

        assert_eq!(
            extract_cached_english_markdown(&raw, &source, Some("fr")).unwrap(),
            None
        );
    }

    #[test]
    fn test_changed_summary_inputs_reject_cache() {
        let source = sample_cache_source();
        let template_fingerprint = source.template_fingerprint.clone();
        let raw = build_summary_result_json(
            "# Reunion\n## Points\nBonjour",
            "# Meeting\n## Points\nHello",
            source,
            Some("fr"),
            false,
            false,
        )
        .unwrap()
        .to_string();

        let changed_sources = [
            build_summary_cache_source(
                "changed transcript",
                "custom prompt",
                "standard_meeting",
                &template_fingerprint,
                3700,
                "ollama",
                "gemma3:1b",
                Some("http://localhost:11434"),
                None,
                None,
                None,
                None,
            ),
            build_summary_cache_source(
                "transcript body",
                "changed prompt",
                "standard_meeting",
                &template_fingerprint,
                3700,
                "ollama",
                "gemma3:1b",
                Some("http://localhost:11434"),
                None,
                None,
                None,
                None,
            ),
            build_summary_cache_source(
                "transcript body",
                "custom prompt",
                "daily_standup",
                &template_fingerprint,
                3700,
                "ollama",
                "gemma3:1b",
                Some("http://localhost:11434"),
                None,
                None,
                None,
                None,
            ),
            build_summary_cache_source(
                "transcript body",
                "custom prompt",
                "standard_meeting",
                &template_fingerprint,
                3700,
                "openai",
                "gemma3:1b",
                Some("http://localhost:11434"),
                None,
                None,
                None,
                None,
            ),
            build_summary_cache_source(
                "transcript body",
                "custom prompt",
                "standard_meeting",
                &template_fingerprint,
                3700,
                "ollama",
                "qwen2.5:3b",
                Some("http://localhost:11434"),
                None,
                None,
                None,
                None,
            ),
            build_summary_cache_source(
                "transcript body",
                "custom prompt",
                "standard_meeting",
                &template_fingerprint,
                3700,
                "ollama",
                "gemma3:1b",
                Some("http://localhost:11500"),
                None,
                None,
                None,
                None,
            ),
            build_summary_cache_source(
                "transcript body",
                "custom prompt",
                "standard_meeting",
                &template_fingerprint,
                3700,
                "ollama",
                "gemma3:1b",
                Some("http://localhost:11434"),
                Some("https://custom.example/v1"),
                Some(2048),
                Some(0.2),
                Some(0.9),
            ),
        ];

        for changed_source in changed_sources {
            assert_eq!(
                extract_cached_english_markdown(&raw, &changed_source, Some("de")).unwrap(),
                None
            );
        }
    }

    #[test]
    fn test_changed_template_content_rejects_cache() {
        let source = sample_cache_source();
        let raw = build_summary_result_json(
            "# Reunion\n## Points\nBonjour",
            "# Meeting\n## Points\nHello",
            source.clone(),
            Some("fr"),
            false,
            false,
        )
        .unwrap()
        .to_string();

        let changed_template = SummaryCacheSource {
            template_fingerprint: stable_text_fingerprint("changed template prompt"),
            ..source
        };

        assert_eq!(
            extract_cached_english_markdown(&raw, &changed_template, Some("de")).unwrap(),
            None
        );
    }

    #[test]
    fn test_changed_token_threshold_rejects_cache() {
        let source = sample_cache_source();
        let raw = build_summary_result_json(
            "# Reunion\n## Points\nBonjour",
            "# Meeting\n## Points\nHello",
            source.clone(),
            Some("fr"),
            false,
            false,
        )
        .unwrap()
        .to_string();

        let changed_threshold = SummaryCacheSource {
            token_threshold: 8192,
            ..source
        };

        assert_eq!(
            extract_cached_english_markdown(&raw, &changed_threshold, Some("de")).unwrap(),
            None
        );
    }

    #[test]
    fn test_result_json_strips_display_markdown_but_keeps_cache_title() {
        let result = build_summary_result_json(
            "# Translated Title\n## Decisions\nDone",
            "# English Title\n## Decisions\nDone",
            sample_cache_source(),
            Some("fr"),
            false,
            false,
        )
        .unwrap();

        assert_eq!(result["markdown"], "## Decisions\nDone");
        assert_eq!(
            result["english_cache"]["markdown"],
            "# English Title\n## Decisions\nDone"
        );
    }

    #[test]
    fn result_json_rejects_title_only_display_markdown() {
        assert_eq!(
            build_summary_result_json(
                "# Title",
                "# Title",
                sample_cache_source(),
                None,
                false,
                false,
            ),
            Err("Final summary contains no visible content after title removal".to_string())
        );
    }

    #[test]
    fn result_json_never_persists_reasoning_text() {
        let result = build_summary_result_json(
            "# Title\nVisible",
            "# Title\nVisible",
            sample_cache_source(),
            None,
            true,
            false,
        )
        .unwrap();
        assert_eq!(result["reasoning_stripped"], true);
        assert!(result.get("reasoning").is_none());
    }

    #[test]
    fn test_extract_cached_english_from_malformed_json_errors() {
        let raw = r#"{ not valid json"#;
        assert!(extract_cached_english_markdown(raw, &sample_cache_source(), Some("de")).is_err());
    }
}
