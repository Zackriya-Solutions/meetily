//! Open-source Sign in with ChatGPT: native credentials and text-only inference.
//! https://developers.openai.com/siwc/token-sharing-open-source
use std::{
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Client;
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Mutex,
};
use url::Url;

const ISSUER: &str = "https://auth.openai.com";
const RESOURCE: &str = "https://api.openai.com/v1";
const TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";
const SCOPE: &str = "chatgpt.tokens.use.direct";
const RECONNECT: &str = "Reconnect ChatGPT in Summary Settings to use your plan.";
static SIGN_IN: Mutex<()> = Mutex::const_new(());
static SESSIONS: Mutex<()> = Mutex::const_new(());

#[derive(Deserialize, Serialize)]
struct Connections {
    host_id: String,
    active: Option<String>,
    #[serde(default)]
    summary_default: bool,
    profiles: Vec<Profile>,
}

#[derive(Deserialize, Serialize)]
struct Profile {
    client_id: String,
    subject: String,
    email: String,
    model: String,
    tokens: Option<Session>,
}

#[derive(Deserialize, Serialize)]
struct Session {
    access_token: String,
    refresh_token: String,
    id_token: String,
    expires_at: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    token_type: String,
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    scope: String,
    id_token: Option<String>,
}

impl TokenResponse {
    fn session(self, prior_id_token: Option<String>) -> Result<Session, String> {
        if !self.scope.split_whitespace().any(|scope| scope == SCOPE) {
            return Err("ChatGPT plan usage was not granted. Check your plan and authorize usage in the browser.".into());
        }
        if !self.token_type.eq_ignore_ascii_case("bearer")
            || self.access_token.is_empty()
            || self.refresh_token.is_empty()
            || !(1..=86400).contains(&self.expires_in)
        {
            return Err("OpenAI returned an invalid ChatGPT session.".into());
        }
        Ok(Session {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            id_token: self
                .id_token
                .or(prior_id_token)
                .ok_or("OpenAI did not return an identity token.")?,
            expires_at: chrono::Utc::now().timestamp() + self.expires_in,
        })
    }
}

#[derive(Serialize)]
pub struct ConnectionStatus {
    summary_default: bool,
    pub active_model: Option<String>,
    profiles: Vec<ProfileStatus>,
}

#[derive(Serialize)]
struct ProfileStatus {
    id: String,
    email: String,
    active: bool,
}

impl Connections {
    fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Could not read the ChatGPT connection.".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Self {
                host_id: format!("urn:uuid:{}", uuid::Uuid::new_v4()),
                active: None,
                summary_default: false,
                profiles: vec![],
            }),
            Err(_) => Err("Could not open the ChatGPT connection.".into()),
        }
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let write = || -> anyhow::Result<()> {
            let dir = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Missing app data directory"))?;
            std::fs::create_dir_all(dir)?;
            // NamedTempFile creates owner-only files on Unix. Persist atomically.
            let mut file = tempfile::NamedTempFile::new_in(dir)?;
            serde_json::to_writer(file.as_file_mut(), self)?;
            file.as_file_mut().flush()?;
            file.as_file().sync_all()?;
            file.persist(path)?;
            Ok(())
        };
        write().map_err(|_| "Could not save the ChatGPT connection.".into())
    }

    fn status(&self) -> ConnectionStatus {
        ConnectionStatus {
            summary_default: self.summary_default,
            active_model: self
                .profiles
                .iter()
                .find(|p| self.active.as_ref() == Some(&p.client_id) && p.tokens.is_some())
                .map(|p| p.model.clone()),
            profiles: self
                .profiles
                .iter()
                .map(|p| ProfileStatus {
                    id: p.client_id.clone(),
                    email: p.email.clone(),
                    active: self.active.as_ref() == Some(&p.client_id) && p.tokens.is_some(),
                })
                .collect(),
        }
    }
}

fn path(dir: &Path) -> PathBuf {
    dir.join("chatgpt-plan.json")
}

fn app_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        return Err("ChatGPT plan summaries are available on macOS.".into());
    }
    app.path()
        .app_data_dir()
        .map(|dir| path(&dir))
        .map_err(|_| "Could not open the app data directory.".into())
}

fn http_client(timeout: Duration) -> Result<Client, String> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|_| "Could not connect to OpenAI.".into())
}

