//! "Keep" manifest: Nix Software's source of truth (never the user's curated
//! config). `keep`/`unkeep` regenerate the Nix Software files along the way
//! (`config_gen::sync_config`).
//!
//! An entry holds the package and, optionally, the **Nix block** the user wrote
//! for it (user and/or system scope). Without a block the app is a plain
//! `home.packages` line; with one it is promoted to a delimited region.

use crate::config_gen;
use crate::package::Package;
use crate::paths::data_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A kept app. The package `flatten` keeps the JSON **flat**, hence backward
/// compatible: an old `kept.json` (a list of bare `Package`s) reads as is, the
/// blocks simply missing. No migration to write, no loss.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeptEntry {
    #[serde(flatten)]
    pub package: Package,
    /// User scope Nix block. `None` = plain app (a `home.packages` entry).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_nix: Option<String>,
    /// System scope Nix block. `None` = the app touches nothing system wide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_nix: Option<String>,
}

impl KeptEntry {
    pub fn simple(package: Package) -> Self {
        Self {
            package,
            home_nix: None,
            system_nix: None,
        }
    }

    /// An app for every user: in the system packages, not the user's.
    pub fn system_wide(package: Package) -> Self {
        let system = format!("environment.systemPackages = [ pkgs.{} ];", package.attr);
        Self {
            package,
            home_nix: Some(String::new()),
            system_nix: Some(system),
        }
    }

    /// True when the user took over this app's Nix.
    pub fn is_promoted(&self) -> bool {
        self.home_nix.is_some() || self.system_nix.is_some()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub kept: Vec<KeptEntry>,
}

impl Manifest {
    /// Kept packages, for consumers that do not care about blocks
    /// (updates, "installed" view, `nix-software-cli list`).
    pub fn packages(&self) -> Vec<Package> {
        self.kept.iter().map(|e| e.package.clone()).collect()
    }

    pub fn entry(&self, attr: &str) -> Option<&KeptEntry> {
        self.kept.iter().find(|e| e.package.attr == attr)
    }
}

fn manifest_path() -> PathBuf {
    data_dir().join("kept.json")
}

/// Loads the manifest; returns an empty manifest when missing or unreadable.
pub fn load_manifest() -> Manifest {
    match std::fs::read(manifest_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Manifest::default(),
    }
}

fn save_manifest(m: &Manifest) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(m).map_err(std::io::Error::other)?;
    // Atomic: a crash mid-write used to leave an empty manifest.
    crate::paths::write_atomic(&manifest_path(), &json)
}

/// Writes the Nix Software files, then the manifest: when the configuration cannot be
/// written (no rights, cancelled password), the manifest stays as it was.
fn commit(m: &Manifest) -> std::io::Result<()> {
    config_gen::sync_config(&m.kept)?;
    save_manifest(m)
}

/// Adds a package to the manifest (idempotent on `attr`).
pub fn keep(pkg: &Package) -> std::io::Result<()> {
    let mut m = load_manifest();
    if !m.kept.iter().any(|e| e.package.attr == pkg.attr) {
        let everyone = crate::settings::load_settings().install_for_everyone
            && crate::detect_layout().system.is_some();
        m.kept.push(if everyone {
            KeptEntry::system_wide(pkg.clone())
        } else {
            KeptEntry::simple(pkg.clone())
        });
        commit(&m)?;
    }
    Ok(())
}

/// Removes a package from the manifest by its `attr`.
pub fn unkeep(attr: &str) -> std::io::Result<()> {
    let mut m = load_manifest();
    let before = m.kept.len();
    m.kept.retain(|e| e.package.attr != attr);
    if m.kept.len() != before {
        commit(&m)?;
    }
    Ok(())
}

