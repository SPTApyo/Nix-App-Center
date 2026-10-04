# Page détail — copie 1:1 de `gs-details-page` (GNOME Software)

> Spec validée le 2026-07-09. Source de référence : clone `gnome copy/` à
> `894f8e54f` (gnome-software `>=1.8.alpha`).

## 1. Objectif

Rendre la page détail d'Onyx visuellement indiscernable de celle de GNOME
Software : même layout, mêmes espacements, mêmes couleurs, même typographie,
même structure de widgets, mêmes sections.

**Portée arrêtée : « visuel strict, sémantique Onyx ».** On copie le contenant,
pas l'identité produit. Les verbes restent ceux d'Onyx (« Lancer » = `nix run`
en action primaire, « Ajouter à ma config » en secondaire), et les tuiles de la
context bar gardent leur sens Nix (taille de fermeture, `broken`/`insecure`,
mainteneurs nixpkgs). Le principe *try-first* figé au CONTEXT.md §2 n'est pas
remis en cause.

Deux divergences assumées, décidées explicitement :

- **Icône de repli.** GNOME affiche un `GtkImage pixel_size 128` avec l'icône
  générique du thème. Onyx garde sa `.fallback-tile` colorée (couleur
  déterministe dérivée du nom du paquet), parce que la majorité des paquets
  nixpkgs n'ont aucune icône AppStream et qu'une page détail sur deux serait
  sinon un carré gris. Sa géométrie s'aligne sur celle de GNOME : 128×128, même
  ombre portée.
- **Sections sans données Nix.** Add-ons, dépôt inclus/absent, EOL, effacement
  des données, avertissement webapp : les blocs sont créés à l'identique mais
  restent masqués tant qu'aucune donnée ne les alimente.

## 2. Architecture

`views/detail.rs` fait aujourd'hui 1813 lignes et mélange trois responsabilités :
l'arbre de widgets, la construction des sous-widgets composites, et la logique de
chargement (threads réseau, fusion de métadonnées, toasts). On les sépare.

Principe directeur : **un module Onyx par widget composite GNOME**, nommé d'après
lui, avec le chemin de son `.ui` source en tête de fichier. Toute divergence
future est alors auditable par diff contre le clone.

```
crates/gui/src/views/detail.rs           assembleur, calque de gs-details-page.ui
crates/gui/src/components/details/
    mod.rs
    lozenge.rs                  ← gs-lozenge.ui
    screenshot_carousel.rs      ← gs-screenshot-carousel.ui
    context_bar.rs              ← gs-app-context-bar.ui
    license_tile.rs             ← gs-license-tile.ui
    version_history_row.rs      ← gs-app-version-history-row.ui
    review_histogram.rs         ← gs-review-histogram.ui
    review_row.rs               ← gs-review-row.ui
    infobox.rs                  ← les 6 blocs `.card .infobox` de gs-details-page.ui
```

Chaque module expose un constructeur rendant un `gtk::Widget` (ou un type GTK
concret) et, quand l'état évolue, une fonction de mise à jour. Aucun module ne
connaît `AppMeta` au-delà des champs qu'il affiche : `detail.rs` traduit
`AppMeta` en paramètres simples (`&str`, `Option<&str>`, `u64`) et les passe.
On peut ainsi construire et tester chaque widget isolément.

`detail.rs` ne conserve que la macro `view!` (ordre des enfants identique au
`.ui`), l'enum `DetailMsg`, et `update()`.

### Infrastructure

**`crates/gui/Cargo.toml`** : `adw` passe de `features = ["v1_5"]` à
`["v1_6"]`. Cela débloque le vrai `adw::ButtonRow` (émulé à la main aujourd'hui
dans `build_version_history` et `community_tile`) et `adw::Banner`. Le crate
`libadwaita` 0.7.2 expose bien `v1_6` ; le système tourne en libadwaita 1.9.0.

