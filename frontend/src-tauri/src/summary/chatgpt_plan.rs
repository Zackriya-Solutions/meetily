//! Open-source Sign in with ChatGPT: native credentials and text-only inference.
//! https://developers.openai.com/siwc/token-sharing-open-source
use std::{
    fs,
    io::ErrorKind,
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
    #[serde(default)]
    auto_summary_profile: Option<String>,
    profiles: Vec<Profile>,
}

#[derive(Deserialize, Serialize)]
struct Profile {
    client_id: String,
    subject: String,
    email: String,
    model: String,
    #[serde(default)]
    models: Vec<PlanModel>,
    tokens: Option<Session>,
}

#[derive(Clone, Deserialize, Serialize)]
struct PlanModel {
    slug: String,
    display_name: String,
}

impl Profile {
    fn update_models(&mut self, models: Vec<PlanModel>) {
        if self.models.is_empty() || !models.iter().any(|model| model.slug == self.model) {
            self.model = default_model(&models).unwrap_or_default().to_owned();
        }
        self.models = models;
    }
}

fn default_model(models: &[PlanModel]) -> Option<&str> {
    [
        "gpt-6-luna",
        "gpt-5.6-luna",
        "gpt-6.1-terra",
        "gpt-5.6-terra",
    ]
    .into_iter()
    .find(|slug| models.iter().any(|model| &model.slug == slug))
    .or_else(|| models.first().map(|model| model.slug.as_str()))
}

fn summary_effort(model: &str) -> Option<&'static str> {
    match model {
        "gpt-6-luna" | "gpt-5.6-luna" => Some("medium"),
        "gpt-6.1-terra" | "gpt-5.6-terra" => Some("low"),
        _ => None,
    }
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
    pub auto_summary_allowed: bool,
    pub active_model: Option<String>,
    models: Vec<PlanModel>,
    profiles: Vec<ProfileStatus>,
}

#[derive(Serialize)]
struct ProfileStatus {
    id: String,
    email: String,
    active: bool,
}

impl Connections {
    // Callers hold SESSIONS so a first-load migration cannot race a refresh or sign-out.
    async fn load(dir: &Path) -> Result<Self, String> {
        let service = keychain_service(dir)?;
        let legacy_path = dir.join("chatgpt-plan.json");
        blocking_keychain(move || {
            #[cfg(target_os = "macos")]
            {
                let stored = match security_framework::passwords::get_generic_password(
                    &service,
                    "chatgpt-plan",
                ) {
                    Ok(bytes) => Some(bytes),
                    Err(error) if error.code() == -25300 => None, // errSecItemNotFound
                    Err(_) => {
                        return Err("Could not read the ChatGPT connection from Keychain.".into())
                    }
                };
                return Self::load_or_migrate(stored, &legacy_path, |bytes| {
                    Self::save_to_keychain(&service, bytes)
                });
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (service, legacy_path);
                Err("ChatGPT plan summaries are available on macOS.".into())
            }
        })
        .await
    }

    #[cfg(any(target_os = "macos", test))]
    fn load_or_migrate(
        stored: Option<Vec<u8>>,
        legacy_path: &Path,
        save: impl FnOnce(&[u8]) -> Result<(), String>,
    ) -> Result<Self, String> {
        let decode = |bytes: &[u8]| {
            serde_json::from_slice::<Self>(bytes)
                .map_err(|_| "Could not read the ChatGPT connection.".to_string())
        };
        let connection = if let Some(bytes) = stored {
            // Keychain already owns the connection. Never import a stale file over it.
            decode(&bytes)?
        } else {
            match fs::symlink_metadata(legacy_path) {
                Ok(metadata) if metadata.is_file() => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    return Ok(Self {
                        host_id: format!("urn:uuid:{}", uuid::Uuid::new_v4()),
                        active: None,
                        summary_default: false,
                        auto_summary_profile: None,
                        profiles: vec![],
                    });
                }
                _ => return Err("Could not read the previous ChatGPT connection.".into()),
            }
            let bytes = fs::read(legacy_path)
                .map_err(|_| "Could not read the previous ChatGPT connection.")?;
            let connection = decode(&bytes)?;
            save(&bytes)?;
            connection
        };

