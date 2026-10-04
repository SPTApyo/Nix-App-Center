//! Tuile Sécurité façon GNOME Software (`gs-app-context-bar.c`) : dérivée des
//! permissions du bundle Flatpak, lues via `flatpak remote-info --show-metadata`
//! (aucune installation). Sans équivalent nixpkgs — c'est la donnée réelle que
//! GNOME affiche pour les apps Flathub qu'on croise.

use std::process::Command;

/// Niveaux de confiance, du plus sûr au moins sûr (comme `SafetyRating` de GNOME).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
enum Level {
    Safe,
    ProbablySafe,
    PotentiallyUnsafe,
}

/// Tuile Sécurité résolue : icône/couleur/titre + descriptions au niveau le plus
/// défavorable (jointes façon GNOME).
#[derive(Debug, Clone)]
pub struct Safety {
    pub icon: &'static str,
    pub color: &'static str,
    pub title: &'static str,
    pub descriptions: Vec<String>,
}

/// Calcule la tuile Sécurité d'une app Flathub par son `app_id`. `None` si
/// `flatpak` absent, app inconnue du remote, ou pas de section `[Context]`.
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

/// Parse le fichier metadata Flatpak (format INI) → tuile Sécurité. Séparé de
/// `safety()` pour être testable sans `flatpak`.
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
                add(Level::ProbablySafe, "A accès au réseau");
            }
            "sockets" => {
                if items.iter().any(|s| *s == "x11" || *s == "fallback-x11") {
                    add(Level::PotentiallyUnsafe, "Utilise un système de fenêtrage désuet");
                }
                if items.contains(&"pulseaudio") {
                    add(Level::ProbablySafe, "Peut accéder aux micros et jouer du son");
                }
            }
            "devices" => {
                if items.contains(&"all") {
                    add(Level::PotentiallyUnsafe, "Peut accéder aux périphériques matériels");
                } else if items.contains(&"input") {
                    add(Level::ProbablySafe, "Peut accéder aux périphériques d'entrée");
                }
            }
            "filesystems" => {
                let full = items.iter().any(|s| {
                    matches!(*s, "home" | "host" | "/") && !s.ends_with(":ro")
                });
                let read = items.iter().any(|s| matches!(*s, "home:ro" | "host:ro"));
                if full {
                    add(Level::PotentiallyUnsafe, "Peut lire et modifier toutes vos données");
                } else if read {
                    add(Level::PotentiallyUnsafe, "Peut lire toutes vos données");
                } else if !items.is_empty() {
                    add(Level::PotentiallyUnsafe, "Peut accéder à certains fichiers spécifiques");
                }
            }
            _ => {}
        }
    }
    if has_session_bus {
        reasons.push((Level::PotentiallyUnsafe, "Utilise des services de session non-portail".into()));
    }
    if has_system_bus {
        reasons.push((Level::PotentiallyUnsafe, "Utilise des services système non-portail".into()));
    }

    if reasons.is_empty() {
        return Safety {
            icon: "app-safety-ok-symbolic",
            color: "green",
            title: "Sûr",
            descriptions: vec!["Aucune permission".into()],
        };
    }

    let max = reasons.iter().map(|(l, _)| *l).fold(Level::Safe, |a, b| if b > a { b } else { a });
    let descriptions: Vec<String> =
        reasons.into_iter().filter(|(l, _)| *l == max).map(|(_, s)| s).collect();

    let (icon, color, title) = match max {
        Level::Safe => ("app-safety-ok-symbolic", "green", "Sûr"),
        Level::ProbablySafe => ("app-safety-ok-symbolic", "yellow", "Probablement sûr"),
        Level::PotentiallyUnsafe => {
            ("app-safety-unknown-symbolic", "orange", "Potentiellement non fiable")
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
        assert_eq!(s.title, "Potentiellement non fiable");
        assert_eq!(s.color, "orange");
        // Toutes les raisons doivent être au niveau max (PotentiallyUnsafe).
        assert!(s.descriptions.iter().any(|d| d.contains("périphériques matériels")));
        assert!(s.descriptions.iter().any(|d| d.contains("fenêtrage désuet")));
        // Le réseau/audio (ProbablySafe) est écrasé, absent des descriptions.
        assert!(!s.descriptions.iter().any(|d| d.contains("réseau")));
    }

    #[test]
    fn no_permissions_is_safe() {
        let s = from_metadata("[Application]\nname=x\n[Context]\n");
        assert_eq!(s.title, "Sûr");
        assert_eq!(s.color, "green");
    }

    #[test]
    fn network_only_is_probably_safe() {
        let s = from_metadata("[Context]\nshared=network;\n");
        assert_eq!(s.title, "Probablement sûr");
        assert_eq!(s.descriptions, vec!["A accès au réseau".to_string()]);
    }
}