fn random_secret() -> Result<String, String> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "Could not start secure ChatGPT sign-in.")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn callback(
    target: &str,
    state: &str,
    expected_client: Option<&str>,
) -> Option<Result<(String, String), String>> {
    if !target.starts_with("/auth/callback?") {
        return None;
    }
    let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    let mut fields = std::collections::HashMap::new();
    for (key, value) in url.query_pairs() {
        if fields.insert(key, value).is_some() {
            return None;
        }
    }
    if fields.get("state").map(|s| s.as_ref()) != Some(state) {
        return None;
    }
    if fields.contains_key("error") {
        return Some(Err("ChatGPT sign-in was declined or unavailable.".into()));
    }
    let client = fields
        .get("client_id")
        .map(|s| s.as_ref())
        .or(expected_client);
    let code = fields.get("code").filter(|s| !s.is_empty());
    match (client, code) {
        (Some(client), Some(code))
            if client.starts_with("oaiapp_")
                && expected_client.is_none_or(|expected| client == expected) =>
        {
            Some(Ok((client.to_owned(), code.to_string())))
        }
        _ => Some(Err("OpenAI returned an invalid sign-in callback.".into())),
    }
}

async fn wait_for_callback(
    listener: TcpListener,
    state: &str,
    client: Option<&str>,
) -> Result<(String, String), String> {
    tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            let (mut stream, _) = listener.accept().await.map_err(|_| "Could not receive the ChatGPT sign-in callback.")?;
            let mut bytes = Vec::new();
            let read = async {
                let mut buf = [0; 1024];
                while bytes.len() < 8192 {
                    let n = stream.read(&mut buf).await.ok()?;
                    if n == 0 { return None; }
                    bytes.extend_from_slice(&buf[..n]);
                    if bytes.windows(4).any(|v| v == b"\r\n\r\n") { break; }
                }
                let mut request = std::str::from_utf8(&bytes).ok()?.lines().next()?.split_whitespace();
                if request.next()? != "GET" { return None; }
                callback(request.next()?, state, client)
            };
            let received = tokio::time::timeout(Duration::from_secs(2), read).await.ok().flatten();
            let reply = if received.is_some() {
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nReturn to Meetily to finish connecting ChatGPT."
            } else {
                "HTTP/1.1 400 Bad Request\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nInvalid callback."
            };
            let _ = stream.write_all(reply.as_bytes()).await;
            if let Some(result) = received { return result; }
        }
    }).await.map_err(|_| "ChatGPT sign-in timed out. Try again from Summary Settings.")?
}

#[derive(Deserialize)]
struct Identity {
    iss: String,
    aud: String,
    sub: String,
    email: String,
    nonce: String,
    exp: i64,
}

fn verify_identity(token: &str, keys: &Value, client_id: &str, nonce: &str) -> Option<Identity> {
    let mut parts = token.split('.');
    let (header, payload, sig) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let json_header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).ok()?).ok()?;
    if json_header["alg"] != "RS256" {
        return None;
    }
    let kid = json_header["kid"].as_str()?;
    let key = keys["keys"]
        .as_array()?
        .iter()
        .find(|k| k["kid"] == kid && k["kty"] == "RSA")?;
    let n = URL_SAFE_NO_PAD.decode(key["n"].as_str()?).ok()?;
    let e = URL_SAFE_NO_PAD.decode(key["e"].as_str()?).ok()?;
    signature::RsaPublicKeyComponents { n, e }
        .verify(
            &signature::RSA_PKCS1_2048_8192_SHA256,
            format!("{header}.{payload}").as_bytes(),
            &URL_SAFE_NO_PAD.decode(sig).ok()?,
        )
        .ok()?;
    let claims: Identity = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    (claims.iss == ISSUER
        && claims.aud == client_id
        && claims.nonce == nonce
        && claims.exp > chrono::Utc::now().timestamp()
        && !claims.sub.is_empty())
    .then_some(claims)
}

async fn exchange(client: &Client, form: &[(&str, &str)]) -> Result<TokenResponse, String> {
    let response = client
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|_| "Could not contact OpenAI to connect ChatGPT.")?;
    if !response.status().is_success() {
        return Err(if response.status().is_server_error() {
            "ChatGPT sign-in is temporarily unavailable. Try again."
        } else {
            RECONNECT
        }
        .into());
    }
    response
        .json()
        .await
        .map_err(|_| "OpenAI returned an invalid ChatGPT session.".into())
}

#[tauri::command]
pub async fn chatgpt_plan_status<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<ConnectionStatus>, String> {
    if !cfg!(target_os = "macos") {
        return Ok(None);
    }
    Ok(Some(Connections::load(&app_path(&app)?)?.status()))
}

