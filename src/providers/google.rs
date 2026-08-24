use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use reqwest::header::CONTENT_TYPE;
use serde_json::{Value, json};

use crate::models::{ConversationMessage, MessageRole};
use crate::providers::sse::consume_sse;
use crate::providers::{Provider, provider_http_client};

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
            client: provider_http_client(),
        }
    }
}

#[async_trait]
impl Provider for GoogleProvider {
    fn display_name(&self) -> String {
        format!("google / {}", self.model)
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let mut models = Vec::new();
        let mut page_token = None::<String>;
        let mut seen_page_tokens = std::collections::HashSet::new();

        loop {
            let mut request = self
                .client
                .get("https://generativelanguage.googleapis.com/v1beta/models")
                .header("x-goog-api-key", &self.api_key)
                .query(&[("pageSize", "1000")]);
            if let Some(token) = page_token.as_deref() {
                request = request.query(&[("pageToken", token)]);
            }
            let response = request.send().await?;
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("Google models API error {}: {}", status, body));
            }

            let body: Value = response.json().await?;
            models.extend(parse_gemini_models(&body));
            let Some(next) = body
                .get("nextPageToken")
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())
            else {
                break;
            };
            if !seen_page_tokens.insert(next.to_string()) {
                return Err(anyhow!("Google models API returned a repeated page token"));
            }
            page_token = Some(next.to_string());
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
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
            self.model
        );

        let response = self
            .client
            .post(url)
            .header("x-goog-api-key", &self.api_key)
            .json(&payload)
            .send()
            .await?;
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

fn parse_gemini_models(body: &Value) -> Vec<String> {
    body.get("models")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|model| {
                    model
                        .get("supportedGenerationMethods")
                        .and_then(Value::as_array)
                        .is_none_or(|methods| {
                            methods
                                .iter()
                                .any(|method| method.as_str() == Some("generateContent"))
                        })
                })
                .filter_map(|model| model.get("name").and_then(|v| v.as_str()))
                .filter_map(|name| name.strip_prefix("models/"))
                .filter(|id| id.contains("gemini"))
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
#[path = "../tests/providers_google.rs"]
mod tests;
