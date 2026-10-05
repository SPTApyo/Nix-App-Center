//! Visual dressing from Flathub (icons/screenshots/ratings/releases).
//!
//! NixOS ships no local AppStream catalog, so the visual dressing comes from
//! the Flathub AppStream API (exactly what GNOME Software does), or from the
//! local AppStream database when present (`appstream_db`, instant, tried
//! first). IMPORTANT: this is purely cosmetic; install and run ALWAYS go
//! through nix. Matching a nixpkgs name to a Flathub app id is heuristic
//! (with a relevance guard, `flathub_relevant`).

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
    /// Release notes ("What's Changed" body, AppStream HTML to text).
    pub description: Option<String>,
    /// Link to the full release notes, when provided.
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
    // Full AppStream listing
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
    /// Extensions/add-ons (local database only).
    pub addons: Vec<Addon>,
    /// Other apps by the same developer (local database).
    pub developer_apps: Vec<DevApp>,
}

/// A Flathub candidate: its icon + its `app_id` (to find the ODRS rating).
#[derive(Clone)]
pub(crate) struct FlathubHit {
    pub(crate) icon: String,
    pub(crate) app_id: String,
    /// Human AppStream name ("Steam", "Firefox") for display.
    pub(crate) name: String,
}

/// One Flathub `/search` call giving the raw hit list. `None` when the
/// network or the JSON fails.
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

/// Builds a "candidate (name/attr-id) → Flathub hit" table from ONE Flathub
/// search call. Used by `search::search_apps` when the local AppStream
/// database is not ready (or incomplete) for some results.
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

/// Is the Flathub hit close enough to the searched package to avoid a false
/// positive (e.g. not showing Krita for "ripgrep")?
///
/// One-way containment: `hit contains query` (e.g. "Firefox ESR" ⊃
/// "firefox"), never the other way. "query contains hit" looked legitimate
/// for cases like "vlc" ⊂ "vlc-media-player", but nixpkgs is full of DISTINCT
/// packages that start with a big app's name without being aliases:
/// `discord-rpc`/`discord-sh`/`discordo` ⊃ "discord", `steam-run`/`steamcmd`
/// ⊃ "steam". That direction matched any related package onto the real app,
/// a confirmed false positive.
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

/// Turns a local AppStream database entry into a `FlathubMeta`.
pub(crate) fn meta_from_db(e: &appstream_db::Entry) -> FlathubMeta {
    FlathubMeta {
        app_id: e.app_id.clone(),
        name: e.name.clone(),
        summary: e.summary.clone(),
        developer: e.developer.clone(),
        icon_url: e.icon_url.clone(),
        screenshot_urls: e.screenshot_urls.clone(),
        description: e.description.clone(),
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
        // Only available through the Flathub REST API (absent from the XML).
        is_free_license: None,
    }
}

/// Looks up Flathub visual metadata for a package (by name). Tries the local
/// AppStream database first (instant), falls back on the Flathub REST API
/// when the database is not ready or does not know the app. Returns `None`
/// when nothing relevant is found; the UI then keeps the generic icon.
pub fn flathub_meta(name: &str, screenshot_target_w: u32) -> Option<FlathubMeta> {
    // ── Fast path: local database ────────────────────────────────────────────
    if let Some(entry) = appstream_db::lookup(name) {
        return Some(meta_from_db(entry));
    }

    // ── Fallback: Flathub REST API ────────────────────────────────────────────
    // 1) Search: app_id + icon + summary.
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

    // 2) Full AppStream listing: everything else.
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

/// Picks the screenshot variant **just big enough** for the target width
/// `target_w` (physical pixels = screen width × scale factor): the smallest
/// variant whose width ≥ target (saves bandwidth/memory, no point pulling 4K
/// for 1080p); failing that, the largest available. The UI then shrinks the
/// display size when the image is below target (anti-upscale), so no
/// sharpness is lost.
fn best_screenshot(sizes: &[serde_json::Value], target_w: u32) -> Option<String> {
    // Width 0 (unknown) = original source image, treated as very large so it
    // is only picked as a last resort (never before a sized variant).
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

/// HTML (AppStream descriptions) to readable plain text.
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
    // `&amp;` decoded LAST: otherwise "&amp;lt;" would become "<" instead of
    // the literal "&lt;" (double decoding).
    let out = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    // Clean lines and collapse multiple empty lines.
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

// ── Tests: Flathub ↔ nixpkgs name matching ───────────────────────────────────
//
// `flathub_relevant` is the gate against false positives: it decides whether
// the Flathub hit returned by the API really is the searched app. These tests
// pin the expected behavior and document edge cases.
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
        // "vlc-media-player" ⊃ "vlc" must NOT match: same shape as the
        // discord-rpc/steam-run false positives (real regression).
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
        // Historical regression: "Extreme Tux Racer" vs nix attr "extremetuxracer".
        // The strip-spaces step is needed since neither a⊂b nor b⊂a holds
        // when a has spaces and b does not.
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
        // Regression: a short nixpkgs name (`go`, `jq`, `fd`...) must not
        // match any Flathub hit that happens to contain it.
        assert!(!flathub_relevant("GoodVibes", "go"));
        assert!(!flathub_relevant("Djview", "jv"));
    }

    #[test]
    fn short_exact_match_still_matches() {
        // The length guard must not prevent a short EXACT match.
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
        // width 0 = source image of unknown size: must only be picked when
        // no sized variant fits.
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