/// Replaces an app's Nix blocks (the "advanced path"). The app is kept if it
/// was not: editing an app's Nix means wanting it.
///
/// Blocks are written **as the user provided them**. Nix Software neither rewrites
/// nor reformats their Nix: they decide.
pub fn set_blocks(
    pkg: &Package,
    home_nix: Option<String>,
    system_nix: Option<String>,
) -> std::io::Result<()> {
    let mut m = load_manifest();
    match m.kept.iter_mut().find(|e| e.package.attr == pkg.attr) {
        Some(e) => {
            e.home_nix = home_nix;
            e.system_nix = system_nix;
        }
        None => m.kept.push(KeptEntry {
            package: pkg.clone(),
            home_nix,
            system_nix,
        }),
    }
    commit(&m)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Isolates `XDG_DATA_HOME` (manifest) and `NIX_SOFTWARE_CONFIG_DIR` (generated files)
    /// in a throwaway directory. No test touches the real `~` or `/etc/nixos`.
    fn sandbox(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "nix-software-manifest-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // Minimal classic NixOS layout, so sync_config has somewhere to write.
        std::fs::write(base.join("configuration.nix"), "{ imports = [ ]; }").unwrap();
        std::env::set_var("XDG_DATA_HOME", &base);
        std::env::set_var("XDG_CONFIG_HOME", &base);
        std::env::set_var("NIX_SOFTWARE_CONFIG_DIR", &base);
        base
    }

    #[test]
    fn keep_for_everyone_lands_in_the_system_packages() {
        let _g = crate::env_lock();
        let base = sandbox("everyone");
        let settings = crate::settings::Settings {
            install_for_everyone: true,
            ..Default::default()
        };
        crate::settings::save_settings(&settings).unwrap();

        keep(&Package::from_attr("ripgrep")).unwrap();
        let home = std::fs::read_to_string(base.join("nix-software-home.nix")).unwrap();
        let system = std::fs::read_to_string(base.join("nix-software-system.nix")).unwrap();
        assert!(!home.contains("ripgrep"), "{home}");
        assert!(system.contains("environment.systemPackages = [ pkgs.ripgrep ];"));
    }

    fn cleanup() {
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("NIX_SOFTWARE_CONFIG_DIR");
    }

    #[test]
    fn keep_then_unkeep_updates_generated_file() {
        let _g = crate::env_lock();
        let base = sandbox("keep");

        keep(&Package::from_attr("ripgrep")).unwrap();
        let after_keep = std::fs::read_to_string(base.join("nix-software-home.nix")).unwrap();
        assert!(after_keep.contains("ripgrep"));

        unkeep("ripgrep").unwrap();
        let after_unkeep = std::fs::read_to_string(base.join("nix-software-home.nix")).unwrap();
        assert!(!after_unkeep.contains("ripgrep"));

        cleanup();
    }

    #[test]
    fn set_blocks_promotes_the_app_into_a_region() {
        let _g = crate::env_lock();
        let base = sandbox("blocks");
        let pkg = Package::from_attr("quickshare");

        keep(&pkg).unwrap();
        set_blocks(
            &pkg,
            Some("services.quickshare.enable = true;".into()),
            Some("networking.firewall.allowedTCPPorts = [ 8080 ];".into()),
        )
        .unwrap();

        let home = std::fs::read_to_string(base.join("nix-software-home.nix")).unwrap();
        let system = std::fs::read_to_string(base.join("nix-software-system.nix")).unwrap();
        assert!(home.contains("# nix-software:begin quickshare"));
        assert!(home.contains("services.quickshare.enable = true;"));
        assert!(system.contains("networking.firewall.allowedTCPPorts = [ 8080 ];"));

        assert!(load_manifest().entry("quickshare").unwrap().is_promoted());
        cleanup();
    }

    #[test]
    fn old_flat_manifest_is_read_without_loss() {
        // Old format: a list of bare Packages, without block fields.
        let _g = crate::env_lock();
        let base = sandbox("migration");
        let data = base.join("nix-software");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(
            data.join("kept.json"),
            r#"{"kept":[{"attr":"firefox","name":"Firefox","version":"1","description":"d"}]}"#,
        )
        .unwrap();

        let m = load_manifest();
        assert_eq!(m.kept.len(), 1);
        assert_eq!(m.kept[0].package.attr, "firefox");
        assert!(!m.kept[0].is_promoted());
        cleanup();
    }
}