        // Also clean up a file left by an interruption after the successful save.
        if let Err(error) = fs::remove_file(legacy_path) {
            if error.kind() != ErrorKind::NotFound {
                log::warn!("Could not remove the previous ChatGPT connection file.");
            }
        }
        Ok(connection)
    }

    #[cfg(target_os = "macos")]
    fn save_to_keychain(service: &str, bytes: &[u8]) -> Result<(), String> {
        security_framework::passwords::set_generic_password(service, "chatgpt-plan", bytes)
            .map_err(|_| "Could not save the ChatGPT connection in Keychain.".into())
    }

    async fn save(&self, dir: &Path) -> Result<(), String> {
        let service = keychain_service(dir)?;
        let bytes =
            serde_json::to_vec(self).map_err(|_| "Could not save the ChatGPT connection.")?;
        blocking_keychain(move || {
            #[cfg(target_os = "macos")]
            return Self::save_to_keychain(&service, &bytes);
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (service, bytes);
                Err("ChatGPT plan summaries are available on macOS.".into())
            }
        })
        .await
    }

    fn auto_summary_allowed(&self, profile_id: &str) -> bool {
        self.summary_default
            && self.active.as_deref() == Some(profile_id)
            && self.auto_summary_profile.as_deref() == Some(profile_id)
            && self
                .profiles
                .iter()
                .any(|p| p.client_id == profile_id && p.tokens.is_some())
    }

    fn status(&self) -> ConnectionStatus {
        let profile = self
            .profiles
            .iter()
            .find(|p| self.active.as_ref() == Some(&p.client_id) && p.tokens.is_some());
        ConnectionStatus {
            summary_default: self.summary_default,
            auto_summary_allowed: self
                .active
                .as_deref()
                .is_some_and(|id| self.auto_summary_allowed(id)),
            active_model: profile.map(|p| p.model.clone()),
            models: profile.map(|p| p.models.clone()).unwrap_or_default(),
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

async fn blocking_keychain<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| "Could not access the ChatGPT connection.".to_string())?
}

fn keychain_service(dir: &Path) -> Result<String, String> {
    let identifier = dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Could not identify the Meetily app profile.")?;
    Ok(format!("{identifier}.chatgpt-plan"))
}

fn app_data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        return Err("ChatGPT plan summaries are available on macOS.".into());
    }
    app.path()
        .app_data_dir()
        .map_err(|_| "Could not identify the Meetily app profile.".into())
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
                && expected_client.map_or(true, |expected| client == expected) =>
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
#[serde(untagged)]
enum Audience {
    Client(String),
    Clients(Vec<String>),
}

impl Audience {
    fn is_for(&self, client_id: &str) -> bool {
        match self {
            Self::Client(client) => client == client_id,
            Self::Clients(clients) => matches!(clients.as_slice(), [client] if client == client_id),
        }
    }
}

#[derive(Deserialize)]
struct Identity {
    iss: String,
    aud: Audience,
    sub: String,
    email: String,
    nonce: String,
    exp: i64,
}

