//! Cache disque TTL des résultats de recherche.

use crate::Package;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".cache")
        })
        .join("onyx")
}

/// Hash FNV-1a 64 bits, stable entre versions de Rust (contrairement à
/// `DefaultHasher`) : la clé de cache doit rester identique d'un build à l'autre.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn key_path(key: &str) -> PathBuf {
    // Préfixe lisible (débogage) borné à 40 chars + hash de la clé COMPLÈTE :
    // les patterns d'alternation des catégories (centaines de caractères)
    // dépassaient la limite de nom de fichier (ENAMETOOLONG silencieux → cache
    // inerte sur le chemin accueil/catégories, le plus coûteux) ; et deux clés
    // distinctes pouvaient s'assainir pareil ("c++"/"c--" → "c__").
    let safe: String = key
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .take(40)
        .collect();
    cache_dir().join(format!("search-{safe}-{:016x}.json", fnv1a(key)))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Entry {
    at: u64,
    results: Vec<Package>,
}

/// Sert `key` depuis le cache si frais (< ttl_secs), sinon appelle `compute`,
/// stocke et renvoie. ttl_secs = 0 → toujours recalculer.
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

    /// Isole le cache dans un dossier temporaire vierge. Retourne le verrou
    /// env : `set_var` est process-global (cf. `crate::env_lock`).
    fn iso() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::env_lock();
        std::env::set_var(
            "XDG_CACHE_HOME",
            std::env::temp_dir().join(format!("onyx-cache-{}", std::process::id())),
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
        // ENAMETOOLONG : les patterns d'alternation des catégories dépassent
        // 255 octets → le nom de fichier doit être borné quel que soit la clé.
        let long_key = "x".repeat(5000);
        let fname = key_path(&long_key)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(fname.len() < 100, "nom de fichier borné, obtenu {}", fname.len());
        // L'assainissement seul collisionnait : "c++" et "c--" → "c__".
        assert_ne!(key_path("c++"), key_path("c--"));
        // Déterminisme (le cache doit se retrouver d'un appel à l'autre).
        assert_eq!(key_path("gimp"), key_path("gimp"));
    }

    #[test]
    fn caches_long_pattern_keys() {
        // Régression : avec une clé longue, read/write échouaient en
        // ENAMETOOLONG (ignoré) → le 2e appel recalculait au lieu de servir.
        let _g = iso();
        let key = format!(
            "^({})$",
            (0..300).map(|i| format!("pkg{i}")).collect::<Vec<_>>().join("|")
        );
        let _ = cached_search(&key, 60, || vec![Package::from_attr("firefox")]);
        let b = cached_search(&key, 60, || vec![Package::from_attr("SHOULD_NOT_RUN")]);
        assert_eq!(b[0].attr, "firefox", "le cache doit servir les clés longues");
    }
}
