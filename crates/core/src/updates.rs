//! Détection des mises à jour : version gardée vs version courante de nixpkgs.

use crate::categories::nix_attr_lookup;
use crate::{load_manifest, Package};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Update {
    pub package: Package,
    pub installed_version: String,
    pub available_version: String,
}

/// Compare deux versions « façon Nix » par segments numériques (point/tiret-séparés).
fn version_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .map(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
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

/// Cœur pur de la détection (testable sans `nix`). Un paquet gardé sans
/// version stockée (ex. `onyx-cli keep`) est ignoré : impossible de comparer,
/// et le traiter comme « 0 » l'affichait éternellement comme à mettre à jour.
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

/// Pour chaque paquet « gardé », compare sa version stockée à celle de nixpkgs —
/// en UN `nix search` groupé (alternation ancrée, comme `categories`), servi par
/// le cache disque, au lieu d'un appel par paquet, séquentiel et jamais caché.
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
        // Paquet retiré de nixpkgs : absent du lookup → pas de fausse update.
        let ups = compute_updates(vec![kept("ghost", "1.0")], &HashMap::new());
        assert!(ups.is_empty());
    }

    #[test]
    fn compute_updates_skips_kept_without_version() {
        // Régression : `onyx-cli keep <attr>` stocke une version vide ;
        // version_newer("x.y", "") est vrai → update éternelle affichée.
        let cur = current(&[("ripgrep", "14.1.0")]);
        let ups = compute_updates(vec![kept("ripgrep", "")], &cur);
        assert!(ups.is_empty(), "version stockée vide = comparaison impossible");
    }

    #[test]
    fn compute_updates_matches_attr_case_insensitively() {
        // Le lookup est indexé en minuscules ; un attr gardé avec majuscules
        // (`superTux`) doit quand même être résolu.
        let cur = current(&[("supertux", "0.7")]);
        let ups = compute_updates(vec![kept("superTux", "0.6")], &cur);
        assert_eq!(ups.len(), 1);
    }
}
