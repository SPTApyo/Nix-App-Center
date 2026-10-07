//! The "app DNS": resolves a nixpkgs attribute to an AppStream/Flathub app id.
//!
//! nixpkgs and Flathub are two worlds with no official mapping. Name
//! heuristics leave gaps (Firefox, every *unfree* app) and sometimes get it
//! wrong (`gimp` to an add-on rather than `org.gimp.GIMP`). This **curated**
//! layer fills and fixes them, by priority:
//!   1. **user** overlay (`data_dir/app-mapping.json`): local fixes, backend of
//!      the "Report an Issue" button;
//!   2. **community** overlay (embedded, curated in the repo through PRs).
//!
//! When neither knows the attr, resolution returns `None` and the caller falls
//! back on the heuristic. Identity (stable, here) is never mixed with content
//! (icon/reviews, fetched live elsewhere from the resolved app id).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Overlay curated in the repository, embedded in the binary.
const COMMUNITY_OVERLAY: &str = include_str!("data/app_mapping.json");

/// Format shared by both overlays. `note` documents the file (community);
/// entries live in `mappings`. A wrapper (rather than a bare map) leaves room
/// for future fields (confidence, report source) without breaking the format.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Overlay {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(default)]
    mappings: HashMap<String, String>,
}

/// The community overlay, parsed once (independent of the environment).
fn community() -> &'static HashMap<String, String> {
    static M: OnceLock<HashMap<String, String>> = OnceLock::new();
    M.get_or_init(|| {
        serde_json::from_str::<Overlay>(COMMUNITY_OVERLAY)
            .map(|o| o.mappings)
            .unwrap_or_default()
    })
}

/// Editable local overlay (next to `kept.json`/`tried.json`).
pub(crate) fn user_overlay_path() -> PathBuf {
    crate::paths::data_dir().join("app-mapping.json")
}

fn user_overlay() -> Overlay {
    std::fs::read(user_overlay_path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Resolves `attr` to an app id through the curated overlays. `None` when
/// unknown: the caller then falls back on the heuristic.
pub fn resolve_app_id(attr: &str) -> Option<String> {
    let mut user = user_overlay();
    if let Some(id) = user.mappings.remove(attr) {
        return Some(id); // the local fix always wins
    }
    community().get(attr).cloned()
}

/// True when the user said `attr` is no Flathub app at all (empty mapping).
pub(crate) fn is_unmatched(attr: &str) -> bool {
    resolve_app_id(attr).is_some_and(|id| id.is_empty())
}

/// Reverse lookup: the nixpkgs attr the overlays pin to `app_id`, if any.
pub(crate) fn attr_for_app_id(app_id: &str) -> Option<String> {
    let find = |m: &HashMap<String, String>| {
        m.iter()
            .find(|(_, id)| id.eq_ignore_ascii_case(app_id))
            .map(|(attr, _)| attr.clone())
    };
    find(&user_overlay().mappings).or_else(|| find(community()))
}

/// Records a local fix/report. **Placeholder** of the "Report an Issue" button:
/// for now it writes to the user overlay (used on the next resolution); sending
/// to a shared community repository will come later.
pub fn report_mapping(attr: &str, app_id: &str) -> std::io::Result<()> {
    let mut o = user_overlay();
    o.mappings.insert(attr.to_string(), app_id.to_string());
    let json = serde_json::to_vec_pretty(&o).map_err(std::io::Error::other)?;
    crate::paths::write_atomic(&user_overlay_path(), &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_mapping_marks_a_package_unmatched() {
        let _g = crate::env_lock();
        let dir =
            std::env::temp_dir().join(format!("nix-software-resolve-u-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_DATA_HOME", &dir);
        assert!(!is_unmatched("zoom"));
        report_mapping("zoom", "").unwrap();
        assert!(is_unmatched("zoom"));
        std::env::remove_var("XDG_DATA_HOME");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn community_overlay_covers_known_gaps() {
        let _g = crate::env_lock();
        let dir =
            std::env::temp_dir().join(format!("nix-software-resolve-c-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_DATA_HOME", &dir); // empty user overlay, falls back on community

        assert_eq!(
            resolve_app_id("firefox").as_deref(),
            Some("org.mozilla.firefox")
        );
        assert_eq!(
            resolve_app_id("discord").as_deref(),
            Some("com.discordapp.Discord")
        );
        // Disambiguation: gimp must give the app id, not an add-on.
        assert_eq!(resolve_app_id("gimp").as_deref(), Some("org.gimp.GIMP"));
        assert_eq!(resolve_app_id("attr-inexistant-xyz"), None);

        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn user_overlay_takes_precedence_and_report_persists() {
        let _g = crate::env_lock();
        let dir =
            std::env::temp_dir().join(format!("nix-software-resolve-u-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_DATA_HOME", &dir);

        assert_eq!(resolve_app_id("my-own-app"), None);

        // The user reports a mapping: it persists and resolves.
        report_mapping("my-own-app", "com.example.MyApp").unwrap();
        assert_eq!(
            resolve_app_id("my-own-app").as_deref(),
            Some("com.example.MyApp")
        );

        // And it wins over the community overlay.
        report_mapping("firefox", "org.mozilla.firefox.custom").unwrap();
        assert_eq!(
            resolve_app_id("firefox").as_deref(),
            Some("org.mozilla.firefox.custom")
        );

        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn attr_for_app_id_inverts_the_overlay() {
        let _g = crate::env_lock();
        let dir =
            std::env::temp_dir().join(format!("nix-software-resolve-r-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_DATA_HOME", &dir);

        assert_eq!(
            attr_for_app_id("com.visualstudio.code").as_deref(),
            Some("vscode")
        );
        assert_eq!(
            attr_for_app_id("COM.VISUALSTUDIO.CODE").as_deref(),
            Some("vscode")
        );
        assert_eq!(attr_for_app_id("org.example.Unknown"), None);

        std::env::remove_var("XDG_DATA_HOME");
    }
}
