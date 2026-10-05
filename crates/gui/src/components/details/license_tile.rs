//! License tile, a copy of `gs-license-tile.ui`.
//!
//! `GtkListBox .boxed-list` with two rows:
//!  1. non activatable, non focusable `GtkListBoxRow` holding a vertical `GtkBox`
//!     (spacing 8, margins 14): three round green lozenges, bold title, description.
//!  2. `AdwButtonRow` "How to Contribute", `end-icon-name external-link-symbolic`.
//!
//! GNOME only shows this "Community Built" card for free licenses, and a
//! "proprietary" variant otherwise. Onyx follows the same rule through
//! `AppMeta::is_free_license`.

use crate::i18n::t;
use adw::prelude::*;

use super::lozenge::{circular, Content};

/// The three badges of the community card (`.ui`: lozenge0/1/2).
const COMMUNITY_ICONS: [&str; 3] = [
    "community-supported-symbolic",
    "community-symbolic",
    "community-approved-symbolic",
];

/// "Community Built" card (free license).
pub fn community(license: Option<&str>, get_involved_url: Option<String>) -> gtk::ListBox {
    let description = match license {
        Some(l) => t(
            "This app is developed in the open by an international community, \
                      and released under the {} license.\n\nYou can participate and \
                      help make it even better.",
        )
        .replace("{}", l),
        None => t("This app is developed in the open by an international \
                   community.\n\nYou can participate and help make it even better."),
    };

    let icons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    icons.set_halign(gtk::Align::Center);
    for name in COMMUNITY_ICONS {
        icons.append(&circular(Content::Icon(name), "green"));
    }

    let list = boxed_list();
    list.append(&info_row(&icons, &t("Community Built"), &description));
    list.append(&get_involved_row(get_involved_url));
    list
}

/// Proprietary / unknown license variant: one lozenge, no "contribute".
pub fn proprietary(license: Option<&str>, is_free: Option<bool>) -> gtk::ListBox {
    let (icon, color, title) = match is_free {
        Some(false) => (
            "app-safety-unsafe-symbolic",
            "orange",
            t("Proprietary Software"),
        ),
        _ => ("app-safety-unknown-symbolic", "grey", t("Unknown License")),
    };
    // AppStream placeholders like LicenseRef-proprietary say nothing.
    let license = license.filter(|l| !l.starts_with("LicenseRef-"));
    let description = match (is_free, license) {
        (Some(false), Some(l)) => t("The source code of this app is not public. It is released \
                      under the {} license.")
        .replace("{}", l),
        (Some(false), None) => t("The source code of this app is not public."),
        (_, Some(l)) => t("This app is released under the {} license.").replace("{}", l),
        (_, None) => t("nixpkgs does not state the license of this package."),
    };

    let icons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    icons.set_halign(gtk::Align::Center);
    icons.append(&circular(Content::Icon(icon), color));

    let list = boxed_list();
    list.append(&info_row(&icons, &title, &description));
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

    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("heading");
    heading.set_justify(gtk::Justification::Center);
    heading.set_wrap(true);
    inner.append(&heading);

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
    row.set_title(&t("How to _Contribute"));
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
