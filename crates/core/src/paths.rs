//! Nix App Center data folder and file writing, including as root.

use std::path::PathBuf;

pub(crate) fn data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = crate::util::home();
            home.join(".local/share")
        });
    base.join("nix-app-center")
}

/// The name the app had before 0.3, still found on existing machines.
pub(crate) const LEGACY_NAME: &str = "nix-software";

/// Moves the data, settings and cache folders of the old name to the new
/// one, once. Run first in `main`.
pub fn migrate_legacy_dirs() {
    let settings = crate::settings::settings_path();
    let dirs = [
        Some(data_dir()),
        settings.parent().map(PathBuf::from),
        Some(crate::cache::cache_dir()),
    ];
    for new in dirs.into_iter().flatten() {
        let old = new.with_file_name(LEGACY_NAME);
        if old.is_dir() && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
}

/// Writes `path` through a temporary file and a rename: never half written.
pub(crate) fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("nix-app-center"),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Writes `files` in order: directly when allowed, the rest in one pkexec
/// call, so one password prompt.
pub(crate) fn write_files(files: &[(PathBuf, Vec<u8>)]) -> std::io::Result<()> {
    let mut denied: Vec<&(PathBuf, Vec<u8>)> = Vec::new();
    for f in files {
        // Keep the order once rights are needed.
        if !denied.is_empty() {
            denied.push(f);
            continue;
        }
        match write_atomic(&f.0, &f.1) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied.push(f),
            Err(e) => return Err(e),
        }
    }
    if denied.is_empty() {
        return Ok(());
    }
    let Some(pkexec) = crate::launch::find_in_path("pkexec") else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            crate::util::n_("Writing there needs administrator rights, and pkexec is missing."),
        ));
    };
    let script = root_write_script(&denied);
    let env = root_tool("env").ok_or_else(|| std::io::Error::other("env not found"))?;
    let status = std::process::Command::new(pkexec)
        .env("SHELL", "/bin/sh")
        .arg(env)
        .arg(format!("PATH={ROOT_PATH}"))
        .args(["sh", "-c", &script])
        .status();
    match status?.code() {
        Some(0) => Ok(()),
        Some(126) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            crate::util::n_("Authentication was cancelled."),
        )),
        Some(127) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            crate::util::n_("Administrator rights were refused."),
        )),
        _ => Err(std::io::Error::other(crate::util::n_(
            "Writing the configuration failed.",
        ))),
    }
}

/// The script root runs to write `files`. The content is inline, so no
/// user file can be swapped; each file goes through `mktemp` and `mv`.
fn root_write_script(files: &[&(PathBuf, Vec<u8>)]) -> String {
    let mut parts = Vec::new();
    for (dest, bytes) in files {
        let text = String::from_utf8_lossy(bytes);
        let mut mark = String::from("NIX_APP_CENTER_EOF");
        while text.contains(&mark) {
            mark.push('_');
        }
        let dir = sh_quote(
            &dest
                .parent()
                .unwrap_or(std::path::Path::new("/"))
                .to_string_lossy(),
        );
        // mktemp: unguessable name, no link followed.
        parts.push(format!(
            "tmp=$(mktemp {dir}/.nix-app-center-XXXXXX)\ncat <<'{mark}' | head -c -1 > \"$tmp\"\n{text}\n{mark}\nchmod 644 \"$tmp\" && mv \"$tmp\" {}",
            sh_quote(&dest.to_string_lossy())
        ));
    }
    // Stop at the first failure.
    format!("set -e\n{}\n", parts.join("\n"))
}

/// PATH for commands run as root: system folders only, never the user's.
pub(crate) const ROOT_PATH: &str = "/run/wrappers/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// `name` looked up in [`ROOT_PATH`] only.
pub(crate) fn root_tool(name: &str) -> Option<PathBuf> {
    ROOT_PATH
        .split(':')
        .map(|dir| PathBuf::from(dir).join(name))
        .find(|p| p.is_file())
}

/// Single-quotes `s` for `sh`.
pub(crate) fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Marks `path` for git, since a flake only sees tracked files.
pub(crate) fn track_in_git(path: &std::path::Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let _ = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["add", "--intent-to-add", "--"])
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_content_and_leaves_no_tmp() {
        // No env mutation, so no env_lock needed.
        let dir =
            std::env::temp_dir().join(format!("nix-app-center-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("kept.json");

        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");

        let tmp_left = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains(".tmp"));
        assert!(!tmp_left, "no temporary file may remain");
    }

    #[test]
    fn write_files_writes_in_order_without_rights_when_allowed() {
        let dir = std::env::temp_dir().join(format!("nix-app-center-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (a, b) = (dir.join("a.nix"), dir.join("b.nix"));
        write_files(&[(a.clone(), b"one".to_vec()), (b.clone(), b"two".to_vec())]).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), b"one");
        assert_eq!(std::fs::read(&b).unwrap(), b"two");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn root_write_script_round_trips_any_content() {
        let dir = std::env::temp_dir().join(format!("nix-app-center-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tricky = b"{ x = \"$HOME `id` 'q' \\n\"; }\nNIX_APP_CENTER_EOF\nend".to_vec();
        let files = [
            (dir.join("a b.nix"), tricky.clone()),
            (dir.join("c.nix"), b"two\n".to_vec()),
        ];
        let refs: Vec<&(PathBuf, Vec<u8>)> = files.iter().collect();
        let status = std::process::Command::new("sh")
            .args(["-c", &root_write_script(&refs)])
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(std::fs::read(dir.join("a b.nix")).unwrap(), tricky);
        assert_eq!(std::fs::read(dir.join("c.nix")).unwrap(), b"two\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn track_in_git_marks_new_files() {
        let dir = std::env::temp_dir().join(format!("nix-app-center-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        let file = dir.join("nix-app-center-home.nix");
        std::fs::write(&file, "{ }").unwrap();
        track_in_git(&file);
        let listed = git(&["ls-files"]);
        assert_eq!(
            String::from_utf8_lossy(&listed.stdout).trim(),
            "nix-app-center-home.nix"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
