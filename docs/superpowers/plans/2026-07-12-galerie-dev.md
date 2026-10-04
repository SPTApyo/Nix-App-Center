# Galerie de développement — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ajouter un onglet « Galerie » de dev (feature Cargo `dev-gallery`, absent du binaire release) listant des scénarios réels étiquetés qui ouvrent les vraies vues recherche/détail/accueil.

**Architecture:** Un seul module nouveau `crates/gui/src/views/gallery.rs` (composant relm4 `GalleryModel`) réutilisant les vrais composants (`AppList`/`DetailModel`/`HomeModel`) et fonctions core. Câblage dans `app.rs` entièrement gardé par `#[cfg(feature = "dev-gallery")]` : feature off ⇒ code inexistant, app bit-pour-bit identique.

**Tech Stack:** Rust, relm4 0.9 + GTK4 0.9 + libadwaita 0.7, `direnv exec . cargo`.

## Global Constraints

- Toute commande cargo passe par `direnv exec . cargo …` (cargo absent du PATH).
- Le binaire GUI a deux profils à faire compiler : **sans** feature (`cargo build -p onyx-gui`) ET **avec** (`cargo build -p onyx-gui --features dev-gallery`). Les deux doivent réussir sans warning à chaque tâche.
- La feature `dev-gallery` est portée **uniquement** par `onyx-gui` — ne jamais toucher `onyx-core` ni `onyx-cli`.
- Commentaires en français, expliquant le pourquoi, pas le quoi.
- **PAS DE COMMIT** : l'arbre de travail porte du WIP non commité de l'utilisateur ; il committera lui-même.
- Lancer l'app : `direnv exec . cargo run -p onyx-gui` (normal) ou `… --features dev-gallery` (avec galerie). `cargo run` seul échoue (workspace à 2 binaires) — toujours `-p onyx-gui`.

---

### Task 1: Feature `dev-gallery` + module galerie squelette (build 2 profils)

Pose la feature Cargo et un module `gallery.rs` minimal mais complet (composant relm4 vide qui compile et affiche un titre), déclaré de façon gardée. Le « test » d'un harnais visuel = **les deux profils compilent**.

**Files:**
- Modify: `crates/gui/Cargo.toml` (ajout section `[features]`)
- Create: `crates/gui/src/views/gallery.rs`
- Modify: `crates/gui/src/views/mod.rs` (déclaration gardée)

**Interfaces:**
- Produces: `crate::views::gallery::GalleryModel` (composant relm4, `Init = ()`), `GalleryOutput` (enum), `GalleryMsg` (enum). Détaillés en Task 2 ; Task 1 les crée dans leur forme finale pour éviter un renommage ultérieur.

- [x] **Step 1: Ajouter la feature dans `Cargo.toml`**

Dans `crates/gui/Cargo.toml`, juste après le bloc `[[bin]]` (avant `[dependencies]`), insérer :

```toml
[features]
# Galerie de dev : onglet supplémentaire montrant chaque type d'app. Désactivée
# par défaut → absente du binaire release. Cf. docs/superpowers/specs/2026-07-12-galerie-dev-design.md
dev-gallery = []
```

- [x] **Step 2: Créer `gallery.rs` (squelette complet des types)**

Créer `crates/gui/src/views/gallery.rs` :

