use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use reqwest::StatusCode;
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
            .map_err(|err| anyhow!("system clock is before UNIX_EPOCH: {}", err))?
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

    async fn stream_complete_responses(
        &self,
        token: &str,
        api_base: &str,
        system: &str,
        messages: &[ConversationMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String> {
        let payload = json!({
            "model": self.model,
            "temperature": 0.2,
            "stream": true,
            "input": map_messages(system, messages),
        });

        let response = self
            .client
            .post(format!("{}/responses", api_base))
            .header("User-Agent", USER_AGENT_VALUE)
            .header("Editor-Version", EDITOR_VERSION)
            .header("Editor-Plugin-Version", PLUGIN_VERSION)
            .header("Copilot-Integration-Id", INTEGRATION_ID)
            .bearer_auth(token)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Copilot Responses API error {}: {}", status, body));
        }

        let is_sse = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false);

        if !is_sse {
            let body: Value = response.json().await?;
            let out = extract_responses_content(&body).unwrap_or_default();
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

            if chunk
                .get("type")
                .and_then(|v| v.as_str())
                .is_some_and(|t| t == "response.output_text.delta")
                && let Some(delta) = chunk.get("delta").and_then(|v| v.as_str())
                && !delta.is_empty()
            {
                full.push_str(delta);
                on_delta(delta.to_string());
            }

            Ok(true)
        })
        .await?;

        Ok(full)
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    fn display_name(&self) -> String {
        format!("copilot / {}", self.model)
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let (token, api_base) = self.ensure_token().await?;

        let response = self
            .client
            .get(format!("{}/models", api_base))
            .header("User-Agent", USER_AGENT_VALUE)
            .header("Editor-Version", EDITOR_VERSION)
            .header("Editor-Plugin-Version", PLUGIN_VERSION)
            .header("Copilot-Integration-Id", INTEGRATION_ID)
            .bearer_auth(&token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to fetch models {}: {}", status, body));
        }

        let body: Value = response.json().await?;
        Ok(parse_models_for_chat_completions(&body))
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

            if status == StatusCode::BAD_REQUEST && is_unsupported_chat_model_error(&body) {
                return self
                    .stream_complete_responses(&token, &api_base, system, messages, on_delta)
                    .await;
            }

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

fn extract_responses_content(body: &Value) -> Option<String> {
    if let Some(text) = body.get("output_text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }

    let mut out = String::new();
    let output_items = body.get("output")?.as_array()?;
    for item in output_items {
        let contents = item.get("content").and_then(|v| v.as_array());
        let Some(contents) = contents else {
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

fn parse_models_for_chat_completions(body: &Value) -> Vec<String> {
    let Some(models) = extract_model_entries(body) else {
        return Vec::new();
    };

    let mut out = Vec::<String>::new();

    for model in models {
        let Some(id) = model_id(model) else {
            continue;
        };

        out.push(id);
    }

    dedup_preserve_order(&mut out);
    out
}

fn extract_model_entries(body: &Value) -> Option<&[Value]> {
    if let Some(arr) = body.get("models").and_then(|v| v.as_array()) {
        return Some(arr);
    }
    if let Some(arr) = body.get("data").and_then(|v| v.as_array()) {
        return Some(arr);
    }
    body.as_array().map(Vec::as_slice)
}

fn model_id(model: &Value) -> Option<String> {
    for key in ["id", "name", "model", "model_id"] {
        let Some(raw) = model.get(key).and_then(|v| v.as_str()) else {
            continue;
        };

        if let Some(normalized) = normalize_model_id(raw) {
            return Some(normalized);
        }
    }

    None
}

fn normalize_model_id(raw: &str) -> Option<String> {
    let mut id = raw.trim();
    if id.is_empty() {
        return None;
    }

    if let Some(stripped) = id.strip_prefix("models/") {
        id = stripped;
    }

    if let Some((_, suffix)) = id.rsplit_once("/models/") {
        id = suffix;
    }

    let lower = id.to_ascii_lowercase();
    if lower == "routers"
        || lower.ends_with("/routers")
        || lower.starts_with("accounts/")
        || lower.contains("/routers/")
    {
        return None;
    }

    if id.contains('/') {
        return None;
    }

    if let Some(canonical) = canonical_copilot_model_id(id) {
        return Some(canonical.to_string());
    }

    Some(id.to_string())
}

fn canonical_copilot_model_id(value: &str) -> Option<&'static str> {
    let key = model_alias_key(value);
    match key.as_str() {
        "gpt41" => Some("gpt-4.1"),
        "gpt4o" => Some("gpt-4o"),
        "gpt5mini" => Some("gpt-5-mini"),
        "gpt51" => Some("gpt-5.1"),
        "gpt52" => Some("gpt-5.2"),
        "gpt52codex" => Some("gpt-5.2-codex"),
        "gpt53codex" => Some("gpt-5.3-codex"),
        "gpt54" => Some("gpt-5.4"),
        "gpt54mini" => Some("gpt-5.4-mini"),
        "claudehaiku45" => Some("claude-haiku-4.5"),
        "claudeopus45" => Some("claude-opus-4.5"),
        "claudeopus46" => Some("claude-opus-4.6"),
        "claudeopus46fastmodepreview" => Some("claude-opus-4.6-fast-mode-preview"),
        "claudesonnet4" => Some("claude-sonnet-4"),
        "claudesonnet45" => Some("claude-sonnet-4.5"),
        "claudesonnet46" => Some("claude-sonnet-4.6"),
        "gemini25pro" => Some("gemini-2.5-pro"),
        "gemini3flash" => Some("gemini-3-flash"),
        "gemini31pro" => Some("gemini-3.1-pro"),
        "grokcodefast1" => Some("grok-code-fast-1"),
        "raptormini" => Some("raptor-mini"),
        "goldeneye" => Some("goldeneye"),
        _ => None,
    }
}

fn model_alias_key(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn dedup_preserve_order(values: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::<String>::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn is_unsupported_chat_model_error(body: &str) -> bool {
    if body.contains("unsupported_api_for_model") || body.contains("/chat/completions") {
        return true;
    }

    let Ok(json) = serde_json::from_str::<Value>(body) else {
        return false;
    };

    let code = json
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(|v| v.as_str());
    let message = json
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    code == Some("unsupported_api_for_model") || message.contains("/chat/completions")
}

#[cfg(test)]
#[path = "../tests/providers_copilot.rs"]
mod tests;