#[tauri::command]
pub async fn chatgpt_plan_set_summary_default<R: Runtime>(
    app: AppHandle<R>,
    enabled: bool,
) -> Result<ConnectionStatus, String> {
    let _sessions = SESSIONS.lock().await;
    let path = app_path(&app)?;
    let mut data = Connections::load(&path)?;
    if enabled && data.status().active_model.is_none() {
        return Err(RECONNECT.into());
    }
    data.summary_default = enabled;
    data.save(&path)?;
    Ok(data.status())
}

#[tauri::command]
pub async fn chatgpt_plan_connect<R: Runtime>(
    app: AppHandle<R>,
    profile_id: Option<String>,
) -> Result<ConnectionStatus, String> {
    let _sign_in = SIGN_IN
        .try_lock()
        .map_err(|_| "A ChatGPT sign-in is already in progress.")?;
    let path = app_path(&app)?;
    let prior = {
        let _sessions = SESSIONS.lock().await;
        let data = Connections::load(&path)?;
        data.save(&path)?; // Persist the installation identifier before opening the browser.
        data
    };
    let returning = profile_id
        .as_ref()
        .map(|id| {
            prior
                .profiles
                .iter()
                .find(|p| &p.client_id == id)
                .ok_or("This ChatGPT connection no longer exists.")
        })
        .transpose()?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "Could not open the sign-in callback.")?;
    let port = listener
        .local_addr()
        .map_err(|_| "Could not open the sign-in callback.")?
        .port();
    let redirect = format!("http://127.0.0.1:{port}/auth/callback");
    let (state, nonce, verifier) = (random_secret()?, random_secret()?, random_secret()?);
    let challenge = URL_SAFE_NO_PAD.encode(ring::digest::digest(
        &ring::digest::SHA256,
        verifier.as_bytes(),
    ));
    let client_hint = returning.map(|p| p.client_id.as_str());
    let mut url =
        Url::parse("https://auth.openai.com/api/accounts/authorize").expect("valid authorize URL");
    url.query_pairs_mut().extend_pairs([
        ("client_id", client_hint.unwrap_or("dynamic_agent_client")),
        ("response_type", "code"),
        ("ext_agent_host_id", prior.host_id.as_str()),
        ("redirect_uri", redirect.as_str()),
        (
            "scope",
            "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
        ),
        ("resource", RESOURCE),
        ("state", &state),
        ("nonce", &nonce),
        ("code_challenge_method", "S256"),
        ("code_challenge", &challenge),
    ]);
    if let Some(profile) = returning {
        url.query_pairs_mut()
            .append_pair("login_hint", &profile.email);
    } else {
        url.query_pairs_mut()
            .append_pair("agent_name_hint", "Meetily");
    }
    open::that_detached(url.as_str())
        .map_err(|_| "Could not open the browser for ChatGPT sign-in.")?;
    let (client_id, code) = wait_for_callback(listener, &state, client_hint).await?;
    let client = http_client(Duration::from_secs(20))?;
    let tokens = exchange(
        &client,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", &client_id),
            ("code", &code),
            ("code_verifier", &verifier),
            ("redirect_uri", &redirect),
            ("resource", RESOURCE),
        ],
    )
    .await?
    .session(None)?;
    let keys: Value = client
        .get(format!("{ISSUER}/.well-known/jwks.json"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| "Could not verify the ChatGPT account.")?
        .json()
        .await
        .map_err(|_| "Could not verify the ChatGPT account.")?;
    let identity = verify_identity(&tokens.id_token, &keys, &client_id, &nonce)
        .filter(|claims| returning.is_none_or(|p| p.subject == claims.sub))
        .ok_or("Could not verify the ChatGPT account.")?;
    let catalog: Value = client
        .get(format!("{RESOURCE}/models"))
        .bearer_auth(&tokens.access_token)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| "Could not list models for this ChatGPT plan.")?
        .json()
        .await
        .map_err(|_| "Could not list models for this ChatGPT plan.")?;
    let model = catalog["models"]
        .as_array()
        .and_then(|models| {
            models
                .iter()
                .filter(|model| model["visibility"] == "list")
                .find_map(|model| model["slug"].as_str().filter(|s| !s.is_empty()))
        })
        .ok_or("No models are available for this ChatGPT plan.")?
        .to_owned();

    let _sessions = SESSIONS.lock().await;
    let mut data = Connections::load(&path)?;
    data.profiles.retain(|p| p.client_id != client_id);
    data.active = Some(client_id.clone());
    data.profiles.push(Profile {
        client_id,
        subject: identity.sub,
        email: identity.email,
        model,
        tokens: Some(tokens),
    });
    data.save(&path)?;
    Ok(data.status())
}

