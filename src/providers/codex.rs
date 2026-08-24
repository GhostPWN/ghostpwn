use std::io::Write;
use std::net::TcpListener;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use reqwest::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use url::Url;

use crate::config::ProviderKind;
use crate::models::{ConversationMessage, MessageRole};
use crate::providers::sse::consume_sse;
use crate::providers::{Provider, provider_http_client};
use crate::secrets::SecretStore;

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const DEVICE_CODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
const DEVICE_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
const REDIRECT_PORTS: &[u16] = &[1455, 1457];
const CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const CODEX_MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
// The catalog uses this as a feature gate. GhostPWN consumes only the stable Responses fields and
// filters the returned picker visibility itself, so request the unpruned authenticated catalog.
const CODEX_CATALOG_CLIENT_VERSION: &str = "999.999.999";
const USER_AGENT_VALUE: &str = concat!("ghostpwn/", env!("CARGO_PKG_VERSION"));
const ORIGINATOR: &str = "ghostpwn";
const CODEX_FALLBACK_MODELS: &[&str] = &["gpt-5.4", "gpt-5.4-mini"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexCredentials {
    pub refresh_token: String,
    pub access_token: String,
    pub expires_at: u64,
    #[serde(default)]
    pub account_id: Option<String>,
}

impl CodexCredentials {
    fn is_fresh(&self, now: u64) -> bool {
        now < self.expires_at.saturating_sub(60)
    }
}

#[derive(Debug)]
pub struct BrowserAuth {
    pub authorization_url: String,
    pub state: String,
    pub verifier: String,
    pub redirect_uri: String,
    listener: TcpListener,
}

#[derive(Debug)]
pub struct BrowserCode {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: Option<String>,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DeviceTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    id_token: Option<String>,
    error: Option<String>,
}

pub struct DeviceAuth {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub interval: u64,
    pub expires_in: u64,
}

pub enum DevicePollResult {
    Success(CodexCredentials),
    Pending,
    Failed(String),
}

pub struct CodexProvider {
    credentials_json: String,
    model: String,
    client: Client,
    secret_store: Option<SecretStore>,
    credentials_cache: Mutex<Option<CodexCredentials>>,
}

impl CodexProvider {
    pub fn new(credentials_json: String, model: String, secret_store: Option<SecretStore>) -> Self {
        Self {
            credentials_json,
            model,
            client: provider_http_client(),
            secret_store,
            credentials_cache: Mutex::new(None),
        }
    }

    async fn ensure_credentials(&self) -> Result<CodexCredentials> {
        let mut cache = self.credentials_cache.lock().await;
        let now = unix_now()?;

        if let Some(credentials) = cache.as_ref()
            && credentials.is_fresh(now)
        {
            return Ok(credentials.clone());
        }

        let mut credentials = if let Some(credentials) = cache.as_ref() {
            credentials.clone()
        } else {
            parse_credentials(&self.credentials_json)?
        };

        if !credentials.is_fresh(now) {
            credentials = refresh_credentials(&self.client, &credentials).await?;
            if let Some(secret_store) = self.secret_store.as_ref() {
                persist_credentials(secret_store, &credentials)?;
            }
        }

        *cache = Some(credentials.clone());
        Ok(credentials)
    }
}

#[async_trait]
impl Provider for CodexProvider {
    fn display_name(&self) -> String {
        format!("codex / {}", self.model)
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let credentials = self.ensure_credentials().await?;
        let response = self
            .client
            .get(CODEX_MODELS_URL)
            .query(&[("client_version", CODEX_CATALOG_CLIENT_VERSION)])
            .headers(codex_headers(&credentials, "application/json")?)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(_) => return Ok(codex_fallback_models()),
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if matches!(
                status,
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
            ) {
                return Err(anyhow!("Codex models API error {}: {}", status, body));
            }
            return Ok(codex_fallback_models());
        }

        let body: Value = response.json().await?;
        let models = parse_codex_models(&body);
        if models.is_empty() {
            return Ok(codex_fallback_models());
        }

        Ok(models)
    }

    async fn stream_complete(
        &self,
        system: &str,
        messages: &[ConversationMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String> {
        let credentials = self.ensure_credentials().await?;
        let payload = json!({
            "model": self.model,
            "instructions": system,
            "input": map_messages(messages),
            "stream": true,
            "store": false,
        });

        let response = self
            .client
            .post(CODEX_RESPONSES_URL)
            .headers(codex_headers(&credentials, "text/event-stream")?)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Codex API error {}: {}", status, body));
        }

        let is_sse = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false);

        if !is_sse {
            let body = response.text().await?;
            let out = extract_response_text_from_body(&body)?;
            if !out.is_empty() {
                on_delta(out.clone());
            }
            return Ok(out);
        }

        let mut full = String::new();
        consume_sse(response, |data| {
            if data.trim() == "[DONE]" {
                return Ok(false);
            }

            let chunk: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => return Ok(true),
            };

            if let Some(delta) = extract_stream_delta(&chunk)
                && !delta.is_empty()
            {
                full.push_str(&delta);
                on_delta(delta);
            }

            Ok(true)
        })
        .await?;

        Ok(full)
    }
}