fn verify_identity(
    token: &str,
    keys: &Value,
    client_id: &str,
    nonce: &str,
) -> Result<Identity, &'static str> {
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err("token format");
    };
    let header_bytes = URL_SAFE_NO_PAD.decode(header).map_err(|_| "header encoding")?;
    let json_header: Value = serde_json::from_slice(&header_bytes).map_err(|_| "header format")?;
    if json_header["alg"] != "RS256" {
        return Err("signing algorithm");
    }
    let kid = json_header["kid"].as_str().ok_or("key identifier")?;
    let key = keys["keys"]
        .as_array()
        .ok_or("signing key list")?
        .iter()
        .find(|k| k["kid"] == kid && k["kty"] == "RSA")
        .ok_or("signing key lookup")?;
    let n = URL_SAFE_NO_PAD.decode(key["n"].as_str().ok_or("key modulus")?).map_err(|_| "key modulus")?;
    let e = URL_SAFE_NO_PAD.decode(key["e"].as_str().ok_or("key exponent")?).map_err(|_| "key exponent")?;
    let signed = URL_SAFE_NO_PAD.decode(sig).map_err(|_| "signature encoding")?;
    signature::RsaPublicKeyComponents { n, e }
        .verify(
            &signature::RSA_PKCS1_2048_8192_SHA256,
            format!("{header}.{payload}").as_bytes(),
            &signed,
        )
        .map_err(|_| "signature check")?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).map_err(|_| "claims encoding")?;
    let claims: Identity = serde_json::from_slice(&bytes).map_err(|_| "claims format")?;
    if claims.iss != ISSUER {
        return Err("issuer check");
    }
    if !claims.aud.is_for(client_id) {
        return Err("audience check");
    }
    if claims.nonce != nonce {
        return Err("nonce check");
    }
    if claims.exp <= chrono::Utc::now().timestamp() {
        return Err("expiration check");
    }
    if claims.sub.is_empty() {
        return Err("subject check");
    }
    Ok(claims)
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

async fn fetch_models(client: &Client, token: &str) -> Result<Vec<PlanModel>, String> {
    let catalog: Value = client
        .get(format!("{RESOURCE}/models"))
        .bearer_auth(token)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| "Could not list models for this ChatGPT plan.")?
        .json()
        .await
        .map_err(|_| "Could not list models for this ChatGPT plan.")?;
    let models: Vec<PlanModel> = catalog["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|model| model["visibility"] == "list")
        .filter_map(|model| serde_json::from_value::<PlanModel>(model.clone()).ok())
        .filter(|model| !model.slug.trim().is_empty() && !model.display_name.trim().is_empty())
        .collect();
    if models.is_empty() {
        return Err("No models are available for this ChatGPT plan.".into());
    }
    Ok(models)
}

#[tauri::command]
pub async fn chatgpt_plan_status<R: Runtime>(
    app: AppHandle<R>,
    refresh_models: Option<bool>,
) -> Result<Option<ConnectionStatus>, String> {
    if !cfg!(target_os = "macos") {
        return Ok(None);
    }
    let _sessions = SESSIONS.lock().await;
    let dir = app_data_dir(&app)?;
    let mut data = Connections::load(&dir).await?;
    if refresh_models == Some(true) {
        let index = data
            .profiles
            .iter()
            .position(|p| data.active.as_ref() == Some(&p.client_id) && p.tokens.is_some())
            .ok_or(RECONNECT)?;
        let (token, renewed) = profile_token(&mut data.profiles[index]).await?;
        if renewed {
            data.save(&dir).await?;
        }
        let client = http_client(Duration::from_secs(20))?;
        data.profiles[index].update_models(fetch_models(&client, &token).await?);
        data.save(&dir).await?;
    }
    Ok(Some(data.status()))
}

pub(super) async fn confirm_summary<R: Runtime>(
    app: &AppHandle<R>,
    profile_id: &str,
    meeting_title: &str,
    automatic: bool,
) -> Result<bool, String> {
    let data = {
        let _sessions = SESSIONS.lock().await;
        Connections::load(&app_data_dir(app)?).await?
    };
    let profile = data
        .profiles
        .iter()
        .find(|p| p.client_id == profile_id && p.tokens.is_some())
        .filter(|_| data.summary_default && data.active.as_deref() == Some(profile_id))
        .ok_or(RECONNECT)?;
    if automatic {
        return Ok(data.auto_summary_allowed(profile_id));
    }
    Ok(confirm(
        app,
        "Generate summary with ChatGPT",
        format!(
            "Send the transcript of “{meeting_title}” to OpenAI to generate a summary using the ChatGPT plan for {}?",
            profile.email
        ),
        "Generate Summary",
    ).await)
}

async fn confirm<R: Runtime>(
    app: &AppHandle<R>,
    title: &str,
    message: String,
    action: &str,
) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
    let (answer, result) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(message)
        .title(title)
        .buttons(MessageDialogButtons::OkCancelCustom(
            action.into(),
            "Cancel".into(),
        ))
        .show(move |accepted| {
            let _ = answer.send(accepted);
        });
    result.await.unwrap_or(false)
}

