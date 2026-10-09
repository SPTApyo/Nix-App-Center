//! The kept apps, Nix Software's source of truth. Each change rewrites the
//! generated files. An app may carry its own Nix blocks, user and system.

use crate::config_gen;
use crate::package::Package;
use crate::paths::data_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A package held at a nixpkgs revision. The revision is the truth; `since`
/// only dates the hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub rev: String,
    /// Hash of the revision's unpacked tarball, as `fetchTarball` wants it.
    pub sha256: String,
    /// When the hold started, Unix seconds.
    pub since: u64,
}

/// A kept app. Flat JSON, so old manifests of bare packages still read.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeptEntry {
    #[serde(flatten)]
    pub package: Package,
    /// User Nix block; `None` for a plain package entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_nix: Option<String>,
    /// System Nix block, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_nix: Option<String>,
    /// In the system packages instead of the user's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub system_wide: bool,
    /// Build options switched from their defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<(String, bool)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<Pin>,
}

impl KeptEntry {
    pub fn simple(package: Package) -> Self {
        Self {
            package,
            ..Default::default()
        }
    }

    /// True when the app has its own Nix blocks.
    pub fn is_promoted(&self) -> bool {
        self.home_nix.is_some() || self.system_nix.is_some()
    }

    /// Turns blocks older versions generated into structured fields.
    fn migrate(&mut self) {
        let attr = self.package.attr.clone();
        let wide = format!("environment.systemPackages = [ pkgs.{attr} ];");
        if self.home_nix.as_deref() == Some("") && self.system_nix.as_deref() == Some(&wide) {
            self.system_wide = true;
            self.home_nix = None;
            self.system_nix = None;
        }
        let Some(block) = self.home_nix.as_deref() else {
            return;
        };
        let flags = crate::build_flags::overridden_flags(block, &attr);
        let generated = format!(
            "= [ {} ];",
            crate::build_flags::with_flags(&format!("pkgs.{attr}"), &flags)
        );
        if !flags.is_empty() && !block.contains('\n') && block.trim_end().ends_with(&generated) {
            self.flags = flags;
            self.home_nix = None;
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub kept: Vec<KeptEntry>,
}

impl Manifest {
    /// The kept packages.
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

/// The manifest; empty when missing or unreadable.
pub fn load_manifest() -> Manifest {
    let mut m: Manifest = std::fs::read(manifest_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    m.kept.iter_mut().for_each(KeptEntry::migrate);
    m
}

fn save_manifest(m: &Manifest) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(m).map_err(std::io::Error::other)?;
    crate::paths::write_atomic(&manifest_path(), &json)
}

/// Writes the Nix files, then the manifest: a failed write keeps it unchanged.
fn commit(m: &Manifest) -> std::io::Result<()> {
    config_gen::sync_config(&m.kept)?;
    save_manifest(m)
}

/// Keeps `pkg`; nothing when already kept.
pub fn keep(pkg: &Package) -> std::io::Result<()> {
    let mut m = load_manifest();
    if !m.kept.iter().any(|e| e.package.attr == pkg.attr) {
        let everyone = crate::settings::load_settings().install_for_everyone
            && crate::detect_layout().system.is_some();
        m.kept.push(KeptEntry {
            system_wide: everyone,
            ..KeptEntry::simple(pkg.clone())
        });
        commit(&m)?;
    }
    Ok(())
}

/// Stops keeping `attr`.
pub fn unkeep(attr: &str) -> std::io::Result<()> {
    let mut m = load_manifest();
    let before = m.kept.len();
    m.kept.retain(|e| e.package.attr != attr);
    if m.kept.len() != before {
        commit(&m)?;
    }
    Ok(())
}

/// Sets the Nix blocks of `pkg`, kept as written, and keeps it.
pub fn set_blocks(
    pkg: &Package,
    home_nix: Option<String>,
    system_nix: Option<String>,
) -> std::io::Result<()> {
    let plain = config_gen::default_home_block(&crate::detect_layout(), &pkg.attr);
    let home_nix = home_nix.filter(|b| b.trim() != plain);
    let mut m = load_manifest();
    match m.kept.iter_mut().find(|e| e.package.attr == pkg.attr) {
        Some(e) => {
            e.home_nix = home_nix;
            e.system_nix = system_nix;
        }
        None => m.kept.push(KeptEntry {
            home_nix,
            system_nix,
            ..KeptEntry::simple(pkg.clone())
        }),
    }
    commit(&m)
}

/// Applies `edit` to the entry of `pkg`, keeping it first.
fn edit_entry(pkg: &Package, edit: impl FnOnce(&mut KeptEntry)) -> std::io::Result<()> {
    let mut m = load_manifest();
    if !m.kept.iter().any(|e| e.package.attr == pkg.attr) {
        m.kept.push(KeptEntry::simple(pkg.clone()));
    }
    if let Some(e) = m.kept.iter_mut().find(|e| e.package.attr == pkg.attr) {
        edit(e);
    }
    commit(&m)
}

/// Keeps `pkg` with build options `flags` switched from their defaults.
pub fn set_flags(pkg: &Package, flags: Vec<(String, bool)>) -> std::io::Result<()> {
    edit_entry(pkg, |e| e.flags = flags)
}

/// Keeps `pkg` held at `pin`, or releases it with `None`.
pub fn set_pin(pkg: &Package, pin: Option<Pin>) -> std::io::Result<()> {
    edit_entry(pkg, |e| e.pin = pin)
}

/// A hold at the nixpkgs revision the configuration uses now. Downloads the
/// revision once to hash it: blocking.
///
/// # Errors
/// When the revision is unknown or cannot be fetched.
pub fn pin_current() -> std::io::Result<Pin> {
    let rev = crate::updates::pinned_rev().ok_or_else(|| {
        std::io::Error::other(crate::util::n_("The nixpkgs revision in use is unknown."))
    })?;
    let out = std::process::Command::new("nix-prefetch-url")
        .args(["--unpack", &crate::config_gen::nixpkgs_tarball(rev)])
        .output()?;
    let sha256 = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || sha256.is_empty() {
        return Err(std::io::Error::other(crate::util::last_error_line(
            &out.stderr,
        )));
    }
    Ok(Pin {
        rev: rev.to_string(),
        sha256,
        since: crate::util::now_unix(),
    })
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
        // A minimal NixOS layout to write to.
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
        assert!(
            system.contains("environment.systemPackages = with pkgs; [\n    ripgrep\n  ];"),
            "{system}"
        );
    }

    #[test]
    fn old_generated_blocks_become_fields() {
        let mut wide = KeptEntry {
            home_nix: Some(String::new()),
            system_nix: Some("environment.systemPackages = [ pkgs.vlc ];".into()),
            ..KeptEntry::simple(Package::from_attr("vlc"))
        };
        wide.migrate();
        assert!(wide.system_wide && !wide.is_promoted());

        let mut flagged = KeptEntry {
            home_nix: Some(
                "home.packages = [ (pkgs.obs-studio.override { browserSupport = false; }) ];"
                    .into(),
            ),
            ..KeptEntry::simple(Package::from_attr("obs-studio"))
        };
        flagged.migrate();
        assert_eq!(flagged.flags, [("browserSupport".to_string(), false)]);
        assert!(flagged.home_nix.is_none());

        let custom = "home.packages = [ (pkgs.obs-studio.override { browserSupport = false; }) ];\nprograms.x = 1;";
        let mut kept = KeptEntry {
            home_nix: Some(custom.into()),
            ..KeptEntry::simple(Package::from_attr("obs-studio"))
        };
        kept.migrate();
        assert_eq!(
            kept.home_nix.as_deref(),
            Some(custom),
            "a user's own block stays"
        );
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
    fn the_default_block_stays_a_plain_entry() {
        let _g = crate::env_lock();
        sandbox("default-block");
        let pkg = Package::from_attr("hello");
        let plain = config_gen::default_home_block(&crate::detect_layout(), "hello");
        set_blocks(
            &pkg,
            Some(plain),
            Some("networking.hostName = \"x\";".into()),
        )
        .unwrap();
        let e = load_manifest().entry("hello").cloned().unwrap();
        assert!(e.home_nix.is_none(), "{:?}", e.home_nix);
        assert!(e.system_nix.is_some());
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
        // Old format: bare packages.
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
