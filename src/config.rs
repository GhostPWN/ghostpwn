use std::env;
use std::path::PathBuf;

use anyhow::{Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    Google,
}

impl ProviderKind {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "anthropic" => Some(Self::Anthropic),
            "openai" => Some(Self::OpenAi),
            "google" => Some(Self::Google),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Google => "google",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub provider: ProviderKind,
    pub model: String,
    pub api_key: String,
    pub workspace_root: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self> {
        let _ = dotenvy::dotenv();

        let provider = env::var("GHOSTPWN_PROVIDER")
            .ok()
            .as_deref()
            .and_then(ProviderKind::parse)
            .unwrap_or(ProviderKind::Google);

        let model = env::var("GHOSTPWN_MODEL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| match provider {
                ProviderKind::Anthropic => "claude-3-7-sonnet-latest".to_string(),
                ProviderKind::OpenAi => "gpt-4.1-mini".to_string(),
                ProviderKind::Google => "gemini-2.5-flash".to_string(),
            });

        let key_var = match provider {
            ProviderKind::Anthropic => "ANTHROPIC_API_KEY",
            ProviderKind::OpenAi => "OPENAI_API_KEY",
            ProviderKind::Google => "GOOGLE_GENERATIVE_AI_API_KEY",
        };

        let api_key = env::var(key_var).map_err(|_| {
            anyhow!(
                "Missing API key for provider '{}'. Set {} in .env",
                provider.as_str(),
                key_var
            )
        })?;

        let workspace_root = env::var("GHOSTPWN_WORKSPACE")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or(env::current_dir()?);

        Ok(Self {
            provider,
            model,
            api_key,
            workspace_root,
        })
    }
}
