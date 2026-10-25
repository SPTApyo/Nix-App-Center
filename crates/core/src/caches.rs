//! Binary caches that added flakes declare in their nixConfig, trusted only
//! with the user's consent.

use serde_json::Value;

/// A binary cache: its server and the keys that sign it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cache {
    pub url: String,
    pub keys: Vec<String>,
}

/// The cache the flake of `source` (an input name or a locked reference)
/// declares, if any. Blocking.
pub fn flake_cache(source: &str) -> Option<Cache> {
    let reference = crate::config_layout::flake_sources()
        .into_iter()
        .find(|(name, _)| name == source)?
        .1;
    // Pure mode: flake.nix cannot read files or environment.
    if !safe(&reference, ":/.-_?=&+~@") {
        return None;
    }
    let expr = format!(
        "(import ((builtins.getFlake \"{reference}\").outPath + \"/flake.nix\")).nixConfig or {{ }}"
    );
    let out = crate::util::nix()
        .args(["eval", "--json", "--expr", &expr])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    parse(&serde_json::from_slice(&out.stdout).ok()?)
}

/// True when Nix already uses `url`, or Nix App Center will. Blocking.
pub fn cache_trusted(url: &str) -> bool {
    let same = |u: &str| u.trim_end_matches('/') == url.trim_end_matches('/');
    let used = crate::util::nix()
        .args(["config", "show", "substituters"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .any(same)
        })
        .unwrap_or(false);
    used || crate::manifest::load_manifest()
        .kept
        .iter()
        .any(|e| e.option && e.package.attr.strip_prefix("cache:").is_some_and(same))
}

/// The first cache of a nixConfig, with its keys. Refuses anything that
/// is not plain https and plain keys, since it ends up in Nix code.
fn parse(config: &Value) -> Option<Cache> {
    let list = |names: [&str; 2]| -> Vec<String> {
        names
            .iter()
            .filter_map(|n| config.get(n))
            .flat_map(|v| match v {
                Value::String(s) => s.split_whitespace().map(str::to_string).collect(),
                Value::Array(a) => a
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect(),
                _ => Vec::new(),
            })
            .collect()
    };
    let url = list(["extra-substituters", "substituters"])
        .into_iter()
        .find(|u| {
            u.strip_prefix("https://")
                .is_some_and(|rest| safe(rest, "./-_:"))
        })?;
    let keys: Vec<String> = list(["extra-trusted-public-keys", "trusted-public-keys"])
        .into_iter()
        .filter(|k| safe(k, ".-_:+/="))
        .collect();
    (!keys.is_empty()).then_some(Cache { url, keys })
}

fn safe(s: &str, extra: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_cache_of_a_nix_config() {
        let config = serde_json::json!({
            "allowInsecure": true,
            "extra-substituters": ["https://nix-gaming.cachix.org"],
            "extra-trusted-public-keys": "nix-gaming.cachix.org-1:nbjl/f3h+Ees8vCUUs4="
        });
        assert_eq!(
            parse(&config),
            Some(Cache {
                url: "https://nix-gaming.cachix.org".into(),
                keys: vec!["nix-gaming.cachix.org-1:nbjl/f3h+Ees8vCUUs4=".into()],
            })
        );
    }

    #[test]
    fn refuses_what_is_not_plain() {
        let config = serde_json::json!({
            "substituters": ["http://plain.example", "https://a\"; evil = \"b"],
            "trusted-public-keys": ["k:1="]
        });
        assert_eq!(parse(&config), None);
        let no_key = serde_json::json!({ "substituters": "https://c.example" });
        assert_eq!(parse(&no_key), None);
    }
}
