//! Updates, the Nix way: every package follows one nixpkgs revision pinned
//! by the config (a flake.lock node or a channel). Updating means moving that
//! pin to the head of its branch, then applying the config.
//!
//! The running revision comes from `nixos-version --json`, the branch from the
//! flake.lock node holding that revision, the branch head from
//! `nix flake metadata`, and the app changes from the versions in the new
//! revision compared with the store paths of the installed apps.

use crate::config_layout::{detect, Layout, LayoutKind};
use crate::profiles::ProfileApp;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// How the nixpkgs pin can be moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateMethod {
    /// A direct input of the config flake: `nix flake update <input>`.
    FlakeInput { dir: PathBuf, input: String },
    /// NixOS on channels: `nixos-rebuild switch --upgrade`.
    Channel,
    /// Standalone Home Manager on channels: `nix-channel --update`.
    HmChannel,
    /// nixpkgs comes through another flake input, updated there.
    Pinned { by: String },
    /// Onyx cannot tell.
    Unknown,
}

/// The nixpkgs the config is built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Branch the pin follows (`nixos-26.05`), when known.
    pub branch: Option<String>,
    pub rev: Option<String>,
    /// Commit time of `rev` (Unix seconds).
    pub last_modified: Option<u64>,
    pub method: UpdateMethod,
}

/// Head of the branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    pub rev: String,
    pub last_modified: u64,
}

/// An installed app that the new revision ships in a newer version.
#[derive(Debug, Clone)]
pub struct AppUpdate {
    pub app: ProfileApp,
    pub new_version: String,
}

/// The nixpkgs revision the config is pinned to, computed once.
pub(crate) fn pinned_rev() -> Option<&'static str> {
    static REV: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    REV.get_or_init(|| current_source().rev).as_deref()
}

/// Flake reference of the nixpkgs the config is pinned to, so what Onyx
/// shows and runs matches what Keep installs. The registry `nixpkgs` when
/// NixOS pins it to the system (local, no download), else the pinned GitHub
/// revision, else the registry as is. Computed once per process.
pub fn nixpkgs_ref() -> &'static str {
    static REF: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    REF.get_or_init(|| {
        let pinned = crate::util::nix()
            .args(["registry", "list"])
            .output()
            .is_ok_and(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.starts_with("system flake:nixpkgs "))
            });
        match pinned_rev() {
            Some(rev) if !pinned => format!("github:NixOS/nixpkgs/{rev}"),
            _ => "nixpkgs".to_string(),
        }
    })
}

/// Finds the nixpkgs source of the detected config.
pub fn current_source() -> Source {
    let layout = detect();
    let system = nixos_version();
    let lock = |dir: &Path| -> Option<Value> {
        let text = std::fs::read_to_string(dir.join("flake.lock")).ok()?;
        serde_json::from_str(&text).ok()
    };
    let from_version = |method: UpdateMethod| {
        let (rev, version) = system.clone().unwrap_or_default();
        Source {
            branch: branch_from_version(&version),
            rev: Some(rev).filter(|r| !r.is_empty()),
            last_modified: date_from_version(&version),
            method,
        }
    };
    match &layout.kind {
        LayoutKind::NixosFlake { dir, .. } => {
            let running = from_version(UpdateMethod::Unknown);
            match lock(dir).and_then(|l| source_from_lock(&l, running.rev.as_deref(), dir)) {
                Some(s) if s.rev == running.rev || running.rev.is_none() => s,
                Some(s) => Source {
                    branch: s.branch,
                    method: s.method,
                    ..running
                },
                None => running,
            }
        }
        LayoutKind::NixosLegacy { .. } => from_version(UpdateMethod::Channel),
        LayoutKind::HmFlake { dir, .. } => lock(dir)
            .and_then(|l| source_from_lock(&l, None, dir))
            .unwrap_or(Source {
                branch: None,
                rev: None,
                last_modified: None,
                method: UpdateMethod::Unknown,
            }),
        LayoutKind::HmStandalone { .. } => Source {
            branch: None,
            rev: None,
            last_modified: None,
            method: UpdateMethod::HmChannel,
        },
        LayoutKind::Unknown => from_version(UpdateMethod::Unknown),
    }
}

