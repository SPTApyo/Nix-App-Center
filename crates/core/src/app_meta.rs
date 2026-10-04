//! Fusion des métadonnées AppStream (Flathub, local ou REST) et Nix (`nix eval
//! .meta`) pour la page détail : **AppStream prioritaire, Nix en fallback**.

use crate::appstream_db;
use crate::detail::{detail, PackageDetail};
use crate::flathub::{flathub_meta, meta_from_db, FlathubMeta, Release};
use crate::package::Package;

/// Métadonnées fusionnées pour la page détail : **AppStream (Flathub) prioritaire,
/// Nix (`meta`) en fallback**. Chaque champ vide ⇒ la section correspondante est
/// masquée côté UI.
#[derive(Debug, Clone, Default)]
pub struct AppMeta {
    pub name: String,
    /// Identifiant AppStream/Flathub résolu (`io.github.…`) — clé fiable pour la
    /// note ODRS (le nom nixpkgs seul provoque de faux appariements).
    pub app_id: Option<String>,
    pub developer: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub is_free_license: Option<bool>,
    pub version: Option<String>,
    pub is_runnable: bool,
    pub verified: bool,
    pub icon_url: Option<String>,
    pub screenshot_urls: Vec<String>,
    pub releases: Vec<Release>,
    pub homepage: Option<String>,
    pub bugtracker: Option<String>,
    pub help: Option<String>,
    pub donation: Option<String>,
    pub translate: Option<String>,
    pub changelog: Option<String>,
    /// Mainteneurs nixpkgs — signal de confiance Nix-natif (pas d'équivalent
    /// AppStream, jamais fourni par `fh`).
    pub maintainers: Vec<String>,
    /// Le paquet est marqué `meta.insecure` dans nixpkgs.
    pub insecure: bool,
    pub known_vulnerabilities: Vec<String>,
    /// Le paquet est marqué `meta.broken` — `nix run` échouera très probablement.
    pub broken: bool,
    /// Légende de chaque screenshot (parallèle à `screenshot_urls`).
    pub screenshot_captions: Vec<Option<String>>,
    /// Classification d'âge OARS (« 12 ») + descriptions, via AppStream.
    pub age_label: Option<String>,
    pub age_descriptions: Vec<String>,
    /// Extensions/addons (AppStream Flathub).
    pub addons: Vec<crate::flathub::Addon>,
    /// Autres apps du même développeur (AppStream Flathub).
    pub developer_apps: Vec<crate::flathub::DevApp>,
}

/// Construit les métadonnées fusionnées d'un paquet (AppStream prioritaire, Nix
/// fallback). Effectue les appels réseau/`nix eval` : à lancer dans un thread.
/// Les deux sources sont indépendantes → interrogées en parallèle : la latence
/// de la page détail est le max des deux, pas leur somme (AppStream peut coûter
/// 2×6 s de timeout réseau, `nix eval` plusieurs secondes à froid).
pub fn app_meta(pkg: &Package, screenshot_target_w: u32) -> AppMeta {
    let attr = pkg.attr.clone();
    let nix_handle = std::thread::spawn(move || detail(&attr));
    let fh = flathub_meta_for(pkg, screenshot_target_w);
    let nix = nix_handle.join().ok().flatten();
    merge_app_meta(pkg, fh.as_ref(), nix.as_ref())
}

/// Comme `flathub_meta(&pkg.name, …)` mais essaie d'abord la base locale par
/// nom PUIS par attr nixpkgs (`appstream_db::lookup_package`) avant de se
/// rabattre sur `flathub_meta`, qui ne connaît que le nom.
///
/// Attend (borné) la fin d'un chargement de base EN COURS : la base locale
/// porte des données que le REST n'a pas (addons, apps du même développeur),
/// et la page détail affiche déjà un spinner pendant que `nix eval` roule en
/// parallèle — mieux vaut ces quelques secondes qu'une fiche appauvrie.
/// Sans chargement en cours (CLI, machine sans Flatpak), retour immédiat.
fn flathub_meta_for(pkg: &Package, screenshot_target_w: u32) -> Option<FlathubMeta> {
    appstream_db::wait_until_settled(std::time::Duration::from_secs(15));
    if let Some(entry) = appstream_db::lookup_package(pkg) {
        return Some(meta_from_db(entry));
    }
    flathub_meta(&pkg.name, screenshot_target_w)
}

