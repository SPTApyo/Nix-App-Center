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

/// TTL of the disk cache of raw `nix search` results (`cache::cached_search`).
/// `nix search` evaluates all of nixpkgs when cold (30 s to 2 min depending on
/// the machine); nixpkgs does not change fast enough for second-level
/// freshness to matter, so this cost is paid once an hour rather than on
/// every app start.
pub(crate) const SEARCH_CACHE_TTL_SECS: u64 = 3600;

/// Runs `nix search nixpkgs <patterns...> --json` and turns each entry into a
/// `Package` (short attr, default fields when missing). `[]` when `nix` fails
/// or is missing, or the JSON is invalid: never panics on a machine without
/// `nix` or with unexpected output.
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

    // The nix attr is "legacyPackages.x86_64-linux.<attrpath>"; keep <attrpath>.
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

/// nixpkgs' own test derivations (`tests.haskell.…`) are not installable apps.
fn is_internal(attr: &str) -> bool {
    attr.starts_with("tests.")
}

/// Searches nixpkgs packages, ranked by relevance like GNOME Software.
///
/// 1. **Every token must match** (each word passed separately to
///    `nix search`, which ANDs them).
/// 2. Results are **scored per field**: exact name/attr ≫ starts with ≫
///    contains ≫ description, with a slight length penalty (favoring short,
///    exact names). This brings the wanted package to the top.
///
/// The raw `nix search` call is disk cached (`cache::cached_search`, TTL
/// [`SEARCH_CACHE_TTL_SECS`]): ranking and dedup below always rerun, only the
/// nixpkgs evaluation cost is avoided.
pub fn search(query: &str) -> Vec<Package> {
    let query = query.trim();
    if query.is_empty() {
        return vec![];
    }

    // One argument per token (AND), each escaped for nix's regex engine.
    let tokens: Vec<String> = query.split_whitespace().map(regex_escape).collect();

    let packages: Vec<Package> =
        crate::cache::cached_search(query, SEARCH_CACHE_TTL_SECS, || raw_nix_search(&tokens))
            .into_iter()
            .filter(|p| !is_internal(&p.attr))
            .collect();

    // nixpkgs exposes CASE aliases (`superTux` ↔ `supertux`, `superTuxKart` ↔
    // `supertuxkart`): two attrs for the SAME package (same pname/version/
    // description). `nix search` returns both, so duplicates show up. Attrs
    // equal up to case are merged, keeping the canonical form (all lowercase,
    // usually = pname).
    let mut canon: HashMap<String, Package> = HashMap::new();
    for p in packages {
        let key = p.attr.to_lowercase();
        match canon.get_mut(&key) {
            None => {
                canon.insert(key, p);
            }
            Some(existing) => {
                // DISPLAYED name: prefer the attr variant WITH capitals
                // (`superTux` → "SuperTux"); the bare `pname` is all lowercase.
                let p_upper = p.attr.chars().any(char::is_uppercase);
                let ex_upper = existing.attr.chars().any(char::is_uppercase);
                if p_upper {
                    existing.name = beautify_attr(&p.attr);
                } else if ex_upper {
                    let a = existing.attr.clone();
                    existing.name = beautify_attr(&a);
                }
                // Attr to RUN: prefer the real lowercase form
                // (`nix run nixpkgs#supertux`, the safest canonical alias).
                if p.attr == key {
                    existing.attr = p.attr;
                }
            }
        }
    }
    let mut packages: Vec<Package> = canon.into_values().collect();

    // Relevance ranking (decreasing score, then name).
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

/// A `search_apps` result: the nix package + optional AppStream dressing.
/// `recognized` tells "matched to an AppStream entry" apart from "has an icon":
/// a recognized app without icon is still an application (official name, ODRS
/// rating) and must not fall into "Other Packages".
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub package: Package,
    pub icon_url: Option<String>,
    pub rating: Option<Rating>,
    pub recognized: bool,
}

/// Enriched search: ranked nix results + **AppStream icon** when the app is
/// recognized (local database when ready, else one Flathub `/search` call per
/// query, mapped to nix packages by name/attr).
///
/// The local database may not know the WHOLE Flathub catalog (stale cache):
/// results it does not recognize fall back, one by one, on the same REST call
/// as when the database is not ready at all, without any network call when
/// the local database already resolved every result (common case, free).
pub fn search_apps(query: &str) -> Vec<SearchResult> {
    let rate_handle = std::thread::spawn(odrs_ratings);
    let db_ready = appstream_db::is_ready();

    // Database not ready: the network is needed anyway, so start it in
    // parallel with `search()`.
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

    // Variants (`gimp`, `gimp3`, `gimp-with-plugins`) share one AppStream entry:
    // only the best ranked one takes the app identity, the rest stay packages.
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

/// A secondary package of an already listed app.
fn variant(package: Package, icon_url: Option<String>) -> SearchResult {
    SearchResult {
        package: named_by_attr(package),
        icon_url,
        rating: None,
        recognized: false,
    }
}

/// Packages without app identity are named by attr: several attrs share one
/// `pname` (`gimpPlugins.bimp`, `gimp3Plugins.bimp`), the attr is unique.
fn named_by_attr(mut p: Package) -> Package {
    if !p.name.eq_ignore_ascii_case(&p.attr) {
        p.name = p.attr.clone();
    }
    p
}

/// Relevance score of a package for a query (weights inspired by GNOME's
/// AppStream fields: name ≫ contains ≫ description).
fn search_score(pkg: &Package, q: &str, toks: &[String]) -> i64 {
    let attr = pkg.attr.to_lowercase();
    let name = pkg.name.to_lowercase();
    let desc = pkg.description.to_lowercase();
    let mut s: i64 = 0;

    if attr == q {
        s += 1000; // exact match
    } else if name == q {
        s += 800; // exact pname, but the attr says otherwise (a package set)
    } else if attr.starts_with(q) || name.starts_with(q) {
        s += 600; // starts with
    } else if attr.contains(q) || name.contains(q) {
        s += 300; // contains (name)
    }

    // Every token present in the name/attr (multi-word).
    if toks
        .iter()
        .all(|t| attr.contains(t.as_str()) || name.contains(t.as_str()))
    {
        s += 150;
    }
    // Every token present in the description (low weight).
    if toks.iter().all(|t| desc.contains(t.as_str())) {
        s += 30;
    }

    // Favor short names (closer to an exact match).
    s -= (attr.chars().count() as i64).min(60);
    // Top-level packages before package sets (`haskellPackages.hello`).
    if attr.contains('.') {
        s -= 40;
    }
    s
}

/// Prettifies a nixpkgs attr for display: capitalizes the first letter and
/// keeps inner case (`superTux` → "SuperTux", `superTuxKart` → "SuperTuxKart").
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
        // Two attrs that "contain" the query without starting with it:
        // the shorter must win (length penalty).
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
