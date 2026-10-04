//! Rangée d'avis — calque de `gs-review-row.ui`.
//!
//! `GtkListBoxRow .review-row` → `GtkBox vertical spacing 6` :
//!  1. `GtkBox spacing 10` : étoiles, résumé `.heading` (ellipsize, hexpand),
//!     date `.dim-label` alignée à droite ;
//!  2. auteur `.dim-label`, `ellipsize end` ;
//!  3. texte de l'avis (`wrap`, `max-width-chars 80`, marges 10/8).
//!
//! Les boutons de vote / signalement / suppression du `.ui` sont hors périmètre :
//! ils exigent un compte ODRS, et Onyx ne fait que lire (cf. spec §8).

use gtk::prelude::*;
use onyx_core::Review;

use crate::util::{format_relative, stars_widget};

/// L'avis vedette, non activable (comme le `GsReviewRow` de la colonne droite).
pub fn row(review: &Review) -> gtk::ListBoxRow {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 6);

    // Ligne 1 : étoiles + résumé + date.
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    head.append(&stars_widget(review.stars, 16));

    let summary = gtk::Label::new(Some(&review.summary));
    summary.set_xalign(0.0);
    summary.set_hexpand(true);
    summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
    summary.add_css_class("heading");
    head.append(&summary);

    let date = gtk::Label::new(Some(&format_relative(review.timestamp)));
    date.set_halign(gtk::Align::End);
    date.add_css_class("dim-label");
    head.append(&date);
    vb.append(&head);

    // Ligne 2 : auteur.
    if let Some(author) = review.author.as_deref() {
        let a = gtk::Label::new(Some(author));
        a.set_xalign(0.0);
        a.set_ellipsize(gtk::pango::EllipsizeMode::End);
        a.add_css_class("dim-label");
        vb.append(&a);
    }

    // Ligne 3 : le texte.
    let text = gtk::Label::new(Some(&review.description));
    text.set_xalign(0.0);
    text.set_wrap(true);
    text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    text.set_max_width_chars(80);
    text.set_margin_top(10);
    text.set_margin_bottom(8);
    vb.append(&text);

    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.add_css_class("review-row");
    row.set_child(Some(&vb));
    row
}