/// Fusionne les métadonnées AppStream (Flathub, local ou REST) et Nix (`nix eval
/// .meta`) d'un paquet — **pure**, sans I/O, donc testable sans réseau ni `nix`.
///
/// Règle générale : **AppStream prioritaire, Nix en fallback** pour les champs
/// éditoriaux (résumé/description/licence/homepage). Deux exceptions
/// délibérées à cette règle, documentées ici pour ne pas être « corrigées »
/// par erreur plus tard :
/// - `version` : la version NIX (celle réellement installée/lançable via
///   `nix run`) prime sur celle du dernier `Release` Flathub, qui peut
///   correspondre à un build Flatpak différent de celui de nixpkgs.
/// - `is_runnable` : déterminé uniquement par `meta.mainProgram` (Nix) —
///   AppStream n'a pas d'équivalent fiable pour cette notion.
fn merge_app_meta(pkg: &Package, fh: Option<&FlathubMeta>, nix: Option<&PackageDetail>) -> AppMeta {
    // a (AppStream) prioritaire, sinon b (Nix).
    let pick = |a: Option<String>, b: Option<String>| a.filter(|s| !s.trim().is_empty()).or(b);

    AppMeta {
        name: fh
            .map(|f| f.name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if pkg.name.is_empty() {
                    pkg.attr.clone()
                } else {
                    pkg.name.clone()
                }
            }),
        app_id: fh.map(|f| f.app_id.clone()).filter(|s| !s.is_empty()),
        developer: fh.and_then(|f| f.developer.clone()),
        summary: pick(
            fh.and_then(|f| f.summary.clone()),
            Some(pkg.description.clone()).filter(|s| !s.is_empty()),
        ),
        description: pick(
            fh.and_then(|f| f.description.clone()),
            nix.and_then(|n| n.long_description.clone())
                .or_else(|| Some(pkg.description.clone()).filter(|s| !s.is_empty())),
        ),
        license: pick(
            fh.and_then(|f| f.license.clone()),
            nix.and_then(|n| n.license.clone()),
        ),
        is_free_license: fh
            .and_then(|f| f.is_free_license)
            .or_else(|| nix.and_then(|n| n.is_free_license)),
        version: Some(pkg.version.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| fh.and_then(|f| f.releases.first().map(|r| r.version.clone()))),
        is_runnable: nix.map(|n| n.is_runnable()).unwrap_or(true),
        verified: fh.map(|f| f.verified).unwrap_or(false),
        icon_url: fh.and_then(|f| f.icon_url.clone()),
        screenshot_urls: fh.map(|f| f.screenshot_urls.clone()).unwrap_or_default(),
        releases: fh.map(|f| f.releases.clone()).unwrap_or_default(),
        homepage: pick(
            fh.and_then(|f| f.homepage.clone()),
            nix.and_then(|n| n.homepage.clone()),
        ),
        bugtracker: fh.and_then(|f| f.bugtracker.clone()),
        help: fh.and_then(|f| f.help.clone()),
        donation: fh.and_then(|f| f.donation.clone()),
        translate: fh.and_then(|f| f.translate.clone()),
        changelog: nix.and_then(|n| n.changelog.clone()),
        maintainers: nix.map(|n| n.maintainers.clone()).unwrap_or_default(),
        insecure: nix.map(|n| n.insecure).unwrap_or(false),
        known_vulnerabilities: nix.map(|n| n.known_vulnerabilities.clone()).unwrap_or_default(),
        broken: nix.map(|n| n.broken).unwrap_or(false),
        screenshot_captions: fh.map(|f| f.screenshot_captions.clone()).unwrap_or_default(),
        age_label: fh.and_then(|f| f.age_label.clone()),
        age_descriptions: fh.map(|f| f.age_descriptions.clone()).unwrap_or_default(),
        addons: fh.map(|f| f.addons.clone()).unwrap_or_default(),
        developer_apps: fh.map(|f| f.developer_apps.clone()).unwrap_or_default(),
    }
}

