//! Flathub collections and categories, resolved to nixpkgs packages.

use crate::appstream_db::DevApp;
use crate::package::Package;
use crate::resolve::{attr_for_app_id, resolve_app_id};
use crate::util::{matching_keys, regex_escape};
use std::collections::HashMap;

/// An app listed by Flathub.
#[derive(Debug, Clone)]
struct FlathubApp {
    app_id: String,
    name: String,
    summary: Option<String>,
    icon_url: Option<String>,
    updated_at: i64, // unix timestamp of the last update
}

/// The apps of a Flathub collection (`popular`, `category/game`).
fn flathub_collection(collection: &str, limit: usize) -> Vec<FlathubApp> {
    let url = format!("https://flathub.org/api/v2/collection/{collection}?page=1&per_page={limit}");
    let v = match crate::http::get_json(&url) {
        Some(v) => v,
        None => return vec![],
    };
    let hits = match v.get("hits").and_then(|x| x.as_array()) {
        Some(h) => h,
        None => return vec![],
    };
    hits.iter()
        .filter_map(|h| {
            Some(FlathubApp {
                app_id: h.get("app_id")?.as_str()?.to_string(),
                name: h.get("name")?.as_str()?.to_string(),
                summary: h
                    .get("summary")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
                icon_url: h.get("icon").and_then(|x| x.as_str()).map(str::to_string),
                updated_at: h.get("updated_at").and_then(|x| x.as_i64()).unwrap_or(0),
            })
        })
        .collect()
}

/// The apps of a freedesktop category on Flathub.
fn flathub_category(category: &str, limit: usize) -> Vec<FlathubApp> {
    flathub_collection(&format!("category/{category}"), limit)
}

/// Candidate nixpkgs attrs for a Flathub app, most trusted first. Curated
/// mappings win and veto fuzzy matches.
fn candidate_attrs(name: &str, app_id: &str) -> Vec<String> {
    if let Some(attr) = attr_for_app_id(app_id) {
        return vec![attr.to_lowercase()];
    }
    matching_keys(name, app_id)
        .into_iter()
        .filter(|k| resolve_app_id(k).is_none_or(|id| id.eq_ignore_ascii_case(app_id)))
        .collect()
}

/// nixpkgs packages whose attr is one of `candidates`, in one cached
/// `nix search`, keyed by lowercase attr.
pub(crate) fn nix_attr_lookup(candidates: &[String]) -> HashMap<String, Package> {
    let mut uniq: Vec<String> = candidates
        .iter()
        .filter(|c| !c.is_empty())
        .cloned()
        .collect();
    uniq.sort();
    uniq.dedup();
    if uniq.is_empty() {
        return HashMap::new();
    }

    let pattern = format!(
        "^({})$",
        uniq.iter()
            .map(|c| regex_escape(c))
            .collect::<Vec<_>>()
            .join("|")
    );
    let packages =
        crate::cache::cached_search(&pattern, crate::search::SEARCH_CACHE_TTL_SECS, || {
            crate::search::raw_nix_search(std::slice::from_ref(&pattern))
        });

    let cand_set: std::collections::HashSet<&String> = uniq.iter().collect();
    let mut map = HashMap::new();
    for p in packages {
        let key = p.attr.to_lowercase();
        if cand_set.contains(&key) {
            map.entry(key).or_insert(p);
        }
    }
    map
}

/// The Flathub apps nixpkgs has, in Flathub order, with their package.
fn resolve_apps(apps: Vec<FlathubApp>) -> Vec<(FlathubApp, Package)> {
    if apps.is_empty() {
        return vec![];
    }

    let mut candidates = Vec::new();
    for a in &apps {
        candidates.extend(candidate_attrs(&a.name, &a.app_id));
    }

    let attr_map = nix_attr_lookup(&candidates);

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for a in apps {
        if let Some(pkg) = candidate_attrs(&a.name, &a.app_id)
            .iter()
            .find_map(|k| attr_map.get(k))
        {
            if seen.insert(pkg.attr.clone()) {
                let mut pkg = pkg.clone();
                pkg.name = a.name.clone();
                // AppStream summaries read better than nix descriptions.
                if let Some(summary) = a.summary.clone().filter(|s| !s.trim().is_empty()) {
                    pkg.description = summary;
                }
                out.push((a, pkg));
            }
        }
    }
    out
}

/// Developer apps found in `found`, deduplicated, without `exclude_attr`:
/// fuzzy matches can land on the current page's attr.
fn match_dev_apps(
    apps: &[DevApp],
    attr_map: &HashMap<String, Package>,
    exclude_attr: &str,
) -> Vec<(Package, Option<String>)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for a in apps {
        if let Some(pkg) = candidate_attrs(&a.name, &a.app_id)
            .iter()
            .find_map(|k| attr_map.get(k))
        {
            if pkg.attr == exclude_attr {
                continue;
            }
            if seen.insert(pkg.attr.clone()) {
                let mut pkg = pkg.clone();
                pkg.name = a.name.clone();
                out.push((pkg, a.icon_url.clone()));
            }
        }
    }
    out
}

/// Other apps of the developer that nixpkgs has, without `exclude_attr`.
pub fn resolve_dev_apps(apps: &[DevApp], exclude_attr: &str) -> Vec<(Package, Option<String>)> {
    if apps.is_empty() {
        return vec![];
    }
    let mut candidates = Vec::new();
    for a in apps {
        candidates.extend(candidate_attrs(&a.name, &a.app_id));
    }
    match_dev_apps(apps, &nix_attr_lookup(&candidates), exclude_attr)
}

