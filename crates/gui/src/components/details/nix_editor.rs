//! Le « chemin avancé » : éditer le Nix brut d'une app.
//!
//! Une app gardée n'est qu'un nom dans une liste `home.packages`. Ici, l'utilisateur
//! prend la main : il écrit le module qu'il veut (options de service, port, surcharge du
//! paquet…), pour la portée utilisateur et/ou système.
//!
//! Deux filets, l'un instantané et l'autre à la demande :
//! - la **syntaxe** est vérifiée à chaque frappe (~50 ms) et « Valider » reste
//!   désactivé tant qu'elle est fausse ;
//! - la **sémantique** (option inconnue, type faux) demande un `dry-build` complet, long
//!   (30 s–2 min) : c'est le bouton « Vérifier », déclenché par l'utilisateur, jamais
//!   imposé — l'imposer à chaque édition rendrait l'éditeur pénible.
//!
//! Onyx n'écrit **que** dans ses propres fichiers ; le Nix de l'utilisateur est repris
//! tel quel, jamais reformaté.

use adw::prelude::*;
use gtk::glib;
use onyx_core::Package;
use std::cell::RefCell;
use std::rc::Rc;

/// Ce que l'éditeur rend quand l'utilisateur valide : les deux blocs, `None` pour une
/// portée laissée vide (l'app redevient alors une simple entrée de liste).
pub struct Blocks {
    pub home: Option<String>,
    pub system: Option<String>,
}

fn text_of(buf: &gtk::TextBuffer) -> String {
    buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string()
}

/// Une portée vide n'est pas un bloc vide : c'est l'absence de bloc.
fn block_or_none(buf: &gtk::TextBuffer) -> Option<String> {
    let s = text_of(buf);
    (!s.trim().is_empty()).then_some(s)
}

fn editor_view(buf: &gtk::TextBuffer) -> gtk::ScrolledWindow {
    let view = gtk::TextView::with_buffer(buf);
    view.set_monospace(true);
    view.set_top_margin(8);
    view.set_bottom_margin(8);
    view.set_left_margin(8);
    view.set_right_margin(8);

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_child(Some(&view));
    scroller.set_vexpand(true);
    scroller.add_css_class("card");
    scroller
}

/// Ouvre l'éditeur. `on_save` reçoit les blocs validés — il n'est appelé que si la
/// syntaxe des deux portées est bonne.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    package: &Package,
    home_nix: Option<String>,
    system_nix: Option<String>,
    system_available: bool,
    on_save: impl Fn(Blocks) + 'static,
) {
    // Promouvoir ne doit rien faire disparaître en silence : à défaut de bloc, on part
    // de l'équivalent exact de ce que faisait l'entrée simple.
    let home_buf = gtk::TextBuffer::new(None);
    home_buf.set_text(
        &home_nix.unwrap_or_else(|| onyx_core::default_home_block(&package.attr)),
    );
    let system_buf = gtk::TextBuffer::new(None);
    system_buf.set_text(&system_nix.unwrap_or_default());

    let stack = adw::ViewStack::new();
    let home_page = stack.add_titled(&editor_view(&home_buf), Some("home"), "Utilisateur");
    home_page.set_icon_name(Some("user-home-symbolic"));

    if system_available {
        let page =
            stack.add_titled(&editor_view(&system_buf), Some("system"), "Système");
        page.set_icon_name(Some("computer-symbolic"));
    } else {
        // Un home-manager standalone n'a aucun moyen d'écrire du NixOS : mieux vaut le
        // dire que laisser croire que ça marchera.
        let msg = adw::StatusPage::new();
        msg.set_icon_name(Some("dialog-information-symbolic"));
        msg.set_title("Portée système indisponible");
        msg.set_description(Some(
            "Ta config est en home-manager seul : Onyx ne peut pas y écrire d'options \
             NixOS (pare-feu, services système).",
        ));
        let page = stack.add_titled(&msg, Some("system"), "Système");
        page.set_icon_name(Some("computer-symbolic"));
    }

    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_selectable(true);
    status.add_css_class("caption");

    let save = gtk::Button::with_label("Valider");
    save.add_css_class("suggested-action");

    let verify = gtk::Button::with_label("Vérifier");
    verify.set_tooltip_text(Some(
        "Évalue toute la config (dry-build) sans rien appliquer. Peut prendre 1–2 min.",
    ));

    let cancel = gtk::Button::with_label("Annuler");

    // Vérification de syntaxe à chaque frappe, sur les deux portées.
    let recheck = {
        let status = status.clone();
        let save = save.clone();
        let home_buf = home_buf.clone();
        let system_buf = system_buf.clone();
        Rc::new(move || {
            let mut errs = Vec::new();
            for (nom, buf) in [("utilisateur", &home_buf), ("système", &system_buf)] {
                if let Some(block) = block_or_none(buf) {
                    if let Err(e) = onyx_core::parse_block(&block) {
                        errs.push(format!("Portée {nom} :\n{e}"));
                    }
                }
            }
            if errs.is_empty() {
                status.set_label("✓ syntaxe valide");
                status.remove_css_class("error");
                save.set_sensitive(true);
            } else {
                status.set_label(&errs.join("\n\n"));
                status.add_css_class("error");
                save.set_sensitive(false);
            }
        })
    };

    // Anti-rebond : sans lui, on lance un `nix-instantiate` par caractère tapé.
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
            let id = glib::timeout_add_local_once(
                std::time::Duration::from_millis(300),
                move || {
                    pending2.borrow_mut().take();
                    recheck();
                },
            );
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
    dialog.set_title(&format!("Options avancées — {}", package.name));
    dialog.set_content_width(720);
    dialog.set_content_height(560);
    dialog.set_child(Some(&toolbar));

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| {
            dialog.close();
        });
    }

    // Le dry-build est long : il tourne hors du thread principal, sinon l'UI gèle.
    {
        let status = status.clone();
        let verify_btn = verify.clone();
        verify.connect_clicked(move |_| {
            verify_btn.set_sensitive(false);
            status.remove_css_class("error");
            status.set_label("⏳ Vérification de la config (dry-build)…");

            let (tx, rx) = async_channel::bounded(1);
            std::thread::spawn(move || {
                let _ = tx.send_blocking(onyx_core::dry_build());
            });

            let status = status.clone();
            let verify_btn = verify_btn.clone();
            glib::spawn_future_local(async move {
                if let Ok(res) = rx.recv().await {
                    match res {
                        onyx_core::DryRun::Ok => {
                            status.remove_css_class("error");
                            status.set_label("✓ la config entière évalue — options et types compris");
                        }
                        onyx_core::DryRun::Failed(e) => {
                            status.add_css_class("error");
                            status.set_label(&e);
                        }
                        onyx_core::DryRun::Unavailable(e) => {
                            status.set_label(&format!("Vérification impossible : {e}"));
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