#[tauri::command]
pub async fn chatgpt_plan_disconnect<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    let _sign_in = SIGN_IN
        .try_lock()
        .map_err(|_| "Wait for the current ChatGPT sign-in to finish.")?;
    let _sessions = SESSIONS.lock().await;
    let path = app_path(&app)?;
    let mut data = Connections::load(&path)?;
    let Some(active) = data.active.take() else {
        return Ok(true);
    };
    let profile = data
        .profiles
        .iter_mut()
        .find(|p| p.client_id == active)
        .ok_or(RECONNECT)?;
    let tokens = profile.tokens.take().ok_or(RECONNECT)?;
    // End local use even when OpenAI cannot confirm remote revocation.
    data.save(&path)?;
    let client = http_client(Duration::from_secs(5))?;
    let Ok(response) = client
        .get(format!("{ISSUER}/.well-known/openid-configuration"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
    else {
        return Ok(false);
    };
    let Ok(discovery) = response.json::<Value>().await else {
        return Ok(false);
    };
    let Some(endpoint) = discovery["revocation_endpoint"]
        .as_str()
        .and_then(|s| Url::parse(s).ok())
        .filter(|url| url.scheme() == "https" && url.host_str() == Some("auth.openai.com"))
    else {
        return Ok(false);
    };
    for delay in [0, 250] {
        tokio::time::sleep(Duration::from_millis(delay)).await;
        match client
            .post(endpoint.clone())
            .form(&[
                ("token", tokens.refresh_token.as_str()),
                ("token_type_hint", "refresh_token"),
                ("client_id", active.as_str()),
            ])
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => return Ok(true),
            Ok(response) if !response.status().is_server_error() => break,
            _ => {}
        }
    }
    Ok(false)
}

pub(super) async fn access_token(
    dir: &Path,
    model: &str,
    approved_profile: &str,
) -> Result<String, String> {
    if !cfg!(target_os = "macos") {
        return Err("ChatGPT plan summaries are available on macOS.".into());
    }
    let _sessions = SESSIONS.lock().await;
    let path = path(dir);
    let mut data = Connections::load(&path)?;
    if !data.summary_default {
        return Err("Choose ChatGPT plan as your summary provider in Settings.".into());
    }
    let profile = data
        .profiles
        .iter_mut()
        .find(|p| {
            data.active.as_ref() == Some(&p.client_id)
                && p.model == model
                && p.client_id == approved_profile
        })
        .ok_or(RECONNECT)?;
    let tokens = profile.tokens.as_ref().ok_or(RECONNECT)?;
    if tokens.expires_at > chrono::Utc::now().timestamp() + 60 {
        return Ok(tokens.access_token.clone());
    }
    let client = http_client(Duration::from_secs(20))?;
    let replacement = exchange(
        &client,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &profile.client_id),
            ("refresh_token", &tokens.refresh_token),
            ("resource", RESOURCE),
        ],
    )
    .await?
    .session(Some(tokens.id_token.clone()))?;
    let access = replacement.access_token.clone();
    profile.tokens = Some(replacement);
    data.save(&path)?;
    Ok(access)
}

#[derive(Default)]
struct SummaryStream {
    text: String,
    data: Vec<String>,
}

fn provider_error(code: &str) -> String {
    match code {
        "subscription_sharing_usage_limit_exceeded" => {
            "Your ChatGPT plan limit was reached. Check usage in ChatGPT Settings."
        }
        "subscription_sharing_usage_unavailable" => {
            "ChatGPT plan usage is unavailable. Check sharing in ChatGPT Settings."
        }
        _ => "OpenAI did not complete the summary. Try again or choose another provider.",
    }
    .into()
}

impl SummaryStream {
    fn line(&mut self, line: &str) -> Result<bool, String> {
        if let Some(data) = line.strip_prefix("data:") {
            self.data.push(data.trim_start().to_owned());
        } else if line.is_empty() && !self.data.is_empty() {
            let event: Value = serde_json::from_str(&std::mem::take(&mut self.data).join("\n"))
                .map_err(|_| "OpenAI returned an invalid summary stream.")?;
            match event["type"].as_str() {
                Some("response.output_text.delta") => {
                    let delta = event["delta"]
                        .as_str()
                        .ok_or("OpenAI returned an invalid text event.")?;
                    if self.text.len() + delta.len() > 2_000_000 {
                        return Err("ChatGPT summary exceeded the result size limit.".into());
                    }
                    self.text.push_str(delta);
                }
                Some("response.completed") if !self.text.trim().is_empty() => return Ok(true),
                Some("response.completed") => {
                    return Err("ChatGPT returned an empty summary.".into())
                }
                Some("response.failed" | "response.incomplete" | "error") => {
                    return Err(provider_error(
                        event["response"]["error"]["code"]
                            .as_str()
                            .or(event["code"].as_str())
                            .unwrap_or_default(),
                    ))
                }
                _ => {}
            }
        }
        Ok(false)
    }
}

