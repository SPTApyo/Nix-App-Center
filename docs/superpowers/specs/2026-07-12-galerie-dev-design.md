# Galerie de développement — Design

**Date :** 2026-07-12
**Statut :** validé, prêt pour plan d'implémentation

## Problème

La page détail (et la recherche) affichent plusieurs *types* d'apps aux comportements différents : certaines tuiles sont cliquables (ouvrent une fiche), d'autres non ; certaines ont une icône, d'autres un repli ; certaines sections n'apparaissent que sous condition (broken, insecure, sans métadonnées, extensions, apps du développeur…). Rien ne permet de voir ces types **côte à côte** pendant le développement, ce qui provoque des erreurs d'inattention (ex. croire qu'une grille d'extensions non navigable devrait s'ouvrir).

## Objectif

Une **galerie de développement** : un onglet supplémentaire qui liste des scénarios réels étiquetés, chacun ouvrant la vraie vue (recherche pré-remplie ou fiche détail) pour observer le comportement fidèle de chaque type. Elle doit être **totalement absente du binaire distribué aux utilisateurs**.

## Décisions (validées)

1. **Gating : feature Cargo `dev-gallery`**, désactivée par défaut. Tout le code galerie est gardé par `#[cfg(feature = "dev-gallery")]` → non compilé dans un build release. Portée **uniquement** par le crate `onyx-gui` (n'affecte ni `onyx-core` ni `onyx-cli`).
2. **Données : vraies apps figées.** La galerie appelle les vraies fonctions core (`search_apps`, `cached_app_meta`, `popular_packages`…) sur une liste d'attrs/requêtes choisie. Aucune fixture synthétique. Comportement 100 % fidèle ; en contrepartie, dépend de l'état de nixpkgs/Flathub (voir Risques).
3. **Accès : onglet « Galerie » supplémentaire** dans le `adw::ViewStack`, à côté d'Explorer/Installées/Mises à jour, visible seulement avec la feature. Il liste des scénarios étiquetés ; un clic ouvre la vue réelle.

## Architecture

### Feature Cargo

`crates/gui/Cargo.toml` gagne :

```toml
[features]
dev-gallery = []
```

Commande de dev : `cargo run -p onyx-gui --features dev-gallery`.
App normale / release : `cargo run -p onyx-gui` / `cargo build --release` → galerie inexistante.

### Nouveau module

Un seul fichier : `crates/gui/src/views/gallery.rs`, déclaré dans `crates/gui/src/views/mod.rs` :

```rust
#[cfg(feature = "dev-gallery")]
pub mod gallery;
#[cfg(feature = "dev-gallery")]
pub use gallery::GalleryModel;
```

`GalleryModel` est un `SimpleComponent` relm4. Il **ne duplique aucune vue** : il réutilise les vrais composants (`AppList`, `DetailModel`, `HomeModel`) et les vraies fonctions core.

### Câblage dans `app.rs` (100 % gardé)

Trois points d'insertion, chacun `#[cfg(feature = "dev-gallery")]` :

1. Construction du `GalleryModel` à l'`init` (comme les autres contrôleurs).
2. Ajout de l'onglet : `view_stack.add_titled(gallery.widget(), Some("gallery"), "Galerie")` + icône dev (`applications-engineering-symbolic`).
3. Réutilisation du flux d'ouverture détail existant : la galerie émet un `Package` (scénario détail) → routé vers `AppMsg::OpenDetail` (déjà en place). Pour la recherche pré-remplie, la galerie émet une `String` → routée vers `AppMsg::Search` (déjà en place).

Quand la feature est off, ces lignes n'existent pas : `app.rs` est bit-pour-bit celui d'aujourd'hui. Le champ `gallery: Controller<GalleryModel>` dans `AppModel` est lui aussi `#[cfg(feature = "dev-gallery")]`.

### Sorties de `GalleryModel`

```rust
#[cfg(feature = "dev-gallery")]
pub enum GalleryOutput {
    /// Lance une recherche réelle pré-remplie (→ AppMsg::Search).
    Search(String),
    /// Ouvre la vraie fiche détail d'un attr (→ AppMsg::OpenDetail).
    OpenDetail(Package),
}
```

L'accueil est montré via une **instance réelle de `HomeModel` intégrée directement** dans un scénario de la galerie (pas de bascule vers l'onglet Explorer) : la galerie reste auto-contenue, sans coupler les onglets entre eux. Ses clics de tuile sont routés comme les autres via `GalleryOutput::OpenDetail`.

## Catalogue de scénarios

