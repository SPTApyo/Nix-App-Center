//! Package metadata from `nix eval <attr>.meta`.

use crate::package::Kind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackageDetail {
    pub long_description: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    /// `meta.license.free`.
    pub is_free_license: Option<bool>,
    pub changelog: Option<String>,
    /// `meta.mainProgram`: set for a runnable package.
    pub main_program: Option<String>,
    /// nixpkgs maintainers, by name or GitHub handle.
    pub maintainers: Vec<String>,
    /// `meta.insecure`: a known unfixed vulnerability.
    pub insecure: bool,
    /// `meta.knownVulnerabilities`: CVE ids, if any.
    pub known_vulnerabilities: Vec<String>,
    /// `meta.broken`: building, so running, will fail.
    pub broken: bool,
    /// Package version, read next to the meta.
    pub version: Option<String>,
    /// `meta.position`: the Nix file defining the package, `file:line`.
    pub position: Option<String>,
    /// `meta.teams[].shortName`: teams maintaining the package.
    pub teams: Vec<String>,
    pub download_page: Option<String>,
}

impl PackageDetail {
    /// Kind of the package, from `mainProgram`.
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

/// The meta and version of `attr`; `None` when evaluation fails.
pub fn detail(pkg: &crate::Package) -> Option<PackageDetail> {
    let output = crate::util::nix()
        .args([
            "eval",
            &crate::installable(pkg),
            "--apply",
            "p: p.meta // { version = p.version or null; }",
            "--json",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let v: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    Some(PackageDetail {
        long_description: v
            .get("longDescription")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        homepage: v.get("homepage").and_then(first_string),
        license: v.get("license").map(license_str),
        is_free_license: v.get("license").and_then(license_is_free),
        changelog: v
            .get("changelog")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        main_program: v
            .get("mainProgram")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        maintainers: v
            .get("maintainers")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(maintainer_name).collect())
            .unwrap_or_default(),
        insecure: v.get("insecure").and_then(|x| x.as_bool()).unwrap_or(false),
        known_vulnerabilities: v
            .get("knownVulnerabilities")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        broken: v.get("broken").and_then(|x| x.as_bool()).unwrap_or(false),
        version: v
            .get("version")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        position: v
            .get("position")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        teams: v
            .get("teams")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.get("shortName").and_then(|n| n.as_str()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        download_page: v
            .get("downloadPage")
            .and_then(|x| x.as_str())
            .map(str::to_string),
    })
}

/// Display name of a nixpkgs maintainer (`name`, else `@github`).
fn maintainer_name(v: &serde_json::Value) -> Option<String> {
    v.get("name")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .or_else(|| {
            v.get("github")
                .and_then(|x| x.as_str())
                .map(|g| format!("@{g}"))
        })
}

/// Download size of the package and its dependencies, asked to the binary
/// cache without building. `None` when the cache does not have it.
pub fn closure_size(pkg: &crate::Package) -> Option<u64> {
    // Read-only: unfree and broken packages need sizes too.
    let out = crate::util::nix()
        .env("NIXPKGS_ALLOW_UNFREE", "1")
        .env("NIXPKGS_ALLOW_BROKEN", "1")
        .env("NIXPKGS_ALLOW_INSECURE", "1")
        .args([
            "eval",
            "--impure",
            "--raw",
            &format!("{}.outPath", crate::installable(pkg)),
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

    let out2 = crate::util::nix()
        .args([
            "path-info",
            "--store",
            "https://cache.nixos.org",
            "--json",
            "-S",
            store_path,
        ])
        .output()
        .ok()?;
    if !out2.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out2.stdout).ok()?;
    v.get(store_path)?.get("closureDownloadSize")?.as_u64()
}

/// The first string of a string or a list.
fn first_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(a) => a.iter().find_map(first_string),
        _ => None,
    }
}

/// True when every license is free; `None` when unknown.
fn license_is_free(v: &serde_json::Value) -> Option<bool> {
    match v {
        serde_json::Value::Object(o) => o.get("free").and_then(|x| x.as_bool()),
        serde_json::Value::Array(a) if !a.is_empty() => a
            .iter()
            .map(license_is_free)
            .collect::<Option<Vec<_>>>()
            .map(|v| v.into_iter().all(|f| f)),
        _ => None,
    }
}

/// License name from a string, an object or a list.
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
        serde_json::Value::Array(a) => a.iter().filter_map(one).collect::<Vec<_>>().join(", "),
        other => one(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_string_variants() {
        assert_eq!(
            first_string(&serde_json::json!("https://a.com")),
            Some("https://a.com".into())
        );
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
        assert_eq!(
            license_is_free(&serde_json::json!({ "free": true })),
            Some(true)
        );
        assert_eq!(
            license_is_free(&serde_json::json!({ "free": false })),
            Some(false)
        );
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
        assert_eq!(
            license_str(&serde_json::json!({ "shortName": "MIT" })),
            "MIT"
        );
        assert_eq!(
            license_str(&serde_json::json!(["MIT", { "fullName": "Apache-2.0" }])),
            "MIT, Apache-2.0"
        );
    }
}
