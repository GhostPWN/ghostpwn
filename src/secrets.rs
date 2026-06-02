use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};

use crate::config::ProviderKind;

const KEYRING_SERVICE: &str = "ghostpwn-rust";
pub const SETTING_PROVIDER: &str = "latest_provider";
pub const SETTING_MODEL: &str = "latest_model";

#[derive(Debug, Clone)]
pub struct SecretStore {
    state_file: Option<PathBuf>,
    keychain_enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SecretMutationReport {
    pub keychain_saved: bool,
    pub keychain_error: Option<String>,
    pub file_saved: bool,
    pub file_error: Option<String>,
}

impl SecretMutationReport {
    pub fn persisted(&self) -> bool {
        self.keychain_saved || self.file_saved
    }

    pub fn backend_name(&self) -> &'static str {
        if self.keychain_saved && self.file_saved {
            "OS keychain/local state file"
        } else if self.keychain_saved {
            "OS keychain"
        } else if self.file_saved {
            "local state file"
        } else {
            "no persistent backend"
        }
    }
}

impl SecretStore {
    pub fn new() -> Self {
        Self {
            state_file: default_state_file_path(),
            keychain_enabled: true,
        }
    }

    #[cfg(test)]
    fn file_only(path: PathBuf) -> Self {
        Self {
            state_file: Some(path),
            keychain_enabled: false,
        }
    }

    pub fn load_key(&self, provider: ProviderKind) -> Option<String> {
        if self.keychain_enabled
            && let Ok(entry) = entry_for(provider)
        {
            match entry.get_password() {
                Ok(value) if !value.trim().is_empty() => return Some(value),
                Ok(_) => {}
                Err(KeyringError::NoEntry) => {}
                Err(_) => {}
            }
        }

        self.load_file_state()
            .ok()
            .and_then(|state| state.keys.get(provider.env_key()).cloned())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    pub fn save_key(&self, provider: ProviderKind, value: &str) -> Result<SecretMutationReport> {
        if value.trim().is_empty() {
            return Err(anyhow!("API key cannot be empty"));
        }
        if value.contains(['\n', '\r']) {
            return Err(anyhow!("API key cannot contain newlines"));
        }

        let mut report = SecretMutationReport::default();

        if self.keychain_enabled {
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
        }

        match self.save_file_key(provider, value) {
            Ok(()) => {
                report.file_saved = true;
            }
            Err(err) => {
                report.file_error = Some(err.to_string());
            }
        }

        Ok(report)
    }

    pub fn delete_key(&self, provider: ProviderKind) -> Result<SecretMutationReport> {
        let mut report = SecretMutationReport::default();

        if self.keychain_enabled {
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
        }

        match self.delete_file_key(provider) {
            Ok(()) => report.file_saved = true,
            Err(err) => report.file_error = Some(err.to_string()),
        }

        Ok(report)
    }

    pub fn load_setting(&self, key: &str) -> Option<String> {
        if self.keychain_enabled {
            match setting_entry_for(key).and_then(|entry| {
                entry
                    .get_password()
                    .map_err(|err| anyhow!("failed to load setting: {}", err))
            }) {
                Ok(value) if !value.trim().is_empty() => return Some(value),
                _ => {}
            }
        }

        self.load_file_state()
            .ok()
            .and_then(|state| state.settings.get(key).cloned())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    pub fn save_setting(&self, key: &str, value: &str) -> Result<()> {
        if value.trim().is_empty() {
            return Err(anyhow!("setting value cannot be empty"));
        }
        if value.contains(['\n', '\r']) {
            return Err(anyhow!("setting value cannot contain newlines"));
        }

        let keychain_result = if self.keychain_enabled {
            match setting_entry_for(key) {
                Ok(entry) => entry
                    .set_password(value)
                    .map_err(|err| anyhow!("failed to save setting: {}", err)),
                Err(err) => Err(err),
            }
        } else {
            Err(anyhow!("OS keychain disabled"))
        };

        let file_result = self.save_file_setting(key, value);

        match (keychain_result, file_result) {
            (Ok(()), _) | (_, Ok(())) => Ok(()),
            (Err(keychain_err), Err(file_err)) => Err(anyhow!(
                "failed to save setting to keychain ({}) or local state file ({})",
                keychain_err,
                file_err
            )),
        }
    }

    fn load_file_state(&self) -> Result<FileState> {
        let Some(path) = self.state_file.as_deref() else {
            return Ok(FileState::default());
        };

        load_file_state(path)
    }

    fn save_file_key(&self, provider: ProviderKind, value: &str) -> Result<()> {
        let Some(path) = self.state_file.as_deref() else {
            return Err(anyhow!("local state file is unavailable"));
        };

        let mut state = load_file_state(path)?;
        state
            .keys
            .insert(provider.env_key().to_string(), value.to_string());
        save_file_state(path, &state)
    }

    fn delete_file_key(&self, provider: ProviderKind) -> Result<()> {
        let Some(path) = self.state_file.as_deref() else {
            return Ok(());
        };

        let mut state = load_file_state(path)?;
        state.keys.remove(provider.env_key());
        save_file_state(path, &state)
    }

    fn save_file_setting(&self, key: &str, value: &str) -> Result<()> {
        let Some(path) = self.state_file.as_deref() else {
            return Err(anyhow!("local state file is unavailable"));
        };

        let mut state = load_file_state(path)?;
        state.settings.insert(key.to_string(), value.to_string());
        save_file_state(path, &state)
    }
}

impl Default for SecretStore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct FileState {
    #[serde(default)]
    keys: HashMap<String, String>,
    #[serde(default)]
    settings: HashMap<String, String>,
}

fn entry_for(provider: ProviderKind) -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, provider.env_key())
        .map_err(|err| anyhow!("failed to initialize keyring entry: {}", err))
}