**CSS** : les ~250 lignes de `APP_CSS` quittent `app.rs` pour
`crates/gui/data/style.css`, chargé via `CssProvider::load_from_string` (le
fichier est inclus par `include_str!`, donc pas de dépendance au runtime — le
passage en GResource reste prévu au packaging). Les règles portées de GNOME y
sont regroupées et annotées avec leur numéro de ligne source.

## 3. Fidélité visuelle — écarts à corriger

### 3.1 En-tête

| Élément | GNOME | Onyx actuel | Action |
|---|---|---|---|
| `box_details_header` | `spacing` non défini (= 0) | `spacing: 18` | passer à 0 |
| `box_details_header_not_icon` | `margin-top 6`, `spacing 6` | idem | ✓ |
| Icône | `GtkImage pixel_size 128`, `.icon-dropshadow`, `halign start` | Box 128×128 + fallback | garder le fallback, aligner la géométrie |
| Titre | `.app-title .title-1`, `selectable`, `wrap`, `max_width_chars 20`, `xalign 0`, `valign start` | `.title-1`, `wrap`, `xalign 0` | ajouter `.app-title`, `selectable`, `max_width_chars 20` |
| Développeur | `.app-developer .dim-label`, `selectable`, `ellipsize end`, `max-width-chars 100`, dans une `GtkBox spacing 3` | `.dim-label`, `ellipsize end` | ajouter `.app-developer`, `selectable`, la box |
| Badge vérifié | `GtkBox spacing 3` : image 16 `app-verified-symbolic` + label « Verified » ; **couleur appliquée en code** via `PackagingBaseCssColor` | `.developer-verified` = `@accent_color`, `spacing 4` | `spacing 3`, couleur via `@nix_packaging_color` |
| Étoiles | `star_box` `valign start` ; `GsStarWidget icon-size 16` ; `label_review_count` `margin_start 5` `.dim-label` | équivalent | ajouter `valign start` |
| `box_install_remove` | `spacing 9`, `halign center` | idem | ✓ |
| Boutons | largeur 105, `.suggested-action` | Lancer 105, Ajouter 130 | Ajouter → 105 |
| `box_with_source` | vertical, `halign end`, `valign center`, `margin-top 6`, `spacing 6` | idem | ✓ |
| Bouton d'origine | `MenuButton .flat .origin-button`, `always-show-arrow`, image `package-generic-symbolic` px 16 `valign center`, label `scale 0.85` `weight normal` `max-width-chars 12` `ellipsize end` | icône `package-x-generic-symbolic`, label sans attributs | corriger icône, ajouter les attributs Pango |
| `sizegroup_origin_button` | `GtkSizeGroup` liant `box_install_remove` et `origin_button` | absent | ajouter |

Règles CSS `window.narrow` manquantes :

```css
window.narrow .app-title     { font-size: 16pt; }
window.narrow .app-developer { font-size: small; }
```

Le mécanisme de couleur de packaging est reproduit tel quel. GNOME déclare
`@define-color flatpak_packaging_color @blue_4;` puis applique `color: @<couleur>`
à `origin_packaging_image`, `developer_verified_image` et
`developer_verified_label`. Onyx déclarera `@define-color nix_packaging_color
@blue_4;` et une classe `.nix-packaging-color { color: @nix_packaging_color; }`
posée sur ces trois widgets.

### 3.2 Carousel de screenshots

Structure de `gs-screenshot-carousel.ui` à reproduire :

```
GtkStack
└─ page "carousel"
   └─ GtkBox vertical .frame .view
      ├─ GtkOverlay
      │  ├─ AdwCarousel (vexpand)
      │  ├─ overlay: GtkRevealer halign start, valign center, crossfade
      │  │            └─ GtkButton 64×64, margin 9, go-previous-symbolic,
      │  │               .circular .image-button .osd
      │  └─ overlay: GtkRevealer halign end  (idem, go-next-symbolic)
      └─ AdwCarouselIndicatorDots
```

CSS source (`style.css:15-22`) :

```css
screenshot-carousel box.frame { border-width: 1px 0; }
screenshot-carousel button, .featured-carousel button { margin: 12px; }
```

