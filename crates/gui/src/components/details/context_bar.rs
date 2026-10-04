//! Context bar — calque de `gs-app-context-bar.ui`.
//!
//! `GtkBox homogeneous .card .app-context-bar .horizontal` contenant DEUX
//! `GtkBox homogeneous` de deux `GtkButton .context-tile .flat` chacune :
//!
//! ```text
//! ┌───────────────┬───────────────┬───────────────┬───────────────┐
//! │   Stockage    │   Sécurité    │   Matériel    │     Âge       │
//! └───────────────┴───────────────┴───────────────┴───────────────┘
//!  └── box 1 (homogeneous) ──┘ └── box 2 (homogeneous) ──┘
//! ```
//!
//! Le cadre arrondi vient de `.card` ; les tuiles n'ont qu'une bordure gauche,
//! qui joue le rôle de séparateur interne.
//!
//! Le contenu des quatre tuiles est spécifique à Nix (taille de fermeture,
//! `broken`/`insecure`, mainteneurs) — c'est la divergence assumée de la spec :
//! on copie le contenant, pas la source de données.

use gtk::prelude::*;

use super::lozenge::{lozenge, Content};

/// Corps d'une tuile : `GtkBox vertical spacing 8` → lozenge, titre `.heading`,
/// description `.caption` sur 3 lignes maximum.
pub fn tile(content: Content, color: &str, title: &str, description: &str) -> gtk::Button {
    let v = gtk::Box::new(gtk::Orientation::Vertical, 8);
    v.append(&lozenge(content, color));

    let t = gtk::Label::new(Some(title));
    t.set_justify(gtk::Justification::Center);
    t.set_wrap(true);
    t.set_xalign(0.5);
    t.add_css_class("heading");
    v.append(&t);

    let d = gtk::Label::new(Some(description));
    d.set_justify(gtk::Justification::Center);
    d.set_wrap(true);
    d.set_lines(3);
    d.set_ellipsize(gtk::pango::EllipsizeMode::End);
    d.set_xalign(0.5);
    d.add_css_class("caption");
    v.append(&d);

    let btn = gtk::Button::builder().child(&v).build();
    btn.add_css_class("context-tile");
    btn.add_css_class("flat");
    btn
}

/// Le conteneur vide, à créer une fois puis remplir via [`fill`].
pub fn container() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.set_homogeneous(true);
    b.add_css_class("app-context-bar");
    b.add_css_class("horizontal");
    b.add_css_class("card");
    b
}

/// Remplace les quatre tuiles. L'ordre est celui du `.ui` : stockage, sécurité,
/// matériel, âge — regroupées deux par deux.
pub fn fill(container: &gtk::Box, tiles: [gtk::Button; 4]) {
    while let Some(c) = container.first_child() {
        container.remove(&c);
    }
    let [storage, safety, hardware, age] = tiles;
    for pair in [[storage, safety], [hardware, age]] {
        let sub = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sub.set_homogeneous(true);
        for t in pair {
            sub.append(&t);
        }
        container.append(&sub);
    }
}
