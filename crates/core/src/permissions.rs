//! Safety tile like GNOME Software (`gs-app-context-bar.c`): derived from the
//! Flatpak bundle permissions, read with `flatpak remote-info --show-metadata`
//! (no install). No nixpkgs equivalent: this is the real data GNOME shows for
//! the matching Flathub apps. Needs `flatpak` on the machine.

use crate::util::n_;
use std::process::Command;

/// Trust levels, safest first (like GNOME `SafetyRating`).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
enum Level {
    Safe,
    ProbablySafe,
    PotentiallyUnsafe,
}

/// Resolved Safety tile: icon/color/title + descriptions at the worst level
/// (joined like GNOME). Strings are English msgids, translated by the front end.
#[derive(Debug, Clone)]
pub struct Safety {
    pub icon: &'static str,
    pub color: &'static str,
    pub title: &'static str,
    pub descriptions: Vec<String>,
}

/// Computes the Safety tile of a Flathub app from its `app_id`. `None` when
/// `flatpak` is missing, the app is unknown to the remote, or has no `[Context]`.
pub fn safety(app_id: &str) -> Option<Safety> {
    let out = Command::new("flatpak")
        .args(["remote-info", "--show-metadata", "flathub", app_id])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let meta = String::from_utf8_lossy(&out.stdout);
    Some(from_metadata(&meta))
}

/// Parses the Flatpak metadata file (INI format) into a Safety tile. Kept apart
/// from `safety()` to be testable without `flatpak`.
fn from_metadata(meta: &str) -> Safety {
    let mut reasons: Vec<(Level, String)> = Vec::new();
    let mut section = "";
    let mut has_session_bus = false;
    let mut has_system_bus = false;

    let mut add = |lvl: Level, s: &str| reasons.push((lvl, s.to_string()));

    for line in meta.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line;
            if section == "[Session Bus Policy]" {
                has_session_bus = true;
            } else if section == "[System Bus Policy]" {
                has_system_bus = true;
            }
            continue;
        }
        if section != "[Context]" {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else { continue };
        let items: Vec<&str> = val.split(';').filter(|s| !s.is_empty()).collect();
        match key {
            "shared" if items.contains(&"network") => {
                add(Level::ProbablySafe, n_("Has network access"));
            }
            "sockets" => {
                if items.iter().any(|s| *s == "x11" || *s == "fallback-x11") {
                    add(Level::PotentiallyUnsafe, n_("Uses a legacy windowing system"));
                }
                if items.contains(&"pulseaudio") {
                    add(Level::ProbablySafe, n_("Can access microphones and play sounds"));
                }
            }
            "devices" => {
                if items.contains(&"all") {
                    add(Level::PotentiallyUnsafe, n_("Can access hardware devices"));
                } else if items.contains(&"input") {
                    add(Level::ProbablySafe, n_("Can access input devices"));
                }
            }
            "filesystems" => {
                let full = items.iter().any(|s| {
                    matches!(*s, "home" | "host" | "/") && !s.ends_with(":ro")
                });
                let read = items.iter().any(|s| matches!(*s, "home:ro" | "host:ro"));
                if full {
                    add(Level::PotentiallyUnsafe, n_("Can read and write all your data"));
                } else if read {
                    add(Level::PotentiallyUnsafe, n_("Can read all your data"));
                } else if !items.is_empty() {
                    add(Level::PotentiallyUnsafe, n_("Can access some specific files"));
                }
            }
            _ => {}
        }
    }
    if has_session_bus {
        reasons.push((Level::PotentiallyUnsafe, n_("Uses session services outside portals").into()));
    }
    if has_system_bus {
        reasons.push((Level::PotentiallyUnsafe, n_("Uses system services outside portals").into()));
    }

    if reasons.is_empty() {
        return Safety {
            icon: "app-safety-ok-symbolic",
            color: "green",
            title: n_("Safe"),
            descriptions: vec![n_("No permissions").into()],
        };
    }

    let max = reasons.iter().map(|(l, _)| *l).fold(Level::Safe, |a, b| if b > a { b } else { a });
    let descriptions: Vec<String> =
        reasons.into_iter().filter(|(l, _)| *l == max).map(|(_, s)| s).collect();

    let (icon, color, title) = match max {
        Level::Safe => ("app-safety-ok-symbolic", "green", n_("Safe")),
        Level::ProbablySafe => ("app-safety-ok-symbolic", "yellow", n_("Probably Safe")),
        Level::PotentiallyUnsafe => {
            ("app-safety-unknown-symbolic", "orange", n_("Potentially Unsafe"))
        }
    };
    Safety { icon, color, title, descriptions }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEAM_META: &str = "\
[Application]
name=com.valvesoftware.Steam
[Context]
shared=network;ipc;
sockets=x11;wayland;pulseaudio;
devices=all;
filesystems=xdg-config/MangoHud:ro;/mnt;/run/media;
[Session Bus Policy]
org.freedesktop.Notifications=talk
[System Bus Policy]
org.freedesktop.UPower=talk
";

    #[test]
    fn steam_is_potentially_unsafe() {
        let s = from_metadata(STEAM_META);
        assert_eq!(s.title, "Potentially Unsafe");
        assert_eq!(s.color, "orange");
        // Every reason must be at the max level (PotentiallyUnsafe).
        assert!(s.descriptions.iter().any(|d| d.contains("hardware devices")));
        assert!(s.descriptions.iter().any(|d| d.contains("legacy windowing")));
        // Network/audio (ProbablySafe) is outranked, absent from descriptions.
        assert!(!s.descriptions.iter().any(|d| d.contains("network")));
    }

    #[test]
    fn no_permissions_is_safe() {
        let s = from_metadata("[Application]\nname=x\n[Context]\n");
        assert_eq!(s.title, "Safe");
        assert_eq!(s.color, "green");
    }

    #[test]
    fn network_only_is_probably_safe() {
        let s = from_metadata("[Context]\nshared=network;\n");
        assert_eq!(s.title, "Probably Safe");
        assert_eq!(s.descriptions, vec!["Has network access".to_string()]);
    }
}