Onyx n'a aujourd'hui ni l'`Overlay`, ni les deux `Revealer`, ni les boutons.
Les révélateurs suivent la position : `previous` visible si `position > 0`,
`next` visible si `position < n_pages - 1` (cf.
`gs_screenshot_carousel_notify_position_cb`).

La lightbox (clic sur un screenshot → image plein écran) est une addition Onyx,
conservée : GNOME ouvre un dialogue équivalent.

### 3.3 Context bar

Structure déjà correcte (deux `GtkBox homogeneous` de deux `GtkButton
.context-tile .flat`, conteneur `homogeneous` + `.card`). Le CSS est déjà porté
verbatim. Restent :

- Le contenu de chaque tuile : `GtkBox vertical spacing 8` → lozenge, puis
  `GtkLabel .heading` (`justify center`, `wrap`, `xalign 0.5`), puis
  `GtkLabel .caption` (`justify center`, `lines 3`, `ellipsize end`, `wrap`,
  `xalign 0.5`). Conforme, à extraire tel quel.
- `GsLozenge` devient `lozenge.rs` : `GtkBox .context-tile-lozenge`,
  `halign center`, `hexpand false`, `valign center`, contenant soit une
  `GtkImage pixel-size 16`, soit un `GtkLabel xalign 0.5` — jamais les deux.
  Propriété `circular` → ajoute/retire le comportement de `gs_lozenge_set_circular`.
- Classes de couleur manquantes dans le CSS Onyx : `.context-tile-lozenge.wide-image`
  (marges `-28px` haut/bas) et les alias `.details-rating-0/5/12/15/18`.

Le mapping sémantique Nix des quatre tuiles (Stockage / Sécurité / Matériel /
Âge) est inchangé.

### 3.4 Infobars

Six blocs, tous `AdwClamp(860/576, marges 12)` → `GtkBox vertical`
`.card .infobox` + une classe de teinte, contenu centré (`justify center`,
`wrap`), titre en `.heading`. Pas d'icône : GNOME n'en met pas.

CSS source (`style.css:195-208`) :

```css
.card.info    { background-color: mix(@accent_bg_color,  @window_bg_color, .7); color: @window_fg_color; }
.card.warning { background-color: mix(@warning_bg_color, @window_bg_color, .7); color: @window_fg_color; }
.infobox      { border-spacing: 6px; padding: 12px; }
```

GNOME en a six. **Une seule est construite** : `infobar_details_problems`, la
seule à avoir une source nixpkgs (`broken`, `insecure`,
`known_vulnerabilities`). Les cinq autres (EOL, dépôt inclus/absent, paquet du
système de base, effacement des données, avertissement webapp) n'ont aucun
équivalent Nix ; les créer pour les laisser masquées à vie serait de l'UI morte.
C'est le choix « visuel strict » (§1) et non « sections vides à l'identique ».

Le bloc `problems` remplace la `.details-warning-card` maison actuelle (qui
était horizontale, avec icône et texte à gauche — pas le rendu GNOME).

Les règles `.card.info` restent dans la feuille de style : elles servent aux
cartes d'information d'autres vues et documentent la teinte GNOME.

### 3.5 Historique des versions

`list_box_version_history` = `GtkListBox .boxed-list` contenant :
1. une `GsAppVersionHistoryRow` (→ `version_history_row.rs`) ;
2. un **vrai** `adw::ButtonRow` « Historique des versions », `end-icon-name
   go-next-symbolic`, visible seulement si `releases.len() > 1`.

`GsAppVersionHistoryRow` : `GtkListBoxRow` non sélectionnable/non activable →
`GtkBox vertical`, marges 15 partout, `valign start`, `hexpand` ; puis en-tête
`GtkBox .history-row-header` (`margin 3` haut/bas) avec le numéro de version en
gras, un badge `.installed-badge` « Installé », et la date à droite
(`hexpand`, `xalign 1`, `.dim-label`) ; puis les notes de version
(`margin_top 6`).

