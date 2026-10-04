# AppStream Session Cache — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ajouter deux caches de session (AppMeta + bytes icônes) pour que les données Flathub survivent entre les vues, et brancher l'onglet Installées sur ce cache pour afficher les vraies icônes.

**Architecture:** Deux `static Mutex<HashMap>` dans `crates/core` exposés via `cached_app_meta()` et `cached_fetch_bytes()`. Les call-sites GUI remplacent leurs appels directs par les variantes cachées. `InstalledModel` passe en mode skeleton → fetch parallèle → `DataReady` au lieu de créer les tuiles sans icônes.

**Tech Stack:** Rust stable, GTK4 + libadwaita, relm4 0.9.1, ureq (HTTP), `std::sync::Mutex` (const init depuis Rust 1.63).

## Global Constraints

- Cargo workspace : `crates/core`, `crates/gui` — toujours builder via `nix develop --command cargo …`
- Versions verrouillées : gtk4 0.9.7, libadwaita 0.7.2, relm4 0.9.1
- Aucun TTL sur les caches — session unique, données stables pendant le processus
- Pas de dépendance supplémentaire (std seulement pour le cache)
- Commits fréquents : un par tâche

---

## Carte des fichiers

| Fichier | Rôle des changements |
|---------|----------------------|
| `crates/core/src/lib.rs` | +2 statics, +2 fonctions publiques `cached_*`, +1 import `Mutex` |
| `crates/gui/src/util.rs` | +`pub(crate) fn screen_metrics()` déplacée depuis detail.rs |
| `crates/gui/src/views/detail.rs` | Supprime `fn screen_metrics()`, 3 substitutions d'appels |
| `crates/gui/src/components/app_tile.rs` | 1 substitution `fetch_bytes` → `cached_fetch_bytes` |
| `crates/gui/src/views/installed.rs` | +`stack`, +`DataReady`, `Reload` → skeleton + threads parallèles |

---

### Task 1 : Core — `cached_app_meta` + `cached_fetch_bytes`

**Files:**
- Modify: `crates/core/src/lib.rs`

**Interfaces:**
- Produit: `pub fn cached_app_meta(pkg: &Package, screenshot_target_w: u32) -> AppMeta`
- Produit: `pub fn cached_fetch_bytes(url: &str) -> Option<Vec<u8>>`
- Produit (internes): `static APP_META_CACHE: Mutex<HashMap<String, AppMeta>>`
- Produit (internes): `static BYTES_CACHE: Mutex<HashMap<String, Vec<u8>>>`

- [ ] **Étape 1 — Écrire les tests (ils échoueront : fonctions inexistantes)**

Dans `crates/core/src/lib.rs`, après le bloc `mod keep_tests` existant, ajouter :

```rust
#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn cached_fetch_bytes_lit_depuis_le_cache() {
        // Pré-alimente le cache avec des bytes connus.
        BYTES_CACHE
            .lock()
            .unwrap()
            .insert("http://sentinel-url-test".to_string(), vec![7u8, 8, 9]);
        let result = cached_fetch_bytes("http://sentinel-url-test");
        assert_eq!(result, Some(vec![7u8, 8, 9]));
        // Nettoyage (les statics persistent entre tests dans le même processus).
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
        APP_META_CACHE
            .lock()
            .unwrap()
            .remove(&pkg.attr);
    }
}
```

- [ ] **Étape 2 — Vérifier que les tests échouent**

```bash
nix develop --command cargo test -p onyx-core cache_tests 2>&1 | tail -20
```

Attendu : erreur de compilation (`BYTES_CACHE`, `APP_META_CACHE`, `cached_fetch_bytes`, `cached_app_meta` introuvables).

- [ ] **Étape 3 — Ajouter `Mutex` aux imports existants**

Ligne 14 de `crates/core/src/lib.rs` — remplacer :
```rust
use std::sync::OnceLock;
```
par :
```rust
use std::sync::{Mutex, OnceLock};
```

- [ ] **Étape 4 — Déclarer les deux statics**

Juste après la ligne `static ODRS: OnceLock<HashMap<String, Rating>> = OnceLock::new();` (ligne ~345), insérer :

```rust
static APP_META_CACHE: Mutex<HashMap<String, AppMeta>> = Mutex::new(HashMap::new());
static BYTES_CACHE:    Mutex<HashMap<String, Vec<u8>>> = Mutex::new(HashMap::new());
```

- [ ] **Étape 5 — Implémenter `cached_app_meta` et `cached_fetch_bytes`**

Juste après la fonction `fetch_bytes` (ligne ~1104), insérer :

