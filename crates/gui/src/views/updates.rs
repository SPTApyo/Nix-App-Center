//! Onglet « Mises à jour » : compare les versions gardées (manifeste) à celles de
//! nixpkgs courant via `core::available_updates()` (lancé en thread, c'est N appels
//! `nix search`). Skeleton pendant le chargement ; illustration « Tout est à jour »
//! si rien ; sinon une boxed-list + bouton « Tout mettre à jour » (`apply_config`).
//! Rechargé à chaque entrée dans l'onglet (depuis `app.rs`).

use adw::prelude::*;
use relm4::prelude::*;
use onyx_core::{Package, Update};

use crate::util;

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Loading,
    Loaded,
}

pub struct UpdatesModel {
    state: State,
    updates: Vec<Update>,
    applying: bool,
    status: Option<String>,
    content: gtk::Box,
}

#[derive(Debug)]
pub enum UpdatesMsg {
    /// (Re)lance la détection en arrière-plan.
    Reload,
    /// Résultat de la détection.
    Loaded(Vec<Update>),
    /// L'utilisateur a cliqué « Tout mettre à jour » → `home-manager switch`.
    ApplyAll,
    /// Résultat du switch (Ok = dernière ligne, Err = message).
    Applied(Result<onyx_core::ApplyOutcome, String>),
    /// Clic sur une ligne → ouvrir le détail.
    Open(Package),
}

#[relm4::component(pub)]
impl SimpleComponent for UpdatesModel {
    type Init = ();
    type Input = UpdatesMsg;
    type Output = Package; // ligne ouverte → AppModel pousse le détail

    view! {
        #[root]
        gtk::ScrolledWindow {
            set_hscrollbar_policy: gtk::PolicyType::Never,
            set_vexpand: true,
            #[name(clamp)]
            adw::Clamp {
                set_maximum_size: 860,
                set_tightening_threshold: 576,
                set_margin_top: 24,
                set_margin_bottom: 24,
                set_margin_start: 12,
                set_margin_end: 12,
            }
        }
    }

    fn init(_: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .vexpand(true)
            .build();

        let model = UpdatesModel {
            state: State::Loading,
            updates: Vec::new(),
            applying: false,
            status: None,
            content: content.clone(),
        };
        let widgets = view_output!();
        widgets.clamp.set_child(Some(&content));

        model.rebuild(&sender);
        sender.input(UpdatesMsg::Reload);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            UpdatesMsg::Reload => {
                self.state = State::Loading;
                self.status = None;
                self.rebuild(&sender);
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let _ = s.send(UpdatesMsg::Loaded(onyx_core::available_updates()));
                });
            }
            UpdatesMsg::Loaded(updates) => {
                self.updates = updates;
                self.state = State::Loaded;
                self.rebuild(&sender);
            }
            UpdatesMsg::ApplyAll => {
                if self.applying {
                    return;
                }
                self.applying = true;
                self.status = Some("Application de la config…".to_string());
                self.rebuild(&sender);
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let res = onyx_core::apply_config().map_err(|e| e.to_string());
                    let _ = s.send(UpdatesMsg::Applied(res));
                });
            }
            UpdatesMsg::Applied(res) => {
                self.applying = false;
                self.status = Some(crate::util::describe_apply(res));
                // Re-détecte après application (les versions ont pu changer).
                self.rebuild(&sender);
                sender.input(UpdatesMsg::Reload);
            }
            UpdatesMsg::Open(pkg) => {
                let _ = sender.output(pkg);
            }
        }
    }
}

impl UpdatesModel {
    /// Vide `content` et le re-peuple selon l'état courant.
    fn rebuild(&self, sender: &ComponentSender<Self>) {
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }

        if let Some(status) = &self.status {
            let lbl = gtk::Label::new(Some(status));
            lbl.add_css_class("dim-label");
            lbl.set_wrap(true);
            lbl.set_xalign(0.0);
            self.content.append(&lbl);
        }

        match self.state {
            State::Loading => {
                self.content.append(&util::skeleton_list(4));
            }
            State::Loaded if self.updates.is_empty() => {
                self.content.append(&up_to_date_view());
            }
            State::Loaded => {
                self.content.append(&self.header(sender));
                self.content.append(&self.list(sender));
            }
        }
    }

    fn header(&self, sender: &ComponentSender<Self>) -> gtk::Box {
        let n = self.updates.len();
        let title = gtk::Label::new(Some(&format!(
            "{n} mise{} à jour disponible{}",
            if n > 1 { "s" } else { "" },
            if n > 1 { "s" } else { "" }
        )));
        title.add_css_class("title-2");
        title.set_xalign(0.0);
        title.set_hexpand(true);

        let btn = gtk::Button::with_label("Tout mettre à jour");
        btn.add_css_class("suggested-action");
        btn.set_valign(gtk::Align::Center);
        btn.set_sensitive(!self.applying);
        btn.connect_clicked({
            let s = sender.input_sender().clone();
            move |_| {
                let _ = s.send(UpdatesMsg::ApplyAll);
            }
        });

        let hb = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();
        hb.append(&title);
        hb.append(&btn);
        hb
    }

    fn list(&self, sender: &ComponentSender<Self>) -> gtk::ListBox {
        let lb = gtk::ListBox::new();
        lb.add_css_class("boxed-list");
        lb.set_selection_mode(gtk::SelectionMode::None);
        lb.set_valign(gtk::Align::Start);

        for u in &self.updates {
            let row = adw::ActionRow::builder()
                .title(util::esc(&u.package.attr))
                .subtitle(util::esc(&format!(
                    "{} → {}",
                    u.installed_version, u.available_version
                )))
                .activatable(true)
                .build();
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            lb.append(&row);
        }

        // row-activated → ouvrir le détail du paquet correspondant (par index).
        let updates: Vec<Package> = self.updates.iter().map(|u| u.package.clone()).collect();
        let s = sender.input_sender().clone();
        lb.connect_row_activated(move |_, row| {
            if let Some(pkg) = updates.get(row.index() as usize) {
                let _ = s.send(UpdatesMsg::Open(pkg.clone()));
            }
        });
        lb
    }
}

/// Illustration « Tout est à jour » (empruntée à GNOME Software), centrée.
fn up_to_date_view() -> gtk::Box {
    let pic = gtk::Picture::for_filename(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/data/assets/up-to-date.svg"
    ));
    pic.set_size_request(256, 192);
    pic.add_css_class("icon-dropshadow");
    let title = gtk::Label::new(Some("Tout est à jour"));
    title.add_css_class("title-1");

    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .spacing(18)
        .build();
    b.append(&pic);
    b.append(&title);
    b
}
