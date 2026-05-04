use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::models::{ConversationMessage, MessageRole};
use crate::providers::Provider;
use crate::providers::sse::consume_sse;

const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";

const USER_AGENT_VALUE: &str = "GitHubCopilotChat/0.26.7";
const EDITOR_VERSION: &str = "vscode/1.99.3";
const PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
const INTEGRATION_ID: &str = "vscode-chat";

// ── Device-code OAuth flow ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CopilotTokenResponse {
    token: String,
    expires_at: u64,
    endpoints: CopilotEndpoints,
}

#[derive(Debug, Deserialize)]
struct CopilotEndpoints {
    api: String,
}

pub struct DeviceAuth {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

pub enum PollResult {
    Success(String),
    Pending,
    Failed,
}

pub async fn authorize() -> Result<DeviceAuth> {
    let client = Client::new();
    let resp = client
        .post(DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT_VALUE)
        .json(&json!({
            "client_id": CLIENT_ID,
            "scope": "read:user",
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Device code request failed {}: {}", status, body));
    }

    let data: DeviceCodeResponse = resp.json().await?;
    Ok(DeviceAuth {
        device_code: data.device_code,
        user_code: data.user_code,
        verification_uri: data.verification_uri,
        interval: data.interval.unwrap_or(5),
        expires_in: data.expires_in,
    })
}

pub async fn poll_authorization(device_code: &str) -> Result<PollResult> {
    let client = Client::new();
    let resp = client
        .post(ACCESS_TOKEN_URL)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT_VALUE)
        .json(&json!({
            "client_id": CLIENT_ID,
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Ok(PollResult::Failed);
    }

    let data: OAuthTokenResponse = resp.json().await?;

    if let Some(token) = data.access_token {
        return Ok(PollResult::Success(token));
    }

    if data.error.as_deref() == Some("authorization_pending") {
        return Ok(PollResult::Pending);
    }

    if data.error.is_some() {
        return Ok(PollResult::Failed);
    }

    Ok(PollResult::Pending)
}

// ── Copilot token exchange ──────────────────────────────────────────

async fn fetch_copilot_token(client: &Client, refresh: &str) -> Result<CopilotTokenResponse> {
    let resp = client
        .get(COPILOT_TOKEN_URL)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT_VALUE)
        .header("Editor-Version", EDITOR_VERSION)
        .header("Editor-Plugin-Version", PLUGIN_VERSION)
        .header("Copilot-Integration-Id", INTEGRATION_ID)
        .bearer_auth(refresh)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Copilot token refresh failed {}: {}", status, body));
    }

    Ok(resp.json().await?)
}

// ── Provider ────────────────────────────────────────────────────────

struct CachedToken {
    access_token: String,
    api_base: String,
    expires_at: u64,
}

pub struct CopilotProvider {
    refresh_token: String,
    model: String,
    client: Client,
    token_cache: Mutex<Option<CachedToken>>,
}

impl CopilotProvider {
    pub fn new(refresh_token: String, model: String) -> Self {
        Self {
            refresh_token,
            model,
            client: Client::new(),
            token_cache: Mutex::new(None),
        }
    }

    async fn ensure_token(&self) -> Result<(String, String)> {
        let mut cache = self.token_cache.lock().await;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if let Some(cached) = cache.as_ref()
            && now < cached.expires_at.saturating_sub(60)
        {
            return Ok((cached.access_token.clone(), cached.api_base.clone()));
        }

        let data = fetch_copilot_token(&self.client, &self.refresh_token).await?;
        let token = data.token.clone();
        let api_base = data.endpoints.api.clone();

        *cache = Some(CachedToken {
            access_token: data.token,
            api_base: data.endpoints.api,
            expires_at: data.expires_at,
        });

        Ok((token, api_base))
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    fn display_name(&self) -> String {
        format!("copilot / {}", self.model)
    }

    async fn stream_complete(
        &self,
        system: &str,
        messages: &[ConversationMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String> {
        let (token, api_base) = self.ensure_token().await?;

        let payload = json!({
            "model": self.model,
            "temperature": 0.2,
            "stream": true,
            "messages": map_messages(system, messages),
        });

        let response = self
            .client
            .post(format!("{}/chat/completions", api_base))
            .header("User-Agent", USER_AGENT_VALUE)
            .header("Editor-Version", EDITOR_VERSION)
            .header("Editor-Plugin-Version", PLUGIN_VERSION)
            .header("Copilot-Integration-Id", INTEGRATION_ID)
            .bearer_auth(&token)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Copilot API error {}: {}", status, body));
        }

        let is_sse = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false);

        if !is_sse {
            let body: Value = response.json().await?;
            let out = extract_content(&body).unwrap_or_default();
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

            if let Some(text) = chunk
                .get("choices")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.get("delta"))
                .and_then(|v| v.get("content"))
                .and_then(|v| v.as_str())
                && !text.is_empty()
            {
                full.push_str(text);
                on_delta(text.to_string());
            }

            Ok(true)
        })
        .await?;

        Ok(full)
    }
}

fn map_messages(system: &str, history: &[ConversationMessage]) -> Vec<Value> {
    let mut out = Vec::with_capacity(history.len() + 1);
    out.push(json!({ "role": "system", "content": system }));

    for m in history {
        match m.role {
            MessageRole::User => out.push(json!({ "role": "user", "content": m.content })),
            MessageRole::Assistant => {
                out.push(json!({ "role": "assistant", "content": m.content }))
            }
            MessageRole::Tool => out.push(json!({
                "role": "user",
                "content": format!("[tool] {}", m.content),
            })),
        }
    }

    out
}

fn extract_content(body: &Value) -> Option<String> {
    body.get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?
        .as_str()
        .map(|s| s.to_string())
}
