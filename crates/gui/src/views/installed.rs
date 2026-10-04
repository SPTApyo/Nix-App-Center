//! Onglet « Installées » — deux sections dans un même scroll.
//!
//! • « Dans ma config » : paquets gardés (`kept.json`) — la config home-manager.
//! • « Essayées »       : historique des `nix run` (`tried.json`) + bouton GC.
//!
//! Rechargé à l'activation de l'onglet via `InstalledMsg::Reload`.
//! Flux : skeleton → fetch AppStream parallèle (cached) → DataReady → tuiles.

use adw::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;
use onyx_core::Package;

use crate::components::AppTile;
use crate::util::{skeleton_grid, AppItem};

pub struct InstalledModel {
    kept_tiles: FactoryVecDeque<AppTile>,
    tried_tiles: FactoryVecDeque<AppTile>,
    kept_empty: gtk::Label,
    tried_empty: gtk::Label,
    gc_btn: gtk::Button,
    gc_label: gtk::Label,
    gc_running: bool,
    stack: gtk::Stack,
}

#[derive(Debug)]
pub enum InstalledMsg {
    Reload,
    DataReady(Vec<AppItem>, Vec<AppItem>), // (kept, tried) avec icon_url rempli
    Open(Package),
    Gc,
    GcDone(String),
}

#[relm4::component(pub)]
impl SimpleComponent for InstalledModel {
    type Init = ();
    type Input = InstalledMsg;
    type Output = Package;

    view! {
        #[root]
        gtk::ScrolledWindow {
            set_hscrollbar_policy: gtk::PolicyType::Never,
            set_vexpand: true,
        }
    }

    fn init(_: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        // ── Grilles (FactoryVecDeque branchées sur un FlowBox) ────────────────
        let kept_tiles = FactoryVecDeque::builder()
            .launch(gtk::FlowBox::new())
            .forward(sender.input_sender(), InstalledMsg::Open);
        configure_flow(kept_tiles.widget());

        let tried_tiles = FactoryVecDeque::builder()
            .launch(gtk::FlowBox::new())
            .forward(sender.input_sender(), InstalledMsg::Open);
        configure_flow(tried_tiles.widget());

        // ── Section « Dans ma config » ────────────────────────────────────────
        let kept_empty = gtk::Label::new(Some(
            "Aucun paquet gardé — ouvrez une page détail et cliquez « Ajouter à ma config ».",
        ));
        kept_empty.add_css_class("dim-label");
        kept_empty.set_xalign(0.0);
        kept_empty.set_wrap(true);
        kept_empty.set_visible(false);

        let kept_section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        kept_section.append(&section_heading("Dans ma config"));
        kept_section.append(kept_tiles.widget());
        kept_section.append(&kept_empty);

        // ── Section « Essayées » ──────────────────────────────────────────────
        let gc_btn = gtk::Button::with_label("Libérer l'espace");
        gc_btn.add_css_class("flat");
        gc_btn.set_valign(gtk::Align::Center);
        gc_btn.set_tooltip_text(Some(
            "nix-collect-garbage : supprime du store les paquets non gardés",
        ));
        gc_btn.connect_clicked({
            let s = sender.input_sender().clone();
            move |_| {
                let _ = s.send(InstalledMsg::Gc);
            }
        });

        let tried_header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        tried_header.append(&section_heading("Essayées"));
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        tried_header.append(&spacer);
        tried_header.append(&gc_btn);

        let tried_empty = gtk::Label::new(Some(
            "Aucun essai récent — lancez un paquet depuis la recherche pour qu'il apparaisse ici.",
        ));
        tried_empty.add_css_class("dim-label");
        tried_empty.set_xalign(0.0);
        tried_empty.set_wrap(true);
        tried_empty.set_visible(false);

        let gc_label = gtk::Label::new(None);
        gc_label.add_css_class("dim-label");
        gc_label.set_xalign(0.0);
        gc_label.set_visible(false);

        let tried_section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();
        tried_section.append(&tried_header);
        tried_section.append(tried_tiles.widget());
        tried_section.append(&tried_empty);
        tried_section.append(&gc_label);

        // ── Page « content » ─────────────────────────────────────────────────
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .margin_top(24)
            .margin_bottom(24)
            .build();
        content.append(&kept_section);
        content.append(&tried_section);

        let content_clamp = adw::Clamp::new();
        content_clamp.set_maximum_size(1000);
        content_clamp.set_tightening_threshold(600);
        content_clamp.set_margin_start(12);
        content_clamp.set_margin_end(12);
        content_clamp.set_child(Some(&content));

        // ── Page « loading » (skeleton calqué sur le contenu) ────────────────
        let loading_content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .margin_top(24)
            .margin_bottom(24)
            .build();
        loading_content.append(&skeleton_section(160)); // « Dans ma config »
        loading_content.append(&skeleton_section(120)); // « Essayées »

        let loading_clamp = adw::Clamp::new();
        loading_clamp.set_maximum_size(1000);
        loading_clamp.set_tightening_threshold(600);
        loading_clamp.set_margin_start(12);
        loading_clamp.set_margin_end(12);
        loading_clamp.set_child(Some(&loading_content));

        // ── Stack skeleton / content ──────────────────────────────────────────
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(150);
        stack.add_named(&loading_clamp, Some("loading"));
        stack.add_named(&content_clamp, Some("content"));
        stack.set_visible_child_name("loading");

        let model = InstalledModel {
            kept_tiles,
            tried_tiles,
            kept_empty: kept_empty.clone(),
            tried_empty: tried_empty.clone(),
            gc_btn: gc_btn.clone(),
            gc_label: gc_label.clone(),
            gc_running: false,
            stack: stack.clone(),
        };
        let widgets = view_output!();
        root.set_child(Some(&stack));
        crate::util::smooth_scroll(&root);

        sender.input(InstalledMsg::Reload);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            InstalledMsg::Reload => {
                self.stack.set_visible_child_name("loading");

                let kept_pkgs: Vec<Package> = onyx_core::load_manifest().packages();
                let tried_pkgs: Vec<Package> = onyx_core::load_tried()
                    .tried
                    .into_iter()
                    .map(|e| e.package)
                    .collect();

                let (target_dev_w, _) = crate::util::screen_metrics();
                let target_w = (target_dev_w.clamp(1280, 3840)) as u32;

                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let kept_items = fetch_items(kept_pkgs, target_w);
                    let tried_items = fetch_items(tried_pkgs, target_w);
                    let _ = s.send(InstalledMsg::DataReady(kept_items, tried_items));
                });
            }
            InstalledMsg::DataReady(kept_items, tried_items) => {
                let kept_empty = kept_items.is_empty();
                {
                    let mut g = self.kept_tiles.guard();
                    g.clear();
                    for item in kept_items {
                        g.push_back(item);
                    }
                }
                self.kept_tiles.widget().set_visible(!kept_empty);
                self.kept_empty.set_visible(kept_empty);

                let tried_empty = tried_items.is_empty();
                {
                    let mut g = self.tried_tiles.guard();
                    g.clear();
                    for item in tried_items {
                        g.push_back(item);
                    }
                }
                self.tried_tiles.widget().set_visible(!tried_empty);
                self.tried_empty.set_visible(tried_empty);

                self.stack.set_visible_child_name("content");
            }
            InstalledMsg::Open(pkg) => {
                let _ = sender.output(pkg);
            }
            InstalledMsg::Gc => {
                if self.gc_running {
                    return;
                }
                self.gc_running = true;
                self.gc_btn.set_sensitive(false);
                self.gc_label.set_label("Nettoyage du store en cours…");
                self.gc_label.set_visible(true);
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let msg = match onyx_core::garbage_collect() {
                        Ok(s) if !s.is_empty() => s,
                        Ok(_) => "Terminé.".to_string(),
                        Err(e) => format!("Échec : {e}"),
                    };
                    let _ = s.send(InstalledMsg::GcDone(msg));
                });
            }
            InstalledMsg::GcDone(status) => {
                self.gc_running = false;
                self.gc_btn.set_sensitive(true);
                self.gc_label.set_label(&status);
            }
        }
    }
}

