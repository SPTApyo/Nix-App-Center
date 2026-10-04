# Corrections & améliorations de la couche données — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Corriger les 8 défauts identifiés par l'audit de la couche données (cache disque inopérant, updates non cachés/faux positifs, latence page détail, RAM non bornée, split recherche, collisions d'index AppStream, écritures non atomiques, threads non bornés).

**Architecture:** Chaque tâche est un correctif localisé dans un module existant, TDD quand la logique est pure (extraction de fonctions pures là où l'I/O gênait le test). Aucun nouveau module, aucune nouvelle dépendance.

**Tech Stack:** Rust (workspace `crates/core` + `crates/gui`), relm4/GTK4 côté GUI, tests `cargo test` via `direnv exec`.

## Global Constraints

- Toute commande cargo passe par `direnv exec . cargo …` (cargo absent du PATH).
- Tests mutant l'environnement process : prendre `crate::env_lock()`.
- Commentaires en français, expliquant le pourquoi/bug historique, pas le quoi.
- **PAS DE COMMIT** : la branche `details-page-gnome-1-1` porte du WIP non commité de l'utilisateur dans les mêmes fichiers — committer mélangerait son travail aux correctifs. L'utilisateur committera lui-même.
- Réseau JSON : uniquement via `crate::http::{get_json,post_json}` (timeout 6 s) — aucun nouveau point d'appel réseau dans ce plan.

---

### Task 1: Cache disque — clés hachées (fix ENAMETOOLONG + collisions)

Le nom de fichier de cache est construit depuis la clé **entière** assainie. Les patterns d'alternation des catégories/accueil (600-6000 caractères) dépassent la limite de 255 octets d'un nom de fichier Linux → `fs::read`/`fs::write` échouent en `ENAMETOOLONG`, silencieusement : le cache est inerte exactement là où le commentaire du module dit qu'il compte le plus. Bonus : `c++` et `c--` s'assainissent tous deux en `c__` (collision).

**Files:**
- Modify: `crates/core/src/cache.rs` (fonction `key_path`, lignes 17-23)
- Test: `crates/core/src/cache.rs` (module `tests` existant)

**Interfaces:**
- Consumes: rien de nouveau.
- Produces: `key_path(key: &str) -> PathBuf` inchangé en signature ; noms de fichiers désormais bornés (< 100 chars) et sans collision (`search-<préfixe40>-<fnv64hex>.json`).

- [x] **Step 1: Écrire les tests qui échouent**

Dans le module `tests` de `crates/core/src/cache.rs`, ajouter :

```rust
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
```

- [x] **Step 2: Vérifier qu'ils échouent**

Run: `direnv exec . cargo test -p onyx-core cache`
Expected: `key_path_bounded_and_collision_free` FAIL (nom ~5011 chars) et `caches_long_pattern_keys` FAIL (`SHOULD_NOT_RUN`).

- [x] **Step 3: Implémenter**

Remplacer `key_path` dans `crates/core/src/cache.rs` :

```rust
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
```

- [x] **Step 4: Vérifier que tout passe**

Run: `direnv exec . cargo test -p onyx-core cache`
Expected: PASS (les 4 tests du module).

---

### Task 2: Updates — un seul `nix search` groupé + garde version vide

`available_updates()` fait un `nix search` **par paquet gardé, séquentiel, sans cache** — incohérent avec le reste du crate. Et un paquet gardé via `onyx-cli keep` (version stockée vide) apparaît éternellement en mise à jour (`version_newer("x.y", "") == true`).

**Files:**
- Modify: `crates/core/src/categories.rs:55` (`fn nix_attr_lookup` → `pub(crate) fn`)
- Modify: `crates/core/src/updates.rs` (supprimer `latest_version`, réécrire `available_updates`, extraire `compute_updates` pur)
- Test: `crates/core/src/updates.rs` (module `tests` existant)

**Interfaces:**
- Consumes: `categories::nix_attr_lookup(candidates: &[String]) -> HashMap<String, Package>` (clé = attr minuscule exact, déjà servi par `cache::cached_search`).
- Produces: `available_updates() -> Vec<Update>` inchangé ; nouveau cœur pur `fn compute_updates(kept: Vec<Package>, current: &HashMap<String, Package>) -> Vec<Update>`.

- [x] **Step 1: Écrire les tests qui échouent**

Dans le module `tests` de `crates/core/src/updates.rs`, ajouter :

```rust
use std::collections::HashMap;

fn kept(attr: &str, version: &str) -> Package {
    let mut p = Package::from_attr(attr);
    p.version = version.to_string();
    p
}

fn current(entries: &[(&str, &str)]) -> HashMap<String, Package> {
    entries
        .iter()
        .map(|(attr, v)| ((*attr).to_lowercase(), kept(attr, v)))
        .collect()
}

#[test]
fn compute_updates_reports_newer_only() {
    let cur = current(&[("firefox", "2.0"), ("btop", "1.0")]);
    let ups = compute_updates(vec![kept("firefox", "1.0"), kept("btop", "1.0")], &cur);
    assert_eq!(ups.len(), 1);
    assert_eq!(ups[0].package.attr, "firefox");
    assert_eq!(ups[0].available_version, "2.0");
    assert_eq!(ups[0].installed_version, "1.0");
}

#[test]
fn compute_updates_skips_unknown_attr() {
    // Paquet retiré de nixpkgs : absent du lookup → pas de fausse update.
    let ups = compute_updates(vec![kept("ghost", "1.0")], &HashMap::new());
    assert!(ups.is_empty());
}

#[test]
fn compute_updates_skips_kept_without_version() {
    // Régression : `onyx-cli keep <attr>` stocke une version vide ;
    // version_newer("x.y", "") est vrai → update éternelle affichée.
    let cur = current(&[("ripgrep", "14.1.0")]);
    let ups = compute_updates(vec![kept("ripgrep", "")], &cur);
    assert!(ups.is_empty(), "version stockée vide = comparaison impossible");
}

#[test]
fn compute_updates_matches_attr_case_insensitively() {
    // Le lookup est indexé en minuscules ; un attr gardé avec majuscules
    // (`superTux`) doit quand même être résolu.
    let cur = current(&[("supertux", "0.7")]);
    let ups = compute_updates(vec![kept("superTux", "0.6")], &cur);
    assert_eq!(ups.len(), 1);
}
```

- [x] **Step 2: Vérifier qu'ils échouent**

Run: `direnv exec . cargo test -p onyx-core updates`
Expected: erreur de compilation « cannot find function `compute_updates` ».

- [x] **Step 3: Implémenter**

Dans `crates/core/src/categories.rs`, ligne 55 : `fn nix_attr_lookup(` → `pub(crate) fn nix_attr_lookup(`.

Dans `crates/core/src/updates.rs`, remplacer les imports, supprimer `latest_version`, réécrire :

```rust
//! Détection des mises à jour : version gardée vs version courante de nixpkgs.

use crate::categories::nix_attr_lookup;
use crate::{load_manifest, Package};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Update {
    pub package: Package,
    pub installed_version: String,
    pub available_version: String,
}

// … version_newer inchangé …

/// Cœur pur de la détection (testable sans `nix`). Un paquet gardé sans
/// version stockée (ex. `onyx-cli keep`) est ignoré : impossible de comparer,
/// et le traiter comme « 0 » l'affichait éternellement comme à mettre à jour.
fn compute_updates(kept: Vec<Package>, current: &HashMap<String, Package>) -> Vec<Update> {
    kept.into_iter()
        .filter_map(|p| {
            if p.version.is_empty() {
                return None;
            }
            let available = current.get(&p.attr.to_lowercase())?.version.clone();
            if version_newer(&available, &p.version) {
                Some(Update {
                    available_version: available,
                    installed_version: p.version.clone(),
                    package: p,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Pour chaque paquet « gardé », compare sa version stockée à celle de nixpkgs —
/// en UN `nix search` groupé (alternation ancrée, comme `categories`), servi par
/// le cache disque, au lieu d'un appel par paquet, séquentiel et jamais caché.
pub fn available_updates() -> Vec<Update> {
    let kept = load_manifest().kept;
    if kept.is_empty() {
        return vec![];
    }
    let attrs: Vec<String> = kept.iter().map(|p| p.attr.to_lowercase()).collect();
    compute_updates(kept, &nix_attr_lookup(&attrs))
}
```

(`use crate::util::regex_escape;` et `use std::process::Command;` deviennent inutiles — les retirer.)

- [x] **Step 4: Vérifier que tout passe**

Run: `direnv exec . cargo test -p onyx-core updates`
Expected: PASS (6 tests : 2 existants version_newer + 4 nouveaux).

---

### Task 3: `app_meta` — AppStream et `nix eval` en parallèle

`app_meta()` enchaîne `flathub_meta_for` (réseau possible, 2×6 s pire cas) **puis** `detail()` (`nix eval`, secondes à froid) : la page détail attend la **somme** de deux I/O indépendants.

**Files:**
- Modify: `crates/core/src/app_meta.rs:56-60` (fonction `app_meta`)

**Interfaces:**
- Consumes: `detail(attr) -> Option<PackageDetail>`, `flathub_meta_for(pkg, w) -> Option<FlathubMeta>` (inchangés).
- Produces: `app_meta(pkg, w) -> AppMeta` inchangé en signature et en sémantique (mêmes entrées de merge — les tests existants de `merge_app_meta` couvrent la fusion).

- [x] **Step 1: Implémenter** (pas de nouveau test : orchestration d'I/O pure, la sémantique de fusion est déjà fixée par les 12 tests de `merge_app_meta`)

```rust
/// Construit les métadonnées fusionnées d'un paquet (AppStream prioritaire, Nix
/// fallback). Effectue les appels réseau/`nix eval` : à lancer dans un thread.
/// Les deux sources sont indépendantes → interrogées en parallèle : la latence
/// de la page détail est le max des deux, pas leur somme.
pub fn app_meta(pkg: &Package, screenshot_target_w: u32) -> AppMeta {
    let attr = pkg.attr.clone();
    let nix_handle = std::thread::spawn(move || detail(&attr));
    let fh = flathub_meta_for(pkg, screenshot_target_w);
    let nix = nix_handle.join().ok().flatten();
    merge_app_meta(pkg, fh.as_ref(), nix.as_ref())
}
```

- [x] **Step 2: Vérifier la non-régression**

Run: `direnv exec . cargo test -p onyx-core app_meta`
Expected: PASS (les 12 tests de fusion existants).

---

### Task 4: `BYTES_CACHE` borné (éviction FIFO)

Le cache d'octets d'images retient chaque icône/screenshot (≤ 20 Mo pièce) **pour toute la session, sans limite** — la RAM croît indéfiniment en navigation.

**Files:**
- Modify: `crates/core/src/session_cache.rs` (remplacer le `HashMap` nu par une structure bornée)
- Test: `crates/core/src/session_cache.rs` (module `tests` existant)

**Interfaces:**
- Consumes: rien de nouveau.
- Produces: `cached_fetch_bytes`, `cached_bytes_if_present` inchangés en signature. Nouvelle struct interne `BoundedBytes { get(&str) -> Option<Vec<u8>>, insert(String, Vec<u8>), #[cfg(test)] remove(&str) }`.

- [x] **Step 1: Écrire les tests qui échouent**

```rust
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
```

- [x] **Step 2: Vérifier qu'ils échouent**

Run: `direnv exec . cargo test -p onyx-core session_cache`
Expected: erreur de compilation « cannot find struct `BoundedBytes` ».

- [x] **Step 3: Implémenter**

Dans `crates/core/src/session_cache.rs` :

```rust
use std::collections::{HashMap, VecDeque};

/// Plafond mémoire des octets d'images (128 Mo décimaux). Chaque fetch est déjà
/// borné à 20 Mo ; sans plafond global, une session de navigation accumulait
/// les screenshots indéfiniment.
const BYTES_CACHE_CAP: usize = 128_000_000;

/// Cache d'octets borné : à l'insertion, évince les entrées les plus anciennes
/// (FIFO) jusqu'à repasser sous `cap`. FIFO suffit ici (pas de LRU) : les
/// re-visites proches réutilisent l'entrée avant qu'elle n'atteigne la tête.
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

static BYTES_CACHE: LazyLock<Mutex<BoundedBytes>> =
    LazyLock::new(|| Mutex::new(BoundedBytes::new(BYTES_CACHE_CAP)));
```

Adapter les trois points d'accès (`cached_fetch_bytes` : `cache.get(url)` / `cache.insert(url.to_string(), b.clone())` ; `cached_bytes_if_present` : `.get(url)` — les signatures publiques ne bougent pas), et le test existant `cached_fetch_bytes_lit_depuis_le_cache` (`insert(...)` / `remove(...)` sur la nouvelle struct).

- [x] **Step 4: Vérifier que tout passe**

Run: `direnv exec . cargo test -p onyx-core session_cache`
Expected: PASS (6 tests).

---

### Task 5: Recherche — section « Applications » = paquet *reconnu*, pas « a une icône »

Le split « Applications / Autres paquets » teste `icon_url.is_some()` : une app appariée AppStream mais sans icône (nom officiel + note présents) tombe à tort dans « Autres paquets ».

**Files:**
- Modify: `crates/core/src/search.rs` (nouveau type `SearchResult`, retour de `search_apps`)
- Modify: `crates/core/src/lib.rs:70` (ré-export)
- Modify: `crates/gui/src/util.rs` (champ `recognized` sur `AppItem`, constructeur `from_search`, suppression de `with_rating`)
- Modify: `crates/gui/src/app.rs:42,276-281` (`AppMsg::SearchDone`, mapping)
- Modify: `crates/gui/src/components/app_list.rs:283` (critère du split)
- Modify: `crates/gui/src/components/package_row.rs:172` (déstructuration avec `..`)
- Test: `crates/gui/src/util.rs`

**Interfaces:**
- Produces (core):
  ```rust
  #[derive(Debug, Clone)]
  pub struct SearchResult {
      pub package: Package,
      pub icon_url: Option<String>,
      pub rating: Option<Rating>,
      /// Une entrée AppStream (locale ou REST) a réellement été appariée.
      pub recognized: bool,
  }
  pub fn search_apps(query: &str) -> Vec<SearchResult>
  ```
- Produces (gui): `AppItem { package, icon_url, rating, recognized }`, `AppItem::new(package, icon_url)` (recognized = icône présente, pour les grilles accueil/catégories), `AppItem::from_search(r: SearchResult)`.

- [x] **Step 1: Écrire le test qui échoue** (dans le module `tests` de `crates/gui/src/util.rs`)

```rust
#[test]
fn app_item_from_search_preserves_recognized_without_icon() {
    // Une app reconnue par AppStream mais sans icône doit rester une
    // « Application » (le split de la recherche lit `recognized`).
    let r = onyx_core::SearchResult {
        package: onyx_core::Package::from_attr("gedit"),
        icon_url: None,
        rating: None,
        recognized: true,
    };
    let item = super::AppItem::from_search(r);
    assert!(item.recognized);
    assert!(item.icon_url.is_none());
}
```

- [x] **Step 2: Vérifier qu'il échoue**

Run: `direnv exec . cargo test -p onyx-gui util`
Expected: erreur de compilation (« no `SearchResult` in onyx_core », « no method from_search »).

- [x] **Step 3: Implémenter côté core**

Dans `crates/core/src/search.rs` :

```rust
/// Un résultat de `search_apps` : le paquet nix + l'habillage AppStream
/// éventuel. `recognized` distingue « apparié à une entrée AppStream » de
/// « a une icône » : une app reconnue sans icône reste une application.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub package: Package,
    pub icon_url: Option<String>,
    pub rating: Option<Rating>,
    pub recognized: bool,
}
```

`search_apps` : signature `pub fn search_apps(query: &str) -> Vec<SearchResult>` ; dans le mapping final, chemin base locale → `SearchResult { recognized: true, .. }`, chemin REST → `recognized: hit.is_some()`.

Dans `crates/core/src/lib.rs` : `pub use search::{search, search_apps, SearchResult};`.

- [x] **Step 4: Implémenter côté GUI**

`crates/gui/src/util.rs` — `AppItem` :

```rust
#[derive(Debug, Clone)]
pub struct AppItem {
    pub package: Package,
    pub icon_url: Option<String>,
    pub rating: Option<onyx_core::Rating>,
    /// Apparié à une entrée AppStream (critère de la section « Applications »).
    pub recognized: bool,
}

impl AppItem {
    pub fn new(package: Package, icon_url: Option<String>) -> Self {
        // Grilles accueil/catégories : tout vient de Flathub, l'icône vaut
        // reconnaissance (le split de la recherche n'y passe pas).
        let recognized = icon_url.is_some();
        Self { package, icon_url, rating: None, recognized }
    }

    pub fn from_search(r: onyx_core::SearchResult) -> Self {
        Self {
            package: r.package,
            icon_url: r.icon_url,
            rating: r.rating,
            recognized: r.recognized,
        }
    }
}
```

`crates/gui/src/app.rs` : `SearchDone(Vec<onyx_core::SearchResult>)` ; mapping `items.into_iter().map(AppItem::from_search).collect()`.

`crates/gui/src/components/app_list.rs`, `apply()` : `if it.recognized { apps.push_back(...) } else { others.push_back(...) }`.

`crates/gui/src/components/package_row.rs`, `init_model` : `let AppItem { package, icon_url, rating, .. } = item;`.

- [x] **Step 5: Vérifier que tout passe**

Run: `direnv exec . cargo test -p onyx-core search && direnv exec . cargo test -p onyx-gui`
Expected: PASS.

---

### Task 6: Base AppStream — noms ambigus neutralisés (symétrie avec les suffixes)

`build_db` retire les **suffixes** d'app-id partagés (faux positif pire qu'une absence) mais garde silencieusement la **première** app en cas de collision de **nom** (`by_name.entry(...).or_insert(i)`). Deux apps nommées « Player » → la première gagne au hasard.

**Files:**
- Modify: `crates/core/src/appstream_db.rs:245-298` (`build_db`)
- Test: `crates/core/src/appstream_db.rs` (module `tests`)

**Interfaces:** aucune signature ne change.

- [x] **Step 1: Écrire le test qui échoue**

```rust
#[test]
fn lookup_ambiguous_name_matches_neither() {
    // Deux apps distinctes portant le MÊME nom : matcher la première apparue
    // serait un faux positif (même politique que les suffixes ambigus).
    let xml = r#"<?xml version="1.0"?>
<components>
  <component type="desktop-application">
    <id>org.example.AAA</id>
    <name>Player</name>
  </component>
  <component type="desktop-application">
    <id>org.example.BBB</id>
    <name>Player</name>
  </component>
</components>"#;
    let db = build_db(parse(std::io::Cursor::new(xml.as_bytes())));
    assert!(db.lookup("player").is_none(), "nom partagé par 2 apps : ne matcher ni l'une ni l'autre");
    // Les app_id complets restent résolvables.
    assert!(db.lookup("org.example.AAA").is_some());
    assert!(db.lookup("org.example.BBB").is_some());
}
```

- [x] **Step 2: Vérifier qu'il échoue**

Run: `direnv exec . cargo test -p onyx-core appstream_db::tests::lookup_ambiguous_name`
Expected: FAIL (la première « Player » est retournée).

- [x] **Step 3: Implémenter**

Dans `build_db`, remplacer `by_name.entry(alnum(&e.name)).or_insert(i);` par la même mécanique que les suffixes :

```rust
// Même politique que les suffixes : un NOM partagé par deux apps est ambigu →
// retiré de l'index (faux positif pire qu'une absence de correspondance).
let mut ambiguous_names: std::collections::HashSet<String> = std::collections::HashSet::new();
```

et dans la boucle :

```rust
let name_slug = alnum(&e.name);
if !name_slug.is_empty() {
    match by_name.entry(name_slug) {
        std::collections::hash_map::Entry::Occupied(o) => {
            ambiguous_names.insert(o.key().clone());
        }
        std::collections::hash_map::Entry::Vacant(v) => {
            v.insert(i);
        }
    }
}
```

après la boucle :

```rust
for s in &ambiguous_names {
    by_name.remove(s);
}
```

- [x] **Step 4: Vérifier que tout passe**

Run: `direnv exec . cargo test -p onyx-core appstream_db`
Expected: PASS (tous les tests du module, dont les régressions gedit/ETR existantes).

---

### Task 7: Écritures atomiques (`kept.json`, `tried.json`, `store-packages.nix`)

`fs::write` direct : un crash mi-écriture laisse un JSON tronqué que `unwrap_or_default()` remplace silencieusement par un état vide au chargement suivant → perte du manifeste (la source de vérité d'Onyx) ou du journal.

**Files:**
- Modify: `crates/core/src/paths.rs` (nouveau helper `write_atomic`)
- Modify: `crates/core/src/manifest.rs:28-32` (`save_manifest`)
- Modify: `crates/core/src/tried.rs:44-48` (`save_tried`)
- Modify: `crates/core/src/config_gen.rs:35-41` (`sync_config`)
- Test: `crates/core/src/paths.rs`

**Interfaces:**
- Produces: `pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()>` (crée le dossier parent, écrit un `.tmp` voisin, `rename`).

- [x] **Step 1: Écrire le test qui échoue**

Dans `crates/core/src/paths.rs`, ajouter un module tests :

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_content_and_leaves_no_tmp() {
        // Pas d'env muté → pas besoin d'env_lock.
        let dir = std::env::temp_dir().join(format!("onyx-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("kept.json");

        write_atomic(&path, b"un").unwrap();
        write_atomic(&path, b"deux").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"deux");

        let tmp_left = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains(".tmp"));
        assert!(!tmp_left, "aucun fichier temporaire ne doit rester");
    }
}
```

- [x] **Step 2: Vérifier qu'il échoue**

Run: `direnv exec . cargo test -p onyx-core paths`
Expected: erreur de compilation « cannot find function `write_atomic` ».

- [x] **Step 3: Implémenter**

Dans `crates/core/src/paths.rs` :

```rust
/// Écriture atomique : fichier temporaire voisin puis `rename` (atomique sur un
/// même système de fichiers). Un crash mi-écriture laissait sinon un JSON
/// tronqué, que `unwrap_or_default()` remplaçait silencieusement par un état
/// vide au chargement suivant — perte du manifeste/journal.
pub(crate) fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("onyx"),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
```

Brancher les trois écrivains :
- `manifest::save_manifest` : `crate::paths::write_atomic(&manifest_path(), &json)` (le `create_dir_all` du helper remplace celui du call-site).
- `tried::save_tried` : idem avec `tried_path()`.
- `config_gen::sync_config` : `crate::paths::write_atomic(&path, generate_store_packages(kept).as_bytes())` (retirer le `create_dir_all` manuel).

- [x] **Step 4: Vérifier que tout passe**

Run: `direnv exec . cargo test -p onyx-core`
Expected: PASS (dont `keep_then_unkeep_updates_store_file` et `sync_config_writes_file` existants, qui exercent les trois écrivains).

---

### Task 8: Onglet Installées — parallélisme borné

`InstalledMsg::Reload` lance **un thread par paquet** (gardés + essayés) : un historique fourni = des centaines de threads, chacun pouvant faire réseau + `nix eval`.

**Files:**
- Modify: `crates/gui/src/views/installed.rs:201-233` (extraction `fetch_items`, chunks de 16)

**Interfaces:**
- Produces: `fn fetch_items(pkgs: Vec<Package>, target_w: u32) -> Vec<AppItem>` (appelée depuis le thread de Reload, jamais du thread GTK).

- [x] **Step 1: Implémenter** (orchestration de threads appelant du I/O core — pas de test unitaire pratique ; la sémantique `cached_app_meta` est déjà testée côté core)

```rust
/// Résout les `AppMeta` d'un lot de paquets avec un parallélisme borné (vagues
/// de `MAX_PAR` threads) : un historique fourni ne doit pas lancer des
/// centaines de threads, chacun pouvant faire du réseau + `nix eval`.
fn fetch_items(pkgs: Vec<Package>, target_w: u32) -> Vec<AppItem> {
    const MAX_PAR: usize = 16;
    let mut out = Vec::with_capacity(pkgs.len());
    for chunk in pkgs.chunks(MAX_PAR) {
        let handles: Vec<_> = chunk
            .iter()
            .cloned()
            .map(|pkg| {
                std::thread::spawn(move || {
                    let meta = onyx_core::cached_app_meta(&pkg, target_w);
                    AppItem::new(pkg, meta.icon_url)
                })
            })
            .collect();
        out.extend(handles.into_iter().filter_map(|h| h.join().ok()));
    }
    out
}
```

Le thread de `Reload` devient :

```rust
let s = sender.input_sender().clone();
std::thread::spawn(move || {
    let kept_items = fetch_items(kept_pkgs, target_w);
    let tried_items = fetch_items(tried_pkgs, target_w);
    let _ = s.send(InstalledMsg::DataReady(kept_items, tried_items));
});
```

- [x] **Step 2: Vérifier la compilation et la non-régression**

Run: `direnv exec . cargo test -p onyx-gui`
Expected: PASS.

---

### Task 9: Vérification finale du workspace

- [x] **Step 1: Suite complète**

Run: `direnv exec . cargo test --workspace`
Expected: PASS partout (les tests GTK `#[ignore]` restent ignorés).

- [x] **Step 2: Warnings**

Run: `direnv exec . cargo build --workspace 2>&1 | grep -i warning`
Expected: aucun warning nouveau (imports morts retirés en Task 2).