La galerie affiche un index (boxed-list) groupé en trois sections. Chaque ligne porte un **titre** (le scénario), un **sous-titre = comportement attendu à vérifier**, et une action (bouton/activation).

### Recherche (émet `GalleryOutput::Search`)

| Scénario | Requête | Comportement attendu |
|---|---|---|
| App avec icône + note | `firefox` | Section « Applications », icône, étoiles, cliquable |
| Mix reconnu / non-reconnu | `editor` | « Applications » (icônes) et « Autres paquets » |
| Filtre jeu de paquets | `aeson` | Menu déroulant `haskellPackages` apparaît |
| Fournit une commande | `rg` | Section « Fournit la commande rg » (nix-locate) |
| Zéro résultat | `zzqxnonexistent` | Page « Aucun résultat » |

### Détail (émet `GalleryOutput::OpenDetail`)

| Scénario | Attr | Comportement attendu |
|---|---|---|
| Fiche complète | `steam` | Screenshots ; **Extensions = grille NON cliquable** (normal) ; **Autres applications = grille CLIQUABLE** ; tuile Âge ; licence propriétaire |
| Libre + avis | `firefox` | Tuile « Logiciel libre » ; histogramme d'avis ; historique versions |
| Nix pur runnable | `ripgrep` | Peu de métadonnées ; bouton Lancer ; pas de screenshots |
| Keep-only / sans méta | `zlib` | Pas de `mainProgram` ; carte « Aucune métadonnée » |
| Vulnérable (insecure) | `python27`* | Tuile rouge « Vulnérable » + bandeau problèmes |

\* `python27` est vérifié `meta.insecure = true` sur la machine de dev. Il allume la **même** tuile rouge + bandeau problèmes que le cas `broken` (chemin de rendu quasi identique, seul le libellé change) — d'où un seul scénario « problème » plutôt qu'un scénario `broken` séparé. Aucun attr `broken` n'est stable dans nixpkgs (retiré dès qu'il l'est), donc pas de scénario `broken` figé. Attr best-effort à rafraîchir si `python27` cesse d'être insecure (voir Risques).

### Accueil

Réutilise la vraie `HomeModel` (carousel + tuiles populaires/récentes + cartes catégories), avec une note listant les états d'icône attendus : chargement (shimmer) → repli coloré → icône réelle.

## Composants et frontières

- `gallery.rs` : **seul** fichier nouveau. Dépend de `AppList`/`DetailModel`/`HomeModel` (widgets) et des types core (`Package`). Ne connaît aucune logique métier ; il ne fait que composer une liste de scénarios et router des sorties.
- `app.rs` : gagne des insertions gardées, aucune logique nouvelle (réutilise `AppMsg::Search`/`AppMsg::OpenDetail`).
- `views/mod.rs`, `Cargo.toml` : déclarations gardées.
- **Aucune modification** de `onyx-core`, `onyx-cli`, ni des composants existants.

## Tests

- **Compilation des deux profils** : `cargo build -p onyx-gui` (sans feature) **et** `cargo build -p onyx-gui --features dev-gallery` doivent réussir sans warning. C'est le garde-fou principal : la feature ne doit jamais casser le build par défaut.
- **Absence en release** : vérifier que `gallery` n'apparaît pas dans le binaire par défaut. Test pragmatique : `cargo build -p onyx-gui` puis `grep -c gallery` sur les symboles (ou simplement : le module n'est pas déclaré donc ne compile pas — garanti par `#[cfg]`).
- Pas de test unitaire sur le contenu de la galerie (c'est un harnais visuel de dev, pas de la logique) ; le catalogue de scénarios est de la donnée constante.

## Risques

1. **Attrs broken/insecure instables** : dépendent de l'état de nixpkgs ; peuvent cesser de l'être. Mitigation : commentaire dans le code invitant à les rafraîchir ; ce sont les deux seuls scénarios fragiles.
2. **Lenteur/réseau** : la galerie hérite des vraies latences (nix eval, Flathub). Acceptable en dev ; c'est même représentatif.
3. **Dérive feature-off** : un refactor pourrait casser le build `--features dev-gallery` sans qu'on s'en aperçoive. Mitigation : documenter la commande de build à deux profils dans le README/CI si présent.

## Hors périmètre (YAGNI)

- Pas de fixtures synthétiques.
- Pas de sélection dynamique d'attrs par l'utilisateur (liste figée).
- Pas de galerie pour les onglets Installées/Mises à jour (pas de type ambigu à y désambiguïser).
- Pas de second binaire ni de seconde fenêtre.