```rust
/// Comme `app_meta()` mais mémoïsé par `pkg.attr` pour toute la session.
pub fn cached_app_meta(pkg: &Package, screenshot_target_w: u32) -> AppMeta {
    if let Ok(cache) = APP_META_CACHE.lock() {
        if let Some(m) = cache.get(&pkg.attr) {
            return m.clone();
        }
    }
    let m = app_meta(pkg, screenshot_target_w);
    if let Ok(mut cache) = APP_META_CACHE.lock() {
        cache.insert(pkg.attr.clone(), m.clone());
    }
    m
}

/// Comme `fetch_bytes()` mais mémoïsé par URL pour toute la session.
pub fn cached_fetch_bytes(url: &str) -> Option<Vec<u8>> {
    if let Ok(cache) = BYTES_CACHE.lock() {
        if let Some(b) = cache.get(url) {
            return Some(b.clone());
        }
    }
    let b = fetch_bytes(url)?;
    if let Ok(mut cache) = BYTES_CACHE.lock() {
        cache.insert(url.to_string(), b.clone());
    }
    Some(b)
}
```

Note : on utilise `if let Ok` au lieu de `unwrap()` — un Mutex empoisonné (panic dans un autre thread) ne doit pas faire crasher le processus ; dans ce cas on retombe sur le chemin non-caché.

- [ ] **Étape 6 — Tests passent**

```bash
nix develop --command cargo test -p onyx-core cache_tests 2>&1
```

Attendu :
```
test cache_tests::cached_fetch_bytes_lit_depuis_le_cache ... ok
test cache_tests::cached_app_meta_lit_depuis_le_cache ... ok
test result: ok. 2 passed; 0 failed
```

- [ ] **Étape 7 — Build propre**

```bash
nix develop --command cargo build -p onyx-core 2>&1 | tail -5
```

Attendu : `Finished 'dev' profile`.

- [ ] **Étape 8 — Commit**

```bash
git add crates/core/src/lib.rs
git commit -m "feat(core): cached_app_meta + cached_fetch_bytes (session cache)"
```

---

### Task 2 : GUI util — `screen_metrics` partagé

**Files:**
- Modify: `crates/gui/src/util.rs`
- Modify: `crates/gui/src/views/detail.rs`

**Interfaces:**
- Consomme: rien de Task 1
- Produit: `pub(crate) fn screen_metrics() -> (i32, i32)` dans `util.rs`
  - Retourne `(largeur_physique_px, facteur_echelle)`. Repli `(1920, 1)` si pas de moniteur.

- [ ] **Étape 1 — Ajouter `screen_metrics` dans `util.rs`**

À la fin de `crates/gui/src/util.rs`, ajouter :

```rust
/// Métriques de l'écran principal : `(largeur_physique_px, facteur_d_échelle)`.
/// Largeur physique = largeur logique × échelle. Repli 1920×1 si pas d'écran.
pub(crate) fn screen_metrics() -> (i32, i32) {
    if let Some(display) = gtk::gdk::Display::default() {
        if let Some(obj) = display.monitors().item(0) {
            if let Ok(m) = obj.downcast::<gtk::gdk::Monitor>() {
                let scale = m.scale_factor().max(1);
                let w = m.geometry().width().max(1);
                return (w * scale, scale);
            }
        }
    }
    (1920, 1)
}
```

- [ ] **Étape 2 — Supprimer `screen_metrics` de `detail.rs` et corriger l'appel**

Dans `crates/gui/src/views/detail.rs`, localiser la fonction privée `fn screen_metrics() -> (i32, i32)` (vers la ligne 768) et la supprimer entièrement.

L'appel dans `init()` (environ ligne 553) :
```rust
let (target_dev_w, scale) = screen_metrics();
```
devient :
```rust
let (target_dev_w, scale) = crate::util::screen_metrics();
```

- [ ] **Étape 3 — Build propre**

```bash
nix develop --command cargo build -p onyx-gui 2>&1 | tail -5
```

Attendu : `Finished 'dev' profile` sans warnings liés à `screen_metrics`.

- [ ] **Étape 4 — Commit**

```bash
git add crates/gui/src/util.rs crates/gui/src/views/detail.rs
git commit -m "refactor(gui): screen_metrics dans util.rs (partagé)"
```

---

### Task 3 : `AppTile` — bytes cachés

**Files:**
- Modify: `crates/gui/src/components/app_tile.rs`

**Interfaces:**
- Consomme: `onyx_core::cached_fetch_bytes(url: &str) -> Option<Vec<u8>>` (Task 1)