fn setting_entry_for(key: &str) -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, key)
        .map_err(|err| anyhow!("failed to initialize keyring setting: {}", err))
}

fn default_state_file_path() -> Option<PathBuf> {
    default_state_file_path_with(read_env_path)
}

fn default_state_file_path_with<F>(mut read_path: F) -> Option<PathBuf>
where
    F: FnMut(&str) -> Option<PathBuf>,
{
    if let Some(path) = read_path("GHOSTPWN_STATE_FILE") {
        return Some(path);
    }

    platform_state_file_path(&mut read_path)
}

#[cfg(windows)]
fn platform_state_file_path<F>(read_path: &mut F) -> Option<PathBuf>
where
    F: FnMut(&str) -> Option<PathBuf>,
{
    read_path("APPDATA")
        .map(|path| path.join("ghostpwn/state.json"))
        .or_else(|| read_path("USERPROFILE").map(|path| path.join(".ghostpwn/state.json")))
}

#[cfg(not(windows))]
fn platform_state_file_path<F>(read_path: &mut F) -> Option<PathBuf>
where
    F: FnMut(&str) -> Option<PathBuf>,
{
    read_path("XDG_CONFIG_HOME")
        .map(|path| path.join("ghostpwn/state.json"))
        .or_else(|| read_path("HOME").map(|path| path.join(".config/ghostpwn/state.json")))
}

fn read_env_path(key: &str) -> Option<PathBuf> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn load_file_state(path: &Path) -> Result<FileState> {
    match fs::read_to_string(path) {
        Ok(content) if content.trim().is_empty() => Ok(FileState::default()),
        Ok(content) => serde_json::from_str(&content)
            .with_context(|| format!("failed to parse local state file '{}'", path.display())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(FileState::default()),
        Err(err) => Err(anyhow!(
            "failed to read local state file '{}': {}",
            path.display(),
            err
        )),
    }
}

fn save_file_state(path: &Path, state: &FileState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create local state directory '{}'",
                parent.display()
            )
        })?;
        secure_directory(parent);
    }

    let mut file = secure_file_options()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open local state file '{}'", path.display()))?;
    let content = serde_json::to_vec_pretty(state)?;
    file.write_all(&content)
        .with_context(|| format!("failed to write local state file '{}'", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("failed to finish local state file '{}'", path.display()))?;
    secure_file(path);
    Ok(())
}

