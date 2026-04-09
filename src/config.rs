use std::env;
use std::path::PathBuf;

use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    Google,
}

impl ProviderKind {
    pub fn parse(value: &str) -> Option<Self> {
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

    pub fn env_key(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Google => "GOOGLE_GENERATIVE_AI_API_KEY",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::Anthropic => "claude-3-7-sonnet-latest",
            Self::OpenAi => "gpt-4.1-mini",
            Self::Google => "gemini-2.5-flash",
        }
    }

    pub fn suggested_models(self) -> &'static [&'static str] {
        match self {
            Self::Anthropic => &[
                "claude-3-7-sonnet-latest",
                "claude-3-5-sonnet-latest",
                "claude-3-5-haiku-latest",
            ],
            Self::OpenAi => &["gpt-4.1-mini", "gpt-4.1", "gpt-4o-mini"],
            Self::Google => &["gemini-2.5-flash", "gemini-2.5-pro", "gemini-2.0-flash"],
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Anthropic, Self::OpenAi, Self::Google]
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProviderKeys {
    anthropic: Option<String>,
    openai: Option<String>,
    google: Option<String>,
}

impl ProviderKeys {
    pub fn from_env() -> Self {
        Self {
            anthropic: read_env("ANTHROPIC_API_KEY"),
            openai: read_env("OPENAI_API_KEY"),
            google: read_env("GOOGLE_GENERATIVE_AI_API_KEY"),
        }
    }

    pub fn get(&self, provider: ProviderKind) -> Option<&str> {
        match provider {
            ProviderKind::Anthropic => self.anthropic.as_deref(),
            ProviderKind::OpenAi => self.openai.as_deref(),
            ProviderKind::Google => self.google.as_deref(),
        }
    }

    pub fn set(&mut self, provider: ProviderKind, value: String) {
        let target = match provider {
            ProviderKind::Anthropic => &mut self.anthropic,
            ProviderKind::OpenAi => &mut self.openai,
            ProviderKind::Google => &mut self.google,
        };

        *target = Some(value);
    }

    pub fn is_connected(&self, provider: ProviderKind) -> bool {
        self.get(provider).is_some()
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub provider: ProviderKind,
    pub model: String,
    pub provider_keys: ProviderKeys,
    pub workspace_root: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self> {
        let _ = dotenvy::dotenv();

        let provider_keys = ProviderKeys::from_env();

        let provider = env::var("GHOSTPWN_PROVIDER")
            .ok()
            .as_deref()
            .and_then(ProviderKind::parse)
            .unwrap_or(ProviderKind::Google);

        let model = env::var("GHOSTPWN_MODEL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| provider.default_model().to_string());

        let workspace_root = env::var("GHOSTPWN_WORKSPACE")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or(env::current_dir()?);

        Ok(Self {
            provider,
            model,
            provider_keys,
            workspace_root,
        })
    }
}

fn read_env(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}