- [ ] **Étape 1 — Substitution**

Dans `crates/gui/src/components/app_tile.rs`, méthode `init_model`, localiser :
```rust
let _ = s.send(TileMsg::IconLoaded(onyx_core::fetch_bytes(&url)));
```
Remplacer par :
```rust
let _ = s.send(TileMsg::IconLoaded(onyx_core::cached_fetch_bytes(&url)));
```

- [ ] **Étape 2 — Build propre**

```bash
nix develop --command cargo build -p onyx-gui 2>&1 | tail -5
```

Attendu : `Finished 'dev' profile`.

- [ ] **Étape 3 — Commit**

```bash
git add crates/gui/src/components/app_tile.rs
git commit -m "feat(gui): AppTile utilise cached_fetch_bytes (bytes icône mis en cache)"
```

---

### Task 4 : `DetailModel` — appels cachés

**Files:**
- Modify: `crates/gui/src/views/detail.rs`

**Interfaces:**
- Consomme: `onyx_core::cached_app_meta` (Task 1), `onyx_core::cached_fetch_bytes` (Task 1)

3 substitutions dans `crates/gui/src/views/detail.rs` :

- [ ] **Étape 1 — `app_meta` → `cached_app_meta` dans `init()`**

Localiser (dans `init()`, environ ligne 592) :
```rust
let _ = s.send(DetailMsg::Loaded(Box::new(onyx_core::app_meta(&pkg_meta, target_w))));
```
Remplacer par :
```rust
let _ = s.send(DetailMsg::Loaded(Box::new(onyx_core::cached_app_meta(&pkg_meta, target_w))));
```

- [ ] **Étape 2 — `fetch_bytes` → `cached_fetch_bytes` pour l'icône dans `update()`**

Localiser dans le handler `DetailMsg::Loaded` (environ ligne 617) :
```rust
if let Some(b) = onyx_core::fetch_bytes(&url) {
    let _ = s.send(DetailMsg::IconLoaded(b));
}
```
Remplacer par :
```rust
if let Some(b) = onyx_core::cached_fetch_bytes(&url) {
    let _ = s.send(DetailMsg::IconLoaded(b));
}
```

- [ ] **Étape 3 — `fetch_bytes` → `cached_fetch_bytes` pour les screenshots dans `update()`**

Localiser dans la boucle screenshots (environ ligne 625) :
```rust
if let Some(b) = onyx_core::fetch_bytes(&url) {
    let _ = s.send(DetailMsg::ScreenshotLoaded(b));
}
```
Remplacer par :
```rust
if let Some(b) = onyx_core::cached_fetch_bytes(&url) {
    let _ = s.send(DetailMsg::ScreenshotLoaded(b));
}
```

- [ ] **Étape 4 — Build propre**

```bash
nix develop --command cargo build -p onyx-gui 2>&1 | tail -5
```

Attendu : `Finished 'dev' profile`.

- [ ] **Étape 5 — Commit**

```bash
git add crates/gui/src/views/detail.rs
git commit -m "feat(gui): DetailModel utilise cached_app_meta + cached_fetch_bytes"
```

---

### Task 5 : `InstalledModel` — skeleton + fetch parallèle AppStream

**Files:**
- Modify: `crates/gui/src/views/installed.rs`

**Interfaces:**
- Consomme: `onyx_core::cached_app_meta` (Task 1), `crate::util::screen_metrics` (Task 2)
- Consomme: `crate::util::{AppItem, skeleton_grid}` (déjà importé pour AppItem, skeleton_grid à ajouter)

- [ ] **Étape 1 — Ajouter `skeleton_grid` aux imports**

Dans `crates/gui/src/views/installed.rs`, ligne des imports crate :
```rust
use crate::util::AppItem;
```
Remplacer par :
```rust
use crate::util::{skeleton_grid, AppItem};
```

- [ ] **Étape 2 — Ajouter `DataReady` à l'enum `InstalledMsg`**

```rust
#[derive(Debug)]
pub enum InstalledMsg {
    Reload,
    DataReady(Vec<AppItem>, Vec<AppItem>), // (kept, tried) avec icon_url rempli
    Open(Package),
    Gc,
    GcDone(String),
}
```

- [ ] **Étape 3 — Ajouter `stack` au struct `InstalledModel`**

```rust
pub struct InstalledModel {
    kept_tiles: FactoryVecDeque<AppTile>,
    tried_tiles: FactoryVecDeque<AppTile>,
    kept_empty: gtk::Label,
    tried_empty: gtk::Label,
    gc_btn: gtk::Button,
    gc_label: gtk::Label,
    gc_running: bool,
    stack: gtk::Stack,
}
```

