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
