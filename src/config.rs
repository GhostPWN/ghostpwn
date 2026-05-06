use std::env;
use std::path::PathBuf;

use anyhow::Result;

use crate::secrets::SecretStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    Google,
    Copilot,
    Ollama,
}

impl ProviderKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "anthropic" => Some(Self::Anthropic),
            "openai" => Some(Self::OpenAi),
            "google" => Some(Self::Google),
            "copilot" | "github" => Some(Self::Copilot),
            "ollama" | "local" => Some(Self::Ollama),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
            Self::Google => "google",
            Self::Copilot => "copilot",
            Self::Ollama => "ollama",
        }
    }

    pub fn env_key(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Google => "GOOGLE_GENERATIVE_AI_API_KEY",
            Self::Copilot => "GITHUB_COPILOT_TOKEN",
            Self::Ollama => "IGNORE",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::Anthropic => "claude-3-7-sonnet-latest",
            Self::OpenAi => "gpt-4.1-mini",
            Self::Google => "gemini-2.5-flash",
            Self::Copilot => "gpt-4o",
            Self::Ollama => "llama3",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Anthropic,
            Self::OpenAi,
            Self::Google,
            Self::Copilot,
            Self::Ollama,
        ]
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProviderKeys {
    anthropic: Option<String>,
    openai: Option<String>,
    google: Option<String>,
    copilot: Option<String>,
    #[allow(dead_code)]
    ollama: Option<String>,
}

impl ProviderKeys {
    pub fn from_env_and_store(secret_store: &SecretStore) -> Self {
        let mut keys = Self {
            anthropic: read_env("ANTHROPIC_API_KEY"),
            openai: read_env("OPENAI_API_KEY"),
            google: read_env("GOOGLE_GENERATIVE_AI_API_KEY"),
            copilot: read_env("GITHUB_COPILOT_TOKEN"),
            ollama: None,
        };

        for provider in ProviderKind::all() {
            if keys.get(*provider).is_none()
                && let Some(stored) = secret_store.load_key(*provider)
            {
                keys.set(*provider, stored);
            }
        }

        keys
    }

    pub fn get(&self, provider: ProviderKind) -> Option<&str> {
        match provider {
            ProviderKind::Anthropic => self.anthropic.as_deref(),
            ProviderKind::OpenAi => self.openai.as_deref(),
            ProviderKind::Google => self.google.as_deref(),
            ProviderKind::Copilot => self.copilot.as_deref(),
            ProviderKind::Ollama => None,
        }
    }

    pub fn set(&mut self, provider: ProviderKind, value: String) {
        let target = match provider {
            ProviderKind::Anthropic => &mut self.anthropic,
            ProviderKind::OpenAi => &mut self.openai,
            ProviderKind::Google => &mut self.google,
            ProviderKind::Copilot => &mut self.copilot,
            ProviderKind::Ollama => &mut self.ollama,
        };

        *target = Some(value);
    }

    pub fn clear(&mut self, provider: ProviderKind) {
        let target = match provider {
            ProviderKind::Anthropic => &mut self.anthropic,
            ProviderKind::OpenAi => &mut self.openai,
            ProviderKind::Google => &mut self.google,
            ProviderKind::Copilot => &mut self.copilot,
            ProviderKind::Ollama => &mut self.ollama,
        };

        *target = None;
    }

    pub fn is_connected(&self, provider: ProviderKind) -> bool {
        if provider == ProviderKind::Ollama {
            return true;
        }
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
        let secret_store = SecretStore::new();

        let provider_keys = ProviderKeys::from_env_and_store(&secret_store);

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
