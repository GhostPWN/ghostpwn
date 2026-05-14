mod anthropic;
pub mod codex;
pub mod copilot;
mod google;
mod ollama;
mod openai;
mod sse;

use anyhow::{Result, anyhow};
use async_trait::async_trait;

use crate::config::{ProviderKeys, ProviderKind};
use crate::models::ConversationMessage;
use crate::secrets::SecretStore;

pub use anthropic::AnthropicProvider;
pub use codex::CodexProvider;
pub use copilot::CopilotProvider;
pub use google::GoogleProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;

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
