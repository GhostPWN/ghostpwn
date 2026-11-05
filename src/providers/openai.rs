use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{Value, json};

use crate::models::{Completion, ConversationMessage};
use crate::providers::native::{read_responses, response_messages};
use crate::providers::{Provider, provider_http_client, request_error};

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
    ) -> Result<Completion> {
        let payload = json!({
            "model": self.model,
            "stream": true,
            "store": false,
            "instructions": system,
            "input": map_messages(messages),
            "tools": crate::tools::schema::response_tools(),
            "include": ["reasoning.encrypted_content"],
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
            return Err(request_error(
                "OpenAI Responses API",
                status,
                &body,
                messages,
            ));
        }

        read_responses(response, on_delta).await
    }
}

fn map_messages(history: &[ConversationMessage]) -> Vec<Value> {
    response_messages(history)
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