/// Résout les `AppMeta` d'un lot de paquets avec un parallélisme borné (vagues
/// de `MAX_PAR` threads) : un historique fourni ne doit pas lancer des
/// centaines de threads, chacun pouvant faire du réseau + `nix eval`.
/// À appeler depuis un thread de fond, jamais du thread GTK.
fn fetch_items(pkgs: Vec<Package>, target_w: u32) -> Vec<AppItem> {
    const MAX_PAR: usize = 16;
    let mut out = Vec::with_capacity(pkgs.len());
    for chunk in pkgs.chunks(MAX_PAR) {
        let handles: Vec<_> = chunk
            .iter()
            .cloned()
            .map(|pkg| {
                std::thread::spawn(move || {
                    let meta = onyx_core::cached_app_meta(&pkg, target_w);
                    AppItem::new(pkg, meta.icon_url)
                })
            })
            .collect();
        out.extend(handles.into_iter().filter_map(|h| h.join().ok()));
    }
    out
}

fn configure_flow(flow: &gtk::FlowBox) {
    flow.set_homogeneous(true);
    flow.set_column_spacing(14);
    flow.set_row_spacing(14);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(3);
    flow.set_hexpand(true);
    flow.set_valign(gtk::Align::Start);
    flow.set_selection_mode(gtk::SelectionMode::None);
}

fn section_heading(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.add_css_class("title-2");
    l
}

/// Section skeleton : titre shimmer + grille de tuiles fantômes, calquée sur le
/// contenu réel (même heading height + même FlowBox config) pour éviter tout saut.
fn skeleton_section(heading_px: i32) -> gtk::Box {
    let sec = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    let h = gtk::Box::builder()
        .width_request(heading_px)
        .height_request(22)
        .halign(gtk::Align::Start)
        .build();
    h.add_css_class("skeleton");
    sec.append(&h);
    sec.append(&skeleton_grid(3));
    sec
}