/// Popular Flathub apps nixpkgs has.
pub fn popular_packages(limit: usize) -> Vec<(Package, Option<String>)> {
    resolve_apps(flathub_collection("popular", limit))
        .into_iter()
        .map(|(a, p)| (p, a.icon_url))
        .collect()
}

/// Recently updated Flathub apps nixpkgs has.
pub fn recently_updated_packages(limit: usize) -> Vec<(Package, Option<String>)> {
    resolve_apps(flathub_collection("recently-updated", limit))
        .into_iter()
        .map(|(a, p)| (p, a.icon_url))
        .collect()
}

/// (recently updated, the rest alphabetically), like GNOME Software.
type Sections = (
    Vec<(Package, Option<String>)>,
    Vec<(Package, Option<String>)>,
);

fn split_sections(mut resolved: Vec<(FlathubApp, Package)>) -> Sections {
    resolved.sort_by_key(|(a, _)| std::cmp::Reverse(a.updated_at));
    const RECENT: usize = 12;
    let mut recent = Vec::new();
    let mut others = Vec::new();
    for (i, (a, p)) in resolved.into_iter().enumerate() {
        if i < RECENT {
            recent.push((p, a.icon_url));
        } else {
            others.push((p, a.icon_url));
        }
    }
    others.sort_by_key(|(p, _)| p.name.to_lowercase());
    (recent, others)
}

/// The apps of several categories, merged and split in two sections.
pub fn category_sections_union(slugs: &[&str], per_cat: usize) -> Sections {
    let mut apps: Vec<FlathubApp> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for slug in slugs {
        for a in flathub_category(slug, per_cat) {
            if seen.insert(a.app_id.clone()) {
                apps.push(a);
            }
        }
    }
    split_sections(resolve_apps(apps))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, app_id: &str, icon: Option<&str>) -> DevApp {
        DevApp {
            app_id: app_id.to_string(),
            name: name.to_string(),
            icon_url: icon.map(str::to_string),
        }
    }

    #[test]
    fn match_dev_apps_keeps_only_nixpkgs_resolved() {
        // Only apps nixpkgs has.
        let map: HashMap<String, Package> =
            [("steamlink".to_string(), Package::from_attr("steamlink"))].into();
        let apps = [
            dev(
                "Steam Link",
                "com.valvesoftware.SteamLink",
                Some("http://icon"),
            ),
            dev("Ghost App", "org.example.Ghost", None),
        ];
        let out = match_dev_apps(&apps, &map, "");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0.attr, "steamlink");
        assert_eq!(out[0].1.as_deref(), Some("http://icon"));
    }

    #[test]
    fn match_dev_apps_excludes_current_package() {
        // Steam Link must not show Steam on its page.
        let map: HashMap<String, Package> =
            [("steam".to_string(), Package::from_attr("steam"))].into();
        let apps = [dev(
            "Steam Link",
            "com.valvesoftware.SteamLink",
            Some("http://icon"),
        )];
        let out = match_dev_apps(&apps, &map, "steam");
        assert!(
            out.is_empty(),
            "the current package must not appear in its own other apps"
        );
    }

    #[test]
    fn match_dev_apps_resolves_by_app_id_suffix() {
        // Matched by the last app id segment.
        let map: HashMap<String, Package> =
            [("boxes".to_string(), Package::from_attr("boxes"))].into();
        let apps = [dev("GNOME Boxes 2000", "org.gnome.Boxes", None)];
        let out = match_dev_apps(&apps, &map, "");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0.attr, "boxes");
    }

    #[test]
    fn match_dev_apps_dedups_by_attr() {
        // Two entries, one attr: one tile.
        let map: HashMap<String, Package> =
            [("gimp".to_string(), Package::from_attr("gimp"))].into();
        let apps = [
            dev("GIMP", "org.gimp.GIMP", None),
            dev("gimp", "org.gimp.GIMP.Devel", None),
        ];
        let out = match_dev_apps(&apps, &map, "");
        assert_eq!(out.len(), 1);
    }

    fn resolved_entry(id: &str, name: &str, updated_at: i64) -> (FlathubApp, Package) {
        (
            FlathubApp {
                app_id: id.to_string(),
                name: name.to_string(),
                summary: None,
                icon_url: None,
                updated_at,
            },
            Package::from_attr(name.to_lowercase()),
        )
    }

    #[test]
    fn candidate_attrs_prefers_overlay_and_rejects_contradicted_keys() {
        let _g = crate::env_lock();
        let dir = std::env::temp_dir().join(format!("nix-software-cand-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_DATA_HOME", &dir);

        // The curated vscode mapping vetoes `code`.
        assert_eq!(
            candidate_attrs("Visual Studio Code", "com.visualstudio.code"),
            ["vscode"]
        );
        // `steam` is pinned to Steam.
        let keys = candidate_attrs("Steam Link", "com.valvesoftware.SteamLink");
        assert!(keys.contains(&"steamlink".to_string()));
        assert!(!keys.contains(&"steam".to_string()));

        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn split_sections_recent_twelve_then_alphabetical_rest() {
        let resolved: Vec<_> = (0..15)
            .map(|i| resolved_entry(&format!("id{i}"), &format!("App{i:02}"), i as i64))
            .collect();
        let (recent, others) = split_sections(resolved);
        assert_eq!(recent.len(), 12);
        assert_eq!(others.len(), 3);
        assert_eq!(recent[0].0.attr, "app14", "most recently updated first");
        assert_eq!(recent[11].0.attr, "app03");
        assert_eq!(others[0].0.attr, "app00", "the rest sorted alphabetically");
        assert_eq!(others[2].0.attr, "app02");
    }
}
