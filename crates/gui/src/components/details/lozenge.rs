//! Pastille de tuile — calque de `gs-lozenge.ui` / `gs-lozenge.c`.
//!
//! `GtkBox .context-tile-lozenge` (halign/valign center, hexpand false) portant
//! SOIT une `GtkImage` 16px, SOIT un `GtkLabel` — jamais les deux.
//!
//! GNOME implémente `circular` avec un `GtkLayoutManager` maison qui force
//! largeur == hauteur (`gs_lozenge_layout_measure`). Sans sous-classe GTK côté
//! Rust, on obtient le même résultat en imposant un `size_request` carré : le
//! CSS donne déjà 40×40 pour une icône (`min-width 18 + padding 11×2`), donc
//! seuls les lozenges à texte ont réellement besoin de la contrainte.

use gtk::prelude::*;

/// Côté d'un lozenge circulaire, dérivé du CSS (`.context-tile-lozenge`).
const CIRCULAR_SIZE: i32 = 40;

/// Contenu d'un lozenge : une icône symbolique OU un texte court (« 35,5 Mo », « 12 »).
pub enum Content<'a> {
    Icon(&'a str),
    Text(&'a str),
}

/// Lozenge de context bar. `color` est une classe de teinte du CSS
/// (`grey`/`green`/`blue`/`yellow`/`orange`/`red`, ou `details-rating-N`).
pub fn lozenge(content: Content, color: &str) -> gtk::Box {
    build(content, color, false)
}

/// Lozenge circulaire (tuile licence : trois pastilles rondes).
pub fn circular(content: Content, color: &str) -> gtk::Box {
    build(content, color, true)
}

fn build(content: Content, color: &str, circular: bool) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.set_halign(gtk::Align::Center);
    b.set_valign(gtk::Align::Center);
    b.set_hexpand(false);
    b.add_css_class("context-tile-lozenge");
    b.add_css_class(color);

    match content {
        Content::Icon(name) => {
            let img = gtk::Image::from_icon_name(name);
            img.set_pixel_size(16);
            img.set_halign(gtk::Align::Center);
            img.set_hexpand(true);
            b.append(&img);
        }
        Content::Text(text) => {
            let l = gtk::Label::new(Some(text));
            l.set_halign(gtk::Align::Center);
            l.set_hexpand(true);
            l.set_xalign(0.5);
            b.append(&l);
        }
    }

    if circular {
        b.set_width_request(CIRCULAR_SIZE);
        b.set_height_request(CIRCULAR_SIZE);
    }
    b
}
