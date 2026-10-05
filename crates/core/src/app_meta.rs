//! Merges AppStream metadata (Flathub, local or REST) and Nix (`nix eval
//! .meta`) for the details page: **AppStream first, Nix as fallback**.

use crate::appstream_db;
use crate::detail::{detail, PackageDetail};
use crate::flathub::{flathub_meta, meta_from_db, FlathubMeta, Release};
use crate::package::Package;

/// Merged metadata for the details page: **AppStream (Flathub) first, Nix
/// (`meta`) as fallback**. Each empty field hides the matching section in the
/// UI.
#[derive(Debug, Clone, Default)]
pub struct AppMeta {
    pub name: String,
    /// Resolved AppStream/Flathub id (`io.github.…`), the reliable key for the
    /// ODRS rating (the nixpkgs name alone causes wrong matches).
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
    /// nixpkgs maintainers, a Nix native trust signal (no AppStream
    /// equivalent, never provided by `fh`).
    pub maintainers: Vec<String>,
    /// Teams maintaining the package in nixpkgs.
    pub teams: Vec<String>,
    /// The package recipe on GitHub, at the revision the config is pinned to.
    pub source_url: Option<String>,
    pub download_page: Option<String>,
    /// The package is marked `meta.insecure` in nixpkgs.
    pub insecure: bool,
    pub known_vulnerabilities: Vec<String>,
    /// The package is marked `meta.broken`: `nix run` will most likely fail.
    pub broken: bool,
    /// Caption of each screenshot (parallel to `screenshot_urls`).
    pub screenshot_captions: Vec<Option<String>>,
    /// OARS age rating ("12") + descriptions, from AppStream.
    pub age_label: Option<String>,
    pub age_descriptions: Vec<String>,
    /// Extensions/add-ons (AppStream).
    pub addons: Vec<crate::flathub::Addon>,
    /// Other apps by the same developer (AppStream).
    pub developer_apps: Vec<crate::flathub::DevApp>,
}

/// Builds the merged metadata of a package (AppStream first, Nix fallback).
/// Makes network/`nix eval` calls: run it in a thread. Both sources are
/// independent and queried in parallel, so the details page latency is the
/// max of the two, not their sum (AppStream may cost 2×6 s of network
/// timeout, `nix eval` several seconds when cold).
pub fn app_meta(pkg: &Package, screenshot_target_w: u32) -> AppMeta {
    let attr = pkg.attr.clone();
    let nix_handle = std::thread::spawn(move || detail(&attr));
    let fh = flathub_meta_for(pkg, screenshot_target_w);
    let nix = nix_handle.join().ok().flatten();
    merge_app_meta(pkg, fh.as_ref(), nix.as_ref())
}

/// Like `flathub_meta(&pkg.name, …)` but first tries the local database by
/// name THEN by nixpkgs attr (`appstream_db::lookup_package`) before falling
/// back on `flathub_meta`, which only knows the name.
///
/// Waits (bounded) for a database load IN PROGRESS: the local database holds
/// data REST lacks (add-ons, same developer apps), and the details page
/// already shows a spinner while `nix eval` runs in parallel, so a few
/// seconds beat a poorer listing. Without a load in progress (CLI), returns
/// at once.
fn flathub_meta_for(pkg: &Package, screenshot_target_w: u32) -> Option<FlathubMeta> {
    if crate::resolve::is_unmatched(&pkg.attr) {
        return None;
    }
    appstream_db::wait_until_settled(std::time::Duration::from_secs(15));
    if let Some(entry) = appstream_db::lookup_package(pkg) {
        return Some(meta_from_db(entry));
    }
    flathub_meta(&pkg.name, screenshot_target_w)
}

/// GitHub link of a `meta.position` (`/nix/store/…-source/pkgs/x.nix:42`).
fn source_url(position: &str, rev: &str) -> Option<String> {
    let (_, rest) = position.split_once("-source/")?;
    let (file, line) = rest.rsplit_once(':').unwrap_or((rest, ""));
    let anchor = if line.is_empty() {
        String::new()
    } else {
        format!("#L{line}")
    };
    Some(format!(
        "https://github.com/NixOS/nixpkgs/blob/{rev}/{file}{anchor}"
    ))
}

