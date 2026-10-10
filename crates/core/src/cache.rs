//! TTL disk cache of search results.

use crate::Package;
use std::path::PathBuf;

pub(crate) fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = crate::util::home();
            home.join(".cache")
        })
        .join("nix-software")
}

/// FNV-1a hash, stable across builds unlike `DefaultHasher`.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn key_path(key: &str) -> PathBuf {
    // Short readable prefix, hash keeps keys unique.
    let safe: String = key
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(40)
        .collect();
    cache_dir().join(format!("search-{safe}-{:016x}.json", fnv1a(key)))
}

fn now() -> u64 {
    crate::util::now_unix()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Entry {
    at: u64,
    results: Vec<Package>,
}

/// Returns the cached result of `key` when younger than `ttl_secs`, else
/// computes and stores it.
pub fn cached_search<F: FnOnce() -> Vec<Package>>(
    key: &str,
    ttl_secs: u64,
    compute: F,
) -> Vec<Package> {
    let path = key_path(key);
    if ttl_secs > 0 {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(e) = serde_json::from_slice::<Entry>(&bytes) {
                if now().saturating_sub(e.at) < ttl_secs {
                    return e.results;
                }
            }
        }
    }
    let results = compute();
    let _ = std::fs::create_dir_all(cache_dir());
    if let Ok(json) = serde_json::to_vec(&Entry {
        at: now(),
        results: results.clone(),
    }) {
        let _ = std::fs::write(&path, json);
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Package;

    /// Isolates the cache in a clean temp folder. Returns the env lock:
    /// `set_var` is process global (see `crate::env_lock`).
    fn iso() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::env_lock();
        std::env::set_var(
            "XDG_CACHE_HOME",
            std::env::temp_dir().join(format!("nix-software-cache-{}", std::process::id())),
        );
        let _ = std::fs::remove_dir_all(cache_dir());
        guard
    }

    #[test]
    fn caches_and_serves() {
        let _g = iso();
        let a = cached_search("ff", 60, || vec![Package::from_attr("firefox")]);
        let b = cached_search("ff", 60, || vec![Package::from_attr("SHOULD_NOT_RUN")]);
        assert_eq!(a.len(), b.len());
        assert_eq!(b[0].attr, "firefox");
    }

    #[test]
    fn expires() {
        let _g = iso();
        let _ = cached_search("xx", 0, || vec![Package::from_attr("a")]);
        let b = cached_search("xx", 0, || vec![Package::from_attr("b")]);
        assert_eq!(b[0].attr, "b");
    }

    #[test]
    fn key_path_bounded_and_collision_free() {
        // File names stay short for any key.
        let long_key = "x".repeat(5000);
        let fname = key_path(&long_key)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(fname.len() < 100, "bounded file name, got {}", fname.len());
        // Sanitizing alone collided: "c++" and "c--" → "c__".
        assert_ne!(key_path("c++"), key_path("c--"));
        assert_eq!(key_path("gimp"), key_path("gimp"));
    }

    #[test]
    fn caches_long_pattern_keys() {
        // Long keys are cached too.
        let _g = iso();
        let key = format!(
            "^({})$",
            (0..300)
                .map(|i| format!("pkg{i}"))
                .collect::<Vec<_>>()
                .join("|")
        );
        let _ = cached_search(&key, 60, || vec![Package::from_attr("firefox")]);
        let b = cached_search(&key, 60, || vec![Package::from_attr("SHOULD_NOT_RUN")]);
        assert_eq!(b[0].attr, "firefox", "the cache must serve long keys");
    }
}
