//! Galerie de développement (feature `dev-gallery`) — onglet listant des
//! scénarios réels étiquetés qui ouvrent les vraies vues recherche/détail, plus
//! l'accueil réel intégré. Harnais visuel : voir chaque type d'app côte à côte
//! (cliquable ou non, avec/sans icône, sections conditionnelles) pour éviter les
//! erreurs d'inattention. Jamais compilé en release.

use adw::prelude::*;
use relm4::prelude::*;
use onyx_core::Package;

use crate::views::home::{HomeModel, HomeOutput};

/// (label, requête réelle, comportement attendu à vérifier).
const SEARCH_SCENARIOS: &[(&str, &str, &str)] = &[
    ("App avec icône + note", "firefox",
     "Section « Applications », icône, étoiles, cliquable"),
    ("Mix reconnu / non-reconnu", "editor",
     "« Applications » (avec icônes) ET « Autres paquets »"),
    ("Filtre jeu de paquets", "aeson",
     "Menu déroulant « haskellPackages » apparaît"),
    ("Fournit une commande", "rg",
     "Section « Fournit la commande rg » (nix-locate)"),
    ("Zéro résultat", "zzqxnonexistent",
     "Page « Aucun résultat »"),
];

/// (label, attr réel, comportement attendu à vérifier).
const DETAIL_SCENARIOS: &[(&str, &str, &str)] = &[
    ("Fiche complète", "steam",
     "Screenshots ; Extensions = grille NON cliquable ; Autres applications = grille CLIQUABLE ; tuile Âge ; licence propriétaire"),
    ("Libre + avis", "firefox",
     "Tuile « Logiciel libre » ; histogramme d'avis ; historique des versions"),
    ("Nix pur runnable", "ripgrep",
     "Peu de métadonnées ; bouton Lancer ; pas de screenshots"),
    ("Keep-only / sans méta", "zlib",
     "Pas de mainProgram ; carte « Aucune métadonnée »"),
    // python27 est vérifié meta.insecure=true ; même rendu (tuile rouge +
    // bandeau problèmes) que broken. À rafraîchir s'il cesse d'être insecure.
    ("Vulnérable (insecure)", "python27",
     "Tuile rouge « Vulnérable » + bandeau problèmes"),
];

pub struct GalleryModel {
    // Gardé vivant : l'accueil réel est intégré dans la galerie.
    #[allow(dead_code)]
    home: Controller<HomeModel>,
}

#[derive(Debug)]
pub enum GalleryMsg {
    /// Clic de tuile dans l'accueil intégré → re-émis vers le parent.
    HomeOpen(Package),
    /// Clic de carte-catégorie dans l'accueil intégré → re-émis vers le parent.
    HomeCategory(&'static [&'static str], &'static str),
}

#[derive(Debug)]
pub enum GalleryOutput {
    /// Lance une recherche réelle pré-remplie (→ AppMsg::Search).
    Search(String),
    /// Ouvre la vraie fiche détail d'un attr (→ AppMsg::OpenDetail).
    OpenDetail(Package),
    /// Ouvre une page catégorie (→ AppMsg::OpenCategory), depuis l'accueil intégré.
    Category(&'static [&'static str], &'static str),
}

#[relm4::component(pub)]
impl SimpleComponent for GalleryModel {
    type Init = ();
    type Input = GalleryMsg;
    type Output = GalleryOutput;

    view! {
        #[root]
        gtk::ScrolledWindow {
            set_vexpand: true,
            set_hscrollbar_policy: gtk::PolicyType::Never,
        }
    }

    fn init(_: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        // L'accueil réel, intégré : ses sorties sont re-routées vers le parent.
        let home = HomeModel::builder()
            .launch(())
            .forward(sender.input_sender(), |o| match o {
                HomeOutput::Open(p) => GalleryMsg::HomeOpen(p),
                HomeOutput::Category(c, l) => GalleryMsg::HomeCategory(c, l),
            });

        let content = build_index(&sender, home.widget());

        let clamp = adw::Clamp::new();
        clamp.set_maximum_size(1000);
        clamp.set_tightening_threshold(600);
        clamp.set_margin_top(24);
        clamp.set_margin_bottom(24);
        clamp.set_margin_start(12);
        clamp.set_margin_end(12);
        clamp.set_child(Some(&content));
        root.set_child(Some(&clamp));

        let model = GalleryModel { home };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            GalleryMsg::HomeOpen(p) => {
                let _ = sender.output(GalleryOutput::OpenDetail(p));
            }
            GalleryMsg::HomeCategory(c, l) => {
                let _ = sender.output(GalleryOutput::Category(c, l));
            }
        }
    }
}

/// Construit l'index : trois sections (Recherche / Détail / Accueil).
/// `home_widget` = la racine de `HomeModel`, un `gtk::ScrolledWindow`.
fn build_index(
    sender: &ComponentSender<GalleryModel>,
    home_widget: &gtk::ScrolledWindow,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);

    // ── Recherche ─────────────────────────────────────────────────────────
    content.append(&heading("Recherche"));
    let search_list = boxed_list();
    for (label, query, expected) in SEARCH_SCENARIOS {
        let row = scenario_row(label, expected);
        let s = sender.clone();
        let q = query.to_string();
        row.connect_activated(move |_| {
            let _ = s.output(GalleryOutput::Search(q.clone()));
        });
        search_list.append(&row);
    }
    content.append(&search_list);

    // ── Détail ────────────────────────────────────────────────────────────
    content.append(&heading("Détail"));
    let detail_list = boxed_list();
    for (label, attr, expected) in DETAIL_SCENARIOS {
        let row = scenario_row(label, expected);
        let s = sender.clone();
        let attr = attr.to_string();
        row.connect_activated(move |_| {
            let _ = s.output(GalleryOutput::OpenDetail(Package::from_attr(attr.clone())));
        });
        detail_list.append(&row);
    }
    content.append(&detail_list);

    // ── Accueil (intégré) ─────────────────────────────────────────────────
    content.append(&heading("Accueil"));
    let note = gtk::Label::new(Some(
        "États d'icône à vérifier : chargement (shimmer) → repli coloré → icône réelle.",
    ));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    content.append(&note);
    // L'accueil est lui-même un ScrolledWindow (vexpand) : borné en hauteur et
    // vexpand désactivé, il devient un panneau à hauteur fixe dans le scroll de
    // la galerie (sinon deux scrollers imbriqués se disputent la hauteur).
    home_widget.set_vexpand(false);
    home_widget.set_height_request(700);
    content.append(home_widget);

    content
}

fn heading(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.add_css_class("title-2");
    l
}

fn boxed_list() -> gtk::ListBox {
    let lb = gtk::ListBox::new();
    lb.add_css_class("boxed-list");
    lb.set_selection_mode(gtk::SelectionMode::None);
    lb.set_valign(gtk::Align::Start);
    lb
}

/// Une ligne de scénario : titre + sous-titre = comportement attendu.
fn scenario_row(label: &str, expected: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(label)
        .subtitle(expected)
        .activatable(true)
        .build();
    row.set_subtitle_lines(0); // pas de troncature du comportement attendu
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row
}
