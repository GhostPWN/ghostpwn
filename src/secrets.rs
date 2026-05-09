use anyhow::{Result, anyhow};
use keyring::{Entry, Error as KeyringError};

use crate::config::ProviderKind;

const KEYRING_SERVICE: &str = "ghostpwn-rust";
pub const SETTING_PROVIDER: &str = "latest_provider";
pub const SETTING_MODEL: &str = "latest_model";

#[derive(Debug, Clone)]
pub struct SecretStore;

#[derive(Debug, Clone, Default)]
pub struct SecretMutationReport {
    pub keychain_saved: bool,
    pub keychain_error: Option<String>,
}

impl SecretStore {
    pub fn new() -> Self {
        Self
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
        None
    }

    pub fn save_key(&self, provider: ProviderKind, value: &str) -> Result<SecretMutationReport> {
        if value.trim().is_empty() {
            return Err(anyhow!("API key cannot be empty"));
        }
        if value.contains(['\n', '\r']) {
            return Err(anyhow!("API key cannot contain newlines"));
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

        Ok(report)
    }

    pub fn backend_name(&self) -> &'static str {
        "OS keychain"
    }

    pub fn load_setting(&self, key: &str) -> Option<String> {
        match setting_entry_for(key).and_then(|entry| {
            entry
                .get_password()
                .map_err(|err| anyhow!("failed to load setting: {}", err))
        }) {
            Ok(value) if !value.trim().is_empty() => Some(value),
            _ => None,
        }
    }

    pub fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        if value.trim().is_empty() {
            return Err(anyhow!("setting value cannot be empty"));
        }
        if value.contains(['\n', '\r']) {
            return Err(anyhow!("setting value cannot contain newlines"));
        }

        setting_entry_for(key)?
            .set_password(value)
            .map_err(|err| anyhow!("failed to save setting: {}", err))
    }
}

fn entry_for(provider: ProviderKind) -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, provider.env_key())
        .map_err(|err| anyhow!("failed to initialize keyring entry: {}", err))
}

fn setting_entry_for(key: &str) -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, key)
        .map_err(|err| anyhow!("failed to initialize keyring setting: {}", err))
}
