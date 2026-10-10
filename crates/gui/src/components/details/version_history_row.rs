//! A release row: version, date, notes, and a badge on the version Nix gives.

use gtk::prelude::*;

use crate::i18n::t;
use crate::util::format_relative;

/// Row body; `badge` labels the version Nix gives ("Installed", "Available").
fn body(
    version: &str,
    timestamp: i64,
    description: Option<&str>,
    badge: Option<&str>,
    url: Option<&str>,
) -> gtk::Box {
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

    let vnum = gtk::Label::new(Some(&t("Version {}").replace("{}", version)));
    vnum.set_xalign(0.0);
    vnum.set_wrap(true);
    vnum.add_css_class("heading");
    head.append(&vnum);

    if let Some(text) = badge {
        let badge = gtk::Label::new(Some(text));
        badge.set_valign(gtk::Align::Center);
        badge.add_css_class("installed-badge");
        head.append(&badge);
    }

    let date = gtk::Label::new(Some(&format_relative(timestamp)));
    date.set_hexpand(true);
    date.set_xalign(1.0);
    date.add_css_class("dim-label");
    head.append(&date);
    if let Some(url) = url.filter(|u| !u.is_empty()) {
        let link = gtk::Button::from_icon_name("external-link-symbolic");
        link.add_css_class("flat");
        link.set_valign(gtk::Align::Center);
        link.set_margin_start(6);
        link.set_tooltip_text(Some(&t("Release Notes")));
        let url = url.to_string();
        link.connect_clicked(move |_| {
            crate::util::open_uri(&url);
        });
        head.append(&link);
    }
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
            let none = gtk::Label::new(Some(&t("No details for this version.")));
            none.set_margin_top(6);
            none.set_xalign(0.0);
            none.add_css_class("dim-label");
            vb.append(&none);
        }
    }
    vb
}

/// The release row.
pub fn row(
    version: &str,
    timestamp: i64,
    description: Option<&str>,
    badge: Option<&str>,
    url: Option<&str>,
) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.set_child(Some(&body(version, timestamp, description, badge, url)));
    row
}
