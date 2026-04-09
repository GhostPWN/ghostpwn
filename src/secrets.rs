use std::fs;
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use keyring::{Entry, Error as KeyringError};

use crate::config::ProviderKind;

const KEYRING_SERVICE: &str = "ghostpwn-rust";

#[derive(Debug, Clone)]
pub struct SecretStore {
    env_path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct SecretMutationReport {
    pub keychain_saved: bool,
    pub env_saved: bool,
    pub keychain_error: Option<String>,
}

impl SecretStore {
    pub fn new() -> Self {
        let env_path = std::env::var("GHOSTPWN_ENV_FILE")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".env"));

        Self { env_path }
    }

    pub fn load_key(&self, provider: ProviderKind) -> Option<String> {
        if let Ok(entry) = entry_for(provider) {
            match entry.get_password() {
                Ok(value) if !value.trim().is_empty() => return Some(value),
                Ok(_) => {}
                Err(KeyringError::NoEntry) => {}
                Err(_) => {}
            }
        }

        read_env_key_from_file(&self.env_path, provider.env_key())
    }

    pub fn save_key(&self, provider: ProviderKind, value: &str) -> Result<SecretMutationReport> {
        if value.trim().is_empty() {
            return Err(anyhow!("API key cannot be empty"));
        }

        let mut report = SecretMutationReport::default();

        match entry_for(provider) {
            Ok(entry) => match entry.set_password(value) {
                Ok(()) => {
                    report.keychain_saved = true;
                }
                Err(err) => {
                    report.keychain_error = Some(err.to_string());
                }
            },
            Err(err) => {
                report.keychain_error = Some(err.to_string());
            }
        }

        upsert_env_key(&self.env_path, provider.env_key(), Some(value))?;
        report.env_saved = true;

        Ok(report)
    }

    pub fn delete_key(&self, provider: ProviderKind) -> Result<SecretMutationReport> {
        let mut report = SecretMutationReport::default();

        match entry_for(provider) {
            Ok(entry) => match entry.delete_credential() {
                Ok(()) | Err(KeyringError::NoEntry) => {
                    report.keychain_saved = true;
                }
                Err(err) => {
                    report.keychain_error = Some(err.to_string());
                }
            },
            Err(err) => {
                report.keychain_error = Some(err.to_string());
            }
        }

        upsert_env_key(&self.env_path, provider.env_key(), None)?;
        report.env_saved = true;

        Ok(report)
    }

    pub fn backend_name(&self) -> &'static str {
        "OS keychain + .env"
    }
}

fn entry_for(provider: ProviderKind) -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, provider.env_key())
        .map_err(|err| anyhow!("failed to initialize keyring entry: {}", err))
}

fn read_env_key_from_file(path: &PathBuf, key: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }

        if let Some((lhs, rhs)) = trimmed.split_once('=')
            && lhs.trim() == key
        {
            let value = rhs.trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }

    None
}

fn upsert_env_key(path: &PathBuf, key: &str, value: Option<&str>) -> Result<()> {
    let mut lines = if path.exists() {
        fs::read_to_string(path)?
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<String>>()
    } else {
        Vec::new()
    };

    let mut found = false;
    lines.retain_mut(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            return true;
        }

        if let Some((lhs, _rhs)) = trimmed.split_once('=')
            && lhs.trim() == key
        {
            found = true;
            if let Some(v) = value {
                *line = format!("{}={}", key, v);
                return true;
            }

            return false;
        }

        true
    });

    if !found && let Some(v) = value {
        lines.push(format!("{}={}", key, v));
    }

    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }

    fs::write(path, out)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }

    Ok(())
}
