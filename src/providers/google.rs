use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use reqwest::header::CONTENT_TYPE;
use serde_json::{Value, json};

use crate::models::{ConversationMessage, MessageRole};
use crate::providers::Provider;
use crate::providers::sse::consume_sse;

pub struct GoogleProvider {
    api_key: String,
    model: String,
    client: Client,
}

impl GoogleProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Provider for GoogleProvider {
    fn display_name(&self) -> String {
        format!("google / {}", self.model)
    }

    async fn stream_complete(
        &self,
        system: &str,
        messages: &[ConversationMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String> {
        let payload = json!({
            "systemInstruction": {
                "role": "system",
                "parts": [{ "text": system }]
            },
            "contents": map_messages(messages),
            "generationConfig": {
                "temperature": 0.2,
                "maxOutputTokens": 2048
            }
        });

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.model, self.api_key
        );

        let response = self.client.post(url).json(&payload).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Google API error {}: {}", status, body));
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
                .get("candidates")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|cand| cand.get("content"))
                .and_then(|content| content.get("parts"))
                .and_then(|parts| parts.as_array())
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
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
            let chunk: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => return Ok(true),
            };

            if let Some(text) = chunk
                .get("candidates")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|cand| cand.get("content"))
                .and_then(|content| content.get("parts"))
                .and_then(|parts| parts.as_array())
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
                        .collect::<Vec<&str>>()
                        .join("")
                })
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

fn map_messages(history: &[ConversationMessage]) -> Vec<Value> {
    history
        .iter()
        .map(|m| match m.role {
            MessageRole::User => json!({
                "role": "user",
                "parts": [{ "text": m.content }],
            }),
            MessageRole::Assistant => json!({
                "role": "model",
                "parts": [{ "text": m.content }],
            }),
            MessageRole::Tool => json!({
                "role": "user",
                "parts": [{ "text": format!("[tool] {}", m.content) }],
            }),
        })
        .collect()
}
