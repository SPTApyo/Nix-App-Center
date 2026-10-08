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

    let snippet = option.snippet();
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

    let scroller = gtk::ScrolledWindow::builder()
        .child(&body)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroller));
    adw::Dialog::builder()
        .title(t("NixOS Option"))
        .content_width(560)
        .content_height(560)
        .child(&toolbar)
        .build()
        .present(Some(parent));
}
