//! "Tried" history (try-first). Nix keeps NO record of past `nix run`s (the
//! store holds thousands of paths, unrelated to "what the user tried"). So
//! Onyx logs every launch, which materializes the try → keep funnel. The log
//! lists packages from most to least recent (deduplicated by `attr`).

use crate::package::Package;
use crate::paths::data_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriedEntry {
    pub package: Package,
    /// Last launch (Unix seconds), for ordering and display.
    pub last_tried: u64,
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
        },
    );
    save_tried(&log)
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
