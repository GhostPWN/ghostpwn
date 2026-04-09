mod anthropic;
mod google;
mod openai;
mod sse;

use anyhow::Result;
use async_trait::async_trait;

use crate::config::{Config, ProviderKind};
use crate::models::ConversationMessage;

pub use anthropic::AnthropicProvider;
pub use google::GoogleProvider;
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
}

pub fn build_provider(config: &Config) -> Result<Box<dyn Provider>> {
    let provider: Box<dyn Provider> = match config.provider {
        ProviderKind::Anthropic => Box::new(AnthropicProvider::new(
            config.api_key.clone(),
            config.model.clone(),
        )),
        ProviderKind::OpenAi => Box::new(OpenAiProvider::new(
            config.api_key.clone(),
            config.model.clone(),
        )),
        ProviderKind::Google => Box::new(GoogleProvider::new(
            config.api_key.clone(),
            config.model.clone(),
        )),
    };

    Ok(provider)
}
