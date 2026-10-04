//! Métadonnées enrichies via `nix eval <attr>.meta --json` (rapide, local,
//! riche — mais pas d'icônes/screenshots, cf. `flathub` pour ça).
//!
//! NB : `nix search` (liste) ne porte PAS `mainProgram` → on enrichit à la
//! demande (ouverture de la page détail), pas sur les 100 résultats.

use crate::package::Kind;
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackageDetail {
    pub long_description: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    /// `meta.license.free` — déjà calculé par nixpkgs, fiable (contrairement à
    /// Flathub qui ne le fournit que via son API REST, jamais la base locale).
    pub is_free_license: Option<bool>,
    pub changelog: Option<String>,
    /// Binaire principal (`meta.mainProgram`) : sa présence ⇒ paquet runnable.
    pub main_program: Option<String>,
    /// Noms des mainteneurs nixpkgs (`meta.maintainers[].name`, ou `.github` à
    /// défaut). Signal de confiance Nix-natif — pas d'équivalent AppStream.
    pub maintainers: Vec<String>,
    /// `meta.insecure` — le paquet est marqué comme ayant une vulnérabilité
    /// connue non corrigée (nécessite `NIXPKGS_ALLOW_INSECURE=1` pour builder).
    pub insecure: bool,
    /// `meta.knownVulnerabilities` — identifiants CVE le cas échéant.
    pub known_vulnerabilities: Vec<String>,
    /// `meta.broken` — nix refuse d'évaluer `outPath`/`drvPath` pour ce paquet
    /// (donc `nix run` ET `closure_size()` échoueront). Contrairement à
    /// `insecure`, `.meta` reste lisible même pour un paquet broken — c'est le
    /// seul signal fiable et gratuit qui explique pourquoi le lancement va
    /// probablement échouer.
    pub broken: bool,
}

impl PackageDetail {
    /// Taxonomie précise (contrairement à `Package::kind`, qui ne voit pas le meta).
    pub fn kind(&self) -> Kind {
        if self.main_program.is_some() {
            Kind::App
        } else {
            Kind::KeepOnly
        }
    }

    pub fn is_runnable(&self) -> bool {
        self.kind() == Kind::App
    }
}

/// Récupère les métadonnées enrichies d'un paquet via `nix eval <attr>.meta --json`.
/// Retourne `None` si le paquet est introuvable ou l'évaluation échoue.
pub fn detail(attr: &str) -> Option<PackageDetail> {
    let output = Command::new("nix")
        .args([
            "eval",
            &format!("nixpkgs#{attr}.meta"),
            "--json",
            "--extra-experimental-features",
            "nix-command flakes",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let v: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    Some(PackageDetail {
        long_description: v.get("longDescription").and_then(|x| x.as_str()).map(str::to_string),
        homepage: v.get("homepage").and_then(first_string),
        license: v.get("license").map(license_str),
        is_free_license: v.get("license").and_then(license_is_free),
        changelog: v.get("changelog").and_then(|x| x.as_str()).map(str::to_string),
        main_program: v.get("mainProgram").and_then(|x| x.as_str()).map(str::to_string),
        maintainers: v
            .get("maintainers")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(maintainer_name).collect())
            .unwrap_or_default(),
        insecure: v.get("insecure").and_then(|x| x.as_bool()).unwrap_or(false),
        known_vulnerabilities: v
            .get("knownVulnerabilities")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        broken: v.get("broken").and_then(|x| x.as_bool()).unwrap_or(false),
    })
}

/// Nom affichable d'un mainteneur nixpkgs (`name`, sinon `@github`).
fn maintainer_name(v: &serde_json::Value) -> Option<String> {
    v.get("name")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .or_else(|| v.get("github").and_then(|x| x.as_str()).map(|g| format!("@{g}")))
}

/// Taille de téléchargement de la fermeture complète (le paquet + toutes ses
/// dépendances), interrogée directement sur le cache binaire `cache.nixos.org`
/// **sans rien télécharger ni construire** : 1) résout l'attr en store path pur
/// (`nix eval .outPath`, local, rapide), 2) interroge ce store path contre le
/// cache pour ses métadonnées NAR (`closureDownloadSize`). `None` si le paquet
/// n'est pas dans le cache binaire (nécessite alors une build source, taille
/// non prédictible sans la faire).
pub fn closure_size(attr: &str) -> Option<u64> {
    // Simple requête de taille en lecture seule : on contourne les portes
    // unfree/broken/insecure de check-meta.nix (elles bloquent `.outPath` mais
    // pas `.meta`, cf. `detail()`) — sinon la tuile Stockage reste "Inconnu"
    // pour Steam, Discord, VS Code et tout paquet non-libre.
    let out = Command::new("nix")
        .env("NIXPKGS_ALLOW_UNFREE", "1")
        .env("NIXPKGS_ALLOW_BROKEN", "1")
        .env("NIXPKGS_ALLOW_INSECURE", "1")
        .args([
            "eval",
            "--impure",
            "--raw",
            &format!("nixpkgs#{attr}.outPath"),
            "--extra-experimental-features",
            "nix-command flakes",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let store_path = String::from_utf8(out.stdout).ok()?;
    let store_path = store_path.trim();
    if store_path.is_empty() {
        return None;
    }

    let out2 = Command::new("nix")
        .args([
            "path-info",
            "--store",
            "https://cache.nixos.org",
            "--json",
            "-S",
            store_path,
            "--extra-experimental-features",
            "nix-command flakes",
        ])
        .output()
        .ok()?;
    if !out2.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out2.stdout).ok()?;
    v.get(store_path)?.get("closureDownloadSize")?.as_u64()
}

/// `homepage` peut être une chaîne ou une liste de chaînes → on prend la 1ère.
fn first_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(a) => a.iter().find_map(first_string),
        _ => None,
    }
}

