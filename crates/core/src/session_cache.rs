//! Mémoïsation en session (process-lifetime, sans TTL) des métadonnées
//! `AppMeta` et des octets d'icônes/screenshots — évite de refaire les mêmes
//! appels réseau/`nix eval` quand la même app est revue (recherche → détail →
//! installées…). Distinct du cache disque à TTL de `cache.rs` (résultats de
//! recherche bruts).

use crate::app_meta::{app_meta, AppMeta};
use crate::package::Package;
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::{LazyLock, Mutex};

/// Plafond mémoire des octets d'images (128 Mo décimaux). Chaque fetch est déjà
/// borné à 20 Mo ; sans plafond global, une session de navigation accumulait
/// les screenshots indéfiniment.
const BYTES_CACHE_CAP: usize = 128_000_000;

/// Cache d'octets borné : à l'insertion, évince les entrées les plus anciennes
/// (FIFO) jusqu'à repasser sous `cap`. FIFO suffit ici (pas de LRU) : les
/// re-visites proches réutilisent l'entrée bien avant qu'elle n'atteigne la tête.
struct BoundedBytes {
    cap: usize,
    total: usize,
    map: HashMap<String, Vec<u8>>,
    order: VecDeque<String>,
}

impl BoundedBytes {
    fn new(cap: usize) -> Self {
        Self { cap, total: 0, map: HashMap::new(), order: VecDeque::new() }
    }

    fn get(&self, k: &str) -> Option<Vec<u8>> {
        self.map.get(k).cloned()
    }

    fn insert(&mut self, k: String, v: Vec<u8>) {
        if self.map.contains_key(&k) {
            return; // déjà présent : ni doublon d'ordre ni double comptage
        }
        self.total += v.len();
        self.order.push_back(k.clone());
        self.map.insert(k, v);
        while self.total > self.cap {
            let Some(old) = self.order.pop_front() else { break };
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

/// Télécharge un fichier (image) en mémoire. `None` si l'URL échoue.
/// Timeout large (les screenshots font plusieurs Mo) mais borné : sans lui,
/// un serveur muet suspendait le thread appelant indéfiniment.
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

/// Comme `app_meta()` mais mémoïsé par `pkg.attr` pour toute la session.
/// Exception : une fiche calculée PENDANT le chargement de la base AppStream
/// (timeout de l'attente bornée) n'est pas mémoïsée — sinon la version REST
/// appauvrie (sans addons ni apps du développeur) collait à l'app pour toute
/// la session alors que la base locale allait devenir prête juste après.
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

/// Comme `fetch_bytes()` mais mémoïsé par URL pour la session (cache borné).
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

/// Octets déjà en cache pour cette URL, sans jamais déclencher de requête —
/// sûr à appeler depuis le thread GTK principal.
pub fn cached_bytes_if_present(url: &str) -> Option<Vec<u8>> {
    BYTES_CACHE.lock().ok()?.get(url)
}

/// Mémorise l'URL d'icône déjà résolue pour un attr (tuile/ligne de liste),
/// pour que la page détail puisse en repartir au lieu de re-résoudre à zéro.
pub fn remember_icon_url(attr: &str, url: &str) {
    if let Ok(mut c) = ICON_URL_CACHE.lock() {
        c.insert(attr.to_string(), url.to_string());
    }
}

/// URL d'icône déjà connue pour un attr, si une tuile/ligne l'a déjà résolue.
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
        BYTES_CACHE.lock().unwrap().remove("http://sentinel-url-test");
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
        assert_eq!(known_icon_url("sentinel-attr-icon"), Some("http://example/icon.png".to_string()));
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
        c.insert("b".into(), vec![0; 6]); // total 12 > 10 → « a » (plus ancien) évincé
        assert!(c.get("a").is_none(), "l'entrée la plus ancienne doit être évincée");
        assert_eq!(c.get("b").map(|v| v.len()), Some(6));
        assert_eq!(c.total, 6, "la comptabilité doit suivre l'éviction");
    }

    #[test]
    fn bounded_bytes_reinsert_same_key_keeps_one_copy() {
        let mut c = BoundedBytes::new(100);
        c.insert("a".into(), vec![0; 6]);
        c.insert("a".into(), vec![0; 6]); // déjà présent → no-op
        assert_eq!(c.total, 6);
    }
}
