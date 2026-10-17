//! Picks the version of an app: each one is held at the nixpkgs revision
//! that shipped it.

use adw::prelude::*;
use nix_app_center_core::VersionChoice;

use crate::i18n::t;
use crate::util::esc;

/// What the user picked.
pub enum Pick {
    Version(VersionChoice),
    /// Back to the version the configuration gives.
    Follow,
    /// Hold the version installed now.
    Installed,
}

/// Lists `choices`, marking the installed version and the one at
/// `held_rev`; `on_pick` gets the choice.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    choices: Vec<VersionChoice>,
    held_rev: Option<String>,
    installed: Option<String>,
    on_pick: impl Fn(Pick) + 'static,
) {
    let dialog = adw::Dialog::builder()
        .title(t("Choose a Version"))
        .content_width(520)
        .content_height(560)
        .build();
    let on_pick = std::rc::Rc::new(on_pick);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_top(12);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);
    let note = crate::util::dim_label(&t(
        "An earlier version comes from an older nixpkgs and gets no security updates.",
    ));
    body.append(&note);

    let list = crate::util::boxed_list();
    let (label, pick) = if held_rev.is_some() {
        (t("_Resume Updates"), Pick::Follow)
    } else {
        (t("_Stop Updates at This Version"), Pick::Installed)
    };
    let top = adw::ButtonRow::builder()
        .title(label)
        .use_underline(true)
        .build();
    let pick = std::cell::Cell::new(Some(pick));
    top.connect_activated({
        let (dialog, on_pick) = (dialog.clone(), on_pick.clone());
        move |_| {
            dialog.close();
            if let Some(p) = pick.take() {
                on_pick(p);
            }
        }
    });
    list.append(&top);
    for choice in choices {
        let source = if choice.cached {
            t("Ready to download")
        } else {
            t("Builds on your computer")
        };
        let row = adw::ActionRow::builder()
            .title(esc(&choice.version))
            .subtitle(esc(&format!("{} · {source}", choice.date)))
            .activatable(true)
            .build();
        if installed.as_deref() == Some(choice.version.as_str()) {
            let badge = gtk::Label::new(Some(&crate::i18n::pgettext("version badge", "Installed")));
            badge.add_css_class("installed-badge");
            badge.set_valign(gtk::Align::Center);
            row.add_suffix(&badge);
        } else if held_rev.as_deref() == Some(choice.rev.as_str()) {
            let current = gtk::Label::new(Some(&t("In use")));
            current.add_css_class("dim-label");
            row.add_suffix(&current);
        }
        row.connect_activated({
            let (dialog, on_pick) = (dialog.clone(), on_pick.clone());
            move |_| {
                dialog.close();
                on_pick(Pick::Version(choice.clone()));
            }
        });
        list.append(&row);
    }
    body.append(&list);

    let scroller = gtk::ScrolledWindow::builder()
        .child(&body)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroller));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}
