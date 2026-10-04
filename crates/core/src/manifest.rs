//! Manifeste « garder » — source de vérité d'Onyx (jamais la config curée de
//! l'utilisateur). `keep`/`unkeep` régénèrent les fichiers Onyx au passage
//! (`config_gen::sync_config`).
//!
//! Une entrée porte le paquet et, éventuellement, le **bloc Nix** que l'utilisateur a
//! écrit pour lui (portée utilisateur et/ou système). Sans bloc, l'app est une simple
//! ligne de `home.packages` ; avec, elle est promue en région délimitée.

use crate::config_gen;
use crate::package::Package;
use crate::paths::data_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Une app gardée. Le `flatten` du paquet garde le JSON **plat**, donc rétrocompatible :
/// un ancien `kept.json` (une liste de `Package` nus) se relit tel quel, les blocs
/// simplement absents. Aucune migration à écrire, aucune perte.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeptEntry {
    #[serde(flatten)]
    pub package: Package,
    /// Bloc Nix de portée utilisateur. `None` = app simple (entrée de `home.packages`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_nix: Option<String>,
    /// Bloc Nix de portée système. `None` = l'app ne touche à rien de système.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_nix: Option<String>,
}

impl KeptEntry {
    pub fn simple(package: Package) -> Self {
        Self { package, home_nix: None, system_nix: None }
    }

    /// Vrai si l'utilisateur a pris la main sur le Nix de cette app.
    pub fn is_promoted(&self) -> bool {
        self.home_nix.is_some() || self.system_nix.is_some()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub kept: Vec<KeptEntry>,
}

impl Manifest {
    /// Les paquets gardés, pour les consommateurs qui n'ont que faire des blocs
    /// (mises à jour, vue « installés », `onyx-cli list`).
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

/// Charge le manifeste ; retourne un manifeste vide si absent ou illisible.
pub fn load_manifest() -> Manifest {
    match std::fs::read(manifest_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Manifest::default(),
    }
}

fn save_manifest(m: &Manifest) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(m).map_err(std::io::Error::other)?;
    // Atomique : le manifeste est la source de vérité d'Onyx — un crash mi-écriture le
    // remplaçait silencieusement par un état vide au chargement.
    crate::paths::write_atomic(&manifest_path(), &json)
}

/// Ajoute un paquet au manifeste (idempotent sur `attr`).
pub fn keep(pkg: &Package) -> std::io::Result<()> {
    let mut m = load_manifest();
    if !m.kept.iter().any(|e| e.package.attr == pkg.attr) {
        m.kept.push(KeptEntry::simple(pkg.clone()));
        save_manifest(&m)?;
        config_gen::sync_config(&m.kept)?;
    }
    Ok(())
}

/// Retire un paquet du manifeste par son `attr`.
pub fn unkeep(attr: &str) -> std::io::Result<()> {
    let mut m = load_manifest();
    let before = m.kept.len();
    m.kept.retain(|e| e.package.attr != attr);
    if m.kept.len() != before {
        save_manifest(&m)?;
        config_gen::sync_config(&m.kept)?;
    }
    Ok(())
}

/// Remplace les blocs Nix d'une app (le « chemin avancé »). L'app est gardée si elle ne
/// l'était pas : éditer le Nix d'une app, c'est vouloir l'avoir.
///
/// Les blocs sont écrits **tels que l'utilisateur les a fournis**. Onyx ne réécrit ni ne
/// reformate son Nix — c'est lui qui décide.
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
        None => m.kept.push(KeptEntry { package: pkg.clone(), home_nix, system_nix }),
    }
    save_manifest(&m)?;
    config_gen::sync_config(&m.kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Isole `XDG_DATA_HOME` (manifeste) et `ONYX_CONFIG_DIR` (fichiers générés) dans un
    /// répertoire jetable. Aucun test ne touche au vrai `~` ni au vrai `/etc/nixos`.
    fn sandbox(name: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("onyx-manifest-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // Disposition NixOS classique minimale, pour que sync_config ait où écrire.
        std::fs::write(base.join("configuration.nix"), "{ imports = [ ]; }").unwrap();
        std::env::set_var("XDG_DATA_HOME", &base);
        std::env::set_var("ONYX_CONFIG_DIR", &base);
        base
    }

    fn cleanup() {
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("ONYX_CONFIG_DIR");
    }

    #[test]
    fn keep_then_unkeep_updates_generated_file() {
        let _g = crate::env_lock();
        let base = sandbox("keep");

        keep(&Package::from_attr("ripgrep")).unwrap();
        let after_keep = std::fs::read_to_string(base.join("onyx-home.nix")).unwrap();
        assert!(after_keep.contains("ripgrep"));

        unkeep("ripgrep").unwrap();
        let after_unkeep = std::fs::read_to_string(base.join("onyx-home.nix")).unwrap();
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

        let home = std::fs::read_to_string(base.join("onyx-home.nix")).unwrap();
        let system = std::fs::read_to_string(base.join("onyx-system.nix")).unwrap();
        assert!(home.contains("# onyx:begin quickshare"));
        assert!(home.contains("services.quickshare.enable = true;"));
        assert!(system.contains("networking.firewall.allowedTCPPorts = [ 8080 ];"));

        assert!(load_manifest().entry("quickshare").unwrap().is_promoted());
        cleanup();
    }

    #[test]
    fn old_flat_manifest_is_read_without_loss() {
        // Ancien format : une liste de Package nus, sans champ de bloc.
        let _g = crate::env_lock();
        let base = sandbox("migration");
        let onyx = base.join("onyx");
        std::fs::create_dir_all(&onyx).unwrap();
        std::fs::write(
            onyx.join("kept.json"),
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
