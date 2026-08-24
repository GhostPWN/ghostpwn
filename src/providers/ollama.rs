use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures_util::future::join_all;
use reqwest::Client;
use serde_json::Value;

use crate::models::{ConversationMessage, MessageRole};
use crate::providers::sse::consume_sse;
use crate::providers::{Provider, provider_http_client};

pub struct OllamaProvider {
    model: String,
    base_url: String,
    client: Client,
}

impl OllamaProvider {
    pub fn new(model: String) -> Self {
        Self {
            model,
            base_url: ollama_base_url(std::env::var("OLLAMA_HOST").ok().as_deref()),
            client: provider_http_client(),
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
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama models API error {}: {}", status, body));
        }

        let body: Value = response.json().await?;
        let discovered: Vec<String> = body
            .get("models")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let checks = discovered.into_iter().map(|model| async move {
            let response = self
                .client
                .post(format!("{}/api/show", self.base_url))
                .json(&serde_json::json!({ "model": &model }))
                .send()
                .await;

            match response {
                Ok(response) if response.status().is_success() => match response.json().await {
                    Ok(details) if !supports_completion(&details) => None,
                    Ok(_) | Err(_) => Some(model),
                },
                Ok(_) | Err(_) => Some(model),
            }
        });

        Ok(join_all(checks).await.into_iter().flatten().collect())
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
            .post(format!("{}/v1/chat/completions", self.base_url))
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

fn ollama_base_url(host: Option<&str>) -> String {
    let host = host.map(str::trim).filter(|host| !host.is_empty());
    let host = host.unwrap_or("http://localhost:11434");
    let url = if host.starts_with("http://") || host.starts_with("https://") {
        host.to_string()
    } else {
        format!("http://{host}")
    };
    url.trim_end_matches('/').to_string()
}

fn supports_completion(body: &Value) -> bool {
    body.get("capabilities")
        .and_then(Value::as_array)
        .is_none_or(|capabilities| {
            capabilities
                .iter()
                .any(|capability| capability.as_str() == Some("completion"))
        })
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

#[cfg(test)]
#[path = "../tests/providers_ollama.rs"]
mod tests;
