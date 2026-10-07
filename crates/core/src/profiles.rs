//! Apps already on the machine, read from the Nix profiles the session uses.
//!
//! No evaluation: every profile exposes `share/applications`, and each desktop
//! file there is a symlink into the store path of its package, whose name
//! carries the pname and the version (`/nix/store/<hash>-vlc-3.0.23`).

use std::path::{Path, PathBuf};

/// Which profile an app comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The NixOS system profile (`environment.systemPackages`).
    System,
    /// The user profiles (`users.users.<u>.packages`, Home Manager, nix profile).
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileApp {
    pub scope: Scope,
    pub desktop_file: PathBuf,
    /// Store path of the package that ships the desktop file.
    pub store_path: PathBuf,
    pub pname: String,
    pub version: String,
}

/// The `share/applications` directories of the session, system first.
fn profile_dirs() -> Vec<(Scope, PathBuf)> {
    let user = crate::config_layout::current_user();
    let home = crate::util::home();
    vec![
        (Scope::System, PathBuf::from("/run/current-system/sw")),
        (
            Scope::User,
            PathBuf::from("/etc/profiles/per-user").join(user),
        ),
        (Scope::User, home.join(".nix-profile")),
    ]
}

/// Every app installed through a Nix profile, deduplicated by desktop id.
pub fn profile_apps() -> Vec<ProfileApp> {
    let mut out: Vec<ProfileApp> = Vec::new();
    for (scope, profile) in profile_dirs() {
        for app in apps_in(&profile.join("share/applications"), scope) {
            if !out
                .iter()
                .any(|a| a.desktop_file.file_name() == app.desktop_file.file_name())
            {
                out.push(app);
            }
        }
    }
    out.sort_by_key(|a| a.pname.to_lowercase());
    out
}

/// The installed app matching a package, by pname or by desktop id.
pub fn installed_app(attr: &str, app_id: Option<&str>) -> Option<ProfileApp> {
    let pname = attr.rsplit('.').next().unwrap_or(attr);
    profile_apps().into_iter().find(|a| {
        a.pname.eq_ignore_ascii_case(pname)
            || app_id.is_some_and(|id| {
                a.desktop_file
                    .file_stem()
                    .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(id))
            })
    })
}

/// Own size (NAR size) of each store path, in one query.
pub fn store_sizes(paths: &[PathBuf]) -> std::collections::HashMap<PathBuf, u64> {
    if paths.is_empty() {
        return Default::default();
    }
    let out = crate::util::nix()
        .args(["path-info", "--json"])
        .args(paths)
        .output();
    let Ok(out) = out else {
        return Default::default();
    };
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    crate::tried::parse_path_info(&v)
        .into_iter()
        .map(|(p, i)| (PathBuf::from(p), i.nar_size))
        .collect()
}

fn apps_in(dir: &Path, scope: Scope) -> Vec<ProfileApp> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "desktop"))
        .filter(|p| std::fs::read_to_string(p).is_ok_and(|text| shown(&text)))
        .filter_map(|desktop_file| {
            let target = std::fs::canonicalize(&desktop_file).ok()?;
            let (pname, version) = store_name(&target)?;
            let top = target
                .strip_prefix("/nix/store")
                .ok()?
                .components()
                .next()?;
            let store_path = Path::new("/nix/store").join(top);
            Some(ProfileApp {
                scope,
                desktop_file,
                store_path,
                pname,
                version,
            })
        })
        .collect()
}

/// False for entries hidden from menus.
fn shown(desktop_entry: &str) -> bool {
    !desktop_entry
        .lines()
        .any(|l| matches!(l.trim(), "NoDisplay=true" | "Hidden=true"))
}

/// pname and version of the store path holding `path`.
fn store_name(path: &Path) -> Option<(String, String)> {
    let rest = path.strip_prefix("/nix/store").ok()?;
    let dir = rest.components().next()?.as_os_str().to_str()?;
    let name = dir.get(33..)?;
    Some(split_name(name))
}

/// Splits `name-version` like Nix does: the version starts at the first dash
/// followed by something other than a letter.
pub(crate) fn split_name(name: &str) -> (String, String) {
    let bytes = name.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'-' && bytes.get(i + 1).is_some_and(|c| !c.is_ascii_alphabetic()) {
            return (name[..i].to_string(), name[i + 1..].to_string());
        }
    }
    (name.to_string(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_name_follows_nix_rules() {
        assert_eq!(split_name("vlc-3.0.23"), ("vlc".into(), "3.0.23".into()));
        assert_eq!(
            split_name("teams-for-linux-2.1.0"),
            ("teams-for-linux".into(), "2.1.0".into())
        );
        assert_eq!(split_name("hello"), ("hello".into(), String::new()));
    }

    #[test]
    fn hidden_entries_are_skipped() {
        assert!(shown("[Desktop Entry]\nName=VLC\n"));
        assert!(!shown("[Desktop Entry]\nNoDisplay=true\n"));
    }

    #[test]
    fn store_name_strips_the_hash() {
        let p = Path::new(
            "/nix/store/0d9e9b832d03ac387417e16ce1febf73-vlc-3.0.23/share/applications/vlc.desktop",
        );
        assert_eq!(store_name(p), Some(("vlc".into(), "3.0.23".into())));
        assert_eq!(
            store_name(Path::new("/usr/share/applications/x.desktop")),
            None
        );
    }
}
