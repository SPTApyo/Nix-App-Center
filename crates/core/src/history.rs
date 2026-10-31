//! Generations to go back to. Each apply records what Nix App Center kept
//! then, so going back restores the configuration with the system: without
//! it, the next apply would bring the newer state back.

use std::path::{Path, PathBuf};

use crate::paths::data_dir;

/// Whose generations: the system's, or standalone Home Manager's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    System,
    Home,
}

impl Scope {
    fn name(self) -> &'static str {
        match self {
            Scope::System => "system",
            Scope::Home => "home",
        }
    }
}

/// A generation one can go back to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    pub scope: Scope,
    pub number: u32,
    /// Creation time (Unix seconds).
    pub date: u64,
    /// NixOS version of a system generation.
    pub version: Option<String>,
    pub current: bool,
    pub booted: bool,
    /// The apps Nix App Center kept at this point; `None` when another
    /// tool made it.
    pub kept: Option<Vec<String>>,
}

/// The generations of the configuration's scope, newest first.
pub fn history() -> Vec<Point> {
    match scope() {
        Some(scope) => points(scope),
        None => Vec::new(),
    }
}

/// The scope going back acts on: the system when Nix App Center writes
/// there, else standalone Home Manager.
pub fn scope() -> Option<Scope> {
    let layout = crate::detect_layout();
    match layout.kind {
        crate::LayoutKind::Unknown => None,
        _ if layout.system.is_some() => Some(Scope::System),
        _ => profile(Scope::Home).map(|_| Scope::Home),
    }
}

/// The profile link of `scope`.
fn profile(scope: Scope) -> Option<PathBuf> {
    match scope {
        Scope::System => Some(PathBuf::from("/nix/var/nix/profiles/system")),
        Scope::Home => home_profile(),
    }
}

/// The current Home Manager generation, where Home Manager keeps it.
pub(crate) fn home_profile() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::util::home().join(".local/state"));
    let user = std::env::var("USER").unwrap_or_default();
    [
        state.join("nix/profiles/home-manager"),
        PathBuf::from(format!(
            "/nix/var/nix/profiles/per-user/{user}/home-manager"
        )),
    ]
    .into_iter()
    .find(|p| p.exists())
}

fn points(scope: Scope) -> Vec<Point> {
    match profile(scope) {
        Some(profile) => points_of(scope, &profile),
        None => Vec::new(),
    }
}

fn points_of(scope: Scope, profile: &Path) -> Vec<Point> {
    let current = std::fs::read_link(profile).ok();
    let booted = std::fs::canonicalize("/run/booted-system").ok();
    let mut out: Vec<Point> = links(profile)
        .into_iter()
        .map(|(number, link)| {
            let name = link.file_name().map(PathBuf::from);
            Point {
                scope,
                number,
                date: std::fs::symlink_metadata(&link)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs()),
                version: std::fs::read_to_string(link.join("nixos-version"))
                    .ok()
                    .map(|v| v.trim().to_string()),
                current: name.is_some() && name == current,
                booted: scope == Scope::System
                    && booted.is_some()
                    && std::fs::canonicalize(&link).ok() == booted,
                kept: snapshot_kept(scope, number),
            }
        })
        .collect();
    out.sort_by_key(|p| std::cmp::Reverse(p.number));
    // Identical generations share the booted system: name the newest.
    let mut seen = false;
    for p in &mut out {
        let first = p.booted && !seen;
        seen |= p.booted;
        p.booted = first;
    }
    out
}

/// The generation links of `profile`: `<profile>-<n>-link`.
fn links(profile: &Path) -> Vec<(u32, PathBuf)> {
    let (Some(dir), Some(base)) = (
        profile.parent(),
        profile.file_name().and_then(|n| n.to_str()),
    ) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let n = name
                .strip_prefix(base)?
                .strip_prefix('-')?
                .strip_suffix("-link")?
                .parse()
                .ok()?;
            Some((n, e.path()))
        })
        .collect()
}

fn snapshot_dir(scope: Scope, number: u32) -> PathBuf {
    data_dir()
        .join("history")
        .join(format!("{}-{number}", scope.name()))
}

fn snapshot_kept(scope: Scope, number: u32) -> Option<Vec<String>> {
    let text = std::fs::read(snapshot_dir(scope, number).join("kept.json")).ok()?;
    let m: crate::Manifest = serde_json::from_slice(&text).ok()?;
    let mut attrs: Vec<String> = m.kept.into_iter().map(|e| e.package.attr).collect();
    attrs.sort();
    Some(attrs)
}

