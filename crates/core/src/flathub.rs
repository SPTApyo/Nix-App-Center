//! Habillage visuel via Flathub (icônes/screenshots/notes/releases).
//!
//! NixOS n'a pas de catalogue AppStream local packagé → on récupère l'habillage
//! visuel depuis l'API AppStream de Flathub (exactement ce que fait GNOME
//! Software), ou depuis le cache Flatpak local s'il est présent (`appstream_db`,
//! instantané, essayé en premier). IMPORTANT : c'est purement cosmétique —
//! l'installation/lancement passe TOUJOURS par nix. Le matching
//! `nom nixpkgs → app-id Flathub` est heuristique (garde-fou de pertinence,
//! `flathub_relevant`).

use crate::appstream_db;
use crate::util::sanitize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use crate::appstream_db::{Addon, DevApp};

/// Une version publiée (pour l'historique des versions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub timestamp: i64,
    /// Notes de version (corps « What's Changed », HTML AppStream → texte).
    pub description: Option<String>,
    /// Lien vers les notes de version complètes, si fourni.
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
    // Fiche AppStream complète
    pub description: Option<String>, // HTML converti en texte
    pub license: Option<String>,
    pub is_free_license: Option<bool>,
    pub homepage: Option<String>,
    pub bugtracker: Option<String>,
    pub help: Option<String>,
    pub donation: Option<String>,
    pub translate: Option<String>,
    pub releases: Vec<Release>,
    pub verified: bool,
    /// Légende de chaque screenshot (parallèle à `screenshot_urls`).
    pub screenshot_captions: Vec<Option<String>>,
    /// Classification d'âge (« 12 ») + descriptions OARS, si disponible.
    pub age_label: Option<String>,
    pub age_descriptions: Vec<String>,
    /// Extensions/addons (uniquement via la base locale Flatpak).
    pub addons: Vec<Addon>,
    /// Autres apps du même développeur (base locale Flatpak).
    pub developer_apps: Vec<DevApp>,
}

/// Un candidat Flathub : son icône + son `app_id` (pour retrouver la note ODRS).
#[derive(Clone)]
pub(crate) struct FlathubHit {
    pub(crate) icon: String,
    pub(crate) app_id: String,
    /// Nom humain AppStream (« Steam », « Firefox ») pour l'affichage.
    pub(crate) name: String,
}

/// Un appel `/search` Flathub → la liste brute des hits. `None` si le réseau
/// ou le JSON échoue.
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

