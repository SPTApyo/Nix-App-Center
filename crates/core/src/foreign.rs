//! Apps installed outside Nix: Flatpak and AppImages. Read, and removed on
//! request; never installed.

use std::path::{Path, PathBuf};

use crate::installed::{Installed, Origin};

/// Flatpak apps of the user and system installations; empty without flatpak.
pub fn flatpaks() -> Vec<Installed> {
    if !crate::launch::in_path("flatpak") {
        return Vec::new();
    }
    std::process::Command::new("flatpak")
        .args([
            "list",
            "--app",
            "--columns=application,version,installation",
        ])
        .output()
        .map(|o| parse_flatpak_list(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

fn parse_flatpak_list(stdout: &str) -> Vec<Installed> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut cols = line.split('\t');
            let id = cols.next()?.trim();
            if id.is_empty() {
                return None;
            }
            let version = cols.next().unwrap_or("").trim().to_string();
            let installation = cols.next().unwrap_or("system").trim();
            let last = id.rsplit('.').next().unwrap_or(id).to_lowercase();
            Some(Installed {
                names: vec![id.to_lowercase(), last],
                version,
                origin: Origin::Flatpak,
                module: false,
                handle: format!("{installation}:{id}"),
            })
        })
        .collect()
}

/// AppImages integrated in the menu (Gear Lever, AppImageLauncher): desktop
/// files whose Exec runs a .AppImage.
pub fn appimages() -> Vec<Installed> {
    let dir = crate::util::home().join(".local/share/applications");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "desktop"))
        .filter_map(|p| appimage_entry(&p, &std::fs::read_to_string(&p).ok()?))
        .collect()
}

fn appimage_entry(desktop_file: &Path, text: &str) -> Option<Installed> {
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .map(|v| v.trim().to_string())
    };
    let exec = field("Exec=")?;
    if !exec.to_lowercase().contains(".appimage") {
        return None;
    }
    let stem = desktop_file.file_stem()?.to_string_lossy().to_lowercase();
    let mut names = vec![stem];
    if let Some(name) = field("Name=") {
        names.push(name.to_lowercase());
    }
    Some(Installed {
        names,
        version: String::new(),
        origin: Origin::AppImage,
        module: false,
        handle: desktop_file.to_string_lossy().into_owned(),
    })
}

/// The AppImage file a desktop file runs.
fn appimage_file(desktop_file: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(desktop_file).ok()?;
    let exec = text.lines().find_map(|l| l.strip_prefix("Exec="))?;
    exec_words(exec)
        .into_iter()
        .find(|w| w.to_lowercase().ends_with(".appimage"))
        .map(PathBuf::from)
}

/// The words of an Exec line; a quoted one keeps its spaces.
fn exec_words(exec: &str) -> Vec<String> {
    let (mut words, mut word, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => word.extend(chars.next()),
            ' ' | '\t' if !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// The files removing an AppImage deletes: the image and its desktop file.
pub fn appimage_files(desktop_file: &Path) -> Vec<PathBuf> {
    appimage_file(desktop_file)
        .into_iter()
        .chain(std::iter::once(desktop_file.to_path_buf()))
        .collect()
}

/// Removes an installed Flatpak or AppImage. Blocking.
pub fn remove_foreign(item: &Installed) -> std::io::Result<()> {
    match item.origin {
        Origin::Flatpak => {
            let (installation, id) = item
                .handle
                .split_once(':')
                .unwrap_or(("system", &item.handle));
            let scope = if installation == "user" {
                "--user"
            } else {
                "--system"
            };
            let status = std::process::Command::new("flatpak")
                .args(["uninstall", "--noninteractive", scope, id])
                .status()?;
            if status.success() {
                Ok(())
            } else {
                Err(std::io::Error::other(crate::util::n_(
                    "Flatpak could not remove the app.",
                )))
            }
        }
        Origin::AppImage => {
            for f in appimage_files(Path::new(&item.handle)) {
                match std::fs::remove_file(&f) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_an_appimage_deletes_both_files() {
        let dir =
            std::env::temp_dir().join(format!("nix-app-center-appimage-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("My Apps")).unwrap();
        let image = dir.join("My Apps/kdenlive.AppImage");
        let desktop = dir.join("kdenlive.desktop");
        std::fs::write(&image, "").unwrap();
        std::fs::write(
            &desktop,
            format!("Name=Kdenlive\nExec=\"{}\" %F\n", image.display()),
        )
        .unwrap();
        let item = appimage_entry(&desktop, &std::fs::read_to_string(&desktop).unwrap()).unwrap();
        remove_foreign(&item).unwrap();
        assert!(!image.exists() && !desktop.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn removing_a_flatpak_calls_flatpak_uninstall() {
        let _g = crate::env_lock();
        let dir =
            std::env::temp_dir().join(format!("nix-app-center-flatpak-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("log");
        let fake = dir.join("flatpak");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\necho \"$@\" > {}\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{}:{}", dir.display(), path.to_string_lossy()),
        );
        let item = parse_flatpak_list("org.videolan.VLC\t3.0\tuser\n").remove(0);
        let res = remove_foreign(&item);
        std::env::set_var("PATH", path);
        res.unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap().trim(),
            "uninstall --noninteractive --user org.videolan.VLC"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_flatpak_list() {
        let apps = parse_flatpak_list(
            "org.videolan.VLC\t3.0.21\tsystem\ncom.valvesoftware.Steam\t1.0\tuser\n",
        );
        assert_eq!(apps[0].names, vec!["org.videolan.vlc", "vlc"]);
        assert_eq!(apps[1].handle, "user:com.valvesoftware.Steam");
        assert_eq!(apps[1].origin, Origin::Flatpak);
    }

    #[test]
    fn only_desktop_files_running_an_appimage() {
        let p = Path::new("/home/u/.local/share/applications/gearlever_kdenlive.desktop");
        let e = appimage_entry(
            p,
            "[Desktop Entry]\nName=Kdenlive\nExec=\"/home/u/AppImages/kdenlive.appimage\" %F\n",
        )
        .unwrap();
        assert_eq!(e.names, vec!["gearlever_kdenlive", "kdenlive"]);
        assert!(appimage_entry(p, "Name=X\nExec=firefox %u\n").is_none());
    }
}