/// `(nixpkgsRevision, nixosVersion)` of the running NixOS system.
fn nixos_version() -> Option<(String, String)> {
    let out = Command::new("nixos-version").arg("--json").output().ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some((
        v["nixpkgsRevision"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        v["nixosVersion"].as_str()?.to_string(),
    ))
}

/// `26.05.20260927.cf5e765` follows `nixos-26.05`, `26.11pre…` unstable.
fn branch_from_version(version: &str) -> Option<String> {
    let mut parts = version.split('.');
    let (major, minor) = (parts.next()?, parts.next()?);
    if minor.contains("pre") {
        return Some("nixos-unstable".into());
    }
    major
        .parse::<u32>()
        .ok()
        .map(|_| format!("nixos-{major}.{minor}"))
}

/// The build date carried by the version (`…20260927…`), as Unix seconds.
fn date_from_version(version: &str) -> Option<u64> {
    let d = version
        .split('.')
        .find(|p| p.len() == 8 && p.bytes().all(|b| b.is_ascii_digit()))?;
    let (y, m, day) = (
        d[..4].parse().ok()?,
        d[4..6].parse().ok()?,
        d[6..].parse().ok()?,
    );
    Some(days_from_civil(y, m, day) * 86_400)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> u64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) as u64
}

/// Reads the nixpkgs node of a flake.lock. With `rev`, the node locked to
/// that revision (the system may reach nixpkgs through other inputs);
/// without, the root input named `nixpkgs`. Pure.
fn source_from_lock(lock: &Value, rev: Option<&str>, dir: &Path) -> Option<Source> {
    let nodes = lock["nodes"].as_object()?;
    let root_name = lock["root"].as_str().unwrap_or("root");
    let root_inputs = nodes.get(root_name)?["inputs"].as_object()?;
    let by_rev = rev.filter(|r| !r.is_empty()).and_then(|rev| {
        nodes
            .iter()
            .find(|(_, n)| n["locked"]["rev"].as_str() == Some(rev))
            .map(|(k, _)| k.clone())
    });
    // A stale lock may not hold the running revision: take the nixpkgs input.
    let name = by_rev.or_else(|| {
        std::iter::once(&nodes[root_name])
            .chain(root_inputs.values().filter_map(|v| nodes.get(v.as_str()?)))
            .find_map(|n| n["inputs"]["nixpkgs"].as_str().map(str::to_string))
    })?;
    let node = &nodes[&name];
    let original = &node["original"];
    let branch = original["ref"].as_str().map(str::to_string).or_else(|| {
        (original["id"].as_str() == Some("nixpkgs")).then(|| "nixpkgs-unstable".into())
    });

    let direct = root_inputs
        .iter()
        .find(|(_, v)| v.as_str() == Some(name.as_str()))
        .map(|(k, _)| k.clone());
    let method = match direct {
        Some(input) => UpdateMethod::FlakeInput {
            dir: dir.to_path_buf(),
            input,
        },
        None => root_inputs
            .iter()
            .find(|(_, v)| v.as_str().is_some_and(|n| reaches(nodes, n, &name)))
            .map(|(k, _)| UpdateMethod::Pinned { by: k.clone() })
            .unwrap_or(UpdateMethod::Unknown),
    };
    Some(Source {
        branch,
        rev: node["locked"]["rev"].as_str().map(str::to_string),
        last_modified: node["locked"]["lastModified"].as_u64(),
        method,
    })
}

/// True when the lock node `from` depends on `target`, directly or not.
fn reaches(nodes: &serde_json::Map<String, Value>, from: &str, target: &str) -> bool {
    let mut stack = vec![from.to_string()];
    let mut seen = std::collections::HashSet::new();
    while let Some(n) = stack.pop() {
        if n == target {
            return true;
        }
        if !seen.insert(n.clone()) {
            continue;
        }
        if let Some(inputs) = nodes.get(&n).and_then(|x| x["inputs"].as_object()) {
            stack.extend(
                inputs
                    .values()
                    .filter_map(|v| v.as_str().map(str::to_string)),
            );
        }
    }
    false
}

/// Head of `branch` on GitHub. Downloads nixpkgs when it moved (cached by Nix
/// for an hour), which the update needs anyway.
pub fn latest(branch: &str) -> Option<Latest> {
    let out = crate::util::nix()
        .args([
            "flake",
            "metadata",
            "--json",
            &format!("github:NixOS/nixpkgs/{branch}"),
        ])
        .output()
        .ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(Latest {
        rev: v["revision"].as_str()?.to_string(),
        last_modified: v["lastModified"].as_u64()?,
    })
}

/// The NixOS release after a stable `branch` (`nixos-26.05` gives `26.11`),
/// when its channel exists already.
pub fn newer_release(branch: &str) -> Option<String> {
    let next = next_release(branch)?;
    let url = format!("https://channels.nixos.org/nixos-{next}/git-revision");
    ureq::get(&url)
        .timeout(crate::http::TIMEOUT)
        .call()
        .ok()
        .map(|_| next)
}