/// Merges the AppStream metadata (Flathub, local or REST) and Nix (`nix eval
/// .meta`) of a package. **Pure**, no I/O, so testable without network or `nix`.
///
/// General rule: **AppStream first, Nix fallback** for editorial fields
/// (summary/description/license/homepage). Two deliberate exceptions to this
/// rule, documented here so nobody "fixes" them by mistake later:
/// - `version`: the NIX version (the one really installable/runnable through
///   `nix run`) wins over the latest Flathub `Release`, which may match a
///   Flatpak build different from nixpkgs.
/// - `is_runnable`: decided only by `meta.mainProgram` (Nix); AppStream has
///   no reliable equivalent for this notion.
fn merge_app_meta(pkg: &Package, fh: Option<&FlathubMeta>, nix: Option<&PackageDetail>) -> AppMeta {
    // a (AppStream) first, else b (Nix).
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
            .or_else(|| nix.and_then(|n| n.version.clone()))
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
        teams: nix.map(|n| n.teams.clone()).unwrap_or_default(),
        source_url: nix
            .and_then(|n| n.position.as_deref())
            .and_then(|p| source_url(p, crate::updates::pinned_rev().unwrap_or("master"))),
        download_page: nix.and_then(|n| n.download_page.clone()),
        insecure: nix.map(|n| n.insecure).unwrap_or(false),
        known_vulnerabilities: nix
            .map(|n| n.known_vulnerabilities.clone())
            .unwrap_or_default(),
        broken: nix.map(|n| n.broken).unwrap_or(false),
        screenshot_captions: fh
            .map(|f| f.screenshot_captions.clone())
            .unwrap_or_default(),
        age_label: fh.and_then(|f| f.age_label.clone()),
        age_descriptions: fh.map(|f| f.age_descriptions.clone()).unwrap_or_default(),
        addons: fh.map(|f| f.addons.clone()).unwrap_or_default(),
        developer_apps: fh.map(|f| f.developer_apps.clone()).unwrap_or_default(),
    }
}

// ── Tests: AppStream ↔ Nix precedence ────────────────────────────────────────
//
// These tests pin the rule documented on `merge_app_meta`: AppStream first /
// Nix fallback for editorial fields, except deliberately for `version` and
// `is_runnable`.
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
        assert_eq!(
            meta.description.as_deref(),
            Some("Full description from AppStream")
        );
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
        // The local AppStream database never carries this boolean (unlike the
        // Flathub REST API); nix.meta.license.free fills the gap.
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
        // A present but empty/blank AppStream field must NOT win against an
        // available Nix fallback (`!s.trim().is_empty()` filter).
        let pkg = sample_pkg();
        let mut fh = sample_fh();
        fh.license = Some("   ".into());
        let nix = sample_nix();
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert_eq!(meta.license.as_deref(), Some("GPL-3.0"));
    }

    #[test]
    fn merge_version_prefers_nix_over_appstream_release() {
        // Documented exception: the runnable (nix) version wins over the latest
        // Flathub release (possibly a different build).
        let pkg = sample_pkg(); // nix version = "1.5"
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
    fn merge_version_uses_nix_meta_before_appstream_release() {
        let mut pkg = sample_pkg();
        pkg.version = String::new();
        let nix = PackageDetail {
            version: Some("1.4".into()),
            ..sample_nix()
        };
        let meta = merge_app_meta(&pkg, Some(&sample_fh()), Some(&nix));
        assert_eq!(meta.version.as_deref(), Some("1.4"));
    }

    #[test]
    fn merge_is_runnable_comes_from_nix_meta_only() {
        let pkg = sample_pkg();
        let fh = sample_fh();
        let mut nix = sample_nix();
        nix.main_program = None; // no mainProgram → KeepOnly
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert!(!meta.is_runnable);
    }

    #[test]
    fn merge_is_runnable_defaults_true_without_nix_detail() {
        // No nix eval (failed/missing): assume runnable by default rather
        // than wrongly hiding the "Run" action.
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
    fn source_url_points_at_the_recipe_line() {
        assert_eq!(
            source_url("/nix/store/abc-source/pkgs/by-name/vl/vlc/package.nix:315", "cf5e765").as_deref(),
            Some("https://github.com/NixOS/nixpkgs/blob/cf5e765/pkgs/by-name/vl/vlc/package.nix#L315")
        );
        assert_eq!(source_url("/home/me/overlay.nix:3", "x"), None);
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
        assert_eq!(
            meta.known_vulnerabilities,
            vec!["CVE-2024-0001".to_string()]
        );

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
