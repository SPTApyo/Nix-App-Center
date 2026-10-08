//! JSON HTTP calls with a short timeout; failures return `None`.

use std::time::Duration;

/// Timeout of every JSON request.
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