fn next_release(branch: &str) -> Option<String> {
    let (yy, mm) = branch.strip_prefix("nixos-")?.split_once('.')?;
    let yy: u32 = yy.parse().ok()?;
    match mm {
        "05" => Some(format!("{yy}.11")),
        "11" => Some(format!("{}.05", yy + 1)),
        _ => None,
    }
}

/// Installed apps that `rev` ships in a newer version. One evaluation for all
/// apps; the pname stands for the attribute, apps without a match are skipped.
pub fn app_updates(rev: &str, apps: &[ProfileApp]) -> Vec<AppUpdate> {
    let mut names: Vec<&str> = apps.iter().map(|a| a.pname.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return Vec::new();
    }
    let list = names
        .iter()
        .map(|n| serde_json::to_string(n).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ");
    let system = format!("{}-linux", std::env::consts::ARCH);
    let expr = format!(
        "let p = (builtins.getFlake \"github:NixOS/nixpkgs/{rev}\").legacyPackages.{system}; \
         v = n: let r = builtins.tryEval (p.${{n}}.version or null); in if r.success then r.value else null; \
         in builtins.listToAttrs (map (n: {{ name = n; value = v n; }}) [ {list} ])"
    );
    let out = crate::util::nix()
        .args(["eval", "--json", "--expr", &expr])
        .output();
    let Ok(out) = out else { return Vec::new() };
    let versions: serde_json::Map<String, Value> =
        serde_json::from_slice(&out.stdout).unwrap_or_default();
    newer_apps(apps, &versions)
}

/// Pure part of [`app_updates`]: one entry per pname with a newer version.
fn newer_apps(apps: &[ProfileApp], versions: &serde_json::Map<String, Value>) -> Vec<AppUpdate> {
    let mut out: Vec<AppUpdate> = Vec::new();
    for app in apps {
        let Some(new) = versions.get(&app.pname).and_then(Value::as_str) else {
            continue;
        };
        if version_newer(new, &app.version) && !out.iter().any(|u| u.app.pname == app.pname) {
            out.push(AppUpdate {
                app: app.clone(),
                new_version: new.to_string(),
            });
        }
    }
    out
}

/// The command that moves the pin and applies the config, when Onyx can run it.
pub fn update_command(source: &Source, layout: &Layout) -> Option<Vec<String>> {
    let shell = |script: String, sudo: bool| -> Vec<String> {
        let mut argv: Vec<String> = Vec::new();
        if sudo {
            argv.push("sudo".into());
        }
        argv.extend(["sh".into(), "-c".into(), script]);
        argv
    };
    let (sudo, apply) = match layout.apply.split_first() {
        Some((first, rest)) if first == "sudo" => (true, quote_all(rest)),
        _ => (false, quote_all(&layout.apply)),
    };
    match &source.method {
        UpdateMethod::FlakeInput { dir, input } if !apply.is_empty() => Some(shell(
            format!(
                "nix --extra-experimental-features 'nix-command flakes' flake update {} --flake {} && {apply}",
                quote(input),
                quote(&dir.to_string_lossy())
            ),
            sudo,
        )),
        UpdateMethod::Channel => Some(
            ["sudo", "nixos-rebuild", "switch", "--upgrade"]
                .map(String::from)
                .to_vec(),
        ),
        UpdateMethod::HmChannel if !apply.is_empty() => {
            Some(shell(format!("nix-channel --update && {apply}"), sudo))
        }
        _ => None,
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn quote_all(argv: &[String]) -> String {
    argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")
}

/// Compares two versions "the Nix way" by numeric segments (dot/dash separated).
fn version_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .map(|s| {
                s.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
            })
            .map(|s| s.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(candidate), parse(current));
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::Scope;

    #[test]
    fn compares_numeric_segments() {
        assert!(version_newer("1.2.0", "1.1.9"));
        assert!(version_newer("2.0", "1.99"));
        assert!(!version_newer("1.0", "1.0"));
        assert!(!version_newer("1.0", "1.1"));
        assert!(version_newer("1.2.1", "1.2"));
    }

    #[test]
    fn next_release_follows_the_may_november_cycle() {
        assert_eq!(next_release("nixos-26.05").as_deref(), Some("26.11"));
        assert_eq!(next_release("nixos-26.11").as_deref(), Some("27.05"));
        assert_eq!(next_release("nixos-unstable"), None);
    }

    #[test]
    fn branch_and_date_from_nixos_version() {
        let v = "26.05.20260927.cf5e765";
        assert_eq!(branch_from_version(v).as_deref(), Some("nixos-26.05"));
        assert_eq!(
            branch_from_version("26.11pre20261001.abc").as_deref(),
            Some("nixos-unstable")
        );
        // 2026-09-27 00:00 UTC
        assert_eq!(date_from_version(v), Some(1_790_467_200));
    }

    fn lock() -> Value {
        serde_json::json!({
            "root": "root",
            "nodes": {
                "root": { "inputs": { "nixpkgs": "nixpkgs", "config": "config" } },
                "nixpkgs": {
                    "locked": { "rev": "aaa", "lastModified": 10 },
                    "original": { "owner": "NixOS", "repo": "nixpkgs", "ref": "nixos-26.05", "type": "github" }
                },
                "config": { "inputs": { "nixpkgs": "nixpkgs_2" } },
                "nixpkgs_2": {
                    "locked": { "rev": "bbb", "lastModified": 20 },
                    "original": { "id": "nixpkgs", "type": "indirect" }
                }
            }
        })
    }

    #[test]
    fn lock_direct_input_is_updatable() {
        let s = source_from_lock(&lock(), None, Path::new("/etc/nixos")).unwrap();
        assert_eq!(s.branch.as_deref(), Some("nixos-26.05"));
        assert_eq!(s.rev.as_deref(), Some("aaa"));
        assert_eq!(s.last_modified, Some(10));
        assert_eq!(
            s.method,
            UpdateMethod::FlakeInput {
                dir: PathBuf::from("/etc/nixos"),
                input: "nixpkgs".into()
            }
        );
    }

    #[test]
    fn stale_lock_falls_back_on_the_nixpkgs_input() {
        let s = source_from_lock(&lock(), Some("zzz"), Path::new("/etc/nixos")).unwrap();
        assert_eq!(s.rev.as_deref(), Some("aaa"));
        let mut only_config = lock();
        only_config["nodes"]["root"]["inputs"] = serde_json::json!({ "config": "config" });
        let s = source_from_lock(&only_config, Some("zzz"), Path::new("/etc/nixos")).unwrap();
        assert_eq!(
            s.method,
            UpdateMethod::Pinned {
                by: "config".into()
            }
        );
    }

    #[test]
    fn lock_nested_input_is_pinned_by_its_parent() {
        let s = source_from_lock(&lock(), Some("bbb"), Path::new("/etc/nixos")).unwrap();
        assert_eq!(s.branch.as_deref(), Some("nixpkgs-unstable"));
        assert_eq!(
            s.method,
            UpdateMethod::Pinned {
                by: "config".into()
            }
        );
    }

    fn app(pname: &str, version: &str) -> ProfileApp {
        ProfileApp {
            scope: Scope::User,
            desktop_file: PathBuf::from(format!("{pname}.desktop")),
            store_path: PathBuf::from(format!("/nix/store/x-{pname}")),
            pname: pname.into(),
            version: version.into(),
        }
    }

    #[test]
    fn newer_apps_keeps_real_upgrades_once() {
        let apps = [
            app("vlc", "3.0.23"),
            app("vlc", "3.0.23"),
            app("gimp", "3.0"),
            app("ghost", "1"),
        ];
        let versions = serde_json::json!({ "vlc": "3.0.24", "gimp": "3.0", "ghost": null });
        let ups = newer_apps(&apps, versions.as_object().unwrap());
        assert_eq!(ups.len(), 1);
        assert_eq!(ups[0].app.pname, "vlc");
        assert_eq!(ups[0].new_version, "3.0.24");
    }

    fn layout(apply: &[&str]) -> Layout {
        Layout {
            kind: LayoutKind::Unknown,
            home: None,
            system: None,
            apply: apply.iter().map(|s| s.to_string()).collect(),
            hm_user: None,
        }
    }

    #[test]
    fn flake_update_then_apply_under_one_sudo() {
        let source = Source {
            branch: None,
            rev: None,
            last_modified: None,
            method: UpdateMethod::FlakeInput {
                dir: PathBuf::from("/etc/nixos"),
                input: "nixpkgs".into(),
            },
        };
        let argv = update_command(
            &source,
            &layout(&[
                "sudo",
                "nixos-rebuild",
                "switch",
                "--flake",
                "/etc/nixos#laptop",
            ]),
        )
        .unwrap();
        assert_eq!(argv[..3], ["sudo", "sh", "-c"]);
        assert!(argv[3].contains("flake update 'nixpkgs' --flake '/etc/nixos' && 'nixos-rebuild'"));
    }

    #[test]
    fn pinned_source_has_no_command() {
        let source = Source {
            branch: None,
            rev: None,
            last_modified: None,
            method: UpdateMethod::Pinned {
                by: "config".into(),
            },
        };
        assert_eq!(update_command(&source, &layout(&["sudo", "x"])), None);
    }
}