Le badge `.installed-badge` est nouveau côté Onyx : il s'affiche quand la
version de la release correspond à celle du paquet nixpkgs installé.

### 3.6 Licence + liens

`box_license` : `GtkBox horizontal spacing 12`, `homogeneous` lié à
`is-narrow` (inversé). Onyx force `homogeneous: true` — on ajoutera le suivi de
la largeur de la fenêtre pour désactiver `homogeneous` en mode étroit.

Gauche : `GsLicenseTile` — déjà porté fidèlement (`boxed-list`, ligne non
activable avec trois lozenges verts circulaires 16px, titre gras, description
`use-markup` + `wrap`), sauf la seconde ligne qui doit devenir un vrai
`adw::ButtonRow` « Comment participer », `end-icon-name external-link-symbolic`.

Droite : `GtkStack` (`hhomogeneous false`, `vhomogeneous false`) à deux pages.

- Page `empty` : `GtkBox .card` → `GtkBox vertical spacing 8`, marges 14,
  `valign center`, `hexpand`, `.dim-label` ; image `dialog-question-symbolic`
  `pixel-size 96` `margin-bottom 8` ; label « Aucune métadonnée » en gras ;
  label d'explication `justify center` `wrap` `xalign 0.5`.
- Page `links` : `GtkListBox .boxed-list` de six `AdwActionRow` **dans cet
  ordre exact**, chacune `activatable`, `subtitle-lines 1`, `use-underline`,
  suffixe `GtkImage external-link-symbolic`, sous-titre = l'URL
  (`subtitle_selectable`).

| Ordre | Titre | `icon-name` | Source |
|---|---|---|---|
| 1 | Site web du projet | `webpage-symbolic` | `homepage` |
| 2 | Faire un don | `donate-symbolic` | `donation` |
| 3 | Contribuer aux traductions | `translations-symbolic` | `translate` |
| 4 | Signaler un problème | `computer-fail-symbolic` | `bugtracker` |
| 5 | Aide | `help-link-symbolic` | `help` |
| 6 | Contact | `contact-symbolic` | *aucune* → non construite |

La rangée « Contact » de GNOME vient d'une URL AppStream `contact` que ni le
catalogue Flathub ni nixpkgs n'exposent dans les données qu'Onyx consomme. Elle
n'est pas construite, comme les infobars sans donnée (§3.4).

Onyx utilise aujourd'hui d'autres icônes, un autre ordre, un suffixe
`adw-external-link-symbolic`, et ajoute une ligne « Notes de version » qui
n'existe pas chez GNOME → supprimée de la liste (l'URL de changelog reste
exposée dans la rangée d'historique de versions, comme chez GNOME).

Les sept SVG sont présents dans le clone sous `data/icons/symbolic/` et seront
copiés dans `crates/gui/data/icons/hicolor/scalable/actions/`. Licence : assets
GNOME = GPL, Onyx reste GPL-compatible (cf. CONTEXT.md §5).

### 3.7 Avis

`box_reviews` : `GtkBox vertical spacing 18`, masqué si aucun avis.
1. `GtkLabel .title-2` « Évaluations », `xalign 0`.
2. `box_reviews_internal` : `GtkBox horizontal spacing 12`, `homogeneous` lié à
   `is-narrow` inversé.
   - Gauche : `GtkListBox .boxed-list valign start` → `histogram_row`
     (`GtkListBoxRow` non activable, masqué si pas de notes) contenant un
     `AdwClamp halign start maximum-size 500 tightening 500` autour du
     `GsReviewHistogram` ; puis `adw::ButtonRow` « Écrire un avis ».
   - Droite : `GtkListBox .boxed-list valign start` → l'avis vedette
     (`GsReviewRow`, ajouté en tête) ; puis `adw::ButtonRow` « Tous les avis ».

