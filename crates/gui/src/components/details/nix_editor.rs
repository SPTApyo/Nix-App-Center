//! Editor of an app's own Nix, user and system scopes. Syntax is checked
//! while typing; a full dry build runs on demand.

use crate::i18n::t;
use adw::prelude::*;
use gtk::glib;
use nix_app_center_core::Package;
use std::cell::RefCell;
use std::rc::Rc;

/// The saved blocks; `None` for an empty scope.
pub struct Blocks {
    pub home: Option<String>,
    pub system: Option<String>,
}

fn text_of(buf: &gtk::TextBuffer) -> String {
    buf.text(&buf.start_iter(), &buf.end_iter(), false)
        .to_string()
}

/// The buffer text, `None` when blank.
fn block_or_none(buf: &gtk::TextBuffer) -> Option<String> {
    let s = text_of(buf);
    (!s.trim().is_empty()).then_some(s)
}

/// A code area with a hint while empty.
fn editor_view(buf: &gtk::TextBuffer, hint: &str) -> gtk::Overlay {
    let view = gtk::TextView::with_buffer(buf);
    view.set_monospace(true);
    view.set_top_margin(8);
    view.set_bottom_margin(8);
    view.set_left_margin(8);
    view.set_right_margin(8);

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_child(Some(&view));
    scroller.set_vexpand(true);
    scroller.set_hexpand(true);
    scroller.add_css_class("card");

    let placeholder = gtk::Label::new(Some(hint));
    placeholder.add_css_class("dim-label");
    placeholder.add_css_class("monospace");
    placeholder.set_wrap(true);
    placeholder.set_xalign(0.0);
    placeholder.set_halign(gtk::Align::Start);
    placeholder.set_valign(gtk::Align::Start);
    placeholder.set_margin_top(9);
    placeholder.set_margin_start(10);
    placeholder.set_margin_end(10);
    placeholder.set_can_target(false);
    placeholder.set_visible(buf.char_count() == 0);
    buf.connect_changed({
        let placeholder = placeholder.clone();
        move |b| placeholder.set_visible(b.char_count() == 0)
    });

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&scroller));
    overlay.add_overlay(&placeholder);
    overlay
}

/// Opens the editor; `on_save` gets the blocks once both parse.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    package: &Package,
    home_nix: Option<String>,
    system_nix: Option<String>,
    system_available: bool,
    on_save: impl Fn(Blocks) + 'static,
) {
    let home_buf = gtk::TextBuffer::new(None);
    home_buf.set_text(&home_nix.unwrap_or_default());
    let system_buf = gtk::TextBuffer::new(None);
    system_buf.set_text(&system_nix.unwrap_or_default());

    let stack = adw::ViewStack::new();
    let home_page = stack.add_titled(
        &editor_view(
            &home_buf,
            &t("Nothing for your user yet. Packages and user options for this app go here."),
        ),
        Some("home"),
        &t("User"),
    );
    home_page.set_icon_name(Some("user-home-symbolic"));

    if system_available {
        let hint = t("No system options for this app yet. NixOS options go here, for example:\n\nprograms.{0}.enable = true;\nnetworking.firewall.allowedTCPPorts = [ 8080 ];")
            .replace("{0}", package.attr.rsplit('.').next().unwrap_or(&package.attr));
        let page = stack.add_titled(
            &editor_view(&system_buf, &hint),
            Some("system"),
            &t("System"),
        );
        page.set_icon_name(Some("computer-symbolic"));
    } else {
        let msg = adw::StatusPage::new();
        msg.set_icon_name(Some("dialog-information-symbolic"));
        msg.set_title(&t("System Scope Unavailable"));
        msg.set_description(Some(&t(
            "Your configuration uses standalone home-manager: Nix App Center cannot write NixOS \
             options there (firewall, system services).",
        )));
        let page = stack.add_titled(&msg, Some("system"), &t("System"));
        page.set_icon_name(Some("computer-symbolic"));
    }

    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_selectable(true);
    status.add_css_class("caption");

    let save = gtk::Button::with_mnemonic(&t("_Save"));
    save.add_css_class("suggested-action");

    let verify = gtk::Button::with_mnemonic(&t("C_heck"));
    verify.set_tooltip_text(Some(&t(
        "Evaluates the whole configuration (dry-build) without applying anything. May take 1 to 2 minutes.",
    )));

    let cancel = gtk::Button::with_mnemonic(&t("_Cancel"));

    let recheck = {
        let status = status.clone();
        let save = save.clone();
        let home_buf = home_buf.clone();
        let system_buf = system_buf.clone();
        Rc::new(move || {
            let mut errs = Vec::new();
            for (scope, buf) in [
                (t("User scope:"), &home_buf),
                (t("System scope:"), &system_buf),
            ] {
                if let Some(block) = block_or_none(buf) {
                    if let Err(e) = nix_app_center_core::parse_block(&block) {
                        errs.push(format!("{scope}\n{e}"));
                    }
                }
            }
            if errs.is_empty() {
                status.set_label(&t("✓ Valid syntax"));
                status.remove_css_class("error");
                save.set_sensitive(true);
            } else {
                status.set_label(&errs.join("\n\n"));
                status.add_css_class("error");
                save.set_sensitive(false);
            }
        })
    };

    // Debounced: one check per pause.
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    for buf in [&home_buf, &system_buf] {
        let recheck = recheck.clone();
        let pending = pending.clone();
        buf.connect_changed(move |_| {
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
            let recheck = recheck.clone();
            let pending2 = pending.clone();
            let id =
                glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                    pending2.borrow_mut().take();
                    recheck();
                });
            *pending.borrow_mut() = Some(id);
        });
    }
    recheck();

    let switcher = adw::ViewSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.set_policy(adw::ViewSwitcherPolicy::Wide);

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&switcher));
    header.pack_start(&cancel);
    header.pack_end(&save);
    header.pack_end(&verify);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&stack);
    content.append(&status);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));

    let dialog = adw::Dialog::new();
    dialog.set_title(&t("Advanced Options: {}").replace("{}", &package.name));
    dialog.set_content_width(720);
    dialog.set_content_height(560);
    dialog.set_child(Some(&toolbar));

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
        });
    }

    {
        let status = status.clone();
        let verify_btn = verify.clone();
        verify.connect_clicked(move |_| {
            verify_btn.set_sensitive(false);
            status.remove_css_class("error");
            status.set_label(&t("⏳ Checking the configuration (dry-build)…"));

            let (tx, rx) = async_channel::bounded(1);
            std::thread::spawn(move || {
                let _ = tx.send_blocking(nix_app_center_core::dry_build());
            });

            let status = status.clone();
            let verify_btn = verify_btn.clone();
            glib::spawn_future_local(async move {
                if let Ok(res) = rx.recv().await {
                    match res {
                        nix_app_center_core::DryRun::Ok => {
                            status.remove_css_class("error");
                            status.set_label(&t(
                                "✓ The whole configuration evaluates, options and types included",
                            ));
                        }
                        nix_app_center_core::DryRun::Failed(e) => {
                            status.add_css_class("error");
                            status.set_label(&e);
                        }
                        nix_app_center_core::DryRun::Unavailable(e) => {
                            status.set_label(&t("Cannot check: {}").replace("{}", &t(&e)));
                        }
                    }
                }
                verify_btn.set_sensitive(true);
            });
        });
    }

    {
        let dialog = dialog.clone();
        save.connect_clicked(move |_| {
            on_save(Blocks {
                home: block_or_none(&home_buf),
                system: block_or_none(&system_buf),
            });
            dialog.close();
        });
    }

    dialog.present(Some(parent));
}
