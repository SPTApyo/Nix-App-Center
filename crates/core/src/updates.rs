//! Updates: every package follows one nixpkgs revision the config pins.
//! Updating moves that pin to the head of its branch, then applies.

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
    /// No managed configuration, apps in the user profile: `nix profile upgrade`.
    Profile,
    /// Unknown.
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
pub fn pinned_rev() -> Option<&'static str> {
    static REV: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    REV.get_or_init(|| current_source().rev).as_deref()
}

/// Flake reference of the pinned nixpkgs, so runs match what Keep
/// installs: the system registry when it pins nixpkgs, else the pinned
/// revision on GitHub. Computed once.
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

/// The flake reference running `pkg`: its flake input when it has one,
/// else the configuration's nixpkgs.
pub fn installable(pkg: &crate::Package) -> String {
    let flake = pkg
        .source
        .as_ref()
        .and_then(|name| {
            crate::config_layout::flake_sources()
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, r)| r)
        })
        .unwrap_or_else(|| nixpkgs_ref().to_string());
    format!("{flake}#{}", pkg.attr)
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
        // Another distribution: apps live in the user profile.
        LayoutKind::Unknown if system.is_none() => Source {
            branch: Some(
                registry_branch(&registry_list()).unwrap_or_else(|| "nixpkgs-unstable".into()),
            ),
            rev: None,
            last_modified: None,
            method: UpdateMethod::Profile,
        },
        LayoutKind::Unknown => from_version(UpdateMethod::Unknown),
    }
}

