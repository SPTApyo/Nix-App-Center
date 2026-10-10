//! The badge of a context tile: an icon or a short text.

use gtk::prelude::*;

/// Side of a circular lozenge, as the CSS sizes icons.
const CIRCULAR_SIZE: i32 = 40;

/// An icon or a short text.
pub enum Content<'a> {
    Icon(&'a str),
    Text(&'a str),
}

/// A lozenge tinted by the CSS class `color`.
pub fn lozenge(content: Content, color: &str) -> gtk::Box {
    build(content, color, false)
}

/// A round lozenge.
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
