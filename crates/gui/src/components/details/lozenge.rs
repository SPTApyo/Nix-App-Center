//! Tile badge, a copy of `gs-lozenge.ui` / `gs-lozenge.c`.
//!
//! `GtkBox .context-tile-lozenge` (halign/valign center, hexpand false) holding
//! EITHER a 16 px `GtkImage` OR a `GtkLabel`, never both.
//!
//! GNOME implements `circular` with a custom `GtkLayoutManager` forcing
//! width == height (`gs_lozenge_layout_measure`). Without a GTK subclass in
//! Rust, a square `size_request` gives the same result: the CSS already makes
//! an icon 40×40 (`min-width 18 + padding 11×2`), so only text lozenges really
//! need the constraint.

use gtk::prelude::*;

/// Side of a circular lozenge, derived from the CSS (`.context-tile-lozenge`).
const CIRCULAR_SIZE: i32 = 40;

/// Lozenge content: a symbolic icon OR a short text ("35.5 MB", "12").
pub enum Content<'a> {
    Icon(&'a str),
    Text(&'a str),
}

/// Context bar lozenge. `color` is a CSS tint class
/// (`grey`/`green`/`blue`/`yellow`/`orange`/`red`, or `details-rating-N`).
pub fn lozenge(content: Content, color: &str) -> gtk::Box {
    build(content, color, false)
}

/// Circular lozenge (license tile: three round badges).
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