/// Records what the current generation holds, after an apply: the
/// manifest and the flake.lock. Forgets generations that are gone.
pub fn record() {
    let Some(scope) = scope() else { return };
    let points = points(scope);
    if let Some(current) = points.iter().find(|p| p.current) {
        let dir = snapshot_dir(scope, current.number);
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(
                dir.join("kept.json"),
                serde_json::to_vec_pretty(&crate::load_manifest()).unwrap_or_default(),
            );
            match flake_lock() {
                Some(lock) => {
                    let _ = std::fs::copy(lock, dir.join("flake.lock"));
                }
                None => {
                    let _ = std::fs::remove_file(dir.join("flake.lock"));
                }
            }
        }
    }
    let alive: Vec<String> = points
        .iter()
        .map(|p| format!("{}-{}", scope.name(), p.number))
        .collect();
    if let Ok(entries) = std::fs::read_dir(data_dir().join("history")) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(scope.name()) && !alive.contains(&name) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

/// The configuration's flake.lock, when it is a flake.
fn flake_lock() -> Option<PathBuf> {
    match crate::detect_layout().kind {
        crate::LayoutKind::NixosFlake { dir, .. } | crate::LayoutKind::HmFlake { dir, .. } => {
            Some(dir.join("flake.lock")).filter(|p| p.exists())
        }
        _ => None,
    }
}

/// The commands switching to `point`; nothing when it is gone.
pub fn go_back_steps(point: &Point) -> Option<Vec<Vec<String>>> {
    let profile = profile(point.scope)?;
    let link = links(&profile)
        .into_iter()
        .find(|(n, _)| *n == point.number)?
        .1;
    match point.scope {
        Scope::System => Some(vec![vec![
            "sudo".into(),
            "sh".into(),
            "-c".into(),
            format!(
                "nix-env -p /nix/var/nix/profiles/system --switch-generation {} && /nix/var/nix/profiles/system/bin/switch-to-configuration switch",
                point.number
            ),
        ]]),
        Scope::Home => {
            let target = std::fs::canonicalize(&link).ok()?;
            let target = target.to_str()?;
            let safe = target.starts_with("/nix/store/")
                && target[11..]
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"+-._".contains(&b));
            safe.then(|| vec![vec![format!("{target}/activate")]])
        }
    }
}

/// True when `point` boots another kernel or initrd than the running
/// computer: it fully applies after a restart.
pub fn needs_restart(point: &Point) -> bool {
    let Some(link) = (point.scope == Scope::System)
        .then(|| links(Path::new("/nix/var/nix/profiles/system")))
        .and_then(|l| l.into_iter().find(|(n, _)| *n == point.number))
    else {
        return false;
    };
    differs(&link.1, Path::new("/run/booted-system"))
}

fn differs(a: &Path, b: &Path) -> bool {
    ["kernel", "initrd"]
        .iter()
        .any(|f| std::fs::canonicalize(a.join(f)).ok() != std::fs::canonicalize(b.join(f)).ok())
}

/// The specialisation running now, if any: going back starts the default
/// configuration of a generation.
pub fn running_specialisation() -> Option<String> {
    let current = std::fs::canonicalize("/run/current-system").ok()?;
    let system = Path::new("/nix/var/nix/profiles/system/specialisation");
    std::fs::read_dir(system).ok()?.flatten().find_map(|e| {
        (std::fs::canonicalize(e.path()).ok()? == current)
            .then(|| e.file_name().to_string_lossy().into_owned())
    })
}

/// After going back to `point`: puts back the manifest and flake.lock it
/// recorded, so the configuration matches the running system again.
/// False when Nix App Center did not make that generation.
pub fn restore(point: &Point) -> std::io::Result<bool> {
    let dir = snapshot_dir(point.scope, point.number);
    let Ok(text) = std::fs::read(dir.join("kept.json")) else {
        return Ok(false);
    };
    let manifest: crate::Manifest = serde_json::from_slice(&text).map_err(std::io::Error::other)?;
    crate::manifest::commit(&manifest)?;
    if let (Ok(lock), Some(target)) = (std::fs::read(dir.join("flake.lock")), flake_lock()) {
        crate::paths::write_files(&[(target, lock)])?;
    }
    crate::applied();
    Ok(true)
}

