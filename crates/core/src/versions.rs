//! Earlier versions of a package and the nixpkgs revision that shipped each,
//! from the Devbox package index (search.devbox.sh, behind nixhub.io).

/// A version of a package, as one nixpkgs revision shipped it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionChoice {
    pub version: String,
    pub rev: String,
    /// Day of the revision, `YYYY-MM-DD`.
    pub date: String,
    /// True when cache.nixos.org has it; else it builds locally.
    pub cached: bool,
}

/// Versions of `attr` for this machine, newest first. Network: blocking.
pub fn versions(attr: &str) -> Vec<VersionChoice> {
    let name: String = attr
        .bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    let url = format!("https://search.devbox.sh/v2/pkg?name={name}");
    let Some(v) = crate::http::get_json(&url) else {
        return Vec::new();
    };
    let system = format!("{}-linux", std::env::consts::ARCH);
    parse_versions(&v, attr, &system)
        .into_iter()
        .map(|(mut choice, out)| {
            choice.cached = in_binary_cache(&out);
            choice
        })
        .collect()
}

/// (choice, store path) per release of `attr` on `system`.
fn parse_versions(v: &serde_json::Value, attr: &str, system: &str) -> Vec<(VersionChoice, String)> {
    let releases = v["releases"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    releases
        .iter()
        .filter_map(|r| {
            let p = r["platforms"].as_array()?.iter().find(|p| {
                p["system"].as_str() == Some(system) && p["attribute_path"].as_str() == Some(attr)
            })?;
            let out = p["outputs"]
                .as_array()?
                .iter()
                .find(|o| o["name"].as_str() == Some("out"))?["path"]
                .as_str()?
                .to_string();
            let rev = p["commit_hash"].as_str().filter(|r| is_rev(r))?;
            let choice = VersionChoice {
                version: r["version"].as_str()?.to_string(),
                rev: rev.to_string(),
                date: p["date"].as_str()?.get(..10)?.to_string(),
                cached: false,
            };
            Some((choice, out))
        })
        .collect()
}

/// True for a full nixpkgs commit hash: 40 lowercase hex digits.
pub(crate) fn is_rev(rev: &str) -> bool {
    rev.len() == 40
        && rev
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// True when cache.nixos.org has the store path `out`.
fn in_binary_cache(out: &str) -> bool {
    let Some(hash) = out.strip_prefix("/nix/store/").and_then(|n| n.get(..32)) else {
        return false;
    };
    ureq::head(&format!("https://cache.nixos.org/{hash}.narinfo"))
        .timeout(crate::http::TIMEOUT)
        .call()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_releases_of_this_system() {
        let v = serde_json::json!({ "releases": [
            { "version": "3.0.21", "platforms": [
                { "system": "aarch64-linux", "attribute_path": "vlc", "commit_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                  "date": "2025-11-23T00:00:00Z", "outputs": [{ "name": "out", "path": "/nix/store/x-vlc" }] },
                { "system": "x86_64-linux", "attribute_path": "vlc", "commit_hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                  "date": "2025-11-23T00:00:00Z",
                  "outputs": [{ "name": "man", "path": "/nix/store/m" }, { "name": "out", "path": "/nix/store/o" }] }
            ]},
            { "version": "1.0", "platforms": [
                { "system": "x86_64-linux", "attribute_path": "vlc-other", "commit_hash": "cccccccccccccccccccccccccccccccccccccccc",
                  "date": "2020-01-01T00:00:00Z", "outputs": [{ "name": "out", "path": "/nix/store/z" }] }
            ]}
        ]});
        let found = parse_versions(&v, "vlc", "x86_64-linux");
        assert_eq!(found.len(), 1, "other systems and attrs are skipped");
        assert_eq!(found[0].0.version, "3.0.21");
        assert_eq!(found[0].0.rev, "b".repeat(40));
        assert_eq!(found[0].0.date, "2025-11-23");
        assert_eq!(found[0].1, "/nix/store/o");
    }

    #[test]
    fn rejects_revisions_that_are_not_commit_hashes() {
        assert!(is_rev(&"0123456789abcdef".repeat(3)[..40]));
        assert!(!is_rev("abc"));
        assert!(!is_rev(&format!("{}\"; x", "a".repeat(37))));
        let v = serde_json::json!({ "releases": [{ "version": "1", "platforms": [
            { "system": "x86_64-linux", "attribute_path": "vlc",
              "commit_hash": "\"; builtins.exec [ ] ; \"", "date": "2025-01-01T00:00:00Z",
              "outputs": [{ "name": "out", "path": "/nix/store/o" }] }
        ]}]});
        assert!(parse_versions(&v, "vlc", "x86_64-linux").is_empty());
    }
}
