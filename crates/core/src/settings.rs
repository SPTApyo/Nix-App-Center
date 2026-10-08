//! User settings, in `$XDG_CONFIG_HOME/nix-software/settings.json`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// NixOS configuration folder, instead of `/etc/nixos`.
    pub config_dir: Option<PathBuf>,
    /// `nixosConfigurations` entry, instead of the hostname.
    pub host: Option<String>,
    /// Check for updates once a day and notify.
    pub auto_check_updates: bool,
    /// Last background update check (Unix seconds).
    pub last_update_check: u64,
    /// The user's random ODRS id.
    pub odrs_user_hash: Option<String>,
    /// Keep apps in the system packages instead of the user's.
    pub install_for_everyone: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            config_dir: None,
            host: None,
            auto_check_updates: true,
            last_update_check: 0,
            odrs_user_hash: None,
            install_for_everyone: false,
        }
    }
}

fn settings_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::util::home().join(".config"));
    base.join("nix-software/settings.json")
}

/// The saved settings, defaults when missing or unreadable.
pub fn load_settings() -> Settings {
    std::fs::read(settings_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_settings(s: &Settings) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(s).map_err(std::io::Error::other)?;
    crate::paths::write_atomic(&settings_path(), &json)
}
