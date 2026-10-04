//! Rich metadata through `nix eval <attr>.meta --json` (fast, local, rich,
//! but no icons/screenshots: see `flathub` for those).
//!
//! NB: `nix search` (list) does NOT carry `mainProgram`, so enrichment happens
//! on demand (when the details page opens), not for the 100 results.

use crate::package::Kind;
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackageDetail {
    pub long_description: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    /// `meta.license.free`, already computed by nixpkgs and reliable (unlike
    /// Flathub, which only provides it through its REST API, never locally).
    pub is_free_license: Option<bool>,
    pub changelog: Option<String>,
    /// Main binary (`meta.mainProgram`): its presence means a runnable package.
    pub main_program: Option<String>,
    /// Names of nixpkgs maintainers (`meta.maintainers[].name`, or `.github`
    /// as fallback). A Nix native trust signal, no AppStream equivalent.
    pub maintainers: Vec<String>,
    /// `meta.insecure`: the package is marked with a known unfixed
    /// vulnerability (needs `NIXPKGS_ALLOW_INSECURE=1` to build).
    pub insecure: bool,
    /// `meta.knownVulnerabilities`: CVE ids, if any.
    pub known_vulnerabilities: Vec<String>,
    /// `meta.broken`: nix refuses to evaluate `outPath`/`drvPath` for this
    /// package (so `nix run` AND `closure_size()` will fail). Unlike
    /// `insecure`, `.meta` stays readable even for a broken package: it is the
    /// only reliable, free signal explaining why running will probably fail.
    pub broken: bool,
}

impl PackageDetail {
    /// Precise taxonomy (unlike `Package::kind`, which cannot see the meta).
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

/// Fetches a package's rich metadata through `nix eval <attr>.meta --json`.
/// Returns `None` when the package is not found or evaluation fails.
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

/// Display name of a nixpkgs maintainer (`name`, else `@github`).
fn maintainer_name(v: &serde_json::Value) -> Option<String> {
    v.get("name")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .or_else(|| v.get("github").and_then(|x| x.as_str()).map(|g| format!("@{g}")))
}

/// Download size of the full closure (the package + all its dependencies),
/// queried directly against the `cache.nixos.org` binary cache **without
/// downloading or building anything**: 1) resolve the attr to a pure store
/// path (`nix eval .outPath`, local, fast), 2) query that store path against
/// the cache for its NAR metadata (`closureDownloadSize`). `None` when the
/// package is not in the binary cache (a source build is then needed, size
/// not predictable without doing it).
pub fn closure_size(attr: &str) -> Option<u64> {
    // Read-only size query: bypass the unfree/broken/insecure gates of
    // check-meta.nix (they block `.outPath` but not `.meta`, see `detail()`),
    // otherwise the Storage tile stays "Unknown" for Steam, Discord, VS Code
    // and every non-free package.
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

/// `homepage` may be a string or a list of strings; take the first.
fn first_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(a) => a.iter().find_map(first_string),
        _ => None,
    }
}

/// `meta.license.free`, direct for an object; for a list, free only when every
/// license is. `None` when the info is not exposed (old plain string format).
fn license_is_free(v: &serde_json::Value) -> Option<bool> {
    match v {
        serde_json::Value::Object(o) => o.get("free").and_then(|x| x.as_bool()),
        serde_json::Value::Array(a) if !a.is_empty() => {
            a.iter().map(license_is_free).collect::<Option<Vec<_>>>().map(|v| v.into_iter().all(|f| f))
        }
        _ => None,
    }
}

/// `license` may be a string, a `{fullName,…}` object, or a list of both.
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
