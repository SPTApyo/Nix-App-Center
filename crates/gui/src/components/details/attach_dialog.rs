//! The attach screen: the only place where Onyx offers to edit a file the user
//! wrote.
//!
//! Hard rule: **the exact diff is shown, and nothing is written without an
//! explicit click.** No silent magic: this is someone else's configuration.
//!
//! There is always a way out: "Do It Myself" closes the dialog and copies the
//! lines to paste. Onyx never makes itself indispensable.

use crate::i18n::t;
use adw::prelude::*;
use onyx_core::AttachPlan;

fn code_block(text: &str) -> gtk::ScrolledWindow {
    let view = gtk::TextView::new();
    view.set_editable(false);
    view.set_monospace(true);
    view.set_top_margin(8);
    view.set_bottom_margin(8);
    view.set_left_margin(8);
    view.set_right_margin(8);
    view.buffer().set_text(text);

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_child(Some(&view));
    scroller.set_vexpand(true);
    scroller.add_css_class("card");
    scroller
}

/// Offers to attach. `on_attached` is only called when the user accepted AND the
/// write succeeded.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    plan: AttachPlan,
    on_attached: impl Fn(Result<(), String>) + 'static,
) {
    let (explanation, body, can_patch) = match &plan {
        AttachPlan::Patch {
            site, backup, diff, ..
        } => (
            t(
                "Nix Software wrote its own files, but nothing imports them yet, so your kept \
               apps have no real effect.\n\n\
               To fix this, Nix Software needs to add the lines below to\n{site}\n\
               A backup will be written to\n{backup}\n\n\
               This is the only change Nix Software will ever make to your files.",
            )
            .replace("{site}", &site.display().to_string())
            .replace("{backup}", &backup.display().to_string()),
            diff.clone(),
            true,
        ),
        AttachPlan::Manual { site, snippet } => (
            t(
                "Nix Software cannot find a top-level “imports” list in\n{site}\n\n\
               It will not guess where to write in your file. Add these lines \
               yourself, wherever you see fit.",
            )
            .replace("{site}", &site.display().to_string()),
            snippet.clone(),
            false,
        ),
        AttachPlan::AlreadyAttached => (
            t("Nix Software is already connected to your configuration. Nothing to do."),
            String::new(),
            false,
        ),
        AttachPlan::NoLayout => (
            t(
                "Nix Software cannot find your Nix configuration (neither /etc/nixos nor \
               ~/.config/home-manager). It will not guess: your apps stay in the \
               manifest, but no file is generated.",
            ),
            String::new(),
            false,
        ),
    };

    let label = gtk::Label::new(Some(&explanation));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_selectable(true);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&label);
    if !body.is_empty() {
        content.append(&code_block(&body));
    }

    let header = adw::HeaderBar::new();
    let dialog = adw::Dialog::new();
    dialog.set_title(&t("Connect Nix Software to Your Configuration"));
    dialog.set_content_width(760);
    dialog.set_content_height(560);

    let close = gtk::Button::with_label(&if can_patch {
        t("Do It Myself")
    } else {
        t("Close")
    });
    header.pack_start(&close);
    {
        let dialog = dialog.clone();
        let body = body.clone();
        close.connect_clicked(move |_| {
            // Always a way out: lines go to the clipboard.
            if !body.is_empty() {
                crate::util::copy_to_clipboard(&body);
            }
            dialog.close();
        });
    }

    if can_patch {
        let accept = gtk::Button::with_label(&t("Add Import Line"));
        accept.add_css_class("suggested-action");
        header.pack_end(&accept);

        let dialog2 = dialog.clone();
        accept.connect_clicked(move |_| {
            let res = onyx_core::attach_commit(&plan).map_err(|e| e.to_string());
            on_attached(res);
            dialog2.close();
        });
    }

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}