```rust
//! Galerie de développement (feature `dev-gallery`) — onglet listant des
//! scénarios réels étiquetés qui ouvrent les vraies vues recherche/détail, plus
//! l'accueil réel intégré. Harnais visuel : voir chaque type d'app côte à côte
//! (cliquable ou non, avec/sans icône, sections conditionnelles) pour éviter les
//! erreurs d'inattention. Jamais compilé en release.

use adw::prelude::*;
use relm4::prelude::*;
use onyx_core::Package;

use crate::views::home::{HomeModel, HomeOutput};

/// (label, requête réelle, comportement attendu à vérifier).
const SEARCH_SCENARIOS: &[(&str, &str, &str)] = &[
    ("App avec icône + note", "firefox",
     "Section « Applications », icône, étoiles, cliquable"),
    ("Mix reconnu / non-reconnu", "editor",
     "« Applications » (avec icônes) ET « Autres paquets »"),
    ("Filtre jeu de paquets", "aeson",
     "Menu déroulant « haskellPackages » apparaît"),
    ("Fournit une commande", "rg",
     "Section « Fournit la commande rg » (nix-locate)"),
    ("Zéro résultat", "zzqxnonexistent",
     "Page « Aucun résultat »"),
];

/// (label, attr réel, comportement attendu à vérifier).
const DETAIL_SCENARIOS: &[(&str, &str, &str)] = &[
    ("Fiche complète", "steam",
     "Screenshots ; Extensions = grille NON cliquable ; Autres applications = grille CLIQUABLE ; tuile Âge ; licence propriétaire"),
    ("Libre + avis", "firefox",
     "Tuile « Logiciel libre » ; histogramme d'avis ; historique des versions"),
    ("Nix pur runnable", "ripgrep",
     "Peu de métadonnées ; bouton Lancer ; pas de screenshots"),
    ("Keep-only / sans méta", "zlib",
     "Pas de mainProgram ; carte « Aucune métadonnée »"),
    // python27 est vérifié meta.insecure=true ; même rendu (tuile rouge +
    // bandeau problèmes) que broken. À rafraîchir s'il cesse d'être insecure.
    ("Vulnérable (insecure)", "python27",
     "Tuile rouge « Vulnérable » + bandeau problèmes"),
];

pub struct GalleryModel {
    // Gardé vivant : l'accueil réel est intégré dans la galerie.
    #[allow(dead_code)]
    home: Controller<HomeModel>,
}

#[derive(Debug)]
pub enum GalleryMsg {
    /// Clic de tuile dans l'accueil intégré → re-émis vers le parent.
    HomeOpen(Package),
    /// Clic de carte-catégorie dans l'accueil intégré → re-émis vers le parent.
    HomeCategory(&'static [&'static str], &'static str),
}

#[derive(Debug)]
pub enum GalleryOutput {
    /// Lance une recherche réelle pré-remplie (→ AppMsg::Search).
    Search(String),
    /// Ouvre la vraie fiche détail d'un attr (→ AppMsg::OpenDetail).
    OpenDetail(Package),
    /// Ouvre une page catégorie (→ AppMsg::OpenCategory), depuis l'accueil intégré.
    Category(&'static [&'static str], &'static str),
}

#[relm4::component(pub)]
impl SimpleComponent for GalleryModel {
    type Init = ();
    type Input = GalleryMsg;
    type Output = GalleryOutput;

    view! {
        #[root]
        gtk::ScrolledWindow {
            set_vexpand: true,
            set_hscrollbar_policy: gtk::PolicyType::Never,
        }
    }

    fn init(_: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        // L'accueil réel, intégré : ses sorties sont re-routées vers le parent.
        let home = HomeModel::builder()
            .launch(())
            .forward(sender.input_sender(), |o| match o {
                HomeOutput::Open(p) => GalleryMsg::HomeOpen(p),
                HomeOutput::Category(c, l) => GalleryMsg::HomeCategory(c, l),
            });

        let content = build_index(&sender, home.widget());

        let clamp = adw::Clamp::new();
        clamp.set_maximum_size(1000);
        clamp.set_tightening_threshold(600);
        clamp.set_margin_top(24);
        clamp.set_margin_bottom(24);
        clamp.set_margin_start(12);
        clamp.set_margin_end(12);
        clamp.set_child(Some(&content));
        root.set_child(Some(&clamp));

        let model = GalleryModel { home };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            GalleryMsg::HomeOpen(p) => {
                let _ = sender.output(GalleryOutput::OpenDetail(p));
            }
            GalleryMsg::HomeCategory(c, l) => {
                let _ = sender.output(GalleryOutput::Category(c, l));
            }
        }
    }
}

/// Construit l'index : trois sections (Recherche / Détail / Accueil).
/// `home_widget` = la racine de `HomeModel`, un `gtk::ScrolledWindow`.
fn build_index(
    sender: &ComponentSender<GalleryModel>,
    home_widget: &gtk::ScrolledWindow,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);

    // ── Recherche ─────────────────────────────────────────────────────────
    content.append(&heading("Recherche"));
    let search_list = boxed_list();
    for (label, query, expected) in SEARCH_SCENARIOS {
        let row = scenario_row(label, expected);
        let s = sender.clone();
        let q = query.to_string();
        row.connect_activated(move |_| {
            let _ = s.output(GalleryOutput::Search(q.clone()));
        });
        search_list.append(&row);
    }
    content.append(&search_list);

    // ── Détail ────────────────────────────────────────────────────────────
    content.append(&heading("Détail"));
    let detail_list = boxed_list();
    for (label, attr, expected) in DETAIL_SCENARIOS {
        let row = scenario_row(label, expected);
        let s = sender.clone();
        let attr = attr.to_string();
        row.connect_activated(move |_| {
            let _ = s.output(GalleryOutput::OpenDetail(Package::from_attr(attr.clone())));
        });
        detail_list.append(&row);
    }
    content.append(&detail_list);

    // ── Accueil (intégré) ─────────────────────────────────────────────────
    content.append(&heading("Accueil"));
    let note = gtk::Label::new(Some(
        "États d'icône à vérifier : chargement (shimmer) → repli coloré → icône réelle.",
    ));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    content.append(&note);
    // L'accueil est lui-même un ScrolledWindow (vexpand) : borné en hauteur et
    // vexpand désactivé, il devient un panneau à hauteur fixe dans le scroll de
    // la galerie (sinon deux scrollers imbriqués se disputent la hauteur).
    home_widget.set_vexpand(false);
    home_widget.set_height_request(700);
    content.append(home_widget);

    content
}

fn heading(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.add_css_class("title-2");
    l
}

fn boxed_list() -> gtk::ListBox {
    let lb = gtk::ListBox::new();
    lb.add_css_class("boxed-list");
    lb.set_selection_mode(gtk::SelectionMode::None);
    lb.set_valign(gtk::Align::Start);
    lb
}

/// Une ligne de scénario : titre + sous-titre = comportement attendu.
fn scenario_row(label: &str, expected: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(label)
        .subtitle(expected)
        .activatable(true)
        .build();
    row.set_subtitle_lines(0); // pas de troncature du comportement attendu
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row
}
```

