//! Shared JSON HTTP calls (Flathub, ODRS). Always a short timeout and a
//! silent failure (`None`): the network is never required, the UI degrades
//! to a generic icon or a hidden section.

use std::time::Duration;

/// Global timeout of every JSON request. Without it, a degraded network to
/// flathub.org blocked the caller until the system timeout (60-120 s), the
/// historical bug of "home stuck on its skeletons for 1-2 minutes".
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
