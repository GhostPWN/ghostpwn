mod anthropic;
pub mod codex;
pub mod copilot;
mod google;
mod ollama;
mod openai;
mod sse;

use std::collections::HashSet;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use serde_json::{Value, json};

use crate::config::{ProviderKeys, ProviderKind};
use crate::models::{ConversationMessage, ConversationPart, ImageAttachment, MessageRole};
use crate::secrets::SecretStore;

/// Upper bound on model output tokens for providers that require an explicit cap.
///
/// The agent contract requires the model to answer with a single JSON envelope that can carry full
/// file bodies (`writeFile`, `applyPatch`). A small cap truncates that JSON mid-string, which makes
/// the envelope unparsable and silently drops the tool calls. Keep this generous so edits survive.
pub(crate) const MAX_OUTPUT_TOKENS: u32 = 8192;

pub use anthropic::AnthropicProvider;
pub use codex::CodexProvider;
pub use copilot::CopilotProvider;
pub use google::GoogleProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;

fn provider_http_client() -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(90))
        .build()
        .unwrap_or_default()
}

fn image_base64(image: &ImageAttachment) -> String {
    base64::engine::general_purpose::STANDARD.encode(&image.data)
}

fn image_data_url(image: &ImageAttachment) -> String {
    format!(
        "data:{};base64,{}",
        image.media_type.as_str(),
        image_base64(image)
    )
}

fn message_text(message: &ConversationMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|part| match part {
            ConversationPart::Text(text) => Some(text.as_str()),
            ConversationPart::Image(_) => None,
        })
        .collect()
}

fn map_chat_messages(system: &str, history: &[ConversationMessage]) -> Vec<Value> {
    let mut out = Vec::with_capacity(history.len() + 1);
    out.push(json!({ "role": "system", "content": system }));

    for message in history {
        match message.role {
            MessageRole::User if message.has_images() => out.push(json!({
                "role": "user",
                "content": message.content.iter().map(|part| match part {
                    ConversationPart::Text(text) => json!({ "type": "text", "text": text }),
                    ConversationPart::Image(image) => json!({
                        "type": "image_url",
                        "image_url": { "url": image_data_url(image) },
                    }),
                }).collect::<Vec<_>>(),
            })),
            MessageRole::User => {
                out.push(json!({ "role": "user", "content": message_text(message) }))
            }
            MessageRole::Assistant => {
                out.push(json!({ "role": "assistant", "content": message_text(message) }))
            }
            MessageRole::Tool => out.push(json!({
                "role": "user",
                "content": format!("[tool] {}", message_text(message)),
            })),
        }
    }

    out
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

fn dedup_preserve_order(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn request_error(
    operation: &str,
    status: reqwest::StatusCode,
    body: &str,
    messages: &[ConversationMessage],
) -> anyhow::Error {
    let image_hint = messages
        .iter()
        .any(ConversationMessage::has_images)
        .then_some(
            " Request included image input; verify that the selected model supports vision.",
        );
    anyhow!(
        "{} error {}: {}{}",
        operation,
        status,
        body,
        image_hint.unwrap_or_default()
    )
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn display_name(&self) -> String;
    async fn stream_complete(
        &self,
        system: &str,
        messages: &[ConversationMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String>;
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
}

pub fn build_provider_with_secret_store(
    provider: ProviderKind,
    model: String,
    keys: &ProviderKeys,
    secret_store: SecretStore,
) -> Box<dyn Provider> {
    match (provider, keys.get(provider)) {
        (ProviderKind::Ollama, _) => Box::new(OllamaProvider::new(model)),
        (_, None) => Box::new(DisconnectedProvider { provider, model }),
        (ProviderKind::Anthropic, Some(key)) => {
            Box::new(AnthropicProvider::new(key.to_string(), model))
        }
        (ProviderKind::OpenAi, Some(key)) => Box::new(OpenAiProvider::new(key.to_string(), model)),
        (ProviderKind::Google, Some(key)) => Box::new(GoogleProvider::new(key.to_string(), model)),
        (ProviderKind::Copilot, Some(key)) => {
            Box::new(CopilotProvider::new(key.to_string(), model))
        }
        (ProviderKind::Codex, Some(key)) => Box::new(CodexProvider::new(
            key.to_string(),
            model,
            Some(secret_store),
        )),
    }
}

struct DisconnectedProvider {
    provider: ProviderKind,
    model: String,
}

#[async_trait]
impl Provider for DisconnectedProvider {
    fn display_name(&self) -> String {
        format!("{} / {} (disconnected)", self.provider.as_str(), self.model)
    }

    async fn stream_complete(
        &self,
        _system: &str,
        _messages: &[ConversationMessage],
        _on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String> {
        Err(anyhow!(
            "No API key connected for {}. open /model and press c on the {} tab",
            self.provider.as_str(),
            self.provider.as_str()
        ))
    }
}

#[cfg(test)]
#[path = "../tests/providers.rs"]
mod tests;
