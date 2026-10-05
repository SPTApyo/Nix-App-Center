//! "Tried" history (try-first). Nix keeps NO record of past `nix run`s (the
//! store holds thousands of paths, unrelated to "what the user tried"). So
//! Onyx logs every launch, which materializes the try → keep funnel. The log
//! lists packages from most to least recent (deduplicated by `attr`).

use crate::package::Package;
use crate::paths::data_dir;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriedEntry {
    pub package: Package,
    /// Last launch (Unix seconds), for ordering and display.
    pub last_tried: u64,
    /// Store paths the launch fetched, to remove them later.
    #[serde(default)]
    pub out_paths: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TriedLog {
    pub tried: Vec<TriedEntry>,
}

fn tried_path() -> PathBuf {
    data_dir().join("tried.json")
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Loads the history; empty when missing or unreadable.
pub fn load_tried() -> TriedLog {
    match std::fs::read(tried_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => TriedLog::default(),
    }
}

fn save_tried(log: &TriedLog) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(log).map_err(std::io::Error::other)?;
    crate::paths::write_atomic(&tried_path(), &json)
}

/// Records (or moves to the top) a package in the tried history.
pub fn record_tried(pkg: &Package) -> std::io::Result<()> {
    let mut log = load_tried();
    log.tried.retain(|e| e.package.attr != pkg.attr);
    log.tried.insert(
        0,
        TriedEntry {
            package: pkg.clone(),
            last_tried: now_unix(),
            out_paths: Vec::new(),
        },
    );
    save_tried(&log)
}

/// Stores the outputs `nix run` fetched for `attr`. Blocks on `nix eval`.
pub(crate) fn record_out_paths(attr: &str) {
    let paths = out_paths(attr);
    if paths.is_empty() {
        return;
    }
    let mut log = load_tried();
    if let Some(e) = log.tried.iter_mut().find(|e| e.package.attr == attr) {
        e.out_paths = paths;
        let _ = save_tried(&log);
    }
}

/// Outputs `nix run nixpkgs#<attr>` installs, as store paths.
fn out_paths(attr: &str) -> Vec<String> {
    let out = Command::new("env")
        .args([
            "NIXPKGS_ALLOW_UNFREE=1",
            "NIXPKGS_ALLOW_BROKEN=1",
            "nix",
            "eval",
            "--impure",
            "--json",
            &format!("{}#{attr}", crate::nixpkgs_ref()),
            "--apply",
            "p: map (o: p.${o}.outPath) (p.meta.outputsToInstall or [ \"out\" ])",
            "--extra-experimental-features",
            "nix-command flakes",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => serde_json::from_slice(&o.stdout).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Removes one tried app from the store and from the history. Only paths no
/// GC root reaches and no other tried app needs are deleted, so the system
/// and the other apps stay intact. Returns the bytes freed. Slow (scans the
/// GC roots): call it from a thread.
pub fn remove_tried(attr: &str) -> std::io::Result<u64> {
    let mut log = load_tried();
    let Some(entry) = log.tried.iter().find(|e| e.package.attr == attr) else {
        return Ok(0);
    };
    let targets = if entry.out_paths.is_empty() {
        out_paths(attr)
    } else {
        entry.out_paths.clone()
    };
    // ponytail: entries without recorded paths are not protected.
    let keep: Vec<String> = log
        .tried
        .iter()
        .filter(|e| e.package.attr != attr)
        .flat_map(|e| e.out_paths.clone())
        .collect();

    let dead = dead_paths()?;
    let (doomed, freed) = plan_removal(&targets, &keep, &dead);
    if !doomed.is_empty() {
        delete_paths(&doomed)?;
    }
    log.tried.retain(|e| e.package.attr != attr);
    save_tried(&log)?;
    Ok(freed)
}

/// What the store says about a dead path.
#[derive(Debug, Default, Clone)]
pub(crate) struct PathInfo {
    pub references: Vec<String>,
    pub deriver: Option<String>,
    pub nar_size: u64,
}

/// Every dead store path with its references, deriver and size.
fn dead_paths() -> std::io::Result<HashMap<String, PathInfo>> {
    let out = Command::new("nix-store")
        .args(["--gc", "--print-dead"])
        .stderr(Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(std::io::Error::other("nix-store --gc --print-dead failed"));
    }
    let mut child = Command::new("nix")
        .args([
            "path-info",
            "--json",
            "--stdin",
            "--extra-experimental-features",
            "nix-command",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let list = out.stdout;
    let writer = std::thread::spawn(move || stdin.write_all(&list));
    let info = child.wait_with_output()?;
    let _ = writer.join();
    let v: serde_json::Value =
        serde_json::from_slice(&info.stdout).map_err(std::io::Error::other)?;
    Ok(parse_path_info(&v))
}

/// Reads `nix path-info --json`: an object keyed by path (Nix 2.19+) or an
/// array of objects with a `path` field (older Nix).
pub(crate) fn parse_path_info(v: &serde_json::Value) -> HashMap<String, PathInfo> {
    let one = |o: &serde_json::Value| PathInfo {
        references: o["references"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|r| r.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        deriver: o["deriver"].as_str().map(str::to_string),
        nar_size: o["narSize"].as_u64().unwrap_or(0),
    };
    match v {
        serde_json::Value::Object(m) => m
            .iter()
            .filter(|(_, o)| o.is_object())
            .map(|(k, o)| (k.clone(), one(o)))
            .collect(),
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|o| Some((o["path"].as_str()?.to_string(), one(o))))
            .collect(),
        _ => HashMap::new(),
    }
}

/// Paths to delete for `targets`, and their total size. Pure.
///
/// Takes the dead closure of the targets (and their derivers, which
/// keep-outputs ties to them), minus what `keep` needs, then drops every path
/// still referred to by a dead path outside the set: `nix store delete`
/// refuses those, and would abort the whole batch.
pub(crate) fn plan_removal(
    targets: &[String],
    keep: &[String],
    dead: &HashMap<String, PathInfo>,
) -> (Vec<String>, u64) {
    let closure = |roots: &[String]| -> HashSet<String> {
        let mut seen = HashSet::new();
        let mut stack: Vec<String> = roots
            .iter()
            .filter(|p| dead.contains_key(*p))
            .cloned()
            .collect();
        while let Some(p) = stack.pop() {
            if !seen.insert(p.clone()) {
                continue;
            }
            for r in &dead[&p].references {
                if dead.contains_key(r) && !seen.contains(r) {
                    stack.push(r.clone());
                }
            }
        }
        seen
    };
    let kept = closure(keep);
    let mut set: HashSet<String> = closure(targets);
    let derivers: Vec<String> = set
        .iter()
        .filter_map(|p| dead[p].deriver.clone())
        .filter(|d| dead.contains_key(d))
        .collect();
    set.extend(derivers);
    // Sibling outputs of those derivers go with them.
    let siblings: Vec<String> = dead
        .iter()
        .filter(|(_, i)| i.deriver.as_ref().is_some_and(|d| set.contains(d)))
        .map(|(p, _)| p.clone())
        .collect();
    set.extend(siblings);
    set.retain(|p| !kept.contains(p));

    // Referrers among dead paths, deriver links counted both ways.
    let mut referrers: HashMap<&str, Vec<&str>> = HashMap::new();
    for (p, info) in dead {
        for r in &info.references {
            if r != p {
                referrers.entry(r).or_default().push(p);
            }
        }
        if let Some(d) = info.deriver.as_deref().filter(|d| dead.contains_key(*d)) {
            referrers.entry(p).or_default().push(d);
            referrers.entry(d).or_default().push(p);
        }
    }
    loop {
        let blocked: Vec<String> = set
            .iter()
            .filter(|p| {
                referrers
                    .get(p.as_str())
                    .is_some_and(|rs| rs.iter().any(|r| !set.contains(*r)))
            })
            .cloned()
            .collect();
        if blocked.is_empty() {
            break;
        }
        for b in blocked {
            set.remove(&b);
        }
    }
    let freed = set.iter().map(|p| dead[p].nar_size).sum();
    let mut doomed: Vec<String> = set.into_iter().collect();
    doomed.sort();
    (doomed, freed)
}

fn delete_paths(paths: &[String]) -> std::io::Result<()> {
    let out = Command::new("nix")
        .args([
            "store",
            "delete",
            "--extra-experimental-features",
            "nix-command",
        ])
        .args(paths)
        .stdout(Stdio::null())
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(std::io::Error::other(
            err.lines()
                .rev()
                .find(|l| l.starts_with("error"))
                .unwrap_or("nix store delete failed")
                .to_string(),
        ))
    }
}

/// Runs the Nix garbage collector (`nix-collect-garbage`): removes every
/// unreferenced path from the store (so apps that were only "tried"). This is
/// a SYSTEM operation (not limited to Onyx). Returns the last output line
/// (typically "… freed").
pub fn garbage_collect() -> std::io::Result<String> {
    let out = Command::new("nix-collect-garbage").output()?;
    let text = if out.stderr.is_empty() {
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    Ok(text.lines().next_back().unwrap_or("").trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(refs: &[&str], deriver: Option<&str>, size: u64) -> PathInfo {
        PathInfo {
            references: refs.iter().map(|s| s.to_string()).collect(),
            deriver: deriver.map(str::to_string),
            nar_size: size,
        }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn removes_dead_closure_with_deriver() {
        let dead = HashMap::from([
            (
                "vlc".to_string(),
                info(&["vlc", "libvlc"], Some("vlc.drv"), 10),
            ),
            ("libvlc".to_string(), info(&[], None, 5)),
            ("vlc.drv".to_string(), info(&[], None, 1)),
        ]);
        let (doomed, freed) = plan_removal(&s(&["vlc"]), &[], &dead);
        assert_eq!(doomed, s(&["libvlc", "vlc", "vlc.drv"]));
        assert_eq!(freed, 16);
    }

    #[test]
    fn keeps_what_other_tried_apps_need() {
        let dead = HashMap::from([
            ("vlc".to_string(), info(&["qt"], None, 10)),
            ("mpv".to_string(), info(&["qt"], None, 7)),
            ("qt".to_string(), info(&[], None, 50)),
        ]);
        let (doomed, freed) = plan_removal(&s(&["vlc"]), &s(&["mpv"]), &dead);
        assert_eq!(doomed, s(&["vlc"]));
        assert_eq!(freed, 10);
    }

    #[test]
    fn skips_paths_used_by_unrelated_dead_paths() {
        // "old" is dead garbage that also uses qt: deleting qt would fail.
        let dead = HashMap::from([
            ("vlc".to_string(), info(&["qt"], None, 10)),
            ("old".to_string(), info(&["qt"], None, 3)),
            ("qt".to_string(), info(&[], None, 50)),
        ]);
        let (doomed, _) = plan_removal(&s(&["vlc"]), &[], &dead);
        assert_eq!(doomed, s(&["vlc"]));
    }

    #[test]
    fn takes_sibling_outputs_along() {
        let dead = HashMap::from([
            ("vlc".to_string(), info(&[], Some("vlc.drv"), 10)),
            ("vlc-man".to_string(), info(&[], Some("vlc.drv"), 2)),
            ("vlc.drv".to_string(), info(&[], None, 1)),
        ]);
        let (doomed, freed) = plan_removal(&s(&["vlc"]), &[], &dead);
        assert_eq!(doomed, s(&["vlc", "vlc-man", "vlc.drv"]));
        assert_eq!(freed, 13);
    }

    #[test]
    fn live_targets_delete_nothing() {
        let (doomed, freed) = plan_removal(&s(&["vlc"]), &[], &HashMap::new());
        assert!(doomed.is_empty());
        assert_eq!(freed, 0);
    }

    #[test]
    fn parses_both_path_info_shapes() {
        let obj =
            serde_json::json!({"/a": {"references": ["/b"], "deriver": "/a.drv", "narSize": 4}});
        let arr = serde_json::json!([{"path": "/a", "references": ["/b"], "deriver": "/a.drv", "narSize": 4}]);
        for v in [obj, arr] {
            let m = parse_path_info(&v);
            assert_eq!(m["/a"].references, s(&["/b"]));
            assert_eq!(m["/a"].deriver.as_deref(), Some("/a.drv"));
            assert_eq!(m["/a"].nar_size, 4);
        }
    }
}
