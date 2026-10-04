//! Recherche nixpkgs classée par pertinence, enrichie d'icônes AppStream.

use crate::appstream_db;
use crate::flathub::flathub_icon_map;
use crate::package::Package;
use crate::ratings::{odrs_ratings, rating_for, Rating};
use crate::util::{regex_escape, sanitize};
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;

#[derive(Deserialize)]
pub(crate) struct NixPkgEntry {
    pub(crate) pname: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) description: Option<String>,
}

/// TTL du cache disque des résultats bruts `nix search` (`cache::cached_search`).
/// `nix search` évalue tout nixpkgs à froid (30s-2min selon la machine, cf.
/// CONTEXT.md « 1er appel lent ») ; nixpkgs ne change pas assez vite pour que la
/// fraîcheur seconde-par-seconde importe, donc on absorbe ce coût une fois par
/// heure plutôt qu'à chaque lancement de l'appli.
pub(crate) const SEARCH_CACHE_TTL_SECS: u64 = 3600;

/// Invoque `nix search nixpkgs <patterns...> --json` et convertit chaque entrée
/// en `Package` (attr raccourci, champs par défaut si absents). `[]` si `nix`
/// échoue/absent ou si le JSON est invalide — jamais de panique sur une
/// machine sans `nix` ou avec une sortie inattendue.
pub(crate) fn raw_nix_search(patterns: &[String]) -> Vec<Package> {
    let mut args: Vec<String> = vec!["search".into(), "nixpkgs".into()];
    args.extend(patterns.iter().cloned());
    args.extend([
        "--json".into(),
        "--extra-experimental-features".into(),
        "nix-command flakes".into(),
    ]);

    let output = match Command::new("nix").args(&args).output() {
        Ok(o) if o.status.success() => o,
        _ => return vec![],
    };

    let raw: HashMap<String, NixPkgEntry> = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    // L'attr nix est "legacyPackages.x86_64-linux.<attrpath>" — on garde <attrpath>.
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

/// Recherche des paquets nixpkgs, classés par pertinence façon GNOME Software.
///
/// 1. **Tous les tokens doivent matcher** (chaque mot passé séparément à
///    `nix search`, qui les combine en ET).
/// 2. Résultats **scorés par champ** : nom/attr exact ≫ commence par ≫ contient ≫
///    description, avec une légère pénalité de longueur (favorise les noms courts
///    et exacts). C'est ce qui fait remonter le paquet voulu en tête.
///
/// L'appel `nix search` brut est mis en cache disque (`cache::cached_search`,
/// TTL [`SEARCH_CACHE_TTL_SECS`]) : le classement/dédoublonnage ci-dessous est
/// systématiquement recalculé, seul le coût de l'évaluation nixpkgs est évité.
pub fn search(query: &str) -> Vec<Package> {
    let query = query.trim();
    if query.is_empty() {
        return vec![];
    }

    // Un argument par token (ET), chaque token échappé pour le moteur regex de nix.
    let tokens: Vec<String> = query.split_whitespace().map(regex_escape).collect();

    let packages: Vec<Package> =
        crate::cache::cached_search(query, SEARCH_CACHE_TTL_SECS, || raw_nix_search(&tokens));

    // nixpkgs expose des alias de CASSE (`superTux` ↔ `supertux`, `superTuxKart` ↔
    // `supertuxkart`) : deux attrs distincts pour le MÊME paquet (même pname/version/
    // description). `nix search` les renvoie tous les deux → doublons à l'affichage.
    // On fusionne les attrs identiques à la casse près, en gardant la forme canonique
    // (tout en minuscules, généralement = pname).
    let mut canon: HashMap<String, Package> = HashMap::new();
    for p in packages {
        let key = p.attr.to_lowercase();
        match canon.get_mut(&key) {
            None => {
                canon.insert(key, p);
            }
            Some(existing) => {
                // Nom AFFICHÉ : préférer la variante d'attribut AVEC majuscules
                // (`superTux` → « SuperTux ») — le `pname` seul est tout en minuscules.
                let p_upper = p.attr.chars().any(char::is_uppercase);
                let ex_upper = existing.attr.chars().any(char::is_uppercase);
                if p_upper {
                    existing.name = beautify_attr(&p.attr);
                } else if ex_upper {
                    let a = existing.attr.clone();
                    existing.name = beautify_attr(&a);
                }
                // Attribut pour LANCER : préférer la forme minuscule réelle
                // (`nix run nixpkgs#supertux`, l'alias canonique le plus sûr).
                if p.attr == key {
                    existing.attr = p.attr;
                }
            }
        }
    }
    let mut packages: Vec<Package> = canon.into_values().collect();

    // Classement par pertinence (score décroissant, puis nom).
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

/// Un résultat de `search_apps` : le paquet nix + l'habillage AppStream
/// éventuel. `recognized` distingue « apparié à une entrée AppStream » de
/// « a une icône » : une app reconnue sans icône reste une application
/// (nom officiel, note ODRS) et ne doit pas tomber dans « Autres paquets ».
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub package: Package,
    pub icon_url: Option<String>,
    pub rating: Option<Rating>,
    pub recognized: bool,
}

/// Recherche enrichie : résultats nix classés + **icône AppStream** quand
/// l'app est reconnue (base locale Flatpak si prête, sinon un seul appel
/// `/search` Flathub par requête, mappé sur les paquets nix par nom/attr).
///
/// La base locale ne connaît pas forcément TOUT le catalogue Flathub (cache
/// Flatpak jamais synchronisé ou périmé) : les résultats qu'elle ne reconnaît
/// pas se rabattent donc, un par un, sur le même appel REST que lorsque la
/// base n'est pas prête du tout — sans refaire d'appel réseau si la base
/// locale a déjà résolu tous les résultats (cas courant, coût nul).
pub fn search_apps(query: &str) -> Vec<SearchResult> {
    let rate_handle = std::thread::spawn(odrs_ratings);
    let db_ready = appstream_db::is_ready();

    // Base pas prête : on sait déjà qu'il faudra le réseau → lancé en
    // parallèle de `search()` (comme avant l'ajout de la base locale).
    let icon_handle = (!db_ready).then(|| {
        let q = query.to_string();
        std::thread::spawn(move || flathub_icon_map(&q))
    });

    let pkgs = search(query);

    let hits = match icon_handle {
        Some(h) => h.join().unwrap_or_default(),
        None if pkgs.iter().any(|p| appstream_db::lookup_package(p).is_none()) => {
            flathub_icon_map(query)
        }
        None => HashMap::new(),
    };

    let _ = rate_handle.join();

    pkgs.into_iter()
        .map(|mut p| {
            if let Some(e) = appstream_db::lookup_package(&p) {
                if !e.name.is_empty() {
                    p.name = e.name.clone();
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
                .or_else(|| hits.get(&sanitize(&p.name)));
            let icon_url = hit.map(|h| h.icon.clone());
            let rating = hit.and_then(|h| rating_for(&h.app_id));
            let recognized = hit.is_some();
            if let Some(h) = hit {
                if !h.name.trim().is_empty() {
                    p.name = h.name.clone();
                }
            }
            SearchResult { package: p, icon_url, rating, recognized }
        })
        .collect()
}

/// Score de pertinence d'un paquet pour une requête (poids inspirés des champs
/// AppStream de GNOME : nom ≫ contient ≫ description).
fn search_score(pkg: &Package, q: &str, toks: &[String]) -> i64 {
    let attr = pkg.attr.to_lowercase();
    let name = pkg.name.to_lowercase();
    let desc = pkg.description.to_lowercase();
    let mut s: i64 = 0;

    if attr == q || name == q {
        s += 1000; // correspondance exacte
    } else if attr.starts_with(q) || name.starts_with(q) {
        s += 600; // commence par
    } else if attr.contains(q) || name.contains(q) {
        s += 300; // contient (nom)
    }

    // Tous les tokens présents dans le nom/attr (multi-mots).
    if toks.iter().all(|t| attr.contains(t.as_str()) || name.contains(t.as_str())) {
        s += 150;
    }
    // Tous les tokens présents dans la description (poids faible).
    if toks.iter().all(|t| desc.contains(t.as_str())) {
        s += 30;
    }

    // Favorise les noms courts (plus proches d'un match exact).
    s -= (attr.chars().count() as i64).min(60);
    s
}

/// Embellit un attribut nixpkgs pour l'affichage : capitalise la 1ʳᵉ lettre en
/// gardant la casse interne (`superTux` → « SuperTux », `superTuxKart` → « SuperTuxKart »).
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
        // Deux attrs qui "contiennent" la requête sans commencer par elle :
        // le plus court doit gagner (pénalité de longueur).
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
