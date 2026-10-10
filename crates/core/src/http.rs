//! HTTP calls with a timeout. JSON helpers return `None` on failure.

use std::io::Read;
use std::time::Duration;

/// Timeout of every JSON request.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(6);

/// GET `url` as a reader; callers cap its size with `take`.
pub(crate) fn get(url: &str, timeout: Duration) -> Result<ureq::BodyReader<'static>, ureq::Error> {
    let resp = ureq::get(url)
        .config()
        .timeout_global(Some(timeout))
        .build()
        .call()?;
    Ok(resp.into_body().into_with_config().limit(u64::MAX).reader())
}

/// True when `url` answers HEAD with a success status.
pub fn exists(url: &str) -> bool {
    ureq::head(url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .call()
        .is_ok()
}

pub(crate) fn get_json(url: &str) -> Option<serde_json::Value> {
    let mut text = String::new();
    get(url, TIMEOUT)
        .ok()?
        .take(50 * 1024 * 1024)
        .read_to_string(&mut text)
        .ok()?;
    serde_json::from_str(&text).ok()
}

pub(crate) fn post_json(url: &str, body: &serde_json::Value) -> Option<serde_json::Value> {
    let resp = ureq::post(url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .build()
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .ok()?;
    serde_json::from_str(&resp.into_body().read_to_string().ok()?).ok()
}
