//! Flathub metadata of an app (icons, screenshots, releases), from the local
//! AppStream database or the Flathub API. Cosmetic: Nix installs and runs.

use crate::appstream_db;
use crate::util::sanitize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use crate::appstream_db::{Addon, DevApp};

/// A published version (for the version history).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub timestamp: i64,
    /// Release notes as text.
    pub description: Option<String>,
    /// Link to the full release notes.
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FlathubMeta {
    pub app_id: String,
    pub name: String,
    pub summary: Option<String>,
    pub developer: Option<String>,
    pub icon_url: Option<String>,
    pub screenshot_urls: Vec<String>,
    pub description: Option<String>, // HTML converted to text
    pub license: Option<String>,
    pub is_free_license: Option<bool>,
    pub homepage: Option<String>,
    pub bugtracker: Option<String>,
    pub help: Option<String>,
    pub donation: Option<String>,
    pub translate: Option<String>,
    pub releases: Vec<Release>,
    pub verified: bool,
    /// Caption of each screenshot (parallel to `screenshot_urls`).
    pub screenshot_captions: Vec<Option<String>>,
    /// Age rating ("12") + OARS descriptions, when available.
    pub age_label: Option<String>,
    pub age_descriptions: Vec<String>,
    /// Add-ons, from the local database only.
    pub addons: Vec<Addon>,
    /// Other apps of the developer, from the local database only.
    pub developer_apps: Vec<DevApp>,
    pub binaries: Vec<String>,
    pub mediatypes: Vec<String>,
    pub launchable: Option<String>,
    pub brand_light: Option<String>,
    pub brand_dark: Option<String>,
}

/// A Flathub search hit: name, app id and icon.
#[derive(Clone)]
pub(crate) struct FlathubHit {
    pub(crate) icon: String,
    pub(crate) app_id: String,
    pub(crate) name: String,
}

/// The hits of one Flathub search; `None` on failure.
fn flathub_search(query: &str) -> Option<Vec<serde_json::Value>> {
    let mut v = crate::http::post_json(
        "https://flathub.org/api/v2/search",
        &serde_json::json!({ "query": query }),
    )?;
    match v.get_mut("hits")?.take() {
        serde_json::Value::Array(hits) => Some(hits),
        _ => None,
    }
}

