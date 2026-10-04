//! L'écran de greffe : le seul endroit où Onyx propose de modifier un fichier que
//! l'utilisateur a écrit.
//!
//! Règle absolue : **le diff exact est montré, et rien n'est écrit sans un clic
//! explicite.** Pas de magie silencieuse — c'est la config de quelqu'un d'autre.
//!
//! Il y a toujours une porte de sortie : « Faire moi-même » ferme le dialogue et copie
//! la ligne à coller. Onyx ne se rend jamais indispensable.

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

/// Propose la greffe. `on_attached` n'est appelé que si l'utilisateur a validé ET que
/// l'écriture a réussi.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    plan: AttachPlan,
    on_attached: impl Fn(Result<(), String>) + 'static,
) {
    let (explication, corps, peut_patcher) = match &plan {
        AttachPlan::Patch { site, backup, diff, .. } => (
            format!(
                "Onyx a écrit ses propres fichiers, mais rien ne les importe encore : \
                 tes apps gardées n'ont donc aucun effet réel.\n\n\
                 Pour y remédier, il doit ajouter les lignes ci-dessous à\n{}\n\
                 Une sauvegarde sera écrite dans\n{}\n\n\
                 C'est la seule modification qu'Onyx fera jamais à tes fichiers.",
                site.display(),
                backup.display()
            ),
            diff.clone(),
            true,
        ),
        AttachPlan::Manual { site, snippet } => (
            format!(
                "Onyx ne trouve pas de liste « imports » au plus haut niveau de\n{}\n\n\
                 Il refuse de deviner où écrire dans ton fichier. Ajoute ces lignes \
                 toi-même, où tu le juges bon.",
                site.display()
            ),
            snippet.clone(),
            false,
        ),
        AttachPlan::AlreadyAttached => (
            "Onyx est déjà greffé sur ta config : il n'y a rien à faire.".to_string(),
            String::new(),
            false,
        ),
        AttachPlan::NoLayout => (
            "Onyx ne trouve pas ta config Nix (ni /etc/nixos, ni ~/.config/home-manager). \
             Il ne devinera pas : tes apps restent dans le manifeste, mais aucun fichier \
             n'est généré."
                .to_string(),
            String::new(),
            false,
        ),
    };

    let label = gtk::Label::new(Some(&explication));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_selectable(true);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&label);
    if !corps.is_empty() {
        content.append(&code_block(&corps));
    }

    let header = adw::HeaderBar::new();
    let dialog = adw::Dialog::new();
    dialog.set_title("Greffer Onyx sur ta config");
    dialog.set_content_width(760);
    dialog.set_content_height(560);

    let close = gtk::Button::with_label(if peut_patcher { "Faire moi-même" } else { "Fermer" });
    header.pack_start(&close);
    {
        let dialog = dialog.clone();
        let corps = corps.clone();
        close.connect_clicked(move |_| {
            // Toujours une porte de sortie : la ligne part dans le presse-papier.
            if !corps.is_empty() {
                crate::util::copy_to_clipboard(&corps);
            }
            dialog.close();
        });
    }

    if peut_patcher {
        let accept = gtk::Button::with_label("Ajouter la ligne d'import");
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