pub(super) async fn summarize(
    token: &str,
    model: &str,
    instructions: &str,
    input: &str,
) -> Result<String, String> {
    let client = http_client(Duration::from_secs(300))?;
    let mut response = client
        .post(format!("{RESOURCE}/responses"))
        .bearer_auth(token)
        .json(&json!({"model": model, "instructions": instructions,
            "input": [{"role": "user", "content": input}], "store": false, "stream": true}))
        .send()
        .await
        .map_err(|_| "Could not reach OpenAI to generate the summary.")?;
    if !response.status().is_success() {
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(RECONNECT.into());
        }
        let error: Value = response.json().await.unwrap_or(Value::Null);
        return Err(provider_error(
            error["error"]["code"].as_str().unwrap_or_default(),
        ));
    }
    let mut stream = SummaryStream::default();
    let mut line = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "The ChatGPT summary stream was interrupted.")?
    {
        for part in chunk.split_inclusive(|byte| *byte == b'\n') {
            line.extend_from_slice(part);
            if line.len() > 2_000_000 {
                return Err("ChatGPT returned an oversized stream event.".into());
            }
            if part.ends_with(b"\n") {
                let text =
                    std::str::from_utf8(&line).map_err(|_| "ChatGPT returned invalid UTF-8.")?;
                if stream.line(text.trim_end_matches(['\r', '\n']))? {
                    return Ok(stream.text);
                }
                line.clear();
            }
        }
    }
    Err("The ChatGPT summary stream ended before completion.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_is_bound_to_state_and_registered_client() {
        assert!(callback(
            "/auth/callback?state=wrong&code=a&client_id=oaiapp_a",
            "right",
            None
        )
        .is_none());
        assert!(callback(
            "/auth/callback?state=right&state=wrong&code=a&client_id=oaiapp_a",
            "right",
            None
        )
        .is_none());
        assert!(callback(
            "/auth/callback?state=right&error=access_denied",
            "right",
            None
        )
        .unwrap()
        .is_err());
        assert!(callback(
            "/auth/callback?state=right&code=a&client_id=oaiapp_b",
            "right",
            Some("oaiapp_a")
        )
        .unwrap()
        .is_err());
        assert_eq!(
            callback(
                "/auth/callback?state=right&code=a",
                "right",
                Some("oaiapp_a")
            )
            .unwrap()
            .unwrap(),
            ("oaiapp_a".into(), "a".into())
        );
    }

    #[test]
    fn identity_access_without_plan_scope_is_rejected() {
        let response = TokenResponse {
            token_type: "Bearer".into(),
            access_token: "unused".into(),
            refresh_token: "unused".into(),
            id_token: Some("unused".into()),
            expires_in: 3600,
            scope: "openid email profile".into(),
        };
        assert!(response.session(None).is_err());
    }

    #[test]
    fn partial_summary_fails_on_plan_limit() {
        let mut stream = SummaryStream::default();
        assert!(!stream
            .line(r#"data: {"type":"response.output_text.delta","delta":"Partial"}"#)
            .unwrap());
        assert!(!stream.line("").unwrap());
        stream.line(r#"data: {"type":"response.failed","response":{"error":{"code":"subscription_sharing_usage_limit_exceeded"}}}"#).unwrap();
        assert!(stream.line("").unwrap_err().contains("plan limit"));
    }

    #[test]
    fn summary_is_available_only_after_completed_event() {
        let mut stream = SummaryStream::default();
        stream.line("event: response.output_text.delta").unwrap();
        stream
            .line(r#"data: {"type":"response.output_text.delta","delta":"Meeting notes"}"#)
            .unwrap();
        assert!(!stream.line("").unwrap());
        stream
            .line(r#"data: {"type":"response.completed"}"#)
            .unwrap();
        assert!(stream.line("").unwrap());
        assert_eq!(stream.text, "Meeting notes");
    }

    #[cfg(unix)]
    #[test]
    fn credential_file_is_private_and_host_id_survives_restart() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = path(dir.path());
        let data = Connections::load(&path).unwrap();
        data.save(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(Connections::load(&path).unwrap().host_id, data.host_id);
    }
}
