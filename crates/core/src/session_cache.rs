//! In-session memoization (process lifetime, no TTL) of `AppMeta` metadata
//! and icon/screenshot bytes: avoids repeating the same network/`nix eval`
//! calls when the same app is seen again (search → details → installed…).
//! Distinct from the TTL disk cache of `cache.rs` (raw search results).

use crate::app_meta::{app_meta, AppMeta};
use crate::package::Package;
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::{LazyLock, Mutex};

/// Memory cap of image bytes (128 MB decimal). Each fetch is already bounded
/// to 20 MB; without a global cap, a browsing session piled up screenshots
/// forever.
const BYTES_CACHE_CAP: usize = 128_000_000;

/// Bounded byte cache: on insertion, evicts the oldest entries (FIFO) until
/// back under `cap`. FIFO is enough (no LRU needed): close revisits reuse the
/// entry long before it reaches the head.
struct BoundedBytes {
    cap: usize,
    total: usize,
    map: HashMap<String, Vec<u8>>,
    order: VecDeque<String>,
}

impl BoundedBytes {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            total: 0,
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&self, k: &str) -> Option<Vec<u8>> {
        self.map.get(k).cloned()
    }

    fn insert(&mut self, k: String, v: Vec<u8>) {
        if self.map.contains_key(&k) {
            return; // already present: no duplicate order or double count
        }
        self.total += v.len();
        self.order.push_back(k.clone());
        self.map.insert(k, v);
        while self.total > self.cap {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            if let Some(v) = self.map.remove(&old) {
                self.total -= v.len();
            }
        }
    }

    #[cfg(test)]
    fn remove(&mut self, k: &str) {
        if let Some(v) = self.map.remove(k) {
            self.total -= v.len();
        }
        self.order.retain(|x| x != k);
    }
}

static APP_META_CACHE: LazyLock<Mutex<HashMap<String, AppMeta>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static BYTES_CACHE: LazyLock<Mutex<BoundedBytes>> =
    LazyLock::new(|| Mutex::new(BoundedBytes::new(BYTES_CACHE_CAP)));
static ICON_URL_CACHE: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Downloads a file (image) into memory. `None` when the URL fails. Generous
/// timeout (screenshots weigh several MB) but bounded: without it, a silent
/// server hung the calling thread forever.
fn fetch_bytes(url: &str) -> Option<Vec<u8>> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .ok()?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(20_000_000)
        .read_to_end(&mut buf)
        .ok()?;
    Some(buf)
}

/// Like `app_meta()` but memoized by `pkg.attr` for the whole session.
/// Exception: a listing computed WHILE the AppStream database loads (bounded
/// wait timed out) is not memoized, otherwise the poorer REST version
/// (without add-ons or developer apps) stuck to the app for the whole session
/// while the local database was about to become ready.
pub fn cached_app_meta(pkg: &Package, screenshot_target_w: u32) -> AppMeta {
    if let Ok(cache) = APP_META_CACHE.lock() {
        if let Some(m) = cache.get(&pkg.attr) {
            return m.clone();
        }
    }
    let m = app_meta(pkg, screenshot_target_w);
    if crate::appstream_db::is_settled() {
        if let Ok(mut cache) = APP_META_CACHE.lock() {
            cache.insert(pkg.attr.clone(), m.clone());
        }
    }
    m
}

/// Like `fetch_bytes()` but memoized by URL for the session (bounded cache).
pub fn cached_fetch_bytes(url: &str) -> Option<Vec<u8>> {
    if let Ok(cache) = BYTES_CACHE.lock() {
        if let Some(b) = cache.get(url) {
            return Some(b);
        }
    }
    let b = fetch_bytes(url)?;
    if let Ok(mut cache) = BYTES_CACHE.lock() {
        cache.insert(url.to_string(), b.clone());
    }
    Some(b)
}

/// Bytes already cached for this URL, never triggering a request: safe to
/// call from the main GTK thread.
pub fn cached_bytes_if_present(url: &str) -> Option<Vec<u8>> {
    BYTES_CACHE.lock().ok()?.get(url)
}

/// Remembers the icon URL already resolved for an attr (list tile/row), so
/// the details page can start from it instead of resolving from scratch.
pub fn remember_icon_url(attr: &str, url: &str) {
    if let Ok(mut c) = ICON_URL_CACHE.lock() {
        c.insert(attr.to_string(), url.to_string());
    }
}

/// Icon URL already known for an attr, if a tile/row resolved it.
pub fn known_icon_url(attr: &str) -> Option<String> {
    ICON_URL_CACHE.lock().ok()?.get(attr).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_fetch_bytes_lit_depuis_le_cache() {
        BYTES_CACHE
            .lock()
            .unwrap()
            .insert("http://sentinel-url-test".to_string(), vec![7u8, 8, 9]);
        let result = cached_fetch_bytes("http://sentinel-url-test");
        assert_eq!(result, Some(vec![7u8, 8, 9]));
        BYTES_CACHE
            .lock()
            .unwrap()
            .remove("http://sentinel-url-test");
    }

    #[test]
    fn cached_app_meta_lit_depuis_le_cache() {
        let pkg = Package::from_attr("test-cache-sentinel-pkg");
        let fausse_meta = AppMeta {
            name: "SentinelApp".to_string(),
            ..Default::default()
        };
        APP_META_CACHE
            .lock()
            .unwrap()
            .insert(pkg.attr.clone(), fausse_meta);
        let result = cached_app_meta(&pkg, 1920);
        assert_eq!(result.name, "SentinelApp");
        APP_META_CACHE.lock().unwrap().remove(&pkg.attr);
    }

    #[test]
    fn known_icon_url_round_trips() {
        assert_eq!(known_icon_url("sentinel-attr-icon"), None);
        remember_icon_url("sentinel-attr-icon", "http://example/icon.png");
        assert_eq!(
            known_icon_url("sentinel-attr-icon"),
            Some("http://example/icon.png".to_string())
        );
        ICON_URL_CACHE.lock().unwrap().remove("sentinel-attr-icon");
    }

    #[test]
    fn cached_bytes_if_present_does_not_fetch() {
        assert_eq!(cached_bytes_if_present("http://sentinel-not-cached"), None);
    }

    #[test]
    fn bounded_bytes_evicts_oldest_when_over_cap() {
        let mut c = BoundedBytes::new(10);
        c.insert("a".into(), vec![0; 6]);
        c.insert("b".into(), vec![0; 6]); // total 12 > 10, so "a" (oldest) is evicted
        assert!(c.get("a").is_none(), "the oldest entry must be evicted");
        assert_eq!(c.get("b").map(|v| v.len()), Some(6));
        assert_eq!(c.total, 6, "accounting must follow eviction");
    }

    #[test]
    fn bounded_bytes_reinsert_same_key_keeps_one_copy() {
        let mut c = BoundedBytes::new(100);
        c.insert("a".into(), vec![0; 6]);
        c.insert("a".into(), vec![0; 6]); // already present, no-op
        assert_eq!(c.total, 6);
    }
}