#[tauri::command]
pub async fn chatgpt_plan_set_summary_default<R: Runtime>(
    app: AppHandle<R>,
    enabled: Option<bool>,
    automatic: Option<bool>,
    model: Option<String>,
) -> Result<ConnectionStatus, String> {
    let _sessions = SESSIONS.lock().await;
    let dir = app_data_dir(&app)?;
    let mut data = Connections::load(&dir).await?;
    let enabled = enabled.unwrap_or(data.summary_default);
    let automatic = enabled && automatic.unwrap_or(false);
    if enabled && (!data.summary_default || automatic) && data.status().active_model.is_none() {
        return Err(RECONNECT.into());
    }

    data.auto_summary_profile = if automatic {
        let profile = data
            .profiles
            .iter()
            .find(|p| data.active.as_ref() == Some(&p.client_id) && p.tokens.is_some())
            .ok_or(RECONNECT)?;
        if !data.auto_summary_allowed(&profile.client_id) && !confirm(
            &app,
            "Allow automatic ChatGPT summaries",
            format!(
                "When Auto Summary is on, Meetily sends new meeting transcripts to OpenAI after recording to summarize them using the ChatGPT plan for {}.",
                profile.email
            ),
            "Allow Auto Summary",
        ).await {
            return Err("Auto Summary was not authorized.".into());
        }
        Some(profile.client_id.clone())
    } else {
        None
    };
    if enabled {
        if let Some(model) = model {
            let profile = data
                .profiles
                .iter_mut()
                .find(|p| data.active.as_ref() == Some(&p.client_id) && p.tokens.is_some())
                .ok_or(RECONNECT)?;
            if !profile
                .models
                .iter()
                .any(|available| available.slug == model)
            {
                return Err("Choose a model available to this ChatGPT plan.".into());
            }
            profile.model = model;
        }
    }
    data.summary_default = enabled;
    data.save(&dir).await?;
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
    let dir = app_data_dir(&app)?;
    let prior = {
        let _sessions = SESSIONS.lock().await;
        let data = Connections::load(&dir).await?;
        data.save(&dir).await?; // Persist the installation identifier before opening the browser.
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
        .map_err(|_| {
            log::warn!("ChatGPT verification could not fetch the signing keys");
            "Could not verify the ChatGPT account."
        })?
        .json()
        .await
        .map_err(|_| {
            log::warn!("ChatGPT verification could not parse the signing keys");
            "Could not verify the ChatGPT account."
        })?;
    let identity = verify_identity(&tokens.id_token, &keys, &client_id, &nonce)
        .map_err(|stage| {
            log::warn!("ChatGPT account verification failed at {stage}");
            "Could not verify the ChatGPT account."
        })?;
    if returning.is_some_and(|profile| profile.subject != identity.sub) {
        return Err("Could not verify the ChatGPT account.".into());
    }
    let models = fetch_models(&client, &tokens.access_token).await?;

    let _sessions = SESSIONS.lock().await;
    let mut data = Connections::load(&dir).await?;
    let previous = data.profiles.iter().find(|p| p.client_id == client_id);
    let mut profile = Profile {
        client_id: client_id.clone(),
        subject: identity.sub,
        email: identity.email,
        model: previous.map(|p| p.model.clone()).unwrap_or_default(),
        models: previous.map(|p| p.models.clone()).unwrap_or_default(),
        tokens: Some(tokens),
    };
    profile.update_models(models);
    data.profiles.retain(|p| p.client_id != client_id);
    data.active = Some(client_id);
    data.profiles.push(profile);
    data.save(&dir).await?;
    Ok(data.status())
}

#[tauri::command]
pub async fn chatgpt_plan_disconnect<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
    let _sign_in = SIGN_IN
        .try_lock()
        .map_err(|_| "Wait for the current ChatGPT sign-in to finish.")?;
    let _sessions = SESSIONS.lock().await;
    let dir = app_data_dir(&app)?;
    let mut data = Connections::load(&dir).await?;
    let Some(active) = data.active.take() else {
        return Ok(true);
    };
    let profile = data
        .profiles
        .iter_mut()
        .find(|p| p.client_id == active)
        .ok_or(RECONNECT)?;
    let tokens = profile.tokens.take().ok_or(RECONNECT)?;
    data.auto_summary_profile = None;
    // End local use even when OpenAI cannot confirm remote revocation.
    data.save(&dir).await?;
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
    let mut data = Connections::load(dir).await?;
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
    let (access, renewed) = profile_token(profile).await?;
    if renewed {
        data.save(dir).await?;
    }
    Ok(access)
}