- [ ] **Étape 4 — Modifier `init()` pour construire le stack skeleton/content**

Remplacer tout le corps de `init()` par la version suivante (les sections kept/tried/gc sont identiques, seule la fin change) :

```rust
fn init(_: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
    // ── Grilles ───────────────────────────────────────────────────────────
    let kept_tiles = FactoryVecDeque::builder()
        .launch(gtk::FlowBox::new())
        .forward(sender.input_sender(), InstalledMsg::Open);
    configure_flow(kept_tiles.widget());

    let tried_tiles = FactoryVecDeque::builder()
        .launch(gtk::FlowBox::new())
        .forward(sender.input_sender(), InstalledMsg::Open);
    configure_flow(tried_tiles.widget());

    // ── Section « Dans ma config » ────────────────────────────────────────
    let kept_empty = gtk::Label::new(Some(
        "Aucun paquet gardé — ouvrez une page détail et cliquez « Ajouter à ma config ».",
    ));
    kept_empty.add_css_class("dim-label");
    kept_empty.set_xalign(0.0);
    kept_empty.set_wrap(true);
    kept_empty.set_visible(false);

    let kept_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    kept_section.append(&section_heading("Dans ma config"));
    kept_section.append(kept_tiles.widget());
    kept_section.append(&kept_empty);

    // ── Section « Essayées » ──────────────────────────────────────────────
    let gc_btn = gtk::Button::with_label("Libérer l'espace");
    gc_btn.add_css_class("flat");
    gc_btn.set_valign(gtk::Align::Center);
    gc_btn.set_tooltip_text(Some(
        "nix-collect-garbage : supprime du store les paquets non gardés",
    ));
    gc_btn.connect_clicked({
        let s = sender.input_sender().clone();
        move |_| {
            let _ = s.send(InstalledMsg::Gc);
        }
    });

    let tried_header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    tried_header.append(&section_heading("Essayées"));
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    tried_header.append(&spacer);
    tried_header.append(&gc_btn);

    let tried_empty = gtk::Label::new(Some(
        "Aucun essai récent — lancez un paquet depuis la recherche pour qu'il apparaître ici.",
    ));
    tried_empty.add_css_class("dim-label");
    tried_empty.set_xalign(0.0);
    tried_empty.set_wrap(true);
    tried_empty.set_visible(false);

    let gc_label = gtk::Label::new(None);
    gc_label.add_css_class("dim-label");
    gc_label.set_xalign(0.0);
    gc_label.set_visible(false);

    let tried_section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    tried_section.append(&tried_header);
    tried_section.append(tried_tiles.widget());
    tried_section.append(&tried_empty);
    tried_section.append(&gc_label);

    // ── Page « content » ──────────────────────────────────────────────────
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .build();
    content.append(&kept_section);
    content.append(&tried_section);

    let content_clamp = adw::Clamp::new();
    content_clamp.set_maximum_size(1000);
    content_clamp.set_tightening_threshold(600);
    content_clamp.set_margin_start(12);
    content_clamp.set_margin_end(12);
    content_clamp.set_child(Some(&content));

    // ── Page « loading » (skeleton calqué sur le contenu) ────────────────
    let build_skel_section = |heading_px: i32| {
        let sec = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        let h = gtk::Box::builder()
            .width_request(heading_px)
            .height_request(22)
            .halign(gtk::Align::Start)
            .build();
        h.add_css_class("skeleton");
        sec.append(&h);
        sec.append(&skeleton_grid(3));
        sec
    };

    let loading_content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .build();
    loading_content.append(&build_skel_section(160)); // « Dans ma config »
    loading_content.append(&build_skel_section(120)); // « Essayées »

    let loading_clamp = adw::Clamp::new();
    loading_clamp.set_maximum_size(1000);
    loading_clamp.set_tightening_threshold(600);
    loading_clamp.set_margin_start(12);
    loading_clamp.set_margin_end(12);
    loading_clamp.set_child(Some(&loading_content));

    // ── Stack ─────────────────────────────────────────────────────────────
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_transition_duration(150);
    stack.add_named(&loading_clamp, Some("loading"));
    stack.add_named(&content_clamp, Some("content"));
    stack.set_visible_child_name("loading");

    let model = InstalledModel {
        kept_tiles,
        tried_tiles,
        kept_empty: kept_empty.clone(),
        tried_empty: tried_empty.clone(),
        gc_btn: gc_btn.clone(),
        gc_label: gc_label.clone(),
        gc_running: false,
        stack: stack.clone(),
    };
    let widgets = view_output!();
    root.set_child(Some(&stack));
    crate::util::smooth_scroll(&root);

    sender.input(InstalledMsg::Reload);
    ComponentParts { model, widgets }
}
```

