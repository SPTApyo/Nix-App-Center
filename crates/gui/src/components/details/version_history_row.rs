//! Rangée d'historique de version — calque de `gs-app-version-history-row.ui`.
//!
//! `GtkListBoxRow` non sélectionnable → `GtkBox vertical` (marges 15, valign
//! start, hexpand) :
//!  - en-tête `GtkBox .history-row-header` (marges 3 haut/bas) : « Version X »
//!    en `.heading`, un badge `.installed-badge` optionnel, la date à droite
//!    (`hexpand`, `xalign 1`, `.dim-label`) ;
//!  - les notes de version (`margin-top 6`), ou « Aucun détail » en `.dim-label`.

use gtk::prelude::*;

use crate::util::format_relative;

/// Corps d'une rangée. `installed` ajoute le badge « Installé » (signal Nix :
/// la release correspond à la version présente dans nixpkgs).
fn body(version: &str, timestamp: i64, description: Option<&str>, installed: bool) -> gtk::Box {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vb.set_margin_top(15);
    vb.set_margin_bottom(15);
    vb.set_margin_start(15);
    vb.set_margin_end(15);
    vb.set_valign(gtk::Align::Start);
    vb.set_hexpand(true);

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    head.set_margin_top(3);
    head.set_margin_bottom(3);
    head.add_css_class("history-row-header");

    let vnum = gtk::Label::new(Some(&format!("Version {version}")));
    vnum.set_xalign(0.0);
    vnum.set_wrap(true);
    vnum.add_css_class("heading");
    head.append(&vnum);

    if installed {
        let badge = gtk::Label::new(Some("Installé"));
        badge.set_valign(gtk::Align::Center);
        badge.add_css_class("installed-badge");
        head.append(&badge);
    }

    let date = gtk::Label::new(Some(&format_relative(timestamp)));
    date.set_hexpand(true);
    date.set_xalign(1.0);
    date.add_css_class("dim-label");
    head.append(&date);
    vb.append(&head);

    match description.filter(|s| !s.trim().is_empty()) {
        Some(desc) => {
            let body = gtk::Label::new(Some(desc));
            body.set_margin_top(6);
            body.set_xalign(0.0);
            body.set_wrap(true);
            vb.append(&body);
        }
        None => {
            let none = gtk::Label::new(Some("Aucun détail pour cette version."));
            none.set_margin_top(6);
            none.set_xalign(0.0);
            none.add_css_class("dim-label");
            vb.append(&none);
        }
    }
    vb
}

/// La rangée complète, non activable (comme `row_latest_version` du `.ui`).
pub fn row(
    version: &str,
    timestamp: i64,
    description: Option<&str>,
    installed: bool,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_child(Some(&body(version, timestamp, description, installed)));
    row
}
