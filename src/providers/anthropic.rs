use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use reqwest::header::CONTENT_TYPE;
use serde_json::{Value, json};

use crate::models::{ConversationMessage, ConversationPart, MessageRole};
use crate::providers::sse::{consume_sse, extract_error_message};
use crate::providers::{Provider, image_base64, message_text, provider_http_client, request_error};

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
            client: provider_http_client(),
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn display_name(&self) -> String {
        format!("anthropic / {}", self.model)
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let mut models = Vec::new();
        let mut after_id = None::<String>;
        let mut seen_after_ids = std::collections::HashSet::new();

        loop {
            let mut request = self
                .client
                .get("https://api.anthropic.com/v1/models")
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .query(&[("limit", "1000")]);
            if let Some(cursor) = after_id.as_deref() {
                request = request.query(&[("after_id", cursor)]);
            }
            let response = request.send().await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("Anthropic models API error {}: {}", status, body));
            }

            let body: Value = response.json().await?;
            models.extend(parse_claude_models(&body));
            if body.get("has_more").and_then(Value::as_bool) != Some(true) {
                break;
            }

            let next = body
                .get("last_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Anthropic models API returned an invalid cursor"))?;
            if !seen_after_ids.insert(next.to_string()) {
                return Err(anyhow!("Anthropic models API returned a repeated cursor"));
            }
            after_id = Some(next.to_string());
        }

        dedup_preserve_order(&mut models);
        Ok(models)
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
            return Err(request_error("Anthropic API", status, &body, messages));
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

            if let Some(error) = extract_error_message(&event) {
                return Err(anyhow!("Anthropic stream error: {}", error));
            }

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
            MessageRole::User if m.has_images() => json!({
                "role": "user",
                "content": m.content.iter().map(|part| match part {
                    ConversationPart::Text(text) => json!({ "type": "text", "text": text }),
                    ConversationPart::Image(image) => json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": image.media_type.as_str(),
                            "data": image_base64(image),
                        }
                    }),
                }).collect::<Vec<_>>(),
            }),
            MessageRole::User => json!({ "role": "user", "content": message_text(m) }),
            MessageRole::Assistant => json!({
                "role": "assistant",
                "content": message_text(m),
            }),
            MessageRole::Tool => json!({
                "role": "user",
                "content": format!("[tool] {}", message_text(m)),
            }),
        })
        .collect()
}

fn parse_claude_models(body: &Value) -> Vec<String> {
    body.get("data")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("id").and_then(|v| v.as_str()))
                .filter(|id| id.starts_with("claude-"))
                .map(ToString::to_string)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default()
}

fn dedup_preserve_order(values: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

#[cfg(test)]
#[path = "../tests/providers_anthropic.rs"]
mod tests;
