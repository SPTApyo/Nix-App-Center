//! nixsoftware.org: apps curated per nixpkgs attr. Fills what the AppStream
//! database and Flathub do not know; every call is best effort.

use std::collections::HashMap;

use serde_json::Value;

use crate::flathub::FlathubMeta;
use crate::Package;

const API: &str = "https://api.nixsoftware.org/api";

/// A package as nixsoftware.org lists it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NsApp {
    pub(crate) attr: String,
    pub(crate) name: String,
    pub(crate) summary: String,
    pub(crate) logo: Option<String>,
    /// Has an app page there: name, logo, screenshots.
    pub(crate) curated: bool,
}

/// Packages nixsoftware.org finds for `query`, by attr. One request.
pub(crate) fn search(query: &str) -> HashMap<String, NsApp> {
    let url = format!("{API}/v1/packages?search={}&per_page=100", encode(query));
    crate::http::get_json(&url)
        .map(|v| apps(&v))
        .unwrap_or_default()
        .into_iter()
        .map(|a| (a.attr.clone(), a))
        .collect()
}

/// Apps similar to `attr` that this nixpkgs has, with their logo. Blocking.
pub fn similar_apps(attr: &str) -> Vec<(Package, Option<String>)> {
    let url = format!("{API}/v2/packages/{}/similar", encode(attr));
    let found = crate::http::get_json(&url)
        .map(|v| apps(&v))
        .unwrap_or_default();
    let attrs: Vec<String> = found.iter().map(|a| a.attr.to_lowercase()).collect();
    let mut here = crate::categories::nix_attr_lookup(&attrs);
    let found_attrs: Vec<String> = here.values().map(|p| p.attr.clone()).collect();
    let runs_here = crate::search::results_meta(&found_attrs);
    found
        .into_iter()
        .filter_map(|a| {
            let mut p = here.remove(&a.attr.to_lowercase())?;
            if !runs_here.get(&p.attr).is_some_and(|m| m.available) {
                return None;
            }
            p.name = a.name;
            Some((p, a.logo))
        })
        .collect()
}

/// The app page of `attr`, when nixsoftware.org curates one. Blocking.
pub(crate) fn meta(attr: &str) -> Option<FlathubMeta> {
    let url = format!("{API}/v2/packages/{}/meta", encode(attr));
    meta_from(crate::http::get_json(&url)?.get("data")?)
}

fn text(v: Option<&Value>, key: &str) -> Option<String> {
    v.and_then(|x| x.get(key))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}

/// GdkPixbuf has no AVIF loader by default.
fn logo(v: Option<&Value>) -> Option<String> {
    text(v, "logo_path").filter(|u| !u.ends_with(".avif"))
}

fn apps(v: &Value) -> Vec<NsApp> {
    v.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let attr = text(Some(p), "attr_path")?;
            let meta = p.get("meta").filter(|m| !m.is_null());
            let tr = meta.and_then(|m| m.get("translation"));
            Some(NsApp {
                name: text(tr, "name").unwrap_or_else(|| attr.clone()),
                summary: text(tr, "summary")
                    .or_else(|| text(Some(p), "summary"))
                    .unwrap_or_default(),
                logo: logo(meta),
                curated: meta.is_some(),
                attr,
            })
        })
        .collect()
}

fn meta_from(m: &Value) -> Option<FlathubMeta> {
    let tr = m.get("translation");
    let links = m.get("links");
    Some(FlathubMeta {
        name: text(tr, "name")?,
        summary: text(tr, "summary"),
        description: text(tr, "description").map(|d| plain_text(&d)),
        icon_url: logo(Some(m)),
        screenshot_urls: m
            .get("screenshots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_string))
            .collect(),
        homepage: text(links, "homepage"),
        bugtracker: text(links, "bugtracker"),
        help: text(links, "help"),
        donation: text(links, "donation"),
        translate: text(links, "translate"),
        ..Default::default()
    })
}

/// Markdown as plain text: no headings, emphasis, code marks or link targets.
fn plain_text(md: &str) -> String {
    let mut out = String::new();
    for line in md.lines() {
        let line = line.trim_start_matches('#').trim_start_matches(' ');
        let line = line.replace("**", "").replace("__", "").replace('`', "");
        let mut rest = line.as_str();
        // [text](url) keeps its text.
        while let Some(open) = rest.find('[') {
            let Some((text, after)) = rest[open + 1..].split_once("](") else {
                break;
            };
            let Some(close) = after.find(')') else {
                break;
            };
            out.push_str(&rest[..open]);
            out.push_str(text);
            rest = &after[close + 1..];
        }
        out.push_str(rest);
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// Percent-encodes `s` for a URL query or path segment.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_escapes_spaces_and_symbols() {
        assert_eq!(encode("image editor"), "image%20editor");
        assert_eq!(encode("python3Packages.numpy"), "python3Packages.numpy");
        assert_eq!(encode("a&b/c"), "a%26b%2Fc");
    }

    #[test]
    fn apps_reads_curated_and_plain_packages() {
        let v = serde_json::json!({ "data": [
            { "attr_path": "gelly", "summary": "nix summary",
              "meta": { "logo_path": "https://x/gelly.png",
                        "translation": { "name": "Gelly", "summary": "Music" } } },
            { "attr_path": "htop", "summary": "Process viewer", "meta": null },
            { "attr_path": "custom", "meta": { "logo_path": "https://x/l.avif",
                                               "translation": { "name": "Custom" } } }
        ]});
        let a = apps(&v);
        assert_eq!(a.len(), 3);
        assert_eq!(
            (a[0].name.as_str(), a[0].summary.as_str()),
            ("Gelly", "Music")
        );
        assert_eq!(a[0].logo.as_deref(), Some("https://x/gelly.png"));
        assert!(a[0].curated);
        assert_eq!(
            (a[1].name.as_str(), a[1].summary.as_str()),
            ("htop", "Process viewer")
        );
        assert!(!a[1].curated);
        assert_eq!(a[2].logo, None);
    }

    #[test]
    fn plain_text_drops_markdown_marks() {
        let md = "### Features\n- **Fast:** uses [libmemscan](https://x/y) and `LD_PRELOAD`\nplain";
        assert_eq!(
            plain_text(md),
            "Features\n- Fast: uses libmemscan and LD_PRELOAD\nplain"
        );
    }

    #[test]
    fn meta_from_reads_the_app_page() {
        let m = serde_json::json!({
            "logo_path": "https://x/j.png",
            "translation": { "name": "Jellyfin", "summary": "Client", "description": "Long" },
            "links": { "bugtracker": "https://bugs", "help": "" },
            "screenshots": ["https://x/1.png", "https://x/2.png"]
        });
        let f = meta_from(&m).unwrap();
        assert_eq!(f.name, "Jellyfin");
        assert_eq!(f.description.as_deref(), Some("Long"));
        assert_eq!(f.bugtracker.as_deref(), Some("https://bugs"));
        assert_eq!(f.help, None);
        assert_eq!(f.screenshot_urls.len(), 2);
        assert!(meta_from(&serde_json::json!({ "translation": {} })).is_none());
    }
}