/// Flathub hits of one search, keyed by candidate name and attr.
pub(crate) fn flathub_icon_map(query: &str) -> HashMap<String, FlathubHit> {
    let mut map = HashMap::new();
    for h in flathub_search(query).unwrap_or_default() {
        let icon = match h.get("icon").and_then(|x| x.as_str()) {
            Some(i) => i.to_string(),
            None => continue,
        };
        let app_id = h
            .get("app_id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let disp_name = h
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let hit = FlathubHit {
            icon,
            app_id: app_id.clone(),
            name: disp_name.clone(),
        };
        if !disp_name.is_empty() {
            map.entry(sanitize(&disp_name))
                .or_insert_with(|| hit.clone());
            if let Some(w) = disp_name.split_whitespace().next() {
                map.entry(w.to_lowercase()).or_insert_with(|| hit.clone());
            }
        }
        if let Some(seg) = app_id.rsplit('.').next() {
            map.entry(seg.to_lowercase()).or_insert_with(|| hit.clone());
        }
    }
    map
}

/// True when the Flathub hit name contains the query. Only that way:
/// `steam-run` must not match the Steam app.
fn flathub_relevant(hit_name: &str, query: &str) -> bool {
    const MIN_SUBSTR_LEN: usize = 3;

    let a = hit_name.to_lowercase();
    let b = query.to_lowercase();
    if a == b {
        return true;
    }
    if a.len() >= MIN_SUBSTR_LEN && b.len() >= MIN_SUBSTR_LEN && a.contains(&b) {
        return true;
    }
    let a_ns = a.replace(' ', "");
    let b_ns = b.replace(' ', "");
    if a_ns == b_ns {
        return true;
    }
    a_ns.len() >= MIN_SUBSTR_LEN && b_ns.len() >= MIN_SUBSTR_LEN && a_ns.contains(&b_ns)
}

/// A local AppStream entry as Flathub metadata.
pub(crate) fn meta_from_db(e: &appstream_db::Entry) -> FlathubMeta {
    FlathubMeta {
        app_id: e.app_id.clone(),
        name: e.shown_name(),
        summary: e.shown_summary(),
        developer: e.developer.clone(),
        icon_url: e.icon_url.clone(),
        screenshot_urls: e.screenshot_urls.clone(),
        description: e.shown_description(),
        license: e.license.clone(),
        homepage: e.homepage.clone(),
        bugtracker: e.bugtracker.clone(),
        help: e.help.clone(),
        donation: e.donation.clone(),
        translate: e.translate.clone(),
        verified: e.verified,
        releases: e
            .releases
            .iter()
            .map(|r| Release {
                version: r.version.clone(),
                timestamp: r.timestamp,
                description: r.description.clone(),
                url: r.url.clone(),
            })
            .collect(),
        screenshot_captions: e.screenshot_captions.clone(),
        age_label: crate::oars::rating(&e.oars)
            .as_ref()
            .map(|(a, _)| a.clone()),
        age_descriptions: crate::oars::rating(&e.oars)
            .map(|(_, d)| d)
            .unwrap_or_default(),
        addons: appstream_db::addons_for(&e.app_id),
        developer_apps: e
            .developer
            .as_deref()
            .map(|dev| appstream_db::apps_by_developer(dev, &e.app_id))
            .unwrap_or_default(),
        // Only the REST API has it.
        is_free_license: None,
        binaries: e.binaries.clone(),
        mediatypes: e.mediatypes.clone(),
        launchable: e.launchable.clone(),
        brand_light: e.brand_light.clone(),
        brand_dark: e.brand_dark.clone(),
    }
}

/// Installs of the last 30 days on Flathub, memoized for the session.
pub fn flathub_installs(app_id: &str) -> Option<u64> {
    static CACHE: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, Option<u64>>>,
    > = std::sync::LazyLock::new(Default::default);
    if let Some(hit) = CACHE.lock().ok().and_then(|c| c.get(app_id).copied()) {
        return hit;
    }
    let n = crate::http::get_json(&format!("https://flathub.org/api/v2/stats/{app_id}"))
        .and_then(|v| v["installs_last_month"].as_u64());
    if let Ok(mut c) = CACHE.lock() {
        c.insert(app_id.to_string(), n);
    }
    n
}

/// Flathub metadata of the app named `name`: local database first, then
/// the Flathub API. `None` when nothing relevant matches.
pub fn flathub_meta(name: &str, screenshot_target_w: u32) -> Option<FlathubMeta> {
    if let Some(entry) = appstream_db::lookup(name) {
        return Some(meta_from_db(entry));
    }

    let hits = flathub_search(name)?;
    let hit = hits.iter().find(|h| {
        h.get("name")
            .and_then(|n| n.as_str())
            .map(|n| flathub_relevant(n, name))
            .unwrap_or(false)
    })?;

    let app_id = hit.get("app_id")?.as_str()?.to_string();
    let mut meta = FlathubMeta {
        app_id: app_id.clone(),
        name: hit
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or(name)
            .to_string(),
        summary: hit
            .get("summary")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        developer: hit
            .get("developer_name")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        icon_url: hit.get("icon").and_then(|x| x.as_str()).map(str::to_string),
        verified: hit
            .get("verification_verified")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        ..Default::default()
    };

    if let Some(d) =
        crate::http::get_json(&format!("https://flathub.org/api/v2/appstream/{app_id}"))
    {
        if meta.icon_url.is_none() {
            meta.icon_url = d.get("icon").and_then(|x| x.as_str()).map(str::to_string);
        }
        if meta.developer.is_none() {
            meta.developer = d
                .get("developer_name")
                .and_then(|x| x.as_str())
                .map(str::to_string);
        }
        meta.description = d
            .get("description")
            .and_then(|x| x.as_str())
            .map(html_to_text)
            .filter(|s| !s.is_empty());
        meta.license = d
            .get("project_license")
            .and_then(|x| x.as_str())
            .map(str::to_string);
        meta.is_free_license = d.get("is_free_license").and_then(|x| x.as_bool());

        if let Some(urls) = d.get("urls") {
            let get = |k: &str| urls.get(k).and_then(|x| x.as_str()).map(str::to_string);
            meta.homepage = get("homepage");
            meta.bugtracker = get("bugtracker");
            meta.help = get("help");
            meta.donation = get("donation");
            meta.translate = get("translate");
        }

        if let Some(rels) = d.get("releases").and_then(|x| x.as_array()) {
            for r in rels.iter().take(8) {
                if let Some(version) = r.get("version").and_then(|x| x.as_str()) {
                    let timestamp = r
                        .get("timestamp")
                        .and_then(|x| {
                            x.as_str()
                                .and_then(|s| s.parse().ok())
                                .or_else(|| x.as_i64())
                        })
                        .unwrap_or(0);
                    let description = r
                        .get("description")
                        .and_then(|x| x.as_str())
                        .map(html_to_text)
                        .filter(|s| !s.is_empty());
                    let url = r.get("url").and_then(|x| x.as_str()).map(str::to_string);
                    meta.releases.push(Release {
                        version: version.to_string(),
                        timestamp,
                        description,
                        url,
                    });
                }
            }
        }

        if let Some(ss) = d.get("screenshots").and_then(|x| x.as_array()) {
            for s in ss {
                if let Some(sizes) = s.get("sizes").and_then(|x| x.as_array()) {
                    if let Some(src) = best_screenshot(sizes, screenshot_target_w) {
                        meta.screenshot_urls.push(src);
                    }
                }
            }
        }
    }

    Some(meta)
}

/// The smallest screenshot at least `target_w` pixels wide, else the
/// largest one.
fn best_screenshot(sizes: &[serde_json::Value], target_w: u32) -> Option<String> {
    // Unknown width: the original, picked last.
    let mut variants: Vec<(u32, &str)> = Vec::new();
    for z in sizes {
        let src = match z.get("src").and_then(|x| x.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let w = z
            .get("width")
            .and_then(|x| {
                x.as_str()
                    .and_then(|s| s.parse::<u32>().ok())
                    .or_else(|| x.as_u64().map(|v| v as u32))
            })
            .unwrap_or(0);
        variants.push((if w == 0 { u32::MAX } else { w }, src));
    }
    variants
        .iter()
        .filter(|(w, _)| *w >= target_w)
        .min_by_key(|(w, _)| *w)
        .or_else(|| variants.iter().max_by_key(|(w, _)| *w))
        .map(|(_, src)| src.to_string())
}

/// AppStream HTML as plain text.
fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' => {
                in_tag = false;
                let t = tag.trim().to_lowercase();
                if t == "/p" || t == "br" || t == "br/" || t == "/ul" || t == "/ol" {
                    out.push('\n');
                } else if t == "li" {
                    out.push_str("\n• ");
                }
            }
            _ if in_tag => tag.push(c),
            _ => out.push(c),
        }
    }
    // `&amp;` last, to avoid double decoding.
    let out = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    let mut lines: Vec<String> = Vec::new();
    for l in out.split('\n') {
        let l = l.trim();
        if l.is_empty() {
            if matches!(lines.last(), Some(last) if last.is_empty()) {
                continue;
            }
            lines.push(String::new());
        } else {
            lines.push(l.to_string());
        }
    }
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod flathub_matching_tests {
    use super::flathub_relevant;

    // ── Expected matches (true positives) ──────────────────────────────────

    #[test]
    fn exact_same() {
        assert!(flathub_relevant("firefox", "firefox"));
    }

    #[test]
    fn case_insensitive_hit_name() {
        // Flathub returns "Firefox" (capitalized), nixpkgs has "firefox"
        assert!(flathub_relevant("Firefox", "firefox"));
    }

    #[test]
    fn case_insensitive_all_caps() {
        // GNOME "GIMP" on Flathub → nix attr "gimp"
        assert!(flathub_relevant("GIMP", "gimp"));
    }

    #[test]
    fn hit_contains_query() {
        // "Firefox ESR" ⊃ "firefox"
        assert!(flathub_relevant("Firefox ESR", "firefox"));
    }

    #[test]
    fn query_does_not_match_just_because_it_contains_hit_name() {
        // A longer package name must not match.
        assert!(!flathub_relevant("vlc", "vlc-media-player"));
    }

    #[test]
    fn no_fp_companion_packages_containing_app_name() {
        assert!(!flathub_relevant("Discord", "discord-rpc"));
        assert!(!flathub_relevant("Discord", "discord-sh"));
        assert!(!flathub_relevant("Discord", "discordo"));
        assert!(!flathub_relevant("Steam", "steam-run"));
        assert!(!flathub_relevant("Steam", "steamcmd"));
    }

    #[test]
    fn spaces_stripped_match() {
        // Spaces are ignored when comparing.
        assert!(
            flathub_relevant("Extreme Tux Racer", "extremetuxracer"),
            "regression: matching with spaces must work"
        );
    }

    #[test]
    fn spaces_stripped_reversed() {
        assert!(flathub_relevant("extremetuxracer", "Extreme Tux Racer"));
    }

    #[test]
    fn libreoffice() {
        assert!(flathub_relevant("LibreOffice", "libreoffice"));
    }

    #[test]
    fn inkscape() {
        assert!(flathub_relevant("Inkscape", "inkscape"));
    }

    #[test]
    fn steam() {
        assert!(flathub_relevant("Steam", "steam"));
    }

    #[test]
    fn discord() {
        assert!(flathub_relevant("Discord", "discord"));
    }

    // ── True negatives (false positives to reject) ─────────────────────────

    #[test]
    fn no_fp_krita_vs_ripgrep() {
        assert!(!flathub_relevant("Krita", "ripgrep"));
    }

    #[test]
    fn no_fp_bython_vs_python3() {
        // "bython" and "python3" do not contain each other
        assert!(!flathub_relevant("Bython", "python3"));
    }

    #[test]
    fn no_fp_vim_vs_emacs() {
        assert!(!flathub_relevant("Vim", "emacs"));
        assert!(!flathub_relevant("Emacs", "vim"));
    }

    #[test]
    fn no_fp_completely_unrelated() {
        assert!(!flathub_relevant("Shotcut", "neovim"));
        assert!(!flathub_relevant("Audacity", "htop"));
    }

    #[test]
    fn no_fp_short_query_substring() {
        // Short names must not match longer hits.
        assert!(!flathub_relevant("GoodVibes", "go"));
        assert!(!flathub_relevant("Djview", "jv"));
    }

    #[test]
    fn short_exact_match_still_matches() {
        // Short exact names still match.
        assert!(flathub_relevant("jq", "jq"));
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    // ── html_to_text ──────────────────────────────────────────────────────

    #[test]
    fn html_to_text_strips_tags_and_decodes_entities() {
        let out = html_to_text("<p>Hello &amp; welcome</p><p>Second line</p>");
        assert!(out.contains("Hello & welcome"));
        assert!(out.contains("Second line"));
        assert!(!out.contains('<'));
    }

    #[test]
    fn html_to_text_converts_list_items_to_bullets() {
        let out = html_to_text("<ul><li>First</li><li>Second</li></ul>");
        assert!(out.contains("• First"));
        assert!(out.contains("• Second"));
    }

    #[test]
    fn html_to_text_collapses_multiple_blank_lines() {
        let out = html_to_text("<p>A</p><p></p><p></p><p>B</p>");
        assert!(!out.contains("\n\n\n"));
    }

    // ── best_screenshot ───────────────────────────────────────────────────

    #[test]
    fn best_screenshot_picks_smallest_variant_above_target() {
        let sizes = serde_json::json!([
            { "src": "small.png", "width": "400" },
            { "src": "medium.png", "width": "800" },
            { "src": "large.png", "width": "1600" }
        ]);
        assert_eq!(
            best_screenshot(sizes.as_array().unwrap(), 700),
            Some("medium.png".to_string())
        );
    }

    #[test]
    fn best_screenshot_falls_back_to_largest_when_none_big_enough() {
        let sizes = serde_json::json!([
            { "src": "small.png", "width": "400" },
            { "src": "medium.png", "width": "800" }
        ]);
        assert_eq!(
            best_screenshot(sizes.as_array().unwrap(), 2000),
            Some("medium.png".to_string())
        );
    }

    #[test]
    fn best_screenshot_empty_sizes_is_none() {
        let sizes: Vec<serde_json::Value> = vec![];
        assert_eq!(best_screenshot(&sizes, 1000), None);
    }

    #[test]
    fn best_screenshot_zero_width_is_last_resort() {
        // Unknown width: picked last.
        let sizes = serde_json::json!([
            { "src": "unknown.png", "width": "0" },
            { "src": "sized.png", "width": "800" }
        ]);
        assert_eq!(
            best_screenshot(sizes.as_array().unwrap(), 100),
            Some("sized.png".to_string())
        );
    }
}