pub fn start_browser_auth() -> Result<BrowserAuth> {
    let (listener, redirect_uri) = bind_redirect_listener()?;
    let verifier = random_urlsafe(64);
    let challenge = pkce_challenge(&verifier);
    let state = random_urlsafe(32);
    let mut url = Url::parse(AUTH_URL)?;
    url.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", "openid profile email offline_access")
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("originator", ORIGINATOR);

    let authorization_url = url.to_string();
    if webbrowser::open(&authorization_url).is_err() {
        return Err(anyhow!("failed to open browser"));
    }

    Ok(BrowserAuth {
        authorization_url,
        state,
        verifier,
        redirect_uri,
        listener,
    })
}

pub async fn wait_for_browser_code(auth: BrowserAuth) -> Result<BrowserCode> {
    tokio::task::spawn_blocking(move || read_browser_callback(auth.listener))
        .await
        .context("browser callback task failed")?
}

pub async fn exchange_browser_code(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<CodexCredentials> {
    let client = provider_http_client();
    let response = client
        .post(TOKEN_URL)
        .header(ACCEPT, "application/json")
        .json(&json!({
            "client_id": CLIENT_ID,
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": redirect_uri,
            "code_verifier": verifier,
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("Codex token exchange failed {}: {}", status, body));
    }

    credentials_from_token_response(response.json().await?)
}

pub async fn authorize_device() -> Result<DeviceAuth> {
    let client = provider_http_client();
    let response = client
        .post(DEVICE_CODE_URL)
        .header(ACCEPT, "application/json")
        .json(&json!({
            "client_id": CLIENT_ID,
            "scope": "openid profile email offline_access",
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "Codex device code request failed {}: {}",
            status,
            body
        ));
    }

    let data: DeviceCodeResponse = response.json().await?;
    Ok(DeviceAuth {
        device_code: data.device_code,
        user_code: data.user_code,
        verification_uri: data
            .verification_uri
            .unwrap_or_else(|| "https://auth.openai.com/activate".to_string()),
        verification_uri_complete: data.verification_uri_complete,
        interval: data.interval.unwrap_or(5).max(1),
        expires_in: data.expires_in,
    })
}

pub async fn poll_device_authorization(device_code: &str) -> Result<DevicePollResult> {
    let client = provider_http_client();
    let response = client
        .post(DEVICE_TOKEN_URL)
        .header(ACCEPT, "application/json")
        .json(&json!({
            "client_id": CLIENT_ID,
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Ok(DevicePollResult::Failed(format!(
            "device token request failed {}: {}",
            status, body
        )));
    }

    let data: DeviceTokenResponse = response.json().await?;
    if let Some(access_token) = data.access_token {
        return Ok(DevicePollResult::Success(credentials_from_device_response(
            access_token,
            data.refresh_token,
            data.expires_in,
            data.id_token,
        )?));
    }

    match data.error.as_deref() {
        Some("authorization_pending") | Some("slow_down") => Ok(DevicePollResult::Pending),
        Some(error) => Ok(DevicePollResult::Failed(error.to_string())),
        None => Ok(DevicePollResult::Pending),
    }
}

pub fn serialize_credentials(credentials: &CodexCredentials) -> Result<String> {
    serde_json::to_string(credentials).context("failed to serialize Codex credentials")
}

pub fn parse_credentials(value: &str) -> Result<CodexCredentials> {
    serde_json::from_str(value).context("failed to parse Codex credentials")
}

fn persist_credentials(secret_store: &SecretStore, credentials: &CodexCredentials) -> Result<()> {
    let serialized = serialize_credentials(credentials)?;
    secret_store
        .save_key(ProviderKind::Codex, &serialized)
        .context("failed to persist refreshed Codex credentials")?;
    Ok(())
}

fn bind_redirect_listener() -> Result<(TcpListener, String)> {
    for port in REDIRECT_PORTS {
        let address = format!("127.0.0.1:{port}");
        if let Ok(listener) = TcpListener::bind(&address) {
            listener.set_nonblocking(true)?;
            let redirect_uri = format!("http://localhost:{port}/auth/callback");
            return Ok((listener, redirect_uri));
        }
    }

    Err(anyhow!("no Codex OAuth callback port is available"))
}

fn read_browser_callback(listener: TcpListener) -> Result<BrowserCode> {
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() > deadline {
                    return Err(anyhow!("browser OAuth callback timed out"));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => return Err(err.into()),
        }
    };
    let mut buffer = [0_u8; 4096];
    let len = std::io::Read::read(&mut stream, &mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..len]);
    let first_line = request.lines().next().unwrap_or_default();
    let target = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("invalid OAuth callback request"))?;
    let url = Url::parse(&format!("http://localhost{target}"))?;
    let code =
        query_value(&url, "code").ok_or_else(|| anyhow!("OAuth callback is missing code"))?;
    let state =
        query_value(&url, "state").ok_or_else(|| anyhow!("OAuth callback is missing state"))?;

    let response = "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\n\r\n<html><body>GhostPWN Codex login complete. You can close this window.</body></html>";
    stream.write_all(response.as_bytes())?;

    Ok(BrowserCode { code, state })
}

fn query_value(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.to_string())
}