fn registry_list() -> String {
    crate::util::nix()
        .args(["registry", "list"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// The branch the registry points `nixpkgs` to, user entry first.
fn registry_branch(list: &str) -> Option<String> {
    let url = ["user", "system", "global"].iter().find_map(|kind| {
        list.lines()
            .find_map(|l| l.strip_prefix(&format!("{kind} flake:nixpkgs ")))
    })?;
    let url = url.split('?').next().unwrap_or(url);
    let branch = url.strip_prefix("github:NixOS/nixpkgs/").or_else(|| {
        url.strip_prefix("https://channels.nixos.org/")
            .and_then(|r| r.split('/').next())
    })?;
    (!branch.is_empty() && !branch.contains('/')).then(|| branch.to_string())
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

/// The nixpkgs node of a flake.lock: the one locked to `rev` when given,
/// else the root's `nixpkgs` input.
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
    // Stale lock: take the nixpkgs input.
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

/// True when lock node `from` depends on `target`.
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

/// Head of `branch` on GitHub. Fetches nixpkgs when it moved.
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

/// The NixOS release after a stable `branch`, once its channel exists.
pub fn newer_release(branch: &str) -> Option<String> {
    let next = next_release(branch)?;
    let url = format!("https://channels.nixos.org/nixos-{next}/git-revision");
    crate::http::get(&url, crate::http::TIMEOUT)
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

/// The installed apps an update of `source` changes.
pub fn updatable_apps(source: &Source) -> Vec<ProfileApp> {
    let mut apps = crate::profiles::profile_apps();
    if source.method == UpdateMethod::Profile {
        let profile = crate::util::home().join(".nix-profile");
        apps.retain(|a| a.desktop_file.starts_with(&profile));
    }
    let held = held_pnames(&crate::manifest::load_manifest());
    apps.retain(|a| !held.contains(&a.pname));
    apps
}

/// pnames of the held packages: their updates are refused on purpose.
fn held_pnames(m: &crate::manifest::Manifest) -> Vec<String> {
    m.kept
        .iter()
        .filter(|e| e.pin.is_some())
        .map(|e| {
            let attr = &e.package.attr;
            attr.rsplit('.').next().unwrap_or(attr).to_string()
        })
        .collect()
}

/// Installed apps `rev` ships in a newer version. Apps whose installed
/// version differs from the one of `current` were changed by the
/// configuration (an overlay): updating nixpkgs may not move them, skipped.
pub fn app_updates(rev: &str, current: Option<&str>, apps: &[ProfileApp]) -> Vec<AppUpdate> {
    let mut names: Vec<&str> = apps.iter().map(|a| a.pname.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return Vec::new();
    }
    let changed = current.map(|c| versions_at(c, &names)).unwrap_or_default();
    let plain: Vec<ProfileApp> = apps
        .iter()
        .filter(|a| {
            changed
                .get(&a.pname)
                .and_then(Value::as_str)
                .is_none_or(|v| v == a.version)
        })
        .cloned()
        .collect();
    newer_apps(&plain, &versions_at(rev, &names))
}

/// `attr` in nixos-unstable, when its version there is newer than
/// `current`. Blocking: one metadata query, one evaluation.
pub fn newer_in_unstable(attr: &str, current: &str) -> Option<crate::versions::VersionChoice> {
    let head = latest("nixos-unstable")?;
    let version = versions_at(&head.rev, &[attr])
        .get(attr)?
        .as_str()?
        .to_string();
    version_newer(&version, current).then(|| crate::versions::VersionChoice {
        version,
        rev: head.rev,
        date: String::new(),
        cached: true,
    })
}

/// Versions of `names` in nixpkgs `rev`, in one evaluation.
fn versions_at(rev: &str, names: &[&str]) -> serde_json::Map<String, Value> {
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
    let Ok(out) = out else {
        return Default::default();
    };
    serde_json::from_slice(&out.stdout).unwrap_or_default()
}

/// The apps whose pname has a newer version in `versions`.
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

/// The steps moving the pin and applying, when Nix App Center can run them.
/// The lock is updated as the user when the user owns it.
pub fn update_command(source: &Source, layout: &Layout) -> Option<Vec<Vec<String>>> {
    let apply = layout.apply.clone();
    match &source.method {
        UpdateMethod::FlakeInput { dir, input } if !apply.is_empty() => {
            let owned = std::fs::OpenOptions::new()
                .append(true)
                .open(dir.join("flake.lock"))
                .is_ok();
            let mut update: Vec<String> = if owned {
                Vec::new()
            } else {
                vec!["sudo".into()]
            };
            update.extend(
                [
                    "nix",
                    "--extra-experimental-features",
                    "nix-command flakes",
                    "flake",
                    "update",
                    input,
                    "--flake",
                    &dir.to_string_lossy(),
                ]
                .map(String::from),
            );
            Some(vec![update, apply])
        }
        UpdateMethod::Channel => Some(vec![["sudo", "nixos-rebuild", "switch", "--upgrade"]
            .map(String::from)
            .to_vec()]),
        UpdateMethod::Profile => Some(vec![[
            "env",
            "NIXPKGS_ALLOW_UNFREE=1",
            "nix",
            "--extra-experimental-features",
            "nix-command flakes",
            "profile",
            "upgrade",
            "--all",
            "--impure",
        ]
        .map(String::from)
        .to_vec()]),
        UpdateMethod::HmChannel if !apply.is_empty() => {
            Some(vec![vec!["nix-channel".into(), "--update".into()], apply])
        }
        _ => None,
    }
}

/// True when `candidate` is newer than `current`, by numeric segments.
pub fn version_newer(candidate: &str, current: &str) -> bool {
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

    #[test]
    fn held_packages_leave_the_updates() {
        let held = crate::manifest::KeptEntry {
            pin: Some(crate::manifest::Pin {
                rev: "abc".into(),
                sha256: "x".into(),
                since: 0,
                version: None,
            }),
            ..crate::manifest::KeptEntry::simple(crate::Package::from_attr("vlc"))
        };
        let m = crate::manifest::Manifest {
            kept: vec![
                held,
                crate::manifest::KeptEntry::simple(crate::Package::from_attr("gimp")),
            ],
        };
        assert_eq!(held_pnames(&m), ["vlc"]);
    }

    #[test]
    fn registry_branch_reads_github_and_channel_urls() {
        let list = "system flake:nixpkgs path:/nix/store/abc-source\n\
global flake:nixpkgs/nixos-26.05 https://channels.nixos.org/nixos-26.05/nixexprs.tar.zst\n\
global flake:nixpkgs https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.zst\n";
        assert_eq!(registry_branch(list), None, "a store path has no branch");
        let global =
            "global flake:nixpkgs https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.zst\n";
        assert_eq!(registry_branch(global).as_deref(), Some("nixpkgs-unstable"));
        let user = "user flake:nixpkgs github:NixOS/nixpkgs/nixos-26.05\n\
global flake:nixpkgs github:NixOS/nixpkgs/nixpkgs-unstable\n";
        assert_eq!(registry_branch(user).as_deref(), Some("nixos-26.05"));
    }

    #[test]
    fn profile_update_upgrades_the_user_profile() {
        let source = Source {
            branch: Some("nixpkgs-unstable".into()),
            rev: None,
            last_modified: None,
            method: UpdateMethod::Profile,
        };
        let layout = Layout {
            kind: LayoutKind::Unknown,
            home: None,
            system: None,
            apply: Vec::new(),
            hm_user: None,
        };
        let steps = update_command(&source, &layout).unwrap();
        let argv = &steps[0];
        assert!(
            argv.windows(2).any(|w| w == ["upgrade", "--all"]),
            "{argv:?}"
        );
        assert!(!argv.contains(&"sudo".to_string()));
    }
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
    fn flake_update_then_apply() {
        let source = Source {
            branch: None,
            rev: None,
            last_modified: None,
            method: UpdateMethod::FlakeInput {
                dir: PathBuf::from("/nonexistent/nixos"),
                input: "nixpkgs".into(),
            },
        };
        let steps = update_command(
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
        assert_eq!(steps.len(), 2);
        assert_eq!(
            steps[0][0], "sudo",
            "a lock the user cannot write needs rights"
        );
        assert!(steps[0].windows(2).any(|w| w == ["update", "nixpkgs"]));
        assert_eq!(steps[1][..2], ["sudo", "nixos-rebuild"]);
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