/// `meta.license.free`, direct pour un objet ; pour une liste, libre seulement
/// si toutes les licences le sont. `None` si l'info n'est pas exposée (vieux
/// format string simple).
fn license_is_free(v: &serde_json::Value) -> Option<bool> {
    match v {
        serde_json::Value::Object(o) => o.get("free").and_then(|x| x.as_bool()),
        serde_json::Value::Array(a) if !a.is_empty() => {
            a.iter().map(license_is_free).collect::<Option<Vec<_>>>().map(|v| v.into_iter().all(|f| f))
        }
        _ => None,
    }
}

/// `license` peut être une chaîne, un objet `{fullName,…}`, ou une liste des deux.
fn license_str(v: &serde_json::Value) -> String {
    fn one(v: &serde_json::Value) -> Option<String> {
        match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(o) => o
                .get("fullName")
                .or_else(|| o.get("shortName"))
                .and_then(|x| x.as_str())
                .map(str::to_string),
            _ => None,
        }
    }
    match v {
        serde_json::Value::Array(a) => {
            a.iter().filter_map(one).collect::<Vec<_>>().join(", ")
        }
        other => one(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_string_variants() {
        assert_eq!(first_string(&serde_json::json!("https://a.com")), Some("https://a.com".into()));
        assert_eq!(
            first_string(&serde_json::json!(["https://a.com", "https://b.com"])),
            Some("https://a.com".into())
        );
        assert_eq!(
            first_string(&serde_json::json!([123, "https://b.com"])),
            Some("https://b.com".into())
        );
        assert_eq!(first_string(&serde_json::json!(42)), None);
    }

    #[test]
    fn maintainer_name_prefers_name_over_github() {
        let v = serde_json::json!({ "name": "Steffen Vogel", "github": "stv0g" });
        assert_eq!(maintainer_name(&v), Some("Steffen Vogel".to_string()));
    }

    #[test]
    fn maintainer_name_falls_back_to_github_handle() {
        let v = serde_json::json!({ "github": "stv0g" });
        assert_eq!(maintainer_name(&v), Some("@stv0g".to_string()));
    }

    #[test]
    fn maintainer_name_none_without_name_or_github() {
        let v = serde_json::json!({ "email": "post@example.com" });
        assert_eq!(maintainer_name(&v), None);
    }

    #[test]
    fn license_is_free_variants() {
        assert_eq!(license_is_free(&serde_json::json!({ "free": true })), Some(true));
        assert_eq!(license_is_free(&serde_json::json!({ "free": false })), Some(false));
        assert_eq!(license_is_free(&serde_json::json!("MIT")), None);
        assert_eq!(
            license_is_free(&serde_json::json!([{ "free": true }, { "free": true }])),
            Some(true)
        );
        assert_eq!(
            license_is_free(&serde_json::json!([{ "free": true }, { "free": false }])),
            Some(false)
        );
    }

    #[test]
    fn license_str_variants() {
        assert_eq!(license_str(&serde_json::json!("MIT")), "MIT");
        assert_eq!(
            license_str(&serde_json::json!({ "fullName": "GPLv3", "shortName": "GPL-3.0" })),
            "GPLv3"
        );
        assert_eq!(license_str(&serde_json::json!({ "shortName": "MIT" })), "MIT");
        assert_eq!(
            license_str(&serde_json::json!(["MIT", { "fullName": "Apache-2.0" }])),
            "MIT, Apache-2.0"
        );
    }
}
