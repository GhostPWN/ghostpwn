use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use reqwest::header::CONTENT_TYPE;
use serde_json::{Value, json};

use crate::models::{ConversationMessage, MessageRole};
use crate::providers::sse::consume_sse;
use crate::providers::{Provider, provider_http_client};

pub struct OpenAiProvider {
    api_key: String,
    model: String,
    client: Client,
}

impl OpenAiProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: provider_http_client(),
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn display_name(&self) -> String {
        format!("openai / {}", self.model)
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get("https://api.openai.com/v1/models")
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI models API error {}: {}", status, body));
        }

        let body: Value = response.json().await?;
        Ok(parse_chat_models(&body))
    }

    async fn stream_complete(
        &self,
        system: &str,
        messages: &[ConversationMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String> {
        let payload = json!({
            "model": self.model,
            "stream": true,
            "store": false,
            "instructions": system,
            "input": map_messages(messages),
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/responses")
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI Responses API error {}: {}", status, body));
        }

        let is_sse = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false);

        if !is_sse {
            let body: Value = response.json().await?;
            let out = extract_response_text(&body).unwrap_or_default();
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

            if let Some(error) = extract_stream_error(&chunk) {
                return Err(anyhow!("OpenAI stream error: {}", error));
            }

            if chunk.get("type").and_then(Value::as_str) == Some("response.output_text.delta")
                && let Some(text) = chunk.get("delta").and_then(Value::as_str)
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

fn extract_stream_error(chunk: &Value) -> Option<String> {
    let event_type = chunk.get("type").and_then(Value::as_str);
    if !matches!(event_type, Some("error" | "response.failed")) {
        return None;
    }

    let error = chunk.get("error").or_else(|| {
        chunk
            .get("response")
            .and_then(|response| response.get("error"))
    })?;
    error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .map(ToString::to_string)
        .or_else(|| Some(error.to_string()))
}

fn map_messages(history: &[ConversationMessage]) -> Vec<Value> {
    history
        .iter()
        .map(|message| match message.role {
            MessageRole::User => json!({ "role": "user", "content": message.content }),
            MessageRole::Assistant => {
                json!({ "role": "assistant", "content": message.content })
            }
            MessageRole::Tool => json!({
                "role": "user",
                "content": format!("[tool] {}", message.content),
            }),
        })
        .collect()
}

fn extract_response_text(body: &Value) -> Option<String> {
    if let Some(text) = body.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }

    let mut out = String::new();
    for item in body.get("output")?.as_array()? {
        if let Some(content) = item.get("content").and_then(Value::as_array) {
            for part in content {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    out.push_str(text);
                }
            }
        }
    }

    (!out.is_empty()).then_some(out)
}

fn parse_chat_models(body: &Value) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    body.get("data")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter(|item| item.get("shutdown_date").is_none_or(Value::is_null))
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .filter(|id| is_chat_model_id(id))
        .map(ToString::to_string)
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

fn is_chat_model_id(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let is_text_family = id.starts_with("gpt-")
        || id.starts_with("o1")
        || id.starts_with("o3")
        || id.starts_with("o4");
    let is_specialized = [
        "gpt-image",
        "gpt-realtime",
        "gpt-audio",
        "gpt-live",
        "gpt-transcribe",
        "gpt-4o-transcribe",
        "gpt-4o-mini-transcribe",
        "gpt-4o-mini-tts",
    ]
    .iter()
    .any(|prefix| id.starts_with(prefix));

    is_text_family && !is_specialized
}

#[cfg(test)]
#[path = "../tests/providers_openai.rs"]
mod tests;
