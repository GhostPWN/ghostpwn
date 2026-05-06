use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::models::{ConversationMessage, MessageRole};
use crate::providers::Provider;
use crate::providers::sse::consume_sse;

pub struct OllamaProvider {
    model: String,
    client: Client,
}

impl OllamaProvider {
    pub fn new(model: String) -> Self {
        Self {
            model,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    fn display_name(&self) -> String {
        format!("ollama / {}", self.model)
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get("http://localhost:11434/api/tags")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!("Failed to list models"));
        }

        let body: Value = response.json().await?;
        let models = body
            .get("models")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }

    async fn stream_complete(
        &self,
        system: &str,
        messages: &[ConversationMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String> {
        let payload = serde_json::json!({
            "model": self.model,
            "stream": true,
            "messages": map_messages(system, messages),
        });

        let response = self
            .client
            .post("http://localhost:11434/v1/chat/completions")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama API error {}: {}", status, body));
        }

        let mut full = String::new();
        consume_sse(response, |data| {
            if data.trim().is_empty() {
                return Ok(true);
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

            if let Some(text) = delta.and_then(|v| v.as_str())
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
    out.push(serde_json::json!({ "role": "system", "content": system }));

    for m in history {
        match m.role {
            MessageRole::User => {
                out.push(serde_json::json!({ "role": "user", "content": m.content }))
            }
            MessageRole::Assistant => {
                out.push(serde_json::json!({ "role": "assistant", "content": m.content }))
            }
            MessageRole::Tool => out.push(serde_json::json!({
                "role": "user",
                "content": format!("[tool] {}", m.content),
            })),
        }
    }

    out
}
