//! Consent to the binary cache an added flake declares: names its server
//! and keys, then writes them in the system file, or explains how to add
//! them without NixOS.

use crate::i18n::t;
use adw::prelude::*;
use nix_app_center_core::Cache;

/// Asks to use `cache` of `flake`; `done` gets the result of saving it.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    flake: &str,
    cache: Cache,
    done: impl Fn(std::io::Result<()>) + 'static,
) {
    let system = nix_app_center_core::detect_layout().system.is_some();
    let body = if system {
        t("Apps from {flake} then download from its server instead of building on this computer. Whoever runs that server can sign the programs you install: only use it if you trust {flake}.")
    } else {
        t("Apps from {flake} could download from its server instead of building. Without NixOS, only an administrator can add it, in /etc/nix/nix.conf:")
    }
    .replace("{flake}", flake);
    let dialog = adw::AlertDialog::new(
        Some(&t("Use the Binary Cache of {}?").replace("{}", flake)),
        Some(&body),
    );

    let details = if system {
        format!("{}\n{}", cache.url, cache.keys.join("\n"))
    } else {
        format!(
            "extra-substituters = {}\nextra-trusted-public-keys = {}",
            cache.url,
            cache.keys.join(" ")
        )
    };
    let label = gtk::Label::new(Some(&details));
    // Selected at once when focused, so only to copy.
    label.set_selectable(!system);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::Char);
    // A hyphen would read as part of the key.
    let attrs = gtk::pango::AttrList::new();
    attrs.insert(gtk::pango::AttrInt::new_insert_hyphens(false));
    label.set_attributes(Some(&attrs));
    label.set_xalign(0.0);
    label.set_margin_top(12);
    label.set_margin_bottom(12);
    label.set_margin_start(12);
    label.set_margin_end(12);
    label.add_css_class("monospace");
    label.add_css_class("caption");
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("card");
    card.append(&label);
    dialog.set_extra_child(Some(&card));

    if !system {
        dialog.add_response("close", &t("_Close"));
        dialog.present(Some(parent));
        return;
    }
    dialog.add_response("cancel", &t("_Cancel"));
    dialog.add_response("use", &t("_Use Cache"));
    dialog.set_response_appearance("use", adw::ResponseAppearance::Suggested);
    dialog.connect_response(Some("use"), move |_, _| {
        done(nix_app_center_core::trust_cache(&cache))
    });
    dialog.present(Some(parent));
}
