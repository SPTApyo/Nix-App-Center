//! nixpkgs search ranked by relevance, enriched with AppStream icons.

use crate::appstream_db;
use crate::flathub::flathub_icon_map;
use crate::package::Package;
use crate::ratings::{odrs_ratings, rating_for, Rating};
use crate::util::{regex_escape, sanitize};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub(crate) struct NixPkgEntry {
    pub(crate) pname: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) description: Option<String>,
}

/// How long raw `nix search` results stay cached: a cold search takes minutes.
pub(crate) const SEARCH_CACHE_TTL_SECS: u64 = 3600;

/// Raw `nix search` results as packages; empty when `nix` fails.
pub(crate) fn raw_nix_search(patterns: &[String]) -> Vec<Package> {
    let mut args: Vec<String> = vec!["search".into(), crate::nixpkgs_ref().into()];
    args.extend(patterns.iter().cloned());
    args.push("--json".into());

    let output = match crate::util::nix().args(&args).output() {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let raw: HashMap<String, NixPkgEntry> = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    // Keep the attr path after `legacyPackages.<system>.`.
    raw.into_iter()
        .map(|(attr, info)| {
            let short = attr.splitn(3, '.').nth(2).unwrap_or(&attr).to_string();
            Package {
                name: info.pname.unwrap_or_else(|| short.clone()),
                version: info.version.unwrap_or_default(),
                description: info.description.unwrap_or_default(),
                attr: short,
            }
        })
        .collect()
}

/// nixpkgs test derivations, not installable apps.
fn is_internal(attr: &str) -> bool {
    attr.starts_with("tests.")
}

/// nixpkgs packages matching every word of `query`, best first: exact
/// name, then prefix, then contains, then description.
pub fn search(query: &str) -> Vec<Package> {
    let query = query.trim();
    if query.is_empty() {
        return vec![];
    }

    let tokens: Vec<String> = query.split_whitespace().map(regex_escape).collect();

    let packages: Vec<Package> =
        crate::cache::cached_search(query, SEARCH_CACHE_TTL_SECS, || raw_nix_search(&tokens))
            .into_iter()
            .filter(|p| !is_internal(&p.attr))
            .collect();

    // Merge case aliases such as `superTux` and `supertux`.
    let mut canon: HashMap<String, Package> = HashMap::new();
    for p in packages {
        let key = p.attr.to_lowercase();
        match canon.get_mut(&key) {
            None => {
                canon.insert(key, p);
            }
            Some(existing) => {
                // Display the capitalized variant.
                let p_upper = p.attr.chars().any(char::is_uppercase);
                let ex_upper = existing.attr.chars().any(char::is_uppercase);
                if p_upper {
                    existing.name = beautify_attr(&p.attr);
                } else if ex_upper {
                    let a = existing.attr.clone();
                    existing.name = beautify_attr(&a);
                }
                // Run the lowercase attr.
                if p.attr == key {
                    existing.attr = p.attr;
                }
            }
        }
    }
    let mut packages: Vec<Package> = canon.into_values().collect();

    let qlow = query.to_lowercase();
    let toks: Vec<String> = query.split_whitespace().map(|t| t.to_lowercase()).collect();
    packages.sort_by(|a, b| {
        search_score(b, &qlow, &toks)
            .cmp(&search_score(a, &qlow, &toks))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    packages.truncate(100);
    packages
}

/// A search result: the package and its AppStream identity, if any.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub package: Package,
    pub icon_url: Option<String>,
    pub rating: Option<Rating>,
    pub recognized: bool,
}

/// `search` results with their AppStream icon and rating, from the local
/// database, else from one Flathub query.
pub fn search_apps(query: &str) -> Vec<SearchResult> {
    let rate_handle = std::thread::spawn(odrs_ratings);
    let db_ready = appstream_db::is_ready();

    // Fetch Flathub while `nix search` runs.
    let icon_handle = (!db_ready).then(|| {
        let q = query.to_string();
        std::thread::spawn(move || flathub_icon_map(&q))
    });

    let pkgs = search(query);

    let hits = match icon_handle {
        Some(h) => h.join().unwrap_or_default(),
        None if pkgs
            .iter()
            .any(|p| appstream_db::lookup_package(p).is_none()) =>
        {
            flathub_icon_map(query)
        }
        None => HashMap::new(),
    };

    let _ = rate_handle.join();

    // Variants share one entry; best ranked wins.
    let mut claimed = std::collections::HashSet::new();
    pkgs.into_iter()
        .map(|mut p| {
            if let Some(e) = appstream_db::lookup_package(&p) {
                if !claimed.insert(e.app_id.clone()) {
                    return variant(p, e.icon_url.clone());
                }
                if !e.name.is_empty() {
                    p.name = e.name.clone();
                }
                if let Some(summary) = &e.local_summary {
                    p.description = summary.clone();
                }
                return SearchResult {
                    icon_url: e.icon_url.clone(),
                    rating: rating_for(&e.app_id),
                    recognized: true,
                    package: p,
                };
            }
            let hit = hits
                .get(&p.attr.to_lowercase())
                .or_else(|| hits.get(&sanitize(&p.name)))
                .filter(|h| {
                    !crate::resolve::is_unmatched(&p.attr)
                        && crate::resolve::attr_for_app_id(&h.app_id)
                            .is_none_or(|a| a.eq_ignore_ascii_case(&p.attr))
                });
            if let Some(h) = hit.filter(|h| !claimed.insert(h.app_id.clone())) {
                return variant(p, Some(h.icon.clone()));
            }
            let icon_url = hit.map(|h| h.icon.clone());
            let rating = hit.and_then(|h| rating_for(&h.app_id));
            let recognized = hit.is_some();
            match hit {
                Some(h) if !h.name.trim().is_empty() => p.name = h.name.clone(),
                Some(_) => {}
                None => p = named_by_attr(p),
            }
            SearchResult {
                package: p,
                icon_url,
                rating,
                recognized,
            }
        })
        .collect()
}

/// A secondary package of an app already listed.
fn variant(package: Package, icon_url: Option<String>) -> SearchResult {
    SearchResult {
        package: named_by_attr(package),
        icon_url,
        rating: None,
        recognized: false,
    }
}

/// Names a package by its attr, unique unlike `pname`.
fn named_by_attr(mut p: Package) -> Package {
    if !p.name.eq_ignore_ascii_case(&p.attr) {
        p.name = p.attr.clone();
    }
    p
}

/// Relevance of `pkg` for the query `q` split into `toks`.
fn search_score(pkg: &Package, q: &str, toks: &[String]) -> i64 {
    let attr = pkg.attr.to_lowercase();
    let name = pkg.name.to_lowercase();
    let desc = pkg.description.to_lowercase();
    let mut s: i64 = 0;

    if attr == q {
        s += 1000; // exact match
    } else if name == q {
        s += 800; // exact pname in a package set
    } else if attr.starts_with(q) || name.starts_with(q) {
        s += 600; // starts with
    } else if attr.contains(q) || name.contains(q) {
        s += 300; // contains (name)
    }

    if toks
        .iter()
        .all(|t| attr.contains(t.as_str()) || name.contains(t.as_str()))
    {
        s += 150;
    }
    if toks.iter().all(|t| desc.contains(t.as_str())) {
        s += 30;
    }

    // Shorter names rank higher.
    s -= (attr.chars().count() as i64).min(60);
    // Top-level before package sets.
    if attr.contains('.') {
        s -= 40;
    }
    s
}

/// Display name of an attr: first letter capitalized (`SuperTux`).
fn beautify_attr(attr: &str) -> String {
    let mut chars = attr.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(q: &str) -> Vec<String> {
        q.split_whitespace().map(|s| s.to_lowercase()).collect()
    }

    // ── search_score ──────────────────────────────────────────────────────

    #[test]
    fn search_score_exact_beats_prefix() {
        let exact = Package::from_attr("firefox");
        let prefix = Package::from_attr("firefox-esr");
        let t = toks("firefox");
        assert!(search_score(&exact, "firefox", &t) > search_score(&prefix, "firefox", &t));
    }

    #[test]
    fn search_score_prefix_beats_contains() {
        let starts = Package::from_attr("firefoxy");
        let contains = Package::from_attr("old-firefox");
        let t = toks("firefox");
        assert!(search_score(&starts, "firefox", &t) > search_score(&contains, "firefox", &t));
    }

    #[test]
    fn search_score_length_penalty_breaks_ties() {
        // The shorter contained match wins.
        let short = Package::from_attr("xfirefox");
        let long = Package::from_attr("xfirefoxlong");
        let t = toks("firefox");
        assert!(search_score(&short, "firefox", &t) > search_score(&long, "firefox", &t));
    }

    #[test]
    fn search_score_name_token_beats_description_only_token() {
        let mut in_name = Package::from_attr("texteditor");
        in_name.description = String::new();
        let mut in_desc = Package::from_attr("gedit");
        in_desc.description = "a simple text editor".to_string();
        let t = toks("editor");
        assert!(search_score(&in_name, "editor", &t) > search_score(&in_desc, "editor", &t));
    }

    #[test]
    fn named_by_attr_disambiguates_shared_pname() {
        let mut p = Package::from_attr("gimp3Plugins.bimp");
        p.name = "gimp-plugin-bimp".into();
        assert_eq!(named_by_attr(p).name, "gimp3Plugins.bimp");

        let mut p = Package::from_attr("superTux");
        p.name = "SuperTux".into();
        assert_eq!(
            named_by_attr(p).name,
            "SuperTux",
            "beautified case alias is kept"
        );
    }

    #[test]
    fn search_score_top_level_beats_package_set_with_same_pname() {
        let top = Package::from_attr("hello");
        let mut set = Package::from_attr("haskellPackages.hello");
        set.name = "hello".into();
        set.description = "Hello World, an example package".into();
        let t = toks("hello");
        assert!(search_score(&top, "hello", &t) > search_score(&set, "hello", &t));
    }

    #[test]
    fn nixpkgs_tests_are_internal() {
        assert!(is_internal("tests.haskell.cabalSdist.helloFromCabalSdist"));
        assert!(!is_internal("hello"));
        assert!(!is_internal("python3Packages.pytest"));
    }

    // ── beautify_attr ─────────────────────────────────────────────────────

    #[test]
    fn beautify_attr_capitalizes_first_letter_only() {
        assert_eq!(beautify_attr("superTux"), "SuperTux");
        assert_eq!(beautify_attr("superTuxKart"), "SuperTuxKart");
        assert_eq!(beautify_attr("firefox"), "Firefox");
    }

    #[test]
    fn beautify_attr_empty_is_empty() {
        assert_eq!(beautify_attr(""), "");
    }
}