- [x] **Step 3: Déclarer le module (gardé) dans `mod.rs`**

Dans `crates/gui/src/views/mod.rs`, après la ligne `pub mod updates;`, ajouter :

```rust
#[cfg(feature = "dev-gallery")]
pub mod gallery;
```

et après `pub use updates::UpdatesModel;`, ajouter :

```rust
#[cfg(feature = "dev-gallery")]
pub use gallery::{GalleryModel, GalleryOutput};
```

- [x] **Step 4: Vérifier que le profil PAR DÉFAUT compile (galerie absente)**

Run: `direnv exec . cargo build -p onyx-gui 2>&1 | grep -iE "warning|error" | grep -v "Git tree"; echo "exit=$?"`
Expected: aucune sortie warning/error — le module `gallery` n'est pas compilé (garde `#[cfg]`).

- [x] **Step 5: Vérifier que le profil `dev-gallery` compile**

Run: `direnv exec . cargo build -p onyx-gui --features dev-gallery 2>&1 | grep -iE "warning|error" | grep -v "Git tree"; echo "exit=$?"`
Expected: aucune sortie warning/error. `GalleryModel` compile (le champ `home` est marqué `#[allow(dead_code)]`, la galerie n'est pas encore branchée dans `app.rs` — c'est la Task 2).

---

### Task 2: Brancher l'onglet « Galerie » dans `app.rs` (recherche + détail)

Câble le `GalleryModel` dans `AppModel` : champ gardé, construction gardée, onglet gardé, routage des sorties vers les `AppMsg` existants. Après cette tâche, l'onglet apparaît avec `--features dev-gallery` et les scénarios recherche/détail fonctionnent.

**Files:**
- Modify: `crates/gui/src/app.rs` (struct `AppModel`, `init`)

**Interfaces:**
- Consumes: `crate::views::gallery::{GalleryModel, GalleryOutput}` (Task 1). `AppMsg::Search(String)`, `AppMsg::OpenDetail(Package)`, `AppMsg::OpenCategory(&'static [&'static str], &'static str)` (déjà existants dans `app.rs`).
- Produces: rien de nouveau pour les tâches suivantes.

- [x] **Step 1: Ajouter le champ gardé dans la struct `AppModel`**

Dans `crates/gui/src/app.rs`, dans la struct `AppModel`, après le champ `updates: Controller<UpdatesModel>,` (garder les attributs `#[allow(dead_code)]` existants tels quels), ajouter :

```rust
    #[cfg(feature = "dev-gallery")]
    #[allow(dead_code)]
    gallery: Controller<crate::views::gallery::GalleryModel>,
```

- [x] **Step 2: Construire le contrôleur galerie (gardé) dans `init`**

Dans `init`, juste après la construction du contrôleur `updates` (le bloc
`let updates = UpdatesModel::builder()…;`), ajouter :

```rust
        #[cfg(feature = "dev-gallery")]
        let gallery = crate::views::gallery::GalleryModel::builder()
            .launch(())
            .forward(sender.input_sender(), |out| match out {
                crate::views::gallery::GalleryOutput::Search(q) => AppMsg::Search(q),
                crate::views::gallery::GalleryOutput::OpenDetail(p) => AppMsg::OpenDetail(p),
                crate::views::gallery::GalleryOutput::Category(c, l) => AppMsg::OpenCategory(c, l),
            });
```

- [x] **Step 3: Ajouter l'onglet (gardé) dans le ViewStack**