/// What changed in the apps Nix App Center kept from `older` to `newer`:
/// (added, removed).
pub fn kept_diff(older: &[String], newer: &[String]) -> (Vec<String>, Vec<String>) {
    let added = newer
        .iter()
        .filter(|a| !older.contains(a))
        .cloned()
        .collect();
    let removed = older
        .iter()
        .filter(|a| !newer.contains(a))
        .cloned()
        .collect();
    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_generation_links() {
        let dir = std::env::temp_dir().join(format!("nix-app-center-gens-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "system-3-link",
            "system-12-link",
            "system",
            "system-x-link",
            "other-1-link",
        ] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        let mut found = links(&dir.join("system"));
        found.sort();
        assert_eq!(
            found,
            vec![
                (3, dir.join("system-3-link")),
                (12, dir.join("system-12-link"))
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn marks_the_current_generation_newest_first() {
        let dir = std::env::temp_dir().join(format!("nix-app-center-hm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in [1, 2, 3] {
            std::os::unix::fs::symlink("/", dir.join(format!("home-manager-{n}-link"))).unwrap();
        }
        std::os::unix::fs::symlink("home-manager-2-link", dir.join("home-manager")).unwrap();
        let points = points_of(Scope::Home, &dir.join("home-manager"));
        let numbers: Vec<(u32, bool)> = points.iter().map(|p| (p.number, p.current)).collect();
        assert_eq!(numbers, [(3, false), (2, true), (1, false)]);
        assert!(points.iter().all(|p| !p.booted));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn another_kernel_needs_a_restart() {
        let dir =
            std::env::temp_dir().join(format!("nix-app-center-kernel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, kernel) in [("a", "k1"), ("b", "k1"), ("c", "k2")] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
            std::fs::write(dir.join(kernel), "").unwrap();
            std::os::unix::fs::symlink(dir.join(kernel), dir.join(name).join("kernel")).unwrap();
        }
        assert!(!differs(&dir.join("a"), &dir.join("b")));
        assert!(differs(&dir.join("a"), &dir.join("c")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_the_home_manager_profile_in_the_state_folder() {
        let _g = crate::env_lock();
        let dir = std::env::temp_dir().join(format!("nix-app-center-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let profiles = dir.join("nix/profiles");
        std::fs::create_dir_all(&profiles).unwrap();
        std::os::unix::fs::symlink("/", profiles.join("home-manager-1-link")).unwrap();
        std::os::unix::fs::symlink("home-manager-1-link", profiles.join("home-manager")).unwrap();
        std::env::set_var("XDG_STATE_HOME", &dir);
        assert_eq!(home_profile(), Some(profiles.join("home-manager")));
        let numbers: Vec<u32> = points(Scope::Home).iter().map(|p| p.number).collect();
        std::env::remove_var("XDG_STATE_HOME");
        assert_eq!(numbers, [1]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn going_back_restores_the_recorded_apps() {
        let _g = crate::env_lock();
        let base =
            std::env::temp_dir().join(format!("nix-app-center-restore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("configuration.nix"), "{ imports = [ ]; }").unwrap();
        std::env::set_var("XDG_DATA_HOME", &base);
        std::env::set_var("XDG_CONFIG_HOME", &base);
        std::env::set_var("NIX_APP_CENTER_CONFIG_DIR", &base);
        let point = |number| Point {
            scope: Scope::System,
            number,
            date: 0,
            version: None,
            current: false,
            booted: false,
            kept: None,
        };
        let dir = snapshot_dir(Scope::System, 5);
        std::fs::create_dir_all(&dir).unwrap();
        let kept = crate::Manifest {
            kept: ["vlc", "gimp"]
                .map(|a| crate::KeptEntry::simple(crate::Package::from_attr(a)))
                .into(),
        };
        std::fs::write(dir.join("kept.json"), serde_json::to_vec(&kept).unwrap()).unwrap();
        assert_eq!(snapshot_kept(Scope::System, 5).unwrap(), ["gimp", "vlc"]);
        assert!(!restore(&point(4)).unwrap(), "nothing recorded for 4");
        assert!(restore(&point(5)).unwrap());
        assert_eq!(crate::load_manifest().packages().len(), 2);
        let home = std::fs::read_to_string(base.join("nix-app-center-home.nix")).unwrap();
        assert!(home.contains("vlc") && home.contains("gimp"), "{home}");
        assert!(
            !crate::apply_pending(),
            "the restored state is the running one"
        );
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("NIX_APP_CENTER_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn diffs_kept_apps() {
        let (added, removed) = kept_diff(
            &["firefox".into(), "vlc".into()],
            &["firefox".into(), "steam".into()],
        );
        assert_eq!(added, ["steam"]);
        assert_eq!(removed, ["vlc"]);
    }
}
