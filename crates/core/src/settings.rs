//! User settings (`$XDG_CONFIG_HOME/nix-software/settings.json`): overrides of the
//! layout detection and the update check preferences.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// NixOS configuration folder, instead of `/etc/nixos`.
    pub config_dir: Option<PathBuf>,
    /// `nixosConfigurations` entry to use, instead of the hostname match.
    pub host: Option<String>,
    /// Check for updates once a day and notify.
    pub auto_check_updates: bool,
    /// Last background update check (Unix seconds).
    pub last_update_check: u64,
    /// Random id sent to ODRS to tie the user's reviews together.
    pub odrs_user_hash: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            config_dir: None,
            host: None,
            auto_check_updates: true,
            last_update_check: 0,
            odrs_user_hash: None,
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
