//! Details page metadata: AppStream first, Nix as fallback.

use crate::appstream_db;
use crate::detail::{detail, PackageDetail};
use crate::flathub::{flathub_meta, meta_from_db, FlathubMeta, Release};
use crate::package::Package;

/// Metadata of the details page; an empty field hides its section.
#[derive(Debug, Clone, Default)]
pub struct AppMeta {
    pub name: String,
    /// AppStream id, the key for ratings and reviews.
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
    /// nixpkgs maintainers.
    pub maintainers: Vec<String>,
    /// nixpkgs teams.
    pub teams: Vec<String>,
    /// The recipe on GitHub, at the pinned revision.
    pub source_url: Option<String>,
    /// Boolean arguments of the recipe (`withQt5 ? true`).
    pub build_flags: Vec<crate::BuildFlag>,
    pub download_page: Option<String>,
    /// Commands the app installs (AppStream `provides`, else `mainProgram`).
    pub binaries: Vec<String>,
    /// File types the app opens.
    pub mediatypes: Vec<String>,
    /// Desktop file id, to recognize the installed app.
    pub launchable: Option<String>,
    /// Brand colors for light and dark backgrounds.
    pub brand_light: Option<String>,
    pub brand_dark: Option<String>,
    /// `meta.insecure`.
    pub insecure: bool,
    pub known_vulnerabilities: Vec<String>,
    /// `meta.broken`.
    pub broken: bool,
    /// Caption of each screenshot (parallel to `screenshot_urls`).
    pub screenshot_captions: Vec<Option<String>>,
    /// OARS age label and descriptions.
    pub age_label: Option<String>,
    pub age_descriptions: Vec<String>,
    /// Add-ons.
    pub addons: Vec<crate::flathub::Addon>,
    /// Other apps of the developer.
    pub developer_apps: Vec<crate::flathub::DevApp>,
}

/// Metadata of `pkg`, AppStream and Nix fetched in parallel. Blocking.
pub fn app_meta(pkg: &Package, screenshot_target_w: u32) -> AppMeta {
    let owned = pkg.clone();
    let nix_handle = std::thread::spawn(move || detail(&owned));
    let fh = flathub_meta_for(pkg, screenshot_target_w);
    let nix = nix_handle.join().ok().flatten();
    merge_app_meta(pkg, fh.as_ref(), nix.as_ref())
}

/// Flathub metadata of `pkg`, by name then by attr. Waits for a
/// database load in progress: it has add-ons and developer apps.
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

/// GitHub link of a `meta.position` at `rev`.
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

/// Merges AppStream and Nix metadata, AppStream first. Except the
/// version and runnability, which come from Nix: that is what runs.
fn merge_app_meta(pkg: &Package, fh: Option<&FlathubMeta>, nix: Option<&PackageDetail>) -> AppMeta {
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
        build_flags: nix
            .and_then(|n| n.position.as_deref())
            .map(crate::build_flags::build_flags)
            .unwrap_or_default(),
        download_page: nix.and_then(|n| n.download_page.clone()),
        binaries: fh
            .map(|f| f.binaries.clone())
            .filter(|b| !b.is_empty())
            .or_else(|| nix.and_then(|n| n.main_program.clone()).map(|m| vec![m]))
            .unwrap_or_default(),
        mediatypes: fh.map(|f| f.mediatypes.clone()).unwrap_or_default(),
        launchable: fh.and_then(|f| f.launchable.clone()),
        brand_light: fh.and_then(|f| f.brand_light.clone()),
        brand_dark: fh.and_then(|f| f.brand_dark.clone()),
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
            source: None,
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
        // Local AppStream lacks it: Nix fills it.
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
        // A blank AppStream field must not win.
        let pkg = sample_pkg();
        let mut fh = sample_fh();
        fh.license = Some("   ".into());
        let nix = sample_nix();
        let meta = merge_app_meta(&pkg, Some(&fh), Some(&nix));
        assert_eq!(meta.license.as_deref(), Some("GPL-3.0"));
    }

    #[test]
    fn merge_version_prefers_nix_over_appstream_release() {
        // The Nix version wins: it is what runs.
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
        // Without Nix data, assume runnable.
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