- [ ] **Étape 5 — Réécrire le handler `Reload` + ajouter `DataReady`**

Dans `update()`, remplacer le bloc `InstalledMsg::Reload => { … }` existant par :

```rust
InstalledMsg::Reload => {
    self.stack.set_visible_child_name("loading");

    let kept_pkgs: Vec<onyx_core::Package> = onyx_core::load_manifest().kept;
    let tried_pkgs: Vec<onyx_core::Package> = onyx_core::load_tried()
        .tried
        .into_iter()
        .map(|e| e.package)
        .collect();

    let (target_dev_w, _) = crate::util::screen_metrics();
    let target_w = (target_dev_w.clamp(1280, 3840)) as u32;

    let s = sender.input_sender().clone();
    std::thread::spawn(move || {
        // Un thread par paquet — temps total = max(durées), pas leur somme.
        let kept_handles: Vec<_> = kept_pkgs
            .into_iter()
            .map(|pkg| {
                std::thread::spawn(move || {
                    let meta = onyx_core::cached_app_meta(&pkg, target_w);
                    AppItem::new(pkg, meta.icon_url)
                })
            })
            .collect();
        let tried_handles: Vec<_> = tried_pkgs
            .into_iter()
            .map(|pkg| {
                std::thread::spawn(move || {
                    let meta = onyx_core::cached_app_meta(&pkg, target_w);
                    AppItem::new(pkg, meta.icon_url)
                })
            })
            .collect();

        let kept_items: Vec<AppItem> = kept_handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .collect();
        let tried_items: Vec<AppItem> = tried_handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .collect();

        let _ = s.send(InstalledMsg::DataReady(kept_items, tried_items));
    });
}
InstalledMsg::DataReady(kept_items, tried_items) => {
    let kept_empty = kept_items.is_empty();
    {
        let mut g = self.kept_tiles.guard();
        g.clear();
        for item in kept_items {
            g.push_back(item);
        }
    }
    self.kept_tiles.widget().set_visible(!kept_empty);
    self.kept_empty.set_visible(kept_empty);

    let tried_empty = tried_items.is_empty();
    {
        let mut g = self.tried_tiles.guard();
        g.clear();
        for item in tried_items {
            g.push_back(item);
        }
    }
    self.tried_tiles.widget().set_visible(!tried_empty);
    self.tried_empty.set_visible(tried_empty);

    self.stack.set_visible_child_name("content");
}
```

- [ ] **Étape 6 — Build propre**

```bash
nix develop --command cargo build -p onyx-gui 2>&1 | tail -10
```

Attendu : `Finished 'dev' profile` sans warnings.

- [ ] **Étape 7 — Commit**

```bash
git add crates/gui/src/views/installed.rs
git commit -m "feat(gui): InstalledModel skeleton + fetch AppStream parallèle (cached)"
```

---

## Self-Review

**Couverture spec :**
- ✅ `APP_META_CACHE` + `BYTES_CACHE` statics (Task 1)
- ✅ `cached_app_meta()` + `cached_fetch_bytes()` (Task 1)
- ✅ `screen_metrics()` dans `util.rs` (Task 2)
- ✅ `AppTile` utilise `cached_fetch_bytes` (Task 3)
- ✅ `DetailModel` utilise `cached_app_meta` + `cached_fetch_bytes` ×3 (Task 4)
- ✅ `InstalledModel` skeleton + `DataReady` + fetch parallèle (Task 5)

**Aucun placeholder.** Chaque étape contient le code exact.

**Cohérence des types :**
- `cached_app_meta(&Package, u32) -> AppMeta` : `AppMeta` dérive `Clone` + `Default` ✅
- `cached_fetch_bytes(&str) -> Option<Vec<u8>>` ✅
- `AppItem::new(Package, Option<String>)` : `meta.icon_url` est `Option<String>` ✅
- `InstalledMsg::DataReady(Vec<AppItem>, Vec<AppItem>)` : `AppItem` dérive `Debug` ✅
- `screen_metrics() -> (i32, i32)` : consommé en `(target_dev_w, _)` ✅

**Ordre des tâches :** Task 1 doit précéder Tasks 3-5. Task 2 doit précéder Task 5. Tasks 3-4 sont indépendantes.