fn secure_file_options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    secure_file_options_permissions(&mut options);
    options
}

#[cfg(unix)]
fn secure_file_options_permissions(options: &mut fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn secure_file_options_permissions(_options: &mut fs::OpenOptions) {}

fn secure_file(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn secure_directory(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{SETTING_MODEL, SETTING_PROVIDER, SecretStore, default_state_file_path_with};
    use crate::config::ProviderKind;

    fn state_path_from(entries: &[(&str, &str)]) -> Option<PathBuf> {
        let values = entries.iter().copied().collect::<HashMap<_, _>>();
        default_state_file_path_with(|key| values.get(key).map(|value| PathBuf::from(*value)))
    }

    #[test]
    fn state_file_override_takes_precedence() {
        assert_eq!(
            state_path_from(&[
                ("GHOSTPWN_STATE_FILE", "/tmp/ghostpwn-state.json"),
                ("APPDATA", "/tmp/appdata"),
                ("XDG_CONFIG_HOME", "/tmp/xdg"),
            ]),
            Some(PathBuf::from("/tmp/ghostpwn-state.json"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn default_state_file_uses_windows_appdata() {
        let appdata = PathBuf::from(r"C:\Users\alice\AppData\Roaming");

        assert_eq!(
            state_path_from(&[("APPDATA", r"C:\Users\alice\AppData\Roaming")]),
            Some(appdata.join("ghostpwn/state.json"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn default_state_file_falls_back_to_windows_userprofile() {
        let userprofile = PathBuf::from(r"C:\Users\alice");

        assert_eq!(
            state_path_from(&[("USERPROFILE", r"C:\Users\alice")]),
            Some(userprofile.join(".ghostpwn/state.json"))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn default_state_file_uses_xdg_config_home() {
        assert_eq!(
            state_path_from(&[("XDG_CONFIG_HOME", "/tmp/xdg")]),
            Some(PathBuf::from("/tmp/xdg").join("ghostpwn/state.json"))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn default_state_file_falls_back_to_home_config() {
        assert_eq!(
            state_path_from(&[("HOME", "/tmp/home")]),
            Some(PathBuf::from("/tmp/home").join(".config/ghostpwn/state.json"))
        );
    }

    #[test]
    fn file_store_persists_settings_without_keychain() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("state.json");
        let store = SecretStore::file_only(path.clone());

        store
            .save_setting(SETTING_PROVIDER, ProviderKind::Copilot.as_str())
            .expect("save provider");
        store
            .save_setting(SETTING_MODEL, ProviderKind::Copilot.default_model())
            .expect("save model");

        let reloaded = SecretStore::file_only(path);
        assert_eq!(
            reloaded.load_setting(SETTING_PROVIDER).as_deref(),
            Some(ProviderKind::Copilot.as_str())
        );
        assert_eq!(
            reloaded.load_setting(SETTING_MODEL).as_deref(),
            Some(ProviderKind::Copilot.default_model())
        );
    }

    #[test]
    fn file_store_persists_keys_without_keychain() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("state.json");
        let store = SecretStore::file_only(path.clone());

        let report = store
            .save_key(ProviderKind::Copilot, "ghu-test-token")
            .expect("save key");

        assert!(report.persisted());
        assert!(report.file_saved);
        assert_eq!(
            SecretStore::file_only(path)
                .load_key(ProviderKind::Copilot)
                .as_deref(),
            Some("ghu-test-token")
        );
    }
}
