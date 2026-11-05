mod anthropic;
pub mod codex;
pub mod copilot;
mod google;
mod native;
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
use crate::models::{
    Completion, ConversationMessage, ConversationPart, ImageAttachment, MessageRole,
};
use crate::secrets::SecretStore;

/// Upper bound on model output tokens for providers that require an explicit cap.
///
/// Tool arguments can contain full file bodies. Leave enough output space for edits.
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
        if let Some(result) = &message.tool_result {
            out.push(json!({"role": "tool", "tool_call_id": result.call_id, "content": result.output.to_string()}));
            continue;
        }
        if !message.tool_calls.is_empty() {
            out.push(json!({"role": "assistant", "content": message_text(message), "tool_calls": message.tool_calls.iter().map(|c| json!({
                "id": c.id, "type": "function", "function": {"name": c.call.name, "arguments": c.call.arguments.to_string()}
            })).collect::<Vec<_>>()}));
            continue;
        }
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
    fn uses_native_tools(&self) -> bool {
        true
    }
    async fn stream_complete(
        &self,
        system: &str,
        messages: &[ConversationMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<Completion>;
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
    ) -> Result<Completion> {
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

fn map_legacy_chat_messages(system: &str, history: &[ConversationMessage]) -> Vec<Value> {
    let history = history.iter().map(|message| {
        let mut message = message.clone();
        if !message.tool_calls.is_empty() {
            message.content = vec![ConversationPart::Text(json!({
                "assistant": message_text(&message), "tool_calls": message.tool_calls.iter().map(|r| &r.call).collect::<Vec<_>>()
            }).to_string())];
            message.tool_calls.clear();
        }
        if let Some(result) = message.tool_result.take() {
            message.content = vec![ConversationPart::Text(format!("tool_{} {}: {}", if result.is_error {"error"} else {"result"}, result.name, result.output))];
        }
        message
    }).collect::<Vec<_>>();
    map_chat_messages(system, &history)
}
