//! Appels HTTP JSON partagés (Flathub, ODRS). Timeout court systématique et
//! échec silencieux (`None`) : le réseau n'est jamais indispensable — l'UI
//! dégrade en icône générique / section masquée.

use std::time::Duration;

/// Timeout global de chaque requête JSON. Sans lui, un réseau dégradé vers
/// flathub.org bloquait l'appelant jusqu'au timeout système (60-120 s) — bug
/// historique de « l'accueil figé sur ses skeletons pendant 1-2 minutes ».
pub(crate) const TIMEOUT: Duration = Duration::from_secs(6);

pub(crate) fn get_json(url: &str) -> Option<serde_json::Value> {
    let resp = ureq::get(url).timeout(TIMEOUT).call().ok()?;
    serde_json::from_str(&resp.into_string().ok()?).ok()
}

pub(crate) fn post_json(url: &str, body: &serde_json::Value) -> Option<serde_json::Value> {
    let resp = ureq::post(url)
        .timeout(TIMEOUT)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .ok()?;
    serde_json::from_str(&resp.into_string().ok()?).ok()
}