`GsReviewHistogram` (→ `review_histogram.rs`) : `GtkGrid` `row/column-spacing 6`,
marges 12. Cellule (0,0) = moyenne en `GtkLabel` `scale 5.0`, `weight light`,
`line-height 0.75`. Cellule (1,0) = `GtkBox vertical spacing 8` (`margin-start
6`, `margin-top 8`) avec cinq étoiles 24px puis « sur 5 étoiles » (`scale 0.8`).
Cellule (0,1), sur deux colonnes = `GtkGrid .review-histogram`
`row/column-spacing 2` : cinq rangées, chacune une barre (`height-request 6`,
`hexpand`, `margin-end 5`) suivie d'autant d'étoiles 10px que le niveau.
Cellule (0,2), sur deux colonnes = total (`margin-top 6`).

Noter que le nombre d'étoiles décroît (5 puis 4 …) alors que les barres restent
alignées à gauche : c'est bien la grille GNOME, pas une colonne d'étoiles fixe.

`GsReviewRow` (→ `review_row.rs`) : `GtkListBoxRow .review-row` non activable →
`GtkBox vertical spacing 6` ; ligne 1 = `GtkBox spacing 10` (étoiles, résumé
`.heading` `ellipsize end` `hexpand`, date `.dim-label` `halign end`) ; ligne 2 =
auteur `.dim-label` `ellipsize end` ; ligne 3 = texte `wrap`, `max_width_chars
80`, `wrap-mode word-char`, `xalign 0`, `margin_top 10`, `margin_bottom 8`.
Les boutons de vote/signalement sont hors périmètre (ils exigent un compte ODRS).

CSS : `.review-row > * { margin: 12px; }`, `.review-row button { font-size:
smaller; }`.

Le bouton « Écrire un avis » est présent mais insensible, avec l'infobulle
« Nécessite un compte ODRS » : GNOME le désactive lui aussi quand l'app n'est pas
installée ou que le réseau est absent.

### 3.8 États de page

`GtkStack` racine à trois pages, comme le `.ui` :

- `spinner` : `AdwStatusPage` avec `AdwSpinnerPaintable`, titre « Chargement des
  détails de l'application ». (Onyx affiche aujourd'hui des skeletons dans la
  page finale — ils sont conservés pour la description, mais la page entière
  démarre sur cet état tant que `cached_app_meta` n'a pas répondu.)
- `ready` : la page.
- `failed` : `AdwStatusPage` `dialog-error-symbolic`, « Impossible d'afficher les
  détails de l'application ».

L'`AdwBanner` « Aider à traduire » est créée, masquée en permanence : aucune
donnée de complétude de traduction côté nixpkgs.

### 3.9 SizeGroup global

Le `.ui` termine par un `GtkSizeGroup` horizontal liant `box_details_header`,
`application_details`, `box_addons`, `context_bar`, `list_box_version_history`,
`label_webapp_warning`, les quatre infobars, `box_license` et `box_reviews`.
C'est lui qui garantit que toutes les sections ont exactement la même largeur,
quel que soit leur contenu. À reproduire.

## 4. Travail côté `core`

Une seule addition : `core::reviews_for(app_id) -> Vec<Review>`.

GNOME interroge `POST https://odrs.gnome.org/1.0/reviews/api/fetch` avec le corps
JSON `{user_hash, app_id, locale, distro, version, limit}` (cf.
`lib/gs-odrs-provider.c:889-915`). La réponse est un tableau d'objets dont on
retient `summary`, `description`, `user_display`, `rating` (0–100, à diviser par
20 pour obtenir des étoiles), `date_created`, `karma_up`, `karma_down`, `score`.

```rust
pub struct Review {
    pub summary: String,
    pub description: String,
    pub author: Option<String>,
    pub stars: f32,
    pub timestamp: i64,
    pub score: i64,
}

pub fn reviews_for(app_id: &str) -> Vec<Review>;
```

- `user_hash` : GNOME dérive un SHA-1 salé de l'utilisateur ; Onyx envoie un
  hash constant (aucun compte, aucune écriture). C'est suffisant pour `fetch`.
- `distro` : `"NixOS"`. `version` : la version du paquet, sinon `""`.
- `limit` : 10 (GNOME prend `n_reviews`).
- Timeout 6 s, comme `fetch_odrs_ratings`, et `unwrap_or_default()` en cas
  d'échec — les avis sont cosmétiques, jamais bloquants.