/// Construit une table « candidat (nom/attr-id) → hit Flathub » à partir d'UN
/// appel de recherche Flathub. Utilisé par `search::search_apps` quand la base
/// AppStream locale n'est pas prête (ou incomplète) pour certains résultats.
pub(crate) fn flathub_icon_map(query: &str) -> HashMap<String, FlathubHit> {
    let mut map = HashMap::new();
    for h in flathub_search(query).unwrap_or_default() {
        let icon = match h.get("icon").and_then(|x| x.as_str()) {
            Some(i) => i.to_string(),
            None => continue,
        };
        let app_id = h.get("app_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let disp_name = h.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let hit = FlathubHit { icon, app_id: app_id.clone(), name: disp_name.clone() };
        if !disp_name.is_empty() {
            map.entry(sanitize(&disp_name)).or_insert_with(|| hit.clone());
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

/// Le hit Flathub est-il assez proche du paquet recherché pour éviter un faux
/// positif (ex. ne pas afficher Krita pour « ripgrep ») ?
///
/// Containment dans un seul sens : `hit contient query` (ex. "Firefox ESR" ⊃
/// "firefox"), jamais l'inverse. "query contient hit" semblait légitime pour
/// des cas comme "vlc" ⊂ "vlc-media-player", mais nixpkgs regorge de paquets
/// DISTINCTS qui contiennent le nom d'une grosse app en préfixe sans en être
/// des alias : `discord-rpc`/`discord-sh`/`discordo` ⊃ "discord",
/// `steam-run`/`steamcmd` ⊃ "steam". Ce sens du containment matchait donc
/// n'importe quel paquet connexe sur la vraie app — faux positif confirmé.
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

/// Convertit une entrée de la base AppStream locale en `FlathubMeta`.
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
        age_label: crate::oars::rating(&e.oars).as_ref().map(|(a, _)| a.clone()),
        age_descriptions: crate::oars::rating(&e.oars).map(|(_, d)| d).unwrap_or_default(),
        addons: appstream_db::addons_for(&e.app_id),
        developer_apps: e
            .developer
            .as_deref()
            .map(|dev| appstream_db::apps_by_developer(dev, &e.app_id))
            .unwrap_or_default(),
        // Uniquement dispo via l'API REST Flathub (absent du XML local).
        is_free_license: None,
    }
}

/// Cherche les métadonnées visuelles Flathub pour un paquet (par nom).
/// Essaie d'abord la base AppStream locale (Flatpak cache, instantané) ;
/// se rabat sur l'API REST Flathub si la base n'est pas prête ou ne connaît pas l'app.
/// Retourne `None` si rien de pertinent — l'UI garde alors l'icône générique.
pub fn flathub_meta(name: &str, screenshot_target_w: u32) -> Option<FlathubMeta> {
    // ── Voie rapide : base locale Flatpak ────────────────────────────────────
    if let Some(entry) = appstream_db::lookup(name) {
        return Some(meta_from_db(entry));
    }

    // ── Fallback : API REST Flathub ───────────────────────────────────────────
    // 1) Recherche → app_id + icône + résumé.
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
        name: hit.get("name").and_then(|x| x.as_str()).unwrap_or(name).to_string(),
        summary: hit.get("summary").and_then(|x| x.as_str()).map(str::to_string),
        developer: hit.get("developer_name").and_then(|x| x.as_str()).map(str::to_string),
        icon_url: hit.get("icon").and_then(|x| x.as_str()).map(str::to_string),
        verified: hit.get("verification_verified").and_then(|x| x.as_bool()).unwrap_or(false),
        ..Default::default()
    };

    // 2) Fiche AppStream complète → tout le reste.
    if let Some(d) = crate::http::get_json(&format!("https://flathub.org/api/v2/appstream/{app_id}")) {
        if meta.icon_url.is_none() {
            meta.icon_url = d.get("icon").and_then(|x| x.as_str()).map(str::to_string);
        }
        if meta.developer.is_none() {
            meta.developer = d.get("developer_name").and_then(|x| x.as_str()).map(str::to_string);
        }
        meta.description = d
            .get("description")
            .and_then(|x| x.as_str())
            .map(html_to_text)
            .filter(|s| !s.is_empty());
        meta.license = d.get("project_license").and_then(|x| x.as_str()).map(str::to_string);
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
                        .and_then(|x| x.as_str().and_then(|s| s.parse().ok()).or_else(|| x.as_i64()))
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

/// Choisit la variante d'un screenshot **juste suffisante** pour la largeur cible
/// `target_w` (en pixels physiques = largeur écran × facteur d'échelle) : la plus
/// petite variante dont la largeur ≥ cible (économie de bande passante/mémoire —
/// inutile de tirer de la 4K pour du 1080p) ; à défaut, la plus grande disponible.
/// L'UI réduit ensuite la taille d'affichage si l'image reçue est sous la cible
/// (anti-upscale), donc aucune perte de netteté.
fn best_screenshot(sizes: &[serde_json::Value], target_w: u32) -> Option<String> {
    // Largeur 0 (inconnue) = image source originale → traitée comme très grande
    // pour n'être choisie qu'en dernier recours (jamais avant une variante dimensionnée).
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

/// Conversion HTML (descriptions AppStream) → texte brut lisible.
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
    // `&amp;` décodé en DERNIER : sinon « &amp;lt; » deviendrait « < » au lieu
    // du littéral « &lt; » (double décodage).
    let out = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    // Nettoie les lignes et écrase les lignes vides multiples.
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

// ── Tests : correspondance Flathub ↔ nom nixpkgs ─────────────────────────────
//
// `flathub_relevant` est la garde-barrière contre les faux positifs :
// elle décide si le hit Flathub renvoyé par l'API est vraiment l'app cherchée.
// Ces tests fixent les comportements attendus et documentent les cas limites.
#[cfg(test)]
mod flathub_matching_tests {
    use super::flathub_relevant;

    // ── Correspondances attendues (vrais positifs) ─────────────────────────

    #[test]
    fn exact_same() {
        assert!(flathub_relevant("firefox", "firefox"));
    }

    #[test]
    fn case_insensitive_hit_name() {
        // Flathub renvoie "Firefox" (majuscule), nixpkgs a "firefox"
        assert!(flathub_relevant("Firefox", "firefox"));
    }

    #[test]
    fn case_insensitive_all_caps() {
        // GNOME "GIMP" dans Flathub → nix attr "gimp"
        assert!(flathub_relevant("GIMP", "gimp"));
    }

    #[test]
    fn hit_contains_query() {
        // "Firefox ESR" ⊃ "firefox"
        assert!(flathub_relevant("Firefox ESR", "firefox"));
    }

    #[test]
    fn query_does_not_match_just_because_it_contains_hit_name() {
        // "vlc-media-player" ⊃ "vlc" ne doit PAS matcher : même forme que les
        // faux positifs discord-rpc/steam-run (regression réelle).
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
        // Régression historique : "Extreme Tux Racer" vs nix attr "extremetuxracer".
        // L'étape strip-espaces est nécessaire car ni a⊂b ni b⊂a ne tiennent
        // quand a a des espaces et b n'en a pas.
        assert!(
            flathub_relevant("Extreme Tux Racer", "extremetuxracer"),
            "régression B2 : la correspondance avec espaces doit fonctionner"
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

    // ── Vrais négatifs (faux positifs à rejeter) ───────────────────────────

    #[test]
    fn no_fp_krita_vs_ripgrep() {
        assert!(!flathub_relevant("Krita", "ripgrep"));
    }

    #[test]
    fn no_fp_bython_vs_python3() {
        // "bython" et "python3" ne se contiennent pas mutuellement
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
        // Régression : un nom nixpkgs court (`go`, `jq`, `fd`...) ne doit pas
        // matcher n'importe quel hit Flathub qui le contient par hasard.
        assert!(!flathub_relevant("GoodVibes", "go"));
        assert!(!flathub_relevant("Djview", "jv"));
    }

    #[test]
    fn short_exact_match_still_matches() {
        // Le garde-fou de longueur ne doit pas empêcher un match EXACT court.
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
        // width 0 = image source de taille inconnue : ne doit être choisie
        // que s'il n'y a aucune variante dimensionnée qui convienne.
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