// ── Tests : l'ordre AppStream ↔ Nix ──────────────────────────────────────────
//
// Ces tests fixent la règle documentée sur `merge_app_meta` : AppStream
// prioritaire / Nix fallback pour les champs éditoriaux, à l'exception
// délibérée de `version` et `is_runnable`.
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_fh() -> FlathubMeta {
        FlathubMeta {
            app_id: "org.example.App".into(),
            name: "Example App".into(),
            summary: Some("An example".into()),
            developer: Some("Example Dev".into()),
            icon_url: Some("https://icon".into()),
            screenshot_urls: vec!["https://shot1".into()],
            description: Some("Full description from AppStream".into()),
            license: Some("MIT".into()),
            is_free_license: Some(true),
            homepage: Some("https://home".into()),
            bugtracker: None,
            help: None,
            donation: None,
            translate: None,
            releases: vec![Release {
                version: "2.0".into(),
                timestamp: 100,
                description: None,
                url: None,
            }],
            verified: true,
            ..Default::default()
        }
    }

    fn sample_nix() -> PackageDetail {
        PackageDetail {
            long_description: Some("Nix long description".into()),
            homepage: Some("https://nix-home".into()),
            license: Some("GPL-3.0".into()),
            changelog: Some("nix changelog".into()),
            main_program: Some("examplebin".into()),
            ..Default::default()
        }
    }

    fn sample_pkg() -> Package {
        Package {
            attr: "example".into(),
            name: "example".into(),
            version: "1.5".into(),
            description: "nix pname description".into(),
        }
    }

    #[test]
    fn merge_prefers_appstream_over_nix_for_editorial_fields() {
        let (pkg, fh, nix) = (sample_pkg(), sample_fh(), sample_nix());
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert_eq!(meta.description.as_deref(), Some("Full description from AppStream"));
        assert_eq!(meta.license.as_deref(), Some("MIT"));
        assert_eq!(meta.homepage.as_deref(), Some("https://home"));
    }

    #[test]
    fn merge_falls_back_to_nix_when_appstream_field_absent() {
        let pkg = sample_pkg();
        let mut fh = sample_fh();
        fh.description = None;
        fh.license = None;
        fh.homepage = None;
        let nix = sample_nix();
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert_eq!(meta.description.as_deref(), Some("Nix long description"));
        assert_eq!(meta.license.as_deref(), Some("GPL-3.0"));
        assert_eq!(meta.homepage.as_deref(), Some("https://nix-home"));
    }

    #[test]
    fn merge_is_free_license_falls_back_to_nix_when_appstream_lacks_it() {
        // La base AppStream locale ne porte jamais ce booléen (contrairement à
        // l'API REST Flathub) — nix.meta.license.free comble le trou.
        let pkg = sample_pkg();
        let mut fh = sample_fh();
        fh.is_free_license = None;
        let mut nix = sample_nix();
        nix.is_free_license = Some(false);
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert_eq!(meta.is_free_license, Some(false));
    }

    #[test]
    fn merge_falls_back_to_nix_when_appstream_field_is_blank() {
        // Un champ AppStream présent mais vide/blanc ne doit PAS gagner
        // contre un fallback Nix disponible (filtre `!s.trim().is_empty()`).
        let pkg = sample_pkg();
        let mut fh = sample_fh();
        fh.license = Some("   ".into());
        let nix = sample_nix();
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert_eq!(meta.license.as_deref(), Some("GPL-3.0"));
    }

    #[test]
    fn merge_version_prefers_nix_over_appstream_release() {
        // Exception documentée : la version lançable (nix) prime sur la
        // dernière release Flathub (potentiellement un build différent).
        let pkg = sample_pkg(); // version nix = "1.5"
        let fh = sample_fh(); // releases[0].version = "2.0"
        let meta = merge_app_meta(&pkg, Some(&fh), None);
        assert_eq!(meta.version.as_deref(), Some("1.5"));
    }

    #[test]
    fn merge_version_falls_back_to_appstream_release_when_nix_version_empty() {
        let mut pkg = sample_pkg();
        pkg.version = String::new();
        let fh = sample_fh();
        let meta = merge_app_meta(&pkg, Some(&fh), None);
        assert_eq!(meta.version.as_deref(), Some("2.0"));
    }

    #[test]
    fn merge_is_runnable_comes_from_nix_meta_only() {
        let pkg = sample_pkg();
        let fh = sample_fh();
        let mut nix = sample_nix();
        nix.main_program = None; // pas de mainProgram → KeepOnly
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert!(!meta.is_runnable);
    }

    #[test]
    fn merge_is_runnable_defaults_true_without_nix_detail() {
        // Pas d'éval nix (échec/absent) : on suppose runnable par défaut
        // plutôt que de masquer l'action « Lancer » à tort.
        let meta = merge_app_meta(&sample_pkg(), None, None);
        assert!(meta.is_runnable);
    }

    #[test]
    fn merge_icon_and_screenshots_only_come_from_appstream() {
        let (pkg, fh, nix) = (sample_pkg(), sample_fh(), sample_nix());
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert_eq!(meta.icon_url.as_deref(), Some("https://icon"));
        assert_eq!(meta.screenshot_urls, vec!["https://shot1".to_string()]);

        let meta_no_fh = merge_app_meta(&pkg, None, Some(&nix));
        assert!(meta_no_fh.icon_url.is_none());
        assert!(meta_no_fh.screenshot_urls.is_empty());
    }

    #[test]
    fn merge_changelog_only_comes_from_nix() {
        let (pkg, fh, nix) = (sample_pkg(), sample_fh(), sample_nix());
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert_eq!(meta.changelog.as_deref(), Some("nix changelog"));

        let meta_no_nix = merge_app_meta(&pkg, Some(&fh), None);
        assert!(meta_no_nix.changelog.is_none());
    }

    #[test]
    fn merge_maintainers_and_insecure_come_from_nix_only() {
        let (pkg, fh) = (sample_pkg(), sample_fh());
        let mut nix = sample_nix();
        nix.maintainers = vec!["Steffen Vogel".into()];
        nix.insecure = true;
        nix.known_vulnerabilities = vec!["CVE-2024-0001".into()];
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert_eq!(meta.maintainers, vec!["Steffen Vogel".to_string()]);
        assert!(meta.insecure);
        assert_eq!(meta.known_vulnerabilities, vec!["CVE-2024-0001".to_string()]);

        let meta_no_nix = merge_app_meta(&pkg, Some(&fh), None);
        assert!(meta_no_nix.maintainers.is_empty());
        assert!(!meta_no_nix.insecure);
    }

    #[test]
    fn merge_broken_comes_from_nix_only() {
        let (pkg, fh) = (sample_pkg(), sample_fh());
        let mut nix = sample_nix();
        nix.broken = true;
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert!(meta.broken);

        let meta_no_nix = merge_app_meta(&pkg, Some(&fh), None);
        assert!(!meta_no_nix.broken);
    }

    #[test]
    fn merge_name_falls_back_to_pkg_attr_when_everything_empty() {
        let mut pkg = sample_pkg();
        pkg.name = String::new();
        let meta = merge_app_meta(&pkg, None, None);
        assert_eq!(meta.name, "example");
    }
}
