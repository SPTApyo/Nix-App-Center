//! Update detection: kept version vs current nixpkgs version.

use crate::categories::nix_attr_lookup;
use crate::{load_manifest, Package};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Update {
    pub package: Package,
    pub installed_version: String,
    pub available_version: String,
}

/// Compares two versions "the Nix way" by numeric segments (dot/dash separated).
fn version_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .map(|s| {
                s.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
            })
            .map(|s| s.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(candidate), parse(current));
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// Pure core of the detection (testable without `nix`). A kept package without
/// a stored version (e.g. `onyx-cli keep`) is skipped: it cannot be compared,
/// and treating it as "0" showed it as outdated forever.
fn compute_updates(kept: Vec<Package>, current: &HashMap<String, Package>) -> Vec<Update> {
    kept.into_iter()
        .filter_map(|p| {
            if p.version.is_empty() {
                return None;
            }
            let available = current.get(&p.attr.to_lowercase())?.version.clone();
            if version_newer(&available, &p.version) {
                Some(Update {
                    available_version: available,
                    installed_version: p.version.clone(),
                    package: p,
                })
            } else {
                None
            }
        })
        .collect()
}

/// For every kept package, compares its stored version with nixpkgs', in ONE
/// grouped `nix search` (anchored alternation, like `categories`) served by the
/// disk cache, instead of one sequential, never cached call per package.
pub fn available_updates() -> Vec<Update> {
    let kept = load_manifest().packages();
    if kept.is_empty() {
        return vec![];
    }
    let attrs: Vec<String> = kept.iter().map(|p| p.attr.to_lowercase()).collect();
    compute_updates(kept, &nix_attr_lookup(&attrs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_numeric_segments() {
        assert!(version_newer("1.2.0", "1.1.9"));
        assert!(version_newer("2.0", "1.99"));
        assert!(!version_newer("1.0", "1.0"));
        assert!(!version_newer("1.0", "1.1"));
    }

    #[test]
    fn handles_unequal_length() {
        assert!(version_newer("1.2.1", "1.2"));
        assert!(!version_newer("1.2", "1.2.1"));
    }

    fn kept(attr: &str, version: &str) -> Package {
        let mut p = Package::from_attr(attr);
        p.version = version.to_string();
        p
    }

    fn current(entries: &[(&str, &str)]) -> HashMap<String, Package> {
        entries
            .iter()
            .map(|(attr, v)| ((*attr).to_lowercase(), kept(attr, v)))
            .collect()
    }

    #[test]
    fn compute_updates_reports_newer_only() {
        let cur = current(&[("firefox", "2.0"), ("btop", "1.0")]);
        let ups = compute_updates(vec![kept("firefox", "1.0"), kept("btop", "1.0")], &cur);
        assert_eq!(ups.len(), 1);
        assert_eq!(ups[0].package.attr, "firefox");
        assert_eq!(ups[0].available_version, "2.0");
        assert_eq!(ups[0].installed_version, "1.0");
    }

    #[test]
    fn compute_updates_skips_unknown_attr() {
        // Package removed from nixpkgs: missing from the lookup, no false update.
        let ups = compute_updates(vec![kept("ghost", "1.0")], &HashMap::new());
        assert!(ups.is_empty());
    }

    #[test]
    fn compute_updates_skips_kept_without_version() {
        // Regression: `onyx-cli keep <attr>` stores an empty version;
        // version_newer("x.y", "") is true, so an eternal update was shown.
        let cur = current(&[("ripgrep", "14.1.0")]);
        let ups = compute_updates(vec![kept("ripgrep", "")], &cur);
        assert!(
            ups.is_empty(),
            "empty stored version = no comparison possible"
        );
    }

    #[test]
    fn compute_updates_matches_attr_case_insensitively() {
        // The lookup is indexed lowercase; a kept attr with capitals
        // (`superTux`) must still resolve.
        let cur = current(&[("supertux", "0.7")]);
        let ups = compute_updates(vec![kept("superTux", "0.6")], &cur);
        assert_eq!(ups.len(), 1);
    }
}
