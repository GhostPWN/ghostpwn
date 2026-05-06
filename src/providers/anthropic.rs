use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use reqwest::header::CONTENT_TYPE;
use serde_json::{Value, json};

use crate::models::{ConversationMessage, MessageRole};
use crate::providers::Provider;
use crate::providers::sse::consume_sse;

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn display_name(&self) -> String {
        format!("anthropic / {}", self.model)
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get("https://api.anthropic.com/v1/models")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Anthropic models API error {}: {}", status, body));
        }

        let body: Value = response.json().await?;
        Ok(parse_claude_models(&body))
    }

    async fn stream_complete(
        &self,
        system: &str,
        messages: &[ConversationMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String> {
        let payload = json!({
            "model": self.model,
            "max_tokens": 2048,
            "temperature": 0.2,
            "system": system,
            "stream": true,
            "messages": map_messages(messages),
        });

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Anthropic API error {}: {}", status, body));
        }

        let is_sse = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false);

        if !is_sse {
            let body: Value = response.json().await?;
            let text = body
                .get("content")
                .and_then(|v| v.as_array())
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
                        .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
                        .collect::<Vec<&str>>()
                        .join("")
                })
                .unwrap_or_default();

            if !text.is_empty() {
                on_delta(text.clone());
            }
            return Ok(text);
        }

        let mut full = String::new();
        consume_sse(response, |data| {
            let event: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => return Ok(true),
            };

            if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
                if event_type == "message_stop" {
                    return Ok(false);
                }

                if event_type == "content_block_delta"
                    && let Some(text) = event
                        .get("delta")
                        .and_then(|v| v.get("text"))
                        .and_then(|v| v.as_str())
                    && !text.is_empty()
                {
                    full.push_str(text);
                    on_delta(text.to_string());
                }
            }

            Ok(true)
        })
        .await?;

        Ok(full)
    }
}

fn map_messages(history: &[ConversationMessage]) -> Vec<Value> {
    history
        .iter()
        .map(|m| match m.role {
            MessageRole::User => json!({
                "role": "user",
                "content": m.content,
            }),
            MessageRole::Assistant => json!({
                "role": "assistant",
                "content": m.content,
            }),
            MessageRole::Tool => json!({
                "role": "user",
                "content": format!("[tool] {}", m.content),
            }),
        })
        .collect()
}

fn parse_claude_models(body: &Value) -> Vec<String> {
    let mut out = body
        .get("data")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("id").and_then(|v| v.as_str()))
                .filter(|id| id.starts_with("claude-"))
                .map(ToString::to_string)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    out.sort();
    out.dedup();
    out
}
