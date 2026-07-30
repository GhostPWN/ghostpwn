use std::env;
use std::path::PathBuf;

use anyhow::Result;

use crate::secrets::{SETTING_MODEL, SETTING_PROVIDER, SecretStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    Google,
    Copilot,
    Codex,
    Ollama,
}

impl ProviderKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "anthropic" => Some(Self::Anthropic),
            "openai" => Some(Self::OpenAi),
            "google" => Some(Self::Google),
            "copilot" | "github" => Some(Self::Copilot),
            "codex" | "openai-codex" => Some(Self::Codex),
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
            Self::Codex => "codex",
            Self::Ollama => "ollama",
        }
    }

    pub fn env_key(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Google => "GOOGLE_GENERATIVE_AI_API_KEY",
            Self::Copilot => "GITHUB_COPILOT_TOKEN",
            Self::Codex => "CODEX_OAUTH_TOKEN",
            Self::Ollama => "OLLAMA_HOST",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::Anthropic => "claude-3-7-sonnet-latest",
            Self::OpenAi => "gpt-4.1-mini",
            Self::Google => "gemini-2.5-flash",
            Self::Copilot => "gpt-4o",
            Self::Codex => "gpt-5.3-codex",
            Self::Ollama => "llama3",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Anthropic,
            Self::OpenAi,
            Self::Google,
            Self::Copilot,
            Self::Codex,
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
    codex: Option<String>,
}

impl ProviderKeys {
    pub fn from_env_and_store(secret_store: &SecretStore) -> Self {
        let mut keys = Self {
            anthropic: read_env("ANTHROPIC_API_KEY"),
            openai: read_env("OPENAI_API_KEY"),
            google: read_env("GOOGLE_GENERATIVE_AI_API_KEY"),
            copilot: read_env("GITHUB_COPILOT_TOKEN"),
            codex: read_env("CODEX_OAUTH_TOKEN"),
        };

        for provider in ProviderKind::all()
            .iter()
            .copied()
            .filter(|provider| *provider != ProviderKind::Ollama)
        {
            if keys.get(provider).is_none()
                && let Some(stored) = secret_store.load_key(provider)
            {
                keys.set(provider, stored);
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
            ProviderKind::Codex => self.codex.as_deref(),
            ProviderKind::Ollama => None,
        }
    }

    pub fn set(&mut self, provider: ProviderKind, value: String) {
        let target = match provider {
            ProviderKind::Anthropic => &mut self.anthropic,
            ProviderKind::OpenAi => &mut self.openai,
            ProviderKind::Google => &mut self.google,
            ProviderKind::Copilot => &mut self.copilot,
            ProviderKind::Codex => &mut self.codex,
            ProviderKind::Ollama => return,
        };

        *target = Some(value);
    }

    pub fn clear(&mut self, provider: ProviderKind) {
        let target = match provider {
            ProviderKind::Anthropic => &mut self.anthropic,
            ProviderKind::OpenAi => &mut self.openai,
            ProviderKind::Google => &mut self.google,
            ProviderKind::Copilot => &mut self.copilot,
            ProviderKind::Codex => &mut self.codex,
            ProviderKind::Ollama => return,
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
        let secret_store = SecretStore::new();

        let provider_keys = ProviderKeys::from_env_and_store(&secret_store);

        let env_provider =
            read_env("GHOSTPWN_PROVIDER").and_then(|value| ProviderKind::parse(&value));
        let env_model = read_env("GHOSTPWN_MODEL");
        let saved_provider = secret_store
            .load_setting(SETTING_PROVIDER)
            .and_then(|value| ProviderKind::parse(&value));
        let saved_model = secret_store.load_setting(SETTING_MODEL);
        let (provider, model) =
            resolve_config_provider_and_model(env_provider, env_model, saved_provider, saved_model);

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

fn resolve_provider_and_model(
    saved_provider: Option<ProviderKind>,
    saved_model: Option<String>,
) -> (ProviderKind, String) {
    let provider = saved_provider.unwrap_or(ProviderKind::Google);
    let model = saved_model.unwrap_or_else(|| provider.default_model().to_string());

    (provider, model)
}

fn resolve_config_provider_and_model(
    env_provider: Option<ProviderKind>,
    env_model: Option<String>,
    saved_provider: Option<ProviderKind>,
    saved_model: Option<String>,
) -> (ProviderKind, String) {
    let model = env_model.or_else(|| {
        if env_provider.is_none() {
            saved_model
        } else {
            None
        }
    });
    resolve_provider_and_model(env_provider.or(saved_provider), model)
}

#[cfg(test)]
#[path = "tests/config.rs"]
mod tests;