Toujours dans `init`, juste après le bloc qui ajoute l'onglet « Mises à jour »
(`let updates_page = view_stack.add_titled(updates.widget(), Some("updates"), "Mises à jour");`
suivi de son `set_icon_name`), ajouter :

```rust
        #[cfg(feature = "dev-gallery")]
        {
            // Icône dev quelconque du thème système ; si absente, l'onglet
            // s'affiche sans icône (sans conséquence, c'est du dev).
            let gallery_page =
                view_stack.add_titled(gallery.widget(), Some("gallery"), "Galerie");
            gallery_page.set_icon_name(Some("applications-engineering-symbolic"));
        }
```

- [x] **Step 4: Inclure le champ gardé dans le littéral `AppModel`**

Dans `init`, dans la construction `let model = AppModel { … };`, après `updates,`
ajouter :

```rust
            #[cfg(feature = "dev-gallery")]
            gallery,
```

- [x] **Step 5: Vérifier le profil PAR DÉFAUT (inchangé)**

Run: `direnv exec . cargo build -p onyx-gui 2>&1 | grep -iE "warning|error" | grep -v "Git tree"; echo "exit=$?"`
Expected: aucune sortie — `app.rs` est identique à avant (toutes les insertions sont gardées).

- [x] **Step 6: Vérifier le profil `dev-gallery`**

Run: `direnv exec . cargo build -p onyx-gui --features dev-gallery 2>&1 | grep -iE "warning|error" | grep -v "Git tree"; echo "exit=$?"`
Expected: aucune sortie warning/error.

- [x] **Step 7: Vérification observationnelle (lancement réel)**

Run: `direnv exec . cargo run -p onyx-gui --features dev-gallery`
Expected (manuel) : un onglet « Galerie » apparaît dans le ViewSwitcher. Cliquer une ligne de la section « Recherche » (ex. « App avec icône + note ») bascule sur l'onglet Explorer avec la recherche `firefox` remplie et des résultats. Cliquer une ligne « Détail » (ex. « Fiche complète » → `steam`) pousse la vraie page détail. Fermer la fenêtre pour terminer.

---

### Task 3: Vérifier l'accueil intégré + la navigation catégorie

L'accueil réel est déjà intégré (Task 1) et son routage catégorie est câblé (Task 2, `GalleryOutput::Category`). Cette tâche vérifie que les clics de l'accueil intégré (tuile ET carte-catégorie) fonctionnent depuis la galerie.

**Files:** aucun changement de code attendu — tâche de vérification. Si un bug apparaît, le corriger dans `crates/gui/src/views/gallery.rs` ou `crates/gui/src/app.rs`.

- [x] **Step 1: Vérification observationnelle de l'accueil intégré**

Run: `direnv exec . cargo run -p onyx-gui --features dev-gallery`
Expected (manuel) : dans l'onglet Galerie, faire défiler jusqu'à la section « Accueil » : le carousel vedette, « Notre sélection », « Nouveautés et mises à jour » et les cartes de catégories s'affichent (données réelles). Cliquer une tuile de l'accueil intégré ouvre sa fiche détail. Cliquer une carte-catégorie (ex. « Jouer ») pousse la page catégorie. Fermer la fenêtre.

- [x] **Step 2: Re-vérifier les deux profils (au cas où un correctif a été appliqué)**

Run: `direnv exec . cargo build -p onyx-gui 2>&1 | grep -iE "warning|error" | grep -v "Git tree"; direnv exec . cargo build -p onyx-gui --features dev-gallery 2>&1 | grep -iE "warning|error" | grep -v "Git tree"; echo "done"`
Expected: aucune sortie warning/error pour les deux profils.

---

### Task 4: Vérification finale (isolation release + suite de tests)

- [x] **Step 1: Suite de tests complète du workspace (feature off)**

Run: `direnv exec . cargo test --workspace 2>&1 | grep -E "test result|FAILED|error"`
Expected: tout PASS (la galerie n'ajoute aucun test ; rien ne doit régresser).

- [x] **Step 2: Confirmer l'absence de la galerie en profil par défaut**

Run: `direnv exec . cargo build -p onyx-gui --release 2>&1 | grep -iE "warning|error" | grep -v "Git tree"; echo "release-build-ok"`
Expected: build release réussi sans warning. Le symbole `gallery` n'est pas compilé (module non déclaré sans la feature) — garanti par `#[cfg(feature = "dev-gallery")]`.

- [x] **Step 3: Confirmer que le profil galerie compile en release aussi**

Run: `direnv exec . cargo build -p onyx-gui --release --features dev-gallery 2>&1 | grep -iE "warning|error" | grep -v "Git tree"; echo "gallery-release-build-ok"`
Expected: build réussi sans warning (la feature doit rester compilable, même en release, pour un éventuel packaging de dev).
