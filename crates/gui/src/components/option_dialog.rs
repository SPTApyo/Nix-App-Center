//! A NixOS option: its facts and a line to paste.

use adw::prelude::*;
use nix_software_core::NixOption;

use crate::i18n::t;
use crate::util::esc;

pub fn present(parent: &impl IsA<gtk::Widget>, option: &NixOption) {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_top(12);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);

    let name = gtk::Label::new(Some(&option.name));
    name.add_css_class("title-4");
    name.add_css_class("monospace");
    name.set_wrap(true);
    name.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    name.set_xalign(0.0);
    name.set_selectable(true);
    body.append(&name);

    let description = gtk::Label::new(Some(&option.description));
    description.set_wrap(true);
    description.set_xalign(0.0);
    description.set_selectable(true);
    body.append(&description);

    let facts = crate::util::boxed_list();
    for (title, value) in [
        (t("Type"), Some(option.type_.clone())),
        (t("Default"), option.default.clone()),
        (t("Example"), option.example.clone()),
        (t("Declared in"), option.declarations.first().cloned()),
    ] {
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            let row = adw::ActionRow::builder()
                .title(esc(&title))
                .subtitle(esc(&v))
                .subtitle_selectable(true)
                .build();
            row.add_css_class("property");
            facts.append(&row);
        }
    }
    body.append(&facts);

    let snippet = if option.type_ == nix_software_core::FLAKE_MODULE {
        option.snippet()
    } else {
        format!("{} = {};", option.name, proposed_value(option))
    };
    let code = gtk::Label::new(Some(&snippet));
    code.add_css_class("monospace");
    code.add_css_class("card");
    code.set_selectable(true);
    code.set_wrap(true);
    code.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    code.set_xalign(0.0);
    code.set_margin_top(6);
    let copy = gtk::Button::with_mnemonic(&t("_Copy"));
    copy.set_halign(gtk::Align::End);
    copy.connect_clicked(move |_| crate::util::copy_to_clipboard(&snippet));
    body.append(&code);
    body.append(&copy);

    if option.type_ == nix_software_core::FLAKE_MODULE {
        body.append(&flake_module_actions(option));
    }

    // Set it from here, in the files of Nix Software.
    let writable = option.type_ != nix_software_core::FLAKE_MODULE
        && if option.home {
            nix_software_core::detect_layout().home.is_some()
        } else {
            nix_software_core::detect_layout().system.is_some()
        };
    if writable {
        let current = nix_software_core::load_manifest().option_value(&option.name);
        let value = adw::EntryRow::builder()
            .title(t("Value"))
            .text(current.clone().unwrap_or_else(|| proposed_value(option)))
            .build();
        value.add_css_class("monospace");
        let list = crate::util::boxed_list();
        list.append(&value);
        let set = gtk::Button::with_mnemonic(&if current.is_some() {
            t("_Change in My Configuration")
        } else {
            t("_Set in My Configuration")
        });
        set.add_css_class("suggested-action");
        set.add_css_class("pill");
        let unset = gtk::Button::with_mnemonic(&t("_Stop Setting"));
        unset.add_css_class("pill");
        unset.set_visible(current.is_some());
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        buttons.set_halign(gtk::Align::Center);
        buttons.append(&unset);
        buttons.append(&set);
        let status = crate::util::dim_label("");
        status.set_wrap(true);
        let (name, home) = (option.name.clone(), option.home);
        set.connect_clicked({
            let (value, status, unset) = (value.clone(), status.clone(), unset.clone());
            let name = name.clone();
            move |_| {
                status.set_label(
                    &match nix_software_core::set_option(&name, &value.text(), home) {
                        Ok(()) => {
                            unset.set_visible(true);
                            t("Saved. Apply your configuration to use it.")
                        }
                        Err(e) => crate::util::failed(&e),
                    },
                );
            }
        });
        unset.connect_clicked({
            let status = status.clone();
            move |b| {
                status.set_label(&match nix_software_core::unset_option(&name) {
                    Ok(()) => {
                        b.set_visible(false);
                        t("Removed. Apply your configuration to undo it.")
                    }
                    Err(e) => crate::util::failed(&e),
                });
            }
        });
        body.append(&list);
        body.append(&buttons);
        body.append(&status);
    }

    let scroller = gtk::ScrolledWindow::builder()
        .child(&body)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroller));
    adw::Dialog::builder()
        .title(if option.home {
            t("Home Manager Option")
        } else {
            t("NixOS Option")
        })
        .content_width(560)
        .content_height(560)
        .child(&toolbar)
        .build()
        .present(Some(parent));
}

/// What setting the option most likely means: the opposite of a boolean's
/// default, else the example or default.
fn proposed_value(option: &NixOption) -> String {
    if option.type_ == "boolean" {
        return (option.default.as_deref() != Some("true")).to_string();
    }
    snippet_value(&option.snippet())
}

/// The value side of `name = value;`.
fn snippet_value(snippet: &str) -> String {
    snippet
        .split_once(" = ")
        .map(|(_, v)| v.trim_end_matches(';').to_string())
        .unwrap_or_default()
}

/// Import and remove buttons of a module a flake ships.
fn flake_module_actions(option: &NixOption) -> gtk::Box {
    let source = option.declarations.first().cloned().unwrap_or_default();
    let module = option
        .name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_string();
    let key = format!("{source}#{module}");
    let home = option.home;
    let imported = || {
        nix_software_core::load_manifest()
            .kept
            .iter()
            .any(|e| e.option && e.package.attr == key)
    };
    let import = gtk::Button::with_mnemonic(&t("_Import in My Configuration"));
    import.add_css_class("suggested-action");
    import.add_css_class("pill");
    let remove = gtk::Button::with_mnemonic(&t("_Stop Importing"));
    remove.add_css_class("pill");
    import.set_visible(!imported());
    remove.set_visible(imported());
    let status = crate::util::dim_label("");
    status.set_wrap(true);
    import.connect_clicked({
        let (remove, status, source, module) = (
            remove.clone(),
            status.clone(),
            source.clone(),
            module.clone(),
        );
        move |b| {
            status.set_label(
                &match nix_software_core::import_flake_module(&source, &module, home) {
                    Ok(()) => {
                        b.set_visible(false);
                        remove.set_visible(true);
                        t("Saved. Apply your configuration to use it.")
                    }
                    Err(e) => crate::util::failed(&e),
                },
            );
        }
    });
    remove.connect_clicked({
        let (import, status) = (import.clone(), status.clone());
        move |b| {
            status.set_label(&match nix_software_core::unset_option(&key) {
                Ok(()) => {
                    b.set_visible(false);
                    import.set_visible(true);
                    t("Removed. Apply your configuration to undo it.")
                }
                Err(e) => crate::util::failed(&e),
            });
        }
    });
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    buttons.set_halign(gtk::Align::Center);
    buttons.append(&remove);
    buttons.append(&import);
    let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
    column.append(&buttons);
    column.append(&status);
    column
}
