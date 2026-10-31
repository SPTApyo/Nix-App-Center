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
            match flake_source(dir, running.rev.as_deref()) {
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
        LayoutKind::HmFlake { dir, .. } => flake_source(dir, None).unwrap_or(Source {
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

/// The nixpkgs of the flake in `dir`. When it comes through a local input
/// (a small /etc/nixos importing the real configuration), that input's own
/// lock is where it can be updated.
fn flake_source(dir: &Path, rev: Option<&str>) -> Option<Source> {
    let lock = |dir: &Path| -> Option<Value> {
        let text = std::fs::read_to_string(dir.join("flake.lock")).ok()?;
        serde_json::from_str(&text).ok()
    };
    let main = lock(dir).and_then(|l| source_from_lock(&l, rev, dir));
    if matches!(&main, Some(s) if matches!(s.method, UpdateMethod::FlakeInput { .. })) {
        return main;
    }
    crate::config_layout::flake_dirs(dir)
        .iter()
        .skip(1)
        .filter_map(|local| source_from_lock(&lock(local)?, rev, local))
        .find(|s| matches!(s.method, UpdateMethod::FlakeInput { .. }))
        .or(main)
}

/// An input of the configuration's flakes besides its nixpkgs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlakeInput {
    /// The flake folder whose lock holds it.
    pub dir: PathBuf,
    pub name: String,
    /// Flake reference following the same branch, to find its head.
    pub reference: String,
    pub rev: Option<String>,
    /// Commit time of the locked revision (Unix seconds).
    pub last_modified: Option<u64>,
    /// The branch or tag it follows, when the lock names one.
    pub branch: Option<String>,
}

/// The inputs of the configuration's flake and its local inputs that can be
/// updated, nixpkgs of `source` aside. Inputs that follow another, and local
/// ones, are left out.
pub fn flake_inputs(source: &Source) -> Vec<FlakeInput> {
    let dir = match detect().kind {
        LayoutKind::NixosFlake { dir, .. } | LayoutKind::HmFlake { dir, .. } => dir,
        _ => return Vec::new(),
    };
    let skip = match &source.method {
        UpdateMethod::FlakeInput { dir, input } => Some((dir.clone(), input.clone())),
        _ => None,
    };
    let mut out = Vec::new();
    for d in crate::config_layout::flake_dirs(&dir) {
        let Some(lock) = std::fs::read_to_string(d.join("flake.lock"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            continue;
        };
        out.extend(
            inputs_from_lock(&lock, &d)
                .into_iter()
                .filter(|i| skip.as_ref() != Some(&(i.dir.clone(), i.name.clone()))),
        );
    }
    out
}

fn inputs_from_lock(lock: &Value, dir: &Path) -> Vec<FlakeInput> {
    let safe = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.~/:+@".contains(c))
    };
    let root = lock["root"].as_str().unwrap_or("root");
    let Some(inputs) = lock["nodes"][root]["inputs"].as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, node) in inputs {
        // An array: the input follows another one.
        let Some(node) = node.as_str() else { continue };
        let original = &lock["nodes"][node]["original"];
        let locked = &lock["nodes"][node]["locked"];
        let branch = original["ref"].as_str().map(str::to_string);
        let reference = match original["type"].as_str() {
            Some(kind @ ("github" | "gitlab" | "sourcehut")) => {
                let (Some(owner), Some(repo)) =
                    (original["owner"].as_str(), original["repo"].as_str())
                else {
                    continue;
                };
                match &branch {
                    Some(b) => format!("{kind}:{owner}/{repo}/{b}"),
                    None => format!("{kind}:{owner}/{repo}"),
                }
            }
            Some("git") => {
                let Some(url) = original["url"].as_str().filter(|u| !u.starts_with("file:")) else {
                    continue;
                };
                match &branch {
                    Some(b) => format!("git+{url}?ref={b}"),
                    None => format!("git+{url}"),
                }
            }
            _ => continue,
        };
        // Starts with a letter: never read as an option of nix.
        let name_ok = name.starts_with(|c: char| c.is_ascii_alphabetic())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c));
        if !name_ok || !safe(&reference) {
            continue;
        }
        out.push(FlakeInput {
            dir: dir.to_path_buf(),
            name: name.clone(),
            reference,
            rev: locked["rev"].as_str().map(str::to_string),
            last_modified: locked["lastModified"].as_u64(),
            branch,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The head of the branch `input` follows. Blocking; fetches it.
pub fn input_latest(input: &FlakeInput) -> Option<Latest> {
    let out = crate::util::nix()
        .args(["flake", "metadata", "--json", &input.reference])
        .output()
        .ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(Latest {
        rev: v["revision"].as_str()?.to_string(),
        last_modified: v["lastModified"].as_u64()?,
    })
}

/// Updating `inputs` in their flakes, then applying.
pub fn inputs_update_command(inputs: &[FlakeInput], layout: &Layout) -> Option<Vec<Vec<String>>> {
    if inputs.is_empty() || layout.apply.is_empty() {
        return None;
    }
    let mut dirs: Vec<&PathBuf> = inputs.iter().map(|i| &i.dir).collect();
    dirs.sort();
    dirs.dedup();
    let mut steps = Vec::new();
    for dir in dirs {
        let owned = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.join("flake.lock"))
            .is_ok();
        let mut step: Vec<String> = if owned {
            Vec::new()
        } else {
            vec!["sudo".into()]
        };
        step.extend(
            [
                "nix",
                "--extra-experimental-features",
                "nix-command flakes",
                "flake",
                "update",
            ]
            .map(String::from),
        );
        step.extend(
            inputs
                .iter()
                .filter(|i| &i.dir == dir)
                .map(|i| i.name.clone()),
        );
        step.extend(["--flake".to_string(), dir.to_string_lossy().to_string()]);
        steps.push(step);
    }
    steps.push(layout.apply.clone());
    Some(steps)
}

/// When Home Manager follows another NixOS release than nixpkgs:
/// (Home Manager's branch, nixpkgs' branch).
pub fn release_mismatch(source: &Source, inputs: &[FlakeInput]) -> Option<(String, String)> {
    let nixpkgs = source.branch.as_deref()?;
    let hm = inputs.iter().find(|i| i.name == "home-manager")?;
    let hm_branch = hm.branch.clone().unwrap_or_else(|| "master".into());
    let expected = match nixpkgs.strip_prefix("nixos-") {
        Some("unstable") | None => "master".to_string(),
        Some(release) => format!("release-{release}"),
    };
    (hm_branch != expected).then(|| (hm_branch, nixpkgs.to_string()))
}

/// Moving the configuration to the next NixOS release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlan {
    pub release: String,
    /// The flake.nix to change and its new text; `None` on channels.
    pub edit: Option<ReleaseEdit>,
    /// What changes, one line each: `nixpkgs.url: nixos-26.05 → nixos-26.11`.
    pub diff: String,
    pub steps: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseEdit {
    pub file: PathBuf,
    pub original: String,
    pub patched: String,
    /// flake.lock before the update, put back with flake.nix on failure.
    pub lock: Vec<u8>,
}

/// The plan moving the configuration from its stable release to `next`:
/// the branches of nixpkgs and Home Manager in flake.nix, or the nixos
/// channel. `None` when the configuration follows no stable release.
pub fn release_plan(next: &str) -> Option<ReleasePlan> {
    let source = current_source();
    let current = source
        .branch
        .as_deref()?
        .strip_prefix("nixos-")?
        .to_string();
    if current == "unstable" || current == next {
        return None;
    }
    let layout = detect();
    match &source.method {
        UpdateMethod::FlakeInput { dir, input } => {
            let file = dir.join("flake.nix");
            let original = std::fs::read_to_string(&file).ok()?;
            let patched = retarget(&original, &current, next);
            if patched == original || layout.apply.is_empty() {
                return None;
            }
            let lock_bytes = std::fs::read(dir.join("flake.lock")).ok()?;
            let lock: Value = serde_json::from_slice(&lock_bytes).ok()?;
            let mut moved: Vec<FlakeInput> = inputs_from_lock(&lock, dir)
                .into_iter()
                .filter(|i| {
                    i.name == *input || i.branch.as_deref() == Some(&format!("release-{current}"))
                })
                .collect();
            moved.dedup_by(|a, b| a.name == b.name);
            Some(ReleasePlan {
                release: next.to_string(),
                diff: line_diff(&original, &patched, &current, next),
                steps: inputs_update_command(&moved, &layout)?,
                edit: Some(ReleaseEdit {
                    file,
                    original,
                    patched,
                    lock: lock_bytes,
                }),
            })
        }
        UpdateMethod::Channel => {
            let url = format!("https://channels.nixos.org/nixos-{next}");
            Some(ReleasePlan {
                release: next.to_string(),
                edit: None,
                diff: format!("nixos channel: nixos-{current} → nixos-{next}\n"),
                steps: vec![
                    ["sudo", "nix-channel", "--add", &url, "nixos"]
                        .map(String::from)
                        .to_vec(),
                    ["sudo", "nixos-rebuild", "switch", "--upgrade"]
                        .map(String::from)
                        .to_vec(),
                ],
            })
        }
        _ => None,
    }
}

/// `src` with the branches of release `from` moved to `to`.
fn retarget(src: &str, from: &str, to: &str) -> String {
    src.replace(&format!("nixos-{from}\""), &format!("nixos-{to}\""))
        .replace(&format!("nixos-{from}/"), &format!("nixos-{to}/"))
        .replace(&format!("release-{from}\""), &format!("release-{to}\""))
        .replace(&format!("release-{from}/"), &format!("release-{to}/"))
}

/// One line per changed line of `old`: what it sets, and the branch move.
fn line_diff(old: &str, new: &str, from: &str, to: &str) -> String {
    let mut out = String::new();
    for (a, b) in old.lines().zip(new.lines()) {
        if a == b {
            continue;
        }
        let key = a.split('=').next().unwrap_or(a).trim();
        let (before, after) = if a.contains(&format!("release-{from}")) {
            (format!("release-{from}"), format!("release-{to}"))
        } else {
            (format!("nixos-{from}"), format!("nixos-{to}"))
        };
        out.push_str(&format!("{key}: {before} → {after}\n"));
    }
    out
}

/// Writes the plan's flake.nix, the original beside it first.
pub fn write_release_edit(edit: &ReleaseEdit) -> std::io::Result<()> {
    let mut backup = edit.file.clone().into_os_string();
    backup.push(".nix-app-center-bak");
    crate::paths::write_files(&[
        (PathBuf::from(backup), edit.original.clone().into_bytes()),
        (edit.file.clone(), edit.patched.clone().into_bytes()),
    ])
}

/// Puts the original flake.nix and flake.lock back, after a failed or
/// cancelled switch.
pub fn revert_release_edit(edit: &ReleaseEdit) -> std::io::Result<()> {
    crate::paths::write_files(&[
        (edit.file.clone(), edit.original.clone().into_bytes()),
        (edit.file.with_file_name("flake.lock"), edit.lock.clone()),
    ])
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
    fn lists_the_inputs_that_can_move() {
        let lock: Value = serde_json::from_str(
            r#"{"root":"root","nodes":{
              "root":{"inputs":{"nixpkgs":"nixpkgs","home-manager":"hm","local":"local","follows":["nixpkgs"],"gaming":"gaming","-x":"gaming"}},
              "nixpkgs":{"original":{"type":"github","owner":"NixOS","repo":"nixpkgs","ref":"nixos-26.05"},"locked":{"rev":"aaa","lastModified":1}},
              "hm":{"original":{"type":"github","owner":"nix-community","repo":"home-manager","ref":"release-25.11"},"locked":{"rev":"bbb","lastModified":2}},
              "local":{"original":{"type":"git","url":"file:///home/me/config"},"locked":{}},
              "gaming":{"original":{"type":"git","url":"https://example.org/gaming.git"},"locked":{"rev":"ccc"}}
            }}"#,
        )
        .unwrap();
        let inputs = inputs_from_lock(&lock, Path::new("/cfg"));
        let names: Vec<&str> = inputs.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["gaming", "home-manager", "nixpkgs"]);
        assert_eq!(inputs[0].reference, "git+https://example.org/gaming.git");
        assert_eq!(
            inputs[1].reference,
            "github:nix-community/home-manager/release-25.11"
        );
        let source = Source {
            branch: Some("nixos-26.05".into()),
            rev: None,
            last_modified: None,
            method: UpdateMethod::Unknown,
        };
        assert_eq!(
            release_mismatch(&source, &inputs),
            Some(("release-25.11".into(), "nixos-26.05".into()))
        );
        let layout = Layout {
            kind: LayoutKind::Unknown,
            home: None,
            system: None,
            apply: vec!["sudo".into(), "nixos-rebuild".into(), "switch".into()],
            hm_user: None,
        };
        let steps = inputs_update_command(&inputs[..2], &layout).unwrap();
        assert_eq!(steps.len(), 2);
        assert!(steps[0].ends_with(&[
            "gaming".into(),
            "home-manager".into(),
            "--flake".into(),
            "/cfg".into()
        ]));
    }

    #[test]
    fn plans_a_release_switch_of_a_flake_configuration() {
        let _g = crate::env_lock();
        let dir = std::env::temp_dir().join(format!("nix-app-center-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("flake.nix"),
            "{\n  inputs.nixpkgs.url = \"github:NixOS/nixpkgs/nixos-26.05\";\n  inputs.home-manager.url = \"github:nix-community/home-manager/release-26.05\";\n  outputs = { nixpkgs, ... }: {\n    nixosConfigurations.pc = nixpkgs.lib.nixosSystem { modules = [ ./configuration.nix ]; };\n  };\n}\n",
        )
        .unwrap();
        let node = |owner: &str, repo: &str, branch: &str| {
            serde_json::json!({
                "locked": { "type": "github", "owner": owner, "repo": repo,
                    "rev": "0123456789abcdef0123456789abcdef01234567", "lastModified": 1700000000 },
                "original": { "type": "github", "owner": owner, "repo": repo, "ref": branch }
            })
        };
        let lock = serde_json::json!({
            "nodes": {
                "nixpkgs": node("NixOS", "nixpkgs", "nixos-26.05"),
                "home-manager": node("nix-community", "home-manager", "release-26.05"),
                "root": { "inputs": { "nixpkgs": "nixpkgs", "home-manager": "home-manager" } }
            },
            "root": "root",
            "version": 7
        });
        std::fs::write(dir.join("flake.lock"), lock.to_string()).unwrap();
        std::env::set_var("NIX_APP_CENTER_CONFIG_DIR", &dir);
        let inputs = flake_inputs(&current_source());
        let plan = release_plan("26.11");
        std::env::remove_var("NIX_APP_CENTER_CONFIG_DIR");
        assert_eq!(
            inputs.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
            ["home-manager"]
        );
        let plan = plan.unwrap();
        let edit = plan.edit.unwrap();
        assert!(edit.patched.contains("nixos-26.11") && edit.patched.contains("release-26.11"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_release_edit_is_written_then_reverted() {
        let dir =
            std::env::temp_dir().join(format!("nix-app-center-release-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let original = "nixpkgs.url = \"github:NixOS/nixpkgs/nixos-26.05\";\n";
        let edit = ReleaseEdit {
            file: dir.join("flake.nix"),
            original: original.into(),
            patched: retarget(original, "26.05", "26.11"),
            lock: b"{ \"old\": 1 }".to_vec(),
        };
        std::fs::write(&edit.file, original).unwrap();
        write_release_edit(&edit).unwrap();
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
        assert!(read("flake.nix").contains("nixos-26.11"));
        assert_eq!(read("flake.nix.nix-app-center-bak"), original);
        std::fs::write(dir.join("flake.lock"), "{ \"new\": 2 }").unwrap();
        revert_release_edit(&edit).unwrap();
        assert_eq!(read("flake.nix"), original);
        assert_eq!(read("flake.lock"), "{ \"old\": 1 }");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retargets_the_release_branches_only() {
        let flake = "{\n  inputs = {\n    nixpkgs.url = \"github:NixOS/nixpkgs/nixos-26.05\";\n    home-manager.url = \"github:nix-community/home-manager/release-26.05\";\n    other.url = \"github:me/nixos-26.05-notes\";\n  };\n}\n";
        let patched = retarget(flake, "26.05", "26.11");
        assert!(patched.contains("nixpkgs/nixos-26.11\""));
        assert!(patched.contains("home-manager/release-26.11\""));
        assert!(patched.contains("me/nixos-26.05-notes"), "{patched}");
        assert_eq!(
            line_diff(flake, &patched, "26.05", "26.11"),
            "nixpkgs.url: nixos-26.05 → nixos-26.11\nhome-manager.url: release-26.05 → release-26.11\n"
        );
    }

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
