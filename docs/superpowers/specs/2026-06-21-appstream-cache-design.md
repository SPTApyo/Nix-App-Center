# Design — Cache AppStream + icônes installées

**Date :** 2026-06-21  
**Scope :** `crates/core` + `crates/gui`  
**Statut :** approuvé

---

## Problèmes résolus

| # | Symptôme | Cause |
|---|----------|-------|
| 1 | Onglet Installées : icônes génériques | `AppItem::new(p, None)` — aucun fetch Flathub |
| 2 | Double fetch icône tuile → détail | `app_meta()` refait search + fiche AppStream à chaque ouverture |
| 3 | Race condition détail | `app_meta()` lent (2-4s) ; si cache froid, page vide pendant ce temps |
| 4 | Données AppStream non persistantes entre vues | Aucun cache session |

---

## Architecture

### A. Core — deux caches statiques

**Fichier :** `crates/core/src/lib.rs`

```rust
static APP_META_CACHE: Mutex<HashMap<String, AppMeta>> = Mutex::new(HashMap::new());
static BYTES_CACHE:    Mutex<HashMap<String, Vec<u8>>> = Mutex::new(HashMap::new());
```

Deux fonctions publiques exposées :

```rust
pub fn cached_app_meta(pkg: &Package, screenshot_target_w: u32) -> AppMeta
pub fn cached_fetch_bytes(url: &str) -> Option<Vec<u8>>
```

- Clé de `APP_META_CACHE` : `pkg.attr` (stable, unique dans nixpkgs)
- Clé de `BYTES_CACHE` : URL de l'icône (même URL → mêmes bytes)
- Pas de TTL : les métadonnées sont fixes pour la durée du processus
- `AppMeta` est `Clone` → copie légère (~quelques Ko)
- `Vec<u8>` est `Clone` → copie ~20-50 Ko par icône, acceptable

### B. GUI — `screen_metrics()` déplacé dans `util.rs`

**Fichier :** `crates/gui/src/util.rs`

Actuellement `fn screen_metrics() -> (i32, i32)` est privée dans `detail.rs`. Déplacée en `pub(crate) fn screen_metrics()` dans `util.rs` pour être partagée par `detail.rs` et `installed.rs`.

### C. GUI — `InstalledModel` : skeleton + fetch parallèle

**Fichier :** `crates/gui/src/views/installed.rs`

**Nouveaux éléments du modèle :**
```rust
stack: gtk::Stack,  // pages : "loading" | "content"
```

**Nouveau variant :**
```rust
InstalledMsg::DataReady(Vec<AppItem>, Vec<AppItem>)  // (kept, tried)
```

**Flux `Reload` :**
1. `stack → "loading"` (skeleton via `util::skeleton_grid(6)` × 2)
2. Lecture synchrone de `load_manifest()` et `load_tried()` (~immédiat, local)
3. `screen_metrics()` appelé sur le thread GTK principal → `target_w`
4. Un thread "collecteur" spawne N threads parallèles (un par paquet) :
   ```rust
   let handles: Vec<_> = pkgs.into_iter().map(|pkg| {
       std::thread::spawn(move || {
           let meta = onyx_core::cached_app_meta(&pkg, target_w);
           AppItem::new(pkg, meta.icon_url)
       })
   }).collect();
   let items = handles.into_iter()
       .filter_map(|h| h.join().ok())
       .collect::<Vec<_>>();
   sender.send(InstalledMsg::DataReady(kept, tried));
   ```
5. Résultat : temps total ≈ max(durées individuelles), pas leur somme.  
   Première visite : ~2-4s (Flathub). Visites suivantes : quasi-instantané (cache).

**Flux `DataReady(kept_items, tried_items)` :**
1. `guard().clear() + push_back(item)` pour `kept_tiles` et `tried_tiles`
2. Gestion labels "empty" + visibilité FlowBox
3. `stack → "content"`

Chaque `AppTile::init_model` reçoit un `AppItem.icon_url = Some(url)` → spawne son thread bytes via `cached_fetch_bytes()`. Si le bytes cache est chaud (même icône vue en recherche), retour quasi-instantané.

### D. GUI — `DetailModel` : utilise les variantes cachées

**Fichier :** `crates/gui/src/views/detail.rs`

Deux substitutions dans `init()` et `update()` :

| Avant | Après |
|-------|-------|
| `onyx_core::app_meta(&pkg, target_w)` | `onyx_core::cached_app_meta(&pkg, target_w)` |
| `onyx_core::fetch_bytes(&url)` | `onyx_core::cached_fetch_bytes(&url)` |

**Effet sur la race condition :** si l'utilisateur a déjà vu ce paquet (recherche ou onglet installées), `cached_app_meta()` retourne en microsecondes → `DetailMsg::Loaded` arrive quasi-instantanément → plus de page vide. Cache froid → comportement actuel (2-4s skeleton, puis données).

### E. GUI — `AppTile` : bytes cachés

**Fichier :** `crates/gui/src/components/app_tile.rs`

```rust
// avant
let _ = s.send(TileMsg::IconLoaded(onyx_core::fetch_bytes(&url)));
// après
let _ = s.send(TileMsg::IconLoaded(onyx_core::cached_fetch_bytes(&url)));
```

Une ligne. Les bytes téléchargés pour la tuile sont ensuite disponibles gratis pour la page détail.

---

## Flux bout en bout après les changements

```
Recherche « firefox »
  → search_apps() → pkgs + icon_urls
  → AppTile: cached_fetch_bytes(icon_url) → [bytes en cache]
  → APP_META_CACHE: vide pour firefox

Clic sur Firefox → DetailModel
  → cached_app_meta("firefox", target_w) → [pas en cache] → fetch 2-4s
  → DetailMsg::Loaded → cached_fetch_bytes(icon_url) → [bytes déjà en cache] → icône INSTANTANÉE
  → APP_META_CACHE["firefox"] = meta (pour toujours dans la session)

Retour → Installées (si firefox est kept)
  → InstalledMsg::Reload → skeleton
  → cached_app_meta("firefox") → [en cache] → INSTANTANÉ
  → AppTile: cached_fetch_bytes(icon_url) → [en cache] → INSTANTANÉ

Réouverture de Firefox → DetailModel
  → cached_app_meta("firefox") → [en cache] → INSTANTANÉ
  → DetailMsg::Loaded arrive en <1ms → plus de race condition
```

---

## Ce qui ne change pas

- `Package`, `AppMeta`, `AppItem` — aucune modification de structure
- `AppList` (résultats recherche) — déjà correct (icon_urls fournis par `search_apps`)
- `CategoryModel`, `HomeModel` — non concernés
- Aucun changement de signature publique dans `core`
- `AppTile::update()`, `AppTile::view!` — inchangés

---

## Risques et mitigations

| Risque | Mitigation |
|--------|-----------|
| Cache Mutex contention (N threads parallèles) | Lock pris brièvement (lecture/écriture HashMap), pas de deadlock possible |
| Cache non borné | Session courte (~quelques dizaines de packages max) ; acceptable |
| `AppMeta.screenshot_target_w` fixé à la résolution du premier appelant | Le premier appelant est toujours `DetailModel` (ou `InstalledModel` avec `screen_metrics()`) — résolution réelle utilisée dans les deux cas |
| `filter_map(h.join().ok())` silencieux si thread panique | Thread de fetch ne fait que des I/O réseaux ; panic impossible en pratique |