async fn profile_token(profile: &mut Profile) -> Result<(String, bool), String> {
    let tokens = profile.tokens.as_ref().ok_or(RECONNECT)?;
    if tokens.expires_at > chrono::Utc::now().timestamp() + 60 {
        return Ok((tokens.access_token.clone(), false));
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
    Ok((access, true))
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

fn completed_summary(data: &str) -> Result<Option<String>, String> {
    let event: Value =
        serde_json::from_str(data).map_err(|_| "OpenAI returned an invalid summary stream.")?;
    match event["type"].as_str() {
        Some("response.completed") => {
            let text = event["response"]["output"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|item| item["type"] == "message" && item["role"] == "assistant")
                .flat_map(|item| item["content"].as_array().into_iter().flatten())
                .filter(|part| part["type"] == "output_text")
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if text.trim().is_empty() {
                return Err("ChatGPT returned an empty summary.".into());
            }
            Ok(Some(text))
        }
        Some("response.failed" | "response.incomplete" | "error") => Err(provider_error(
            event["response"]["error"]["code"]
                .as_str()
                .or(event["code"].as_str())
                .unwrap_or_default(),
        )),
        _ => Ok(None),
    }
}

pub(super) async fn summarize(
    token: &str,
    model: &str,
    instructions: &str,
    input: &str,
) -> Result<String, String> {
    let client = http_client(Duration::from_secs(300))?;
    let mut payload = json!({"model": model, "instructions": instructions,
        "input": [{"role": "user", "content": input}], "store": false, "stream": true});
    if let Some(effort) = summary_effort(model) {
        payload["reasoning"] = json!({"effort": effort});
    }
    let mut response = client
        .post(format!("{RESOURCE}/responses"))
        .bearer_auth(token)
        .json(&payload)
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
    let mut event = String::new();
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
                let text = text.trim_end_matches(['\r', '\n']);
                if let Some(data) = text.strip_prefix("data:") {
                    if event.len() + data.len() > 2_000_000 {
                        return Err("ChatGPT returned an oversized stream event.".into());
                    }
                    event.push_str(data.strip_prefix(' ').unwrap_or(data));
                    event.push('\n');
                } else if text.is_empty() && !event.is_empty() {
                    if let Some(summary) = completed_summary(&event)? {
                        return Ok(summary);
                    }
                    event.clear();
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
    fn id_token_audience_accepts_only_the_issued_client() {
        let client_id = "oaiapp_meetily";
        for audience in [json!(client_id), json!([client_id])] {
            let parsed: Audience = serde_json::from_value(audience).unwrap();
            assert!(parsed.is_for(client_id));
        }
        for audience in [
            json!("another_client"),
            json!([]),
            json!([client_id, "another_client"]),
        ] {
            let parsed: Audience = serde_json::from_value(audience).unwrap();
            assert!(!parsed.is_for(client_id));
        }
    }

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
    fn only_completed_assistant_text_becomes_the_summary() {
        assert_eq!(
            completed_summary(r#"{"type":"response.output_text.delta","delta":"Partial"}"#),
            Ok(None)
        );
        assert!(completed_summary(r#"{"type":"response.failed","response":{"error":{"code":"subscription_sharing_usage_limit_exceeded"}}}"#)
            .unwrap_err().contains("plan limit"));
        assert_eq!(
            completed_summary(
                r#"{"type":"response.completed","response":{"output":[
            {"type":"reasoning","content":[{"type":"output_text","text":"Internal"}]},
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"Meeting notes"}]}
        ]}}"#
            ),
            Ok(Some("Meeting notes".into()))
        );
        assert!(
            completed_summary(r#"{"type":"response.completed","response":{"output":[]}}"#).is_err()
        );
    }

    #[test]
    fn legacy_connection_survives_failed_save_then_migrates_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chatgpt-plan.json");
        let legacy = br#"{
            "host_id":"existing-host", "active":"existing-profile", "summary_default":true,
            "profiles":[{"client_id":"existing-profile","subject":"test","email":"test@example.invalid",
            "model":"existing-model","tokens":{"access_token":"synthetic","refresh_token":"synthetic",
            "id_token":"synthetic","expires_at":1234}}]
        }"#;
        fs::write(&path, legacy).unwrap();

        assert!(Connections::load_or_migrate(None, &path, |_| Err("save denied".into())).is_err());
        assert_eq!(fs::read(&path).unwrap(), legacy);

        let connection = Connections::load_or_migrate(None, &path, |bytes| {
            assert_eq!(bytes, legacy);
            assert!(path.exists());
            Ok(())
        })
        .unwrap();
        assert_eq!(connection.host_id, "existing-host");
        assert_eq!(connection.active.as_deref(), Some("existing-profile"));
        assert!(connection.summary_default);
        assert_eq!(
            connection.status().active_model.as_deref(),
            Some("existing-model")
        );
        assert!(!path.exists());
    }

    #[test]
    fn existing_keychain_item_is_authoritative_over_leftover_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chatgpt-plan.json");
        fs::write(&path, b"stale invalid data").unwrap();
        let stored = br#"{"host_id":"keychain-host","active":null,"profiles":[]}"#.to_vec();
        let connection = Connections::load_or_migrate(Some(stored), &path, |_| {
            panic!("a stale file must not overwrite Keychain")
        })
        .unwrap();
        assert_eq!(connection.host_id, "keychain-host");
        assert!(!path.exists());
    }

    #[test]
    fn summary_default_prefers_available_luna_then_terra() {
        let model = |slug: &str| PlanModel {
            slug: slug.into(),
            display_name: slug.into(),
        };
        let models = [
            model("gpt-6.2-sol"),
            model("gpt-6.1-terra"),
            model("gpt-6-luna"),
        ];
        assert_eq!(default_model(&models), Some("gpt-6-luna"));
        assert_eq!(default_model(&models[..2]), Some("gpt-6.1-terra"));
        assert_eq!(default_model(&models[..1]), Some("gpt-6.2-sol"));
        assert_eq!(
            summary_effort(default_model(&models).unwrap()),
            Some("medium")
        );
        assert_eq!(
            summary_effort(default_model(&models[..2]).unwrap()),
            Some("low")
        );
        assert_eq!(summary_effort(default_model(&models[..1]).unwrap()), None);
    }

    #[test]
    fn automatic_summaries_require_consent_for_the_active_connection() {
        let mut data: Connections = serde_json::from_value(json!({
            "host_id": "host", "active": "first", "summary_default": true,
            "profiles": [{
                "client_id": "first", "subject": "subject", "email": "test@example.invalid",
                "model": "test", "tokens": {
                    "access_token": "test", "refresh_token": "test", "id_token": "test", "expires_at": 1
                }
            }]
        })).unwrap();
        assert!(!data.auto_summary_allowed("first"));
        data.auto_summary_profile = Some("first".into());
        assert!(data.auto_summary_allowed("first"));
        data.active = Some("second".into());
        assert!(!data.auto_summary_allowed("first"));
        assert!(!data.auto_summary_allowed("second"));
        data.active = Some("first".into());
        data.summary_default = false;
        assert!(!data.auto_summary_allowed("first"));
    }

    #[test]
    fn keychain_entries_are_separate_between_app_profiles() {
        assert_ne!(
            keychain_service(Path::new("/app-data/com.meetily.ai")).unwrap(),
            keychain_service(Path::new("/app-data/com.meetily.consent-test")).unwrap(),
        );
    }
}
