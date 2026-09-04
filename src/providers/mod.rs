mod anthropic;
pub mod codex;
pub mod copilot;
mod google;
mod ollama;
mod openai;
mod sse;

use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;

use crate::config::{ProviderKeys, ProviderKind};
use crate::models::{ConversationMessage, ConversationPart, ImageAttachment};
use crate::secrets::SecretStore;

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
    build_provider_inner(provider, model, keys, Some(secret_store))
}

fn build_provider_inner(
    provider: ProviderKind,
    model: String,
    keys: &ProviderKeys,
    secret_store: Option<SecretStore>,
) -> Box<dyn Provider> {
    match provider {
        ProviderKind::Ollama => Box::new(OllamaProvider::new(model)),
        _ => {
            let Some(api_key) = keys.get(provider) else {
                return Box::new(DisconnectedProvider { provider, model });
            };

            match provider {
                ProviderKind::Anthropic => {
                    Box::new(AnthropicProvider::new(api_key.to_string(), model))
                }
                ProviderKind::OpenAi => Box::new(OpenAiProvider::new(api_key.to_string(), model)),
                ProviderKind::Google => Box::new(GoogleProvider::new(api_key.to_string(), model)),
                ProviderKind::Copilot => Box::new(CopilotProvider::new(api_key.to_string(), model)),
                ProviderKind::Codex => {
                    Box::new(CodexProvider::new(api_key.to_string(), model, secret_store))
                }
                ProviderKind::Ollama => Box::new(OllamaProvider::new(model)),
            }
        }
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

    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn stream_complete(
        &self,
        _system: &str,
        _messages: &[ConversationMessage],
        _on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<String> {
        let usage = match self.provider {
            ProviderKind::Copilot => "open /model and press c on the copilot tab".to_string(),
            ProviderKind::Codex => "open /model and press c on the codex tab".to_string(),
            _ => format!(
                "open /model and press c on the {} tab",
                self.provider.as_str()
            ),
        };

        Err(anyhow!(
            "No API key connected for {}. {}",
            self.provider.as_str(),
            usage
        ))
    }
}
