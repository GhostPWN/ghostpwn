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
            "temperature": 0.2,
            "stream": true,
            "messages": map_messages(system, messages),
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("OpenAI API error {}: {}", status, body));
        }

        let is_sse = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false);

        if !is_sse {
            let body: Value = response.json().await?;
            let content = body
                .get("choices")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.get("message"))
                .and_then(|v| v.get("content"));

            let out = extract_content_text(content).unwrap_or_default();
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

            let delta = chunk
                .get("choices")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.get("delta"))
                .and_then(|v| v.get("content"));

            if let Some(text) = extract_content_text(delta)
                && !text.is_empty()
            {
                full.push_str(&text);
                on_delta(text);
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

fn extract_content_text(content: Option<&Value>) -> Option<String> {
    let value = content?;
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }

    if let Some(parts) = value.as_array() {
        let mut buf = String::new();
        for part in parts {
            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                buf.push_str(text);
            }
        }
        if !buf.is_empty() {
            return Some(buf);
        }
    }

    None
}

fn parse_chat_models(body: &Value) -> Vec<String> {
    let mut out = body
        .get("data")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("id").and_then(|v| v.as_str()))
                .filter(|id| is_chat_model_id(id))
                .map(ToString::to_string)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    out.sort();
    out.dedup();
    out
}

fn is_chat_model_id(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    id.starts_with("gpt-")
        || id.starts_with("chatgpt-")
        || id.starts_with("o1")
        || id.starts_with("o3")
        || id.starts_with("o4")
}
