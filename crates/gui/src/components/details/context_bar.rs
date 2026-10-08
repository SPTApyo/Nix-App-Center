//! The four context tiles of the details page: storage, safety, hardware
//! and age, filled with Nix facts.

use gtk::prelude::*;

use super::lozenge::{lozenge, Content};

/// A tile: lozenge, title and a short description.
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

/// The empty bar, filled by [`fill`].
pub fn container() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.set_homogeneous(true);
    b.add_css_class("app-context-bar");
    b.add_css_class("horizontal");
    b.add_css_class("card");
    b
}

/// Replaces the four tiles, two per group.
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