**Avis vedette** : GNOME (`get_featured_review`) prend l'avis de `score` le plus
élevé. Même règle.

Le résultat est mis en cache par `app_id` pour la session (`session_cache`),
comme les métadonnées.

## 5. Flux de données

Inchangé dans son principe : `detail.rs` lance des threads, qui renvoient des
`DetailMsg` au fil de l'eau.

```
init()
  ├─ icône du cache disque (synchrone, instantané)          → icon_tex
  └─ thread: cached_app_meta(pkg, target_w)                 → Loaded(meta)

Loaded(meta)      → stack = "ready" | "failed" si meta vide
  ├─ thread: closure_size(attr)                             → SizeLoaded
  ├─ thread: safety(app_id)                                 → SafetyLoaded
  ├─ thread: cached_fetch_bytes(icon_url)                   → IconLoaded
  ├─ thread × N: cached_fetch_bytes(screenshot_url)         → ScreenshotLoaded
  ├─ thread: rating_for(app_id)                             → RatingLoaded
  └─ thread: reviews_for(app_id)              [nouveau]     → ReviewsLoaded
```

`RatingLoaded` alimente les étoiles de l'en-tête **et** l'histogramme.
`ReviewsLoaded` alimente l'avis vedette. La section « Évaluations » n'apparaît
que si `rating.total > 0` ; l'avis vedette seulement si la liste est non vide.
Les deux arrivent indépendamment : la colonne gauche peut s'afficher avant la
droite, chacune est masquée jusqu'à sa donnée.

## 6. Gestion d'erreur

- **Réseau indisponible** : `reviews_for` et `rating_for` renvoient vide, les
  sections restent masquées. Aucun toast : ce sont des enrichissements.
- **Métadonnées introuvables** (`cached_app_meta` ne rend ni résumé ni
  description ni version) : `stack` bascule sur `failed`, comme GNOME.
- **Paquet `broken`** : l'infobar `problems` s'affiche, la tuile Sécurité passe
  en rouge, mais le bouton « Lancer » reste actif — avec un toast d'avertissement
  au clic. L'utilisateur reste maître, comportement actuel conservé.
- **Icône ou screenshot en échec** : silencieux, on garde le fallback / on
  n'ajoute pas la page au carousel.

## 7. Tests

Les widgets GTK ne se testent pas sans display. On teste donc la **logique pure**,
qui est là où les bugs se logent :

- `format_size` (existant) — conservé.
- `format_relative` (existant, non testé) — ajouter des cas aux frontières
  (0/1/6/7/13/14 jours…).
- `desc_display` / `desc_needs_toggle` — troncature sur frontière de mot, seuil.
- `age_color` — les quatre paliers.
- `core::reviews_for` : test du **parsing** sur une réponse JSON ODRS figée
  (fixture), pas d'appel réseau. Vérifie la conversion `rating/20 → étoiles` et
  le choix de l'avis vedette par `score`.
- `featured_review(&[Review]) -> Option<&Review>` — fonction pure, testée.

Vérification visuelle finale : `cargo run -p onyx-gui`, ouvrir la page détail de
`firefox` (AppStream riche : screenshots, releases, liens, avis ODRS) puis de
`ripgrep` (nix-only : aucune icône, aucun lien → carte « Aucune métadonnée »,
carousel masqué, avis masqués). Les captures d'écran ne fonctionnent pas dans
cet environnement — la vérification passe par les logs et l'inspection GTK.

## 8. Hors périmètre

- Boutons de vote / signalement / suppression d'un avis (compte ODRS requis).
- Section Add-ons alimentée (pas d'équivalent nixpkgs).
- `origin_popover` multi-sources (Onyx n'a que nixpkgs).
- Barre de progression d'installation (`GsProgressButton`, `.install-progress`) :
  `nix run` n'expose pas de progression exploitable.
- Passage des assets en GResource (prévu au packaging).
