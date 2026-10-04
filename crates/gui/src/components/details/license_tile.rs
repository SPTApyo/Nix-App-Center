//! Tuile de licence — calque de `gs-license-tile.ui`.
//!
//! `GtkListBox .boxed-list` avec deux rangées :
//!  1. `GtkListBoxRow` non activable/non focusable → `GtkBox vertical spacing 8`
//!     (marges 14) : trois lozenges verts circulaires, titre gras, description.
//!  2. `AdwButtonRow` « Comment participer », `end-icon-name external-link-symbolic`.
//!
//! GNOME n'affiche cette carte « Construit par la communauté » que pour les
//! licences libres ; sinon une variante « propriétaire ». Onyx suit la même règle
//! via `AppMeta::is_free_license`.

use adw::prelude::*;


use super::lozenge::{circular, Content};

/// Les trois pastilles de la carte communautaire (`.ui` : lozenge0/1/2).
const COMMUNITY_ICONS: [&str; 3] = [
    "community-supported-symbolic",
    "community-symbolic",
    "community-approved-symbolic",
];

/// Carte « Construit par la communauté » (licence libre).
pub fn community(license: Option<&str>, get_involved_url: Option<String>) -> gtk::ListBox {
    let description = match license {
        Some(l) => format!(
            "Il s'agit d'une application libre développée par une communauté \
             internationale, et publiée sous la licence {l}.\n\nVous pouvez \
             participer et aider à son amélioration."
        ),
        None => "Il s'agit d'une application libre développée par une communauté \
                 internationale.\n\nVous pouvez participer et aider à son amélioration."
            .to_string(),
    };

    let icons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    icons.set_halign(gtk::Align::Center);
    for name in COMMUNITY_ICONS {
        icons.append(&circular(Content::Icon(name), "green"));
    }

    let list = boxed_list();
    list.append(&info_row(&icons, "Construit par la communauté", &description));
    list.append(&get_involved_row(get_involved_url));
    list
}

/// Variante propriétaire / licence inconnue : un seul lozenge, pas de « participer ».
pub fn proprietary(license: Option<&str>, is_free: Option<bool>) -> gtk::ListBox {
    let (icon, color, title) = match is_free {
        Some(false) => ("app-safety-unsafe-symbolic", "orange", "Logiciel propriétaire"),
        _ => ("app-safety-unknown-symbolic", "grey", "Licence inconnue"),
    };
    let description = match license {
        Some(l) => format!(
            "Le code source de cette application n'est pas public. Elle est \
             publiée sous la licence {l}."
        ),
        None => "La licence de ce paquet n'est pas renseignée dans nixpkgs.".to_string(),
    };

    let icons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    icons.set_halign(gtk::Align::Center);
    icons.append(&circular(Content::Icon(icon), color));

    let list = boxed_list();
    list.append(&info_row(&icons, title, &description));
    list
}

fn boxed_list() -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_valign(gtk::Align::Start);
    list.add_css_class("boxed-list");
    list
}

fn info_row(icons: &gtk::Box, title: &str, description: &str) -> gtk::ListBoxRow {
    let inner = gtk::Box::new(gtk::Orientation::Vertical, 8);
    inner.set_margin_top(14);
    inner.set_margin_bottom(14);
    inner.set_margin_start(14);
    inner.set_margin_end(14);
    inner.append(icons);

    let t = gtk::Label::new(Some(title));
    t.add_css_class("heading");
    t.set_justify(gtk::Justification::Center);
    t.set_wrap(true);
    inner.append(&t);

    let d = gtk::Label::new(Some(description));
    d.set_wrap(true);
    d.set_justify(gtk::Justification::Center);
    d.set_xalign(0.5);
    inner.append(&d);

    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.set_focusable(false);
    row.set_child(Some(&inner));
    row
}

fn get_involved_row(url: Option<String>) -> adw::ButtonRow {
    let row = adw::ButtonRow::new();
    row.set_title("Comment _participer");
    row.set_use_underline(true);
    row.set_end_icon_name(Some("external-link-symbolic"));
    match url {
        Some(u) => {
            row.connect_activated(move |_| {
                let _ = gtk::gio::AppInfo::launch_default_for_uri(
                    &u,
                    None::<&gtk::gio::AppLaunchContext>,
                );
            });
        }
        None => row.set_sensitive(false),
    }
    row
}