fn random_urlsafe(bytes: usize) -> String {
    let mut data = vec![0_u8; bytes];
    rand::rng().fill_bytes(&mut data);
    URL_SAFE_NO_PAD.encode(data)
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

async fn refresh_credentials(
    client: &Client,
    credentials: &CodexCredentials,
) -> Result<CodexCredentials> {
    let response = client
        .post(TOKEN_URL)
        .header(ACCEPT, "application/json")
        .json(&json!({
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": credentials.refresh_token,
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!("Codex token refresh failed {}: {}", status, body));
    }

    let data: TokenResponse = response.json().await?;
    let mut refreshed = credentials_from_token_response(data)?;
    if refreshed.refresh_token.is_empty() {
        refreshed.refresh_token = credentials.refresh_token.clone();
    }
    if refreshed.account_id.is_none() {
        refreshed.account_id = credentials.account_id.clone();
    }
    Ok(refreshed)
}

fn credentials_from_token_response(data: TokenResponse) -> Result<CodexCredentials> {
    let refresh_token = data
        .refresh_token
        .ok_or_else(|| anyhow!("Codex token response is missing refresh token"))?;
    Ok(CodexCredentials {
        refresh_token,
        access_token: data.access_token,
        expires_at: unix_now()?.saturating_add(data.expires_in.unwrap_or(3600)),
        account_id: data.id_token.as_deref().and_then(extract_account_id),
    })
}

fn credentials_from_device_response(
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    id_token: Option<String>,
) -> Result<CodexCredentials> {
    Ok(CodexCredentials {
        refresh_token: refresh_token
            .ok_or_else(|| anyhow!("Codex device response is missing refresh token"))?,
        access_token,
        expires_at: unix_now()?.saturating_add(expires_in.unwrap_or(3600)),
        account_id: id_token.as_deref().and_then(extract_account_id),
    })
}

fn extract_account_id(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let value: Value = serde_json::from_slice(&decoded).ok()?;
    for key in [
        "https://api.openai.com/auth/account_id",
        "account_id",
        "org_id",
    ] {
        if let Some(account_id) = value.get(key).and_then(|v| v.as_str()) {
            return Some(account_id.to_string());
        }
    }
    None
}

fn codex_headers(credentials: &CodexCredentials, accept: &'static str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    headers.insert(ACCEPT, HeaderValue::from_static(accept));
    headers.insert("originator", HeaderValue::from_static(ORIGINATOR));
    headers.insert("session_id", HeaderValue::from_str(&random_urlsafe(18))?);
    headers.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", credentials.access_token))?,
    );
    if let Some(account_id) = credentials.account_id.as_ref() {
        headers.insert("ChatGPT-Account-Id", HeaderValue::from_str(account_id)?);
    }
    Ok(headers)
}

fn parse_codex_models(body: &Value) -> Vec<String> {
    let mut models = body
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|model| {
            model
                .get("visibility")
                .and_then(Value::as_str)
                .is_none_or(|visibility| visibility == "list")
        })
        .filter_map(|model| model.get("slug").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    dedup_preserve_order(&mut models);
    models
}

fn codex_fallback_models() -> Vec<String> {
    CODEX_FALLBACK_MODELS
        .iter()
        .map(|model| model.to_string())
        .collect()
}

fn dedup_preserve_order(values: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn map_messages(history: &[ConversationMessage]) -> Vec<Value> {
    history
        .iter()
        .map(|message| {
            let role = match message.role {
                MessageRole::User | MessageRole::Tool => "user",
                MessageRole::Assistant => "assistant",
            };
            let content_type = if message.role == MessageRole::Assistant {
                "output_text"
            } else {
                "input_text"
            };
            let text = if message.role == MessageRole::Tool {
                format!("[tool] {}", message.content)
            } else {
                message.content.clone()
            };
            json!({
                "type": "message",
                "role": role,
                "content": [{ "type": content_type, "text": text }],
            })
        })
        .collect()
}

fn extract_stream_delta(chunk: &Value) -> Option<String> {
    let event_type = chunk
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if matches!(
        event_type,
        "response.output_text.delta" | "response.text.delta"
    ) {
        return chunk
            .get("delta")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
    }

    chunk
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("delta"))
        .and_then(|v| v.get("content"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

fn extract_response_text(body: &Value) -> Option<String> {
    if let Some(text) = body.get("output_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    let mut out = String::new();
    for item in body.get("output")?.as_array()? {
        let Some(contents) = item.get("content").and_then(|v| v.as_array()) else {
            continue;
        };
        for content in contents {
            if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
                out.push_str(text);
            }
        }
    }

    if out.is_empty() { None } else { Some(out) }
}

fn extract_response_text_from_body(body: &str) -> Result<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    if trimmed.starts_with("event:") || trimmed.starts_with("data:") || trimmed.contains("\ndata:")
    {
        return Ok(extract_text_from_sse_body(trimmed));
    }

    let value: Value = serde_json::from_str(trimmed).with_context(|| {
        format!(
            "failed to decode Codex response body: {}",
            preview_body(body)
        )
    })?;
    Ok(extract_response_text(&value).unwrap_or_default())
}

fn extract_text_from_sse_body(body: &str) -> String {
    let mut out = String::new();
    let mut final_text = String::new();
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(delta) = extract_stream_delta(&value) {
            out.push_str(&delta);
        } else if let Some(text) = extract_response_text(&value) {
            final_text.push_str(&text);
        }
    }
    if out.is_empty() { final_text } else { out }
}

fn preview_body(body: &str) -> String {
    const MAX_PREVIEW: usize = 500;
    let mut preview = body.trim().chars().take(MAX_PREVIEW).collect::<String>();
    if body.trim().chars().count() > MAX_PREVIEW {
        preview.push_str("...");
    }
    preview
}

fn unix_now() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| anyhow!("system clock is before UNIX_EPOCH: {}", err))?
        .as_secs())
}

#[cfg(test)]
#[path = "../tests/providers_codex.rs"]
mod tests;
