//! Historique « essayés » (try-first). Nix ne tient AUCUN registre des
//! `nix run` passés (le store contient des milliers de chemins, sans lien
//! avec « ce que l'utilisateur a essayé »). C'est donc Onyx qui journalise
//! chaque lancement → matérialise l'entonnoir try → keep. Le journal liste
//! les paquets du plus récent au plus ancien (dédup par `attr`).

use crate::package::Package;
use crate::paths::data_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriedEntry {
    pub package: Package,
    /// Dernier lancement (secondes Unix), pour l'ordre et l'affichage.
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

/// Charge l'historique ; vide si absent ou illisible.
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

/// Enregistre (ou remonte en tête) un paquet dans l'historique des essais.
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

/// Lance le ramasse-miettes Nix (`nix-collect-garbage`) : supprime du store tous
/// les chemins non référencés (donc les apps seulement « essayées »). C'est une
/// opération SYSTÈME (pas limitée à Onyx). Retourne la dernière ligne de sortie
/// (typiquement « … freed »).
pub fn garbage_collect() -> std::io::Result<String> {
    let out = Command::new("nix-collect-garbage").output()?;
    let text = if out.stderr.is_empty() {
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    Ok(text.lines().next_back().unwrap_or("").trim().to_string())
}
