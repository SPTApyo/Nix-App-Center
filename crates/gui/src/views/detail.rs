//! Page détail — calque de `gs-details-page.ui` (GNOME Software).
//!
//! Ce fichier est un **assembleur** : il reproduit l'arbre du `.ui` et pilote le
//! chargement. Les widgets composites vivent dans `components::details`, un
//! module par widget GNOME (cf. `docs/superpowers/specs/2026-07-09-page-detail-gnome-1-1-design.md`).
//!
//! ```text
//! GtkStack
//! ├─ "spinner" → AdwStatusPage + AdwSpinnerPaintable
//! ├─ "ready"   → GtkBox vertical
//! │              ├─ AdwBanner (traduction — jamais révélée, pas de donnée Nix)
//! │              └─ GtkOverlay → GtkScrolledWindow → GtkBox .details-page (spacing 18)
//! │                 ├─ Clamp → en-tête (icône 128 + titre/dev/étoiles + actions)
//! │                 ├─ GsScreenshotCarousel  (PLEINE LARGEUR, hors clamp)
//! │                 ├─ Clamp → résumé (title-2) + description repliable
//! │                 ├─ Clamp → extensions
//! │                 ├─ Clamp → context bar
//! │                 ├─ Clamp → historique des versions
//! │                 ├─ Clamp → infobar « problèmes »
//! │                 ├─ Clamp → licence + liens (côte à côte)
//! │                 ├─ Clamp → avis (histogramme + avis vedette)
//! │                 └─ Clamp → autres applis du développeur
//! └─ "failed"  → AdwStatusPage dialog-error-symbolic
//! ```
//!
//! Un `GtkSizeGroup` horizontal lie toutes les sections, comme le `.ui` : c'est
//! lui qui garantit qu'elles ont la même largeur quel que soit leur contenu.
//!
//! **Sémantique Onyx** sous la disposition GNOME : l'action primaire reste
//! `nix run` (« Lancer »), l'ajout permanent est secondaire. Les tuiles de la
//! context bar portent des signaux Nix (taille de fermeture, `broken`/`insecure`).
//! Divergences assumées, cf. spec §1.

use adw::prelude::*;
use onyx_core::{AppMeta, Package, Rating, Review};
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;

use crate::components::details::{
    attach_dialog, context_bar, infobox, license_tile, lozenge::Content, nix_editor,
    review_histogram, review_row, screenshot_carousel::ScreenshotCarousel, version_history_row,
};
use crate::components::AppTile;
use crate::util::{
    esc, fallback_class, stars_widget, texture_from_bytes, texture_from_bytes_sized, AppItem,
    FALLBACK_GLYPH,
};

/// Clamp de section, répété à l'identique pour chaque enfant du `.ui`.
const CLAMP_MAX: i32 = 860;
/// ~⅔ de `CLAMP_MAX`, comme le commentaire du `.ui`.
const CLAMP_TIGHTEN: i32 = 576;

/// Icône de l'origine des paquets (bouton « nixpkgs »). Deux variantes livrées
/// dans `data/icons/hicolor/scalable/apps` :
/// - `nix-snowflake` : logo officiel bi-couleur, couleurs figées (celle en place) ;
/// - `nix-snowflake-symbolic` : monochrome, recoloré par `.nix-packaging-color`.
const ORIGIN_ICON: &str = "nix-snowflake";

pub struct DetailModel {
    package: Package,
    meta: Option<AppMeta>,
    loading: bool,
    kept: bool,
    icon_tex: Option<gtk::gdk::Texture>,
    // Taille de la fermeture (tuile Stockage) et permissions (tuile Sécurité) :
    // appels plus lents que les métadonnées, chargés à part. Les drapeaux
    // `*_loaded` distinguent « pas encore su » de « su et inconnu ».
    size_loaded: bool,
    closure_size: Option<u64>,
    safety_loaded: bool,
    safety: Option<onyx_core::Safety>,
    // Description longue : repliable façon GsDescriptionBox.
    desc_expanded: bool,
    desc_needs_toggle: bool,
    // Facteur d'échelle écran (hi-DPI) : borne l'affichage des images à leur
    // résolution native (anti-upscale → jamais flou).
    scale: i32,
    // La note alimente les étoiles d'en-tête ET l'histogramme ; les avis, la
    // colonne droite. Les deux arrivent indépendamment.
    rating: Option<Rating>,
    // Lightbox screenshots (addition Onyx).
    shots: Vec<gtk::gdk::Texture>,
    shot_widths: Vec<i32>,
    lightbox_tex: Option<gtk::gdk::Texture>,
    lightbox_clamp: adw::Clamp,
    lightbox_visible: bool,
    // Conteneurs peuplés au chargement.
    stack: gtk::Stack,
    carousel: ScreenshotCarousel,
    star_box: gtk::Box,
    ctx_bar: gtk::Box,
    addons_box: gtk::Box,
    version_box: gtk::Box,
    problems_box: gtk::Box,
    license_box: gtk::Box,
    reviews_box: gtk::Box,
    dev_apps_box: gtk::Box,
    // « Autres applications par X » : tuiles AppTile navigables (les DevApps
    // AppStream sont résolues vers nixpkgs en arrière-plan, cf. DevAppsResolved).
    dev_apps_heading: gtk::Label,
    dev_tiles: FactoryVecDeque<AppTile>,
    toast_overlay: adw::ToastOverlay,
}

#[derive(Debug)]
pub enum DetailMsg {
    Loaded(Box<AppMeta>),
    LoadFailed,
    RatingLoaded(Rating),
    ReviewsLoaded(Vec<Review>),
    IconLoaded(Vec<u8>),
    ScreenshotLoaded(Vec<u8>, Option<String>),
    ShowShot(usize),
    DismissShot,
    Run,
    /// Entrée unique du menu : garde si non gardé, retire sinon.
    ToggleKeep,
    Keep,
    Unkeep,
    /// `nix shell` dans un terminal — le try-first des outils CLI.
    Shell,
    CopyCommand,
    /// Ouvre l'éditeur Nix avancé (options de service, port, surcharge…).
    AdvancedEdit,
    /// Blocs édités enregistrés (via l'éditeur avancé) : Ok = l'app est gardée.
    BlocksSaved(Result<(), String>),
    /// Ramasse-miettes du store (`nix-collect-garbage`), depuis le toast de retrait.
    CollectGarbage,
    GarbageCollected(Result<String, String>),
    /// Apps du même développeur résolues vers nixpkgs (paquet + icône).
    DevAppsResolved(Vec<(Package, Option<String>)>),
    /// Clic sur une tuile « Autres applications » → remonte à AppModel.
    OpenApp(Package),
    SizeLoaded(Option<u64>),
    SafetyLoaded(Option<onyx_core::Safety>),
    ToggleDescription,
    /// Déclenche `home-manager switch` (depuis l'action du toast « Garder »).
    ApplyConfig,
    Applied(Result<onyx_core::ApplyOutcome, String>),
}

impl DetailModel {
    /// Après avoir gardé une app (ou édité ses blocs) : si Onyx est déjà greffé sur la
    /// config, un toast « applique » suffit. Sinon le fichier généré est un orphelin sans
    /// effet → on propose la greffe consentie (diff affiché, rien d'écrit sans clic).
    fn prompt_apply_or_attach(&self, sender: &ComponentSender<Self>) {
        let layout = onyx_core::detect_layout();
        if onyx_core::is_attached(&layout) {
            let toast = adw::Toast::new("Ajouté à ta config. Applique pour installer.");
            toast.set_button_label(Some("Appliquer"));
            let s = sender.input_sender().clone();
            toast.connect_button_clicked(move |_| {
                let _ = s.send(DetailMsg::ApplyConfig);
            });
            self.toast_overlay.add_toast(toast);
        } else {
            let overlay = self.toast_overlay.clone();
            attach_dialog::present(
                &self.toast_overlay,
                onyx_core::attach_plan(&layout),
                move |res| {
                    crate::util::toast(
                        &overlay,
                        &match res {
                            Ok(()) => {
                                "Greffé sur ta config. Applique pour installer.".to_string()
                            }
                            Err(e) => format!("Échec de la greffe : {e}"),
                        },
                    );
                },
            );
        }
    }
}

#[relm4::component(pub)]
impl SimpleComponent for DetailModel {
    type Init = Package;
    type Input = DetailMsg;
    /// Paquet à ouvrir en page détail (tuile « Autres applications » cliquée) —
    /// AppModel pousse alors une nouvelle page, comme depuis n'importe quelle liste.
    type Output = Package;

    view! {
        adw::NavigationPage {
            set_title: &model.package.name,

            #[wrap(Some)]
            set_child = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {},

                // Ce Box devient la page « ready » du GtkStack, monté en init().
                #[wrap(Some)]
                #[name(ready_page)]
                set_content = &gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,

                    // GNOME propose d'aider à traduire quand la locale est mal
                    // couverte. nixpkgs n'expose rien de tel → jamais révélée.
                    adw::Banner {
                        set_button_label: Some("Aider à _traduire"),
                        set_revealed: false,
                    },

                    gtk::Overlay {
                        #[wrap(Some)]
                        #[name(scroller)]
                        set_child = &gtk::ScrolledWindow {
                            #[watch]
                            set_sensitive: !model.lightbox_visible,

                            gtk::Box {
                                add_css_class: "details-page",
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 18,
                                set_halign: gtk::Align::Fill,
                                set_valign: gtk::Align::Start,
                                set_vexpand: true,

                                // ── 1. En-tête (clampé) ───────────────────────
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    // `box_details_header` : spacing posé au runtime par
                                    // GNOME (24 large / 12 étroit) ; pas de mode étroit ici.
                                    #[name(header_box)]
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Horizontal,
                                        set_spacing: 24,

                                        // Icône 128. GNOME met un simple GtkImage ;
                                        // Onyx garde sa vignette de repli colorée
                                        // (divergence assumée : la plupart des paquets
                                        // nixpkgs n'ont pas d'icône AppStream).
                                        gtk::Box {
                                            set_width_request: 128,
                                            set_height_request: 128,
                                            set_halign: gtk::Align::Start,
                                            set_valign: gtk::Align::Start,

                                            gtk::Box {
                                                #[watch]
                                                set_visible: model.icon_tex.is_none(),
                                                set_width_request: 128,
                                                set_height_request: 128,
                                                // Bloque la remontée du hexpand de l'image interne.
                                                set_hexpand: false,
                                                set_vexpand: false,
                                                add_css_class: "fallback-tile",
                                                add_css_class: fallback_class(&model.package.name),

                                                gtk::Image {
                                                    set_icon_name: Some(FALLBACK_GLYPH),
                                                    set_pixel_size: 96,
                                                    // hexpand/vexpand : sinon halign Center est
                                                    // ignoré sur l'axe principal.
                                                    set_hexpand: true,
                                                    set_vexpand: true,
                                                    set_halign: gtk::Align::Center,
                                                    set_valign: gtk::Align::Center,
                                                    add_css_class: "fallback-glyph",
                                                },
                                            },
                                            gtk::Picture {
                                                #[watch]
                                                set_visible: model.icon_tex.is_some(),
                                                #[watch]
                                                set_paintable: model.icon_tex.as_ref(),
                                                set_width_request: 128,
                                                set_height_request: 128,
                                                add_css_class: "icon-dropshadow",
                                            },
                                        },

                                        // `box_details_header_not_icon`
                                        gtk::Box {
                                            set_orientation: gtk::Orientation::Horizontal,
                                            set_margin_top: 6,
                                            set_spacing: 6,
                                            set_hexpand: true,

                                            // `box_details_header2` : titre / dev / vérifié / étoiles
                                            gtk::Box {
                                                set_orientation: gtk::Orientation::Vertical,
                                                set_halign: gtk::Align::Fill,
                                                set_valign: gtk::Align::Center,
                                                set_hexpand: true,
                                                set_spacing: 6,

                                                gtk::Label {
                                                    #[watch]
                                                    set_label: &meta_name(&model.meta, &model.package.name),
                                                    set_halign: gtk::Align::Fill,
                                                    set_valign: gtk::Align::Start,
                                                    set_hexpand: true,
                                                    set_xalign: 0.0,
                                                    set_selectable: true,
                                                    set_wrap: true,
                                                    set_max_width_chars: 20,
                                                    add_css_class: "app-title",
                                                    add_css_class: "title-1",
                                                },

                                                gtk::Box {
                                                    set_orientation: gtk::Orientation::Horizontal,
                                                    set_spacing: 3,
                                                    set_hexpand: true,
                                                    #[watch]
                                                    set_visible: model.meta.as_ref()
                                                        .and_then(|m| m.developer.as_ref()).is_some(),

                                                    gtk::Label {
                                                        #[watch]
                                                        set_label: model.meta.as_ref()
                                                            .and_then(|m| m.developer.as_deref()).unwrap_or(""),
                                                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                                                        set_selectable: true,
                                                        set_max_width_chars: 100,
                                                        set_xalign: 0.0,
                                                        set_yalign: 0.5,
                                                        set_hexpand: false,
                                                        add_css_class: "app-developer",
                                                        add_css_class: "dim-label",
                                                    },
                                                },

                                                // Badge « Vérifié ». GNOME le colore avec la
                                                // couleur de packaging (@blue_4 pour Flatpak) ;
                                                // Onyx fait pareil via @nix_packaging_color.
                                                gtk::Box {
                                                    set_orientation: gtk::Orientation::Horizontal,
                                                    set_spacing: 3,
                                                    set_hexpand: true,
                                                    #[watch]
                                                    set_visible: model.meta.as_ref()
                                                        .map(|m| m.verified).unwrap_or(false),

                                                    gtk::Image {
                                                        set_icon_name: Some("app-verified-symbolic"),
                                                        set_pixel_size: 16,
                                                        set_tooltip_text: Some(
                                                            "L'identité du développeur est vérifiée"),
                                                        add_css_class: "nix-packaging-color",
                                                    },
                                                    gtk::Label {
                                                        set_label: "Vérifié",
                                                        set_xalign: 0.0,
                                                        set_yalign: 0.5,
                                                        set_hexpand: false,
                                                        add_css_class: "nix-packaging-color",
                                                    },
                                                },

                                                // `star_box` — rempli sur RatingLoaded.
                                                #[local_ref]
                                                star_box -> gtk::Box {
                                                    set_orientation: gtk::Orientation::Horizontal,
                                                    set_valign: gtk::Align::Start,
                                                    set_visible: false,
                                                },
                                            },

                                            // `box_with_source` : actions + bouton d'origine.
                                            gtk::Box {
                                                set_orientation: gtk::Orientation::Vertical,
                                                set_halign: gtk::Align::End,
                                                set_hexpand: false,
                                                set_valign: gtk::Align::Center,
                                                set_margin_top: 6,
                                                set_spacing: 6,

                                                // Le point UNIQUE où l'on agit sur un paquet : primaire
                                                // = try-first (`nix run`), menu = tout le reste.
                                                #[name(box_install_remove)]
                                                gtk::Box {
                                                    set_orientation: gtk::Orientation::Horizontal,
                                                    set_halign: gtk::Align::Center,

                                                    adw::SplitButton {
                                                        set_label: "_Lancer",
                                                        set_use_underline: true,
                                                        set_valign: gtk::Align::Center,
                                                        add_css_class: "suggested-action",
                                                        connect_clicked => DetailMsg::Run,

                                                        #[wrap(Some)]
                                                        set_popover = &gtk::Popover {
                                                            gtk::Box {
                                                                set_orientation: gtk::Orientation::Vertical,
                                                                set_spacing: 4,

                                                                // Garder / retirer : une seule entrée qui
                                                                // bascule (remplace l'ancienne poubelle).
                                                                gtk::Button {
                                                                    #[watch]
                                                                    set_label: if model.kept {
                                                                        "Retirer de ma config"
                                                                    } else {
                                                                        "Ajouter à ma config"
                                                                    },
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::ToggleKeep,
                                                                },
                                                                gtk::Button {
                                                                    set_label: "Essayer dans un shell",
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::Shell,
                                                                },
                                                                gtk::Button {
                                                                    set_label: "Copier la commande",
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::CopyCommand,
                                                                },
                                                                // Le chemin avancé : éditer le Nix brut de
                                                                // l'app (options de service, port, surcharge).
                                                                gtk::Button {
                                                                    set_label: "Options avancées…",
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::AdvancedEdit,
                                                                },
                                                                gtk::Separator {
                                                                    set_orientation: gtk::Orientation::Horizontal,
                                                                },
                                                                // Jusqu'ici atteignable seulement via le bouton
                                                                // d'un toast éphémère : raté = irrattrapable.
                                                                gtk::Button {
                                                                    set_label: "Appliquer la config",
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::ApplyConfig,
                                                                },
                                                            },
                                                        },
                                                    },
                                                },

                                                // `origin_box`
                                                gtk::Box {
                                                    set_orientation: gtk::Orientation::Horizontal,
                                                    set_spacing: 4,
                                                    set_halign: gtk::Align::Center,
                                                    set_valign: gtk::Align::Center,
                                                    add_css_class: "origin-button",

                                                    #[name(origin_btn)]
                                                    gtk::MenuButton {
                                                        set_always_show_arrow: true,
                                                        add_css_class: "flat",

                                                        #[wrap(Some)]
                                                        set_child = &gtk::Box {
                                                            set_orientation: gtk::Orientation::Horizontal,
                                                            set_spacing: 4,
                                                            set_halign: gtk::Align::Center,
                                                            set_valign: gtk::Align::Center,

                                                            gtk::Image {
                                                                set_icon_name: Some(ORIGIN_ICON),
                                                                set_pixel_size: 16,
                                                                set_valign: gtk::Align::Center,
                                                                add_css_class: "nix-packaging-color",
                                                            },
                                                            #[name(origin_label)]
                                                            gtk::Label {
                                                                set_label: "nixpkgs",
                                                                set_valign: gtk::Align::Center,
                                                                set_max_width_chars: 12,
                                                                set_ellipsize: gtk::pango::EllipsizeMode::End,
                                                            },
                                                        },

                                                        #[wrap(Some)]
                                                        set_popover = &gtk::Popover {
                                                            add_css_class: "menu",
                                                            gtk::ListBox {
                                                                set_selection_mode: gtk::SelectionMode::None,
                                                                adw::ActionRow {
                                                                    set_title: "nixpkgs",
                                                                    set_subtitle: "Collection de paquets Nix",
                                                                },
                                                            },
                                                        },
                                                    },
                                                },
                                            },
                                        },
                                    },
                                },

                                // ── 2. Screenshots (PLEINE LARGEUR, hors clamp) ─
                                #[local_ref]
                                carousel_widget -> gtk::Stack {},

                                // ── 3. Résumé + description (clampé) ───────────
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[name(application_details)]
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 12,

                                        gtk::Label {
                                            #[watch]
                                            set_visible: model.meta.as_ref()
                                                .and_then(|m| m.summary.as_ref()).is_some(),
                                            #[watch]
                                            set_label: model.meta.as_ref()
                                                .and_then(|m| m.summary.as_deref()).unwrap_or(""),
                                            set_halign: gtk::Align::Fill,
                                            set_valign: gtk::Align::Start,
                                            set_hexpand: true,
                                            set_xalign: 0.0,
                                            set_selectable: true,
                                            set_wrap: true,
                                            set_max_width_chars: 60,
                                            add_css_class: "title-2",
                                        },

                                        // Skeleton pendant le chargement de la description.
                                        gtk::Box {
                                            #[watch]
                                            set_visible: model.loading,
                                            set_orientation: gtk::Orientation::Vertical,
                                            set_spacing: 8,

                                            gtk::Box { add_css_class: "skeleton", set_height_request: 13, set_hexpand: true },
                                            gtk::Box { add_css_class: "skeleton", set_height_request: 13, set_hexpand: true },
                                            gtk::Box { add_css_class: "skeleton", set_height_request: 13, set_width_request: 320, set_halign: gtk::Align::Start },
                                        },
                                        gtk::Label {
                                            #[watch]
                                            set_visible: !model.loading,
                                            #[watch]
                                            set_label: &desc_display(&model.meta, &model.package.description, model.desc_expanded),
                                            set_halign: gtk::Align::Fill,
                                            set_hexpand: true,
                                            set_valign: gtk::Align::Start,
                                            set_wrap: true,
                                            set_xalign: 0.0,
                                        },
                                        gtk::Button {
                                            #[watch]
                                            set_visible: !model.loading && model.desc_needs_toggle,
                                            #[watch]
                                            set_label: if model.desc_expanded { "Afficher moins" } else { "Afficher davantage" },
                                            add_css_class: "pill",
                                            set_halign: gtk::Align::Center,
                                            set_margin_bottom: 14,
                                            connect_clicked => DetailMsg::ToggleDescription,
                                        },
                                    },
                                },

                                // ── 4. Extensions (masqué si aucune) ───────────
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[local_ref]
                                    addons_box -> gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_visible: false,
                                    },
                                },

                                // ── 5. Context bar ─────────────────────────────
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[local_ref]
                                    ctx_bar -> gtk::Box {},
                                },

                                // ── 6. Historique des versions ─────────────────
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[local_ref]
                                    version_box -> gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_visible: false,
                                    },
                                },

                                // ── 7. Infobar « problèmes » (broken/insecure) ─
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[local_ref]
                                    problems_box -> gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_visible: false,
                                    },
                                },

                                // ── 8. Licence + liens côte à côte ─────────────
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[local_ref]
                                    license_box -> gtk::Box {},
                                },

                                // ── 9. Évaluations (ODRS) ──────────────────────
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[local_ref]
                                    reviews_box -> gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 18,
                                        set_visible: false,
                                    },
                                },

                                // ── 10. Autres applis du développeur ───────────
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[local_ref]
                                    dev_apps_box -> gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 6,
                                        set_visible: false,
                                    },
                                },
                            }
                        },

                        // Lightbox (addition Onyx) : fondu d'ouverture/fermeture.
                        add_overlay = &gtk::Revealer {
                            #[watch]
                            set_reveal_child: model.lightbox_visible,
                            #[watch]
                            set_can_target: model.lightbox_visible,
                            set_transition_type: gtk::RevealerTransitionType::Crossfade,
                            set_transition_duration: 200,
                            set_halign: gtk::Align::Fill,
                            set_valign: gtk::Align::Fill,

                            gtk::Button {
                                add_css_class: "lightbox",
                                set_hexpand: true,
                                set_vexpand: true,
                                connect_clicked => DetailMsg::DismissShot,

                                add_controller = gtk::EventControllerKey {
                                    connect_key_pressed[sender] => move |_, key, _, _| {
                                        if key == gtk::gdk::Key::Escape {
                                            sender.input(DetailMsg::DismissShot);
                                            gtk::glib::Propagation::Stop
                                        } else {
                                            gtk::glib::Propagation::Proceed
                                        }
                                    },
                                },

                                // Clamp anti-upscale : maximum_size = largeur native /
                                // scale → une image sous la résolution attendue est
                                // affichée à sa taille native centrée (DPI net) plutôt
                                // qu'étirée plein écran (flou).
                                #[local_ref]
                                lightbox_clamp -> adw::Clamp {
                                    set_margin_top: 48,
                                    set_margin_bottom: 48,
                                    set_margin_start: 48,
                                    set_margin_end: 48,

                                    gtk::Picture {
                                        #[watch]
                                        set_paintable: model.lightbox_tex.as_ref(),
                                    },
                                },
                            },
                        },
                    },
                }
            }
        }
    }

    fn init(pkg: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let kept = onyx_core::load_manifest().entry(&pkg.attr).is_some();

        let toast_overlay = adw::ToastOverlay::new();
        let stack = gtk::Stack::new();
        let carousel = ScreenshotCarousel::new();
        let carousel_widget = carousel.widget().clone();
        let star_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let ctx_bar = context_bar::container();
        let addons_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let version_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let problems_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let license_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let reviews_box = gtk::Box::new(gtk::Orientation::Vertical, 18);
        let dev_apps_box = gtk::Box::new(gtk::Orientation::Vertical, 6);

        // Section « Autres applications » : titre + grille de tuiles AppTile
        // (mêmes tuiles cliquables que l'accueil/les catégories). Peuplée sur
        // DevAppsResolved ; la box reste invisible tant que rien n'est résolu.
        let dev_apps_heading = gtk::Label::new(None);
        dev_apps_heading.set_xalign(0.0);
        dev_apps_heading.set_margin_top(21);
        dev_apps_heading.set_margin_bottom(6);
        dev_apps_heading.add_css_class("heading");
        let dev_tiles: FactoryVecDeque<AppTile> = FactoryVecDeque::builder()
            .launch(tile_grid())
            .forward(sender.input_sender(), DetailMsg::OpenApp);
        dev_apps_box.append(&dev_apps_heading);
        dev_apps_box.append(dev_tiles.widget());

        let lightbox_clamp = adw::Clamp::new();
        lightbox_clamp.set_maximum_size(i32::MAX);

        // Résolution cible des screenshots = largeur écran × échelle (px physiques),
        // bornée [1280, 3840] : assez pour un zoom plein écran net, jamais plus.
        let (target_dev_w, scale) = crate::util::screen_metrics();
        let target_w = target_dev_w.clamp(1280, 3840) as u32;

        // La tuile/ligne cliquée a déjà téléchargé cette icône : on la réutilise
        // plutôt que d'afficher le glyphe générique le temps de tout re-résoudre.
        let icon_tex = onyx_core::known_icon_url(&pkg.attr)
            .and_then(|url| onyx_core::cached_bytes_if_present(&url))
            .and_then(|bytes| texture_from_bytes_sized(&bytes, 128));

        let model = DetailModel {
            package: pkg.clone(),
            meta: None,
            loading: true,
            kept,
            icon_tex,
            size_loaded: false,
            closure_size: None,
            safety_loaded: false,
            safety: None,
            desc_expanded: false,
            desc_needs_toggle: false,
            scale,
            rating: None,
            shots: Vec::new(),
            shot_widths: Vec::new(),
            lightbox_tex: None,
            lightbox_clamp: lightbox_clamp.clone(),
            lightbox_visible: false,
            stack: stack.clone(),
            carousel,
            star_box: star_box.clone(),
            ctx_bar: ctx_bar.clone(),
            addons_box: addons_box.clone(),
            version_box: version_box.clone(),
            problems_box: problems_box.clone(),
            license_box: license_box.clone(),
            reviews_box: reviews_box.clone(),
            dev_apps_box: dev_apps_box.clone(),
            dev_apps_heading: dev_apps_heading.clone(),
            dev_tiles,
            toast_overlay: toast_overlay.clone(),
        };
        let widgets = view_output!();

        crate::util::smooth_scroll(&widgets.scroller);

        // Le SizeGroup terminal du `.ui` : toutes les sections partagent une largeur.
        let sg = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        for w in [
            widgets.header_box.clone().upcast::<gtk::Widget>(),
            widgets.application_details.clone().upcast(),
            addons_box.clone().upcast(),
            ctx_bar.clone().upcast(),
            version_box.clone().upcast(),
            problems_box.clone().upcast(),
            license_box.clone().upcast(),
            reviews_box.clone().upcast(),
            dev_apps_box.clone().upcast(),
        ] {
            sg.add_widget(&w);
        }
        // `sizegroup_origin_button` : le bouton d'origine s'aligne sur les actions.
        let sg_origin = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        sg_origin.add_widget(&widgets.box_install_remove);
        sg_origin.add_widget(&widgets.origin_btn);

        // Le label d'origine porte les attributs Pango du `.ui` (scale .85, weight normal).
        let attrs = gtk::pango::AttrList::new();
        attrs.insert(gtk::pango::AttrFloat::new_scale(0.85));
        attrs.insert(gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Normal));
        widgets.origin_label.set_attributes(Some(&attrs));

        // Le Box « ready » produit par `view!` devient une page du GtkStack, qui
        // prend sa place comme contenu de la ToolbarView.
        let toolbar = root.child().and_downcast::<adw::ToolbarView>();
        if let Some(toolbar) = &toolbar {
            toolbar.set_content(None::<&gtk::Widget>);
            model.build_stack(&widgets.ready_page);
            toolbar.set_content(Some(&stack));
        }

        // NavigationPage ne supporte pas container_add : on insère le toast_overlay
        // entre elle et la ToolbarView.
        if let Some(toolbar) = root.child() {
            root.set_child(None::<&gtk::Widget>);
            model.toast_overlay.set_child(Some(&toolbar));
            root.set_child(Some(&model.toast_overlay));
        }

        // Métadonnées fusionnées (AppStream + Nix) en arrière-plan.
        let s = sender.input_sender().clone();
        let pkg_meta = pkg.clone();
        std::thread::spawn(move || {
            let meta = onyx_core::cached_app_meta(&pkg_meta, target_w);
            let _ = s.send(if is_empty_meta(&meta) {
                DetailMsg::LoadFailed
            } else {
                DetailMsg::Loaded(Box::new(meta))
            });
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            DetailMsg::Loaded(meta) => {
                let meta = *meta;
                self.loading = false;
                self.stack.set_visible_child_name("ready");

                self.rebuild_context_bar(&meta);
                self.rebuild_problems(&meta);
                build_addons(&self.addons_box, &meta);
                build_version_history(
                    &self.version_box,
                    &meta,
                    Some(self.package.version.as_str()),
                    self.toast_overlay.clone().upcast(),
                );
                build_license_box(&self.license_box, &meta);

                // Apps du même développeur : résolution AppStream → nixpkgs en
                // arrière-plan (1 `nix search` caché) ; la section n'apparaît
                // que si au moins une app existe dans nixpkgs (tuile morte
                // = pire qu'une section absente).
                if !meta.developer_apps.is_empty() {
                    let apps = meta.developer_apps.clone();
                    let attr = self.package.attr.clone();
                    let s = sender.input_sender().clone();
                    std::thread::spawn(move || {
                        // Exclut l'attr courant : « Steam Link » retombe sur
                        // l'attr `steam` et réaffichait Steam sur sa propre page.
                        let resolved = onyx_core::resolve_dev_apps(&apps, &attr);
                        if !resolved.is_empty() {
                            let _ = s.send(DetailMsg::DevAppsResolved(resolved));
                        }
                    });
                }

                if meta.screenshot_urls.is_empty() {
                    self.carousel.show_fallback();
                }

                // Taille de la fermeture (`nix path-info`) : plus lent que le reste.
                let attr = self.package.attr.clone();
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let _ = s.send(DetailMsg::SizeLoaded(onyx_core::closure_size(&attr)));
                });

                if let Some(app_id) = meta.app_id.clone() {
                    // Permissions Flatpak (tuile Sécurité).
                    let s = sender.input_sender().clone();
                    let id = app_id.clone();
                    std::thread::spawn(move || {
                        let _ = s.send(DetailMsg::SafetyLoaded(onyx_core::safety(&id)));
                    });
                    // Note ODRS via l'app_id résolu (fiable, contrairement au nom nixpkgs).
                    let s = sender.input_sender().clone();
                    let id = app_id.clone();
                    std::thread::spawn(move || {
                        if let Some(r) = onyx_core::rating_for(&id) {
                            let _ = s.send(DetailMsg::RatingLoaded(r));
                        }
                    });
                    // Texte des avis (colonne droite de la section « Évaluations »).
                    let s = sender.input_sender().clone();
                    std::thread::spawn(move || {
                        let reviews = onyx_core::reviews_for(&app_id);
                        if !reviews.is_empty() {
                            let _ = s.send(DetailMsg::ReviewsLoaded(reviews));
                        }
                    });
                }

                if let Some(url) = meta.icon_url.clone() {
                    let s = sender.input_sender().clone();
                    std::thread::spawn(move || {
                        if let Some(b) = onyx_core::cached_fetch_bytes(&url) {
                            let _ = s.send(DetailMsg::IconLoaded(b));
                        }
                    });
                }
                for (i, url) in meta.screenshot_urls.iter().take(4).cloned().enumerate() {
                    let caption = meta.screenshot_captions.get(i).cloned().flatten();
                    let s = sender.input_sender().clone();
                    std::thread::spawn(move || {
                        if let Some(b) = onyx_core::cached_fetch_bytes(&url) {
                            let _ = s.send(DetailMsg::ScreenshotLoaded(b, caption));
                        }
                    });
                }

                self.meta = Some(meta);
                self.desc_needs_toggle = desc_needs_toggle(&self.meta, &self.package.description);
                self.desc_expanded = false;
            }
            DetailMsg::LoadFailed => {
                self.loading = false;
                self.stack.set_visible_child_name("failed");
            }
            DetailMsg::DevAppsResolved(list) => {
                let dev = self
                    .meta
                    .as_ref()
                    .and_then(|m| m.developer.as_deref())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                self.dev_apps_heading.set_label(&if dev.is_empty() {
                    "Autres applications".to_string()
                } else {
                    format!("Autres applications par {dev}")
                });
                let mut g = self.dev_tiles.guard();
                g.clear();
                for (p, icon) in list.into_iter().take(6) {
                    g.push_back(AppItem::new(p, icon));
                }
                drop(g);
                self.dev_apps_box.set_visible(true);
            }
            DetailMsg::OpenApp(pkg) => {
                let _ = sender.output(pkg);
            }
            DetailMsg::SizeLoaded(size) => {
                self.size_loaded = true;
                self.closure_size = size;
                if let Some(meta) = self.meta.clone() {
                    self.rebuild_context_bar(&meta);
                }
            }
            DetailMsg::SafetyLoaded(safety) => {
                self.safety_loaded = true;
                self.safety = safety;
                if let Some(meta) = self.meta.clone() {
                    self.rebuild_context_bar(&meta);
                }
            }
            DetailMsg::ToggleDescription => {
                self.desc_expanded = !self.desc_expanded;
            }
            DetailMsg::RatingLoaded(rating) => {
                build_stars(&self.star_box, &rating);
                self.rating = Some(rating);
                self.rebuild_reviews(None);
            }
            DetailMsg::ReviewsLoaded(reviews) => {
                self.rebuild_reviews(Some(&reviews));
            }
            DetailMsg::IconLoaded(bytes) => {
                self.icon_tex = texture_from_bytes_sized(&bytes, 128);
            }
            DetailMsg::ScreenshotLoaded(bytes, caption) => {
                if let Some(tex) = texture_from_bytes(&bytes) {
                    let idx = self.shots.len();
                    // Anti-upscale : largeur logique max = px natifs / échelle écran.
                    let max_logical = (tex.width() / self.scale).max(1);
                    self.shots.push(tex.clone());
                    self.shot_widths.push(max_logical);

                    // La `Picture` garde son ratio et se centre : une gouttière
                    // apparaît dès que la page est plus large que l'image et grandit
                    // avec la fenêtre. Les marges garantissent un écart minimal ;
                    // le clamp borne la largeur à la résolution native (DPI net).
                    let pic = gtk::Picture::for_paintable(&tex);
                    let clamp = adw::Clamp::new();
                    clamp.set_maximum_size(max_logical);
                    clamp.set_child(Some(&pic));
                    let btn = gtk::Button::builder().child(&clamp).build();
                    btn.add_css_class("flat");
                    btn.add_css_class("screenshot-button");
                    btn.set_margin_start(18);
                    btn.set_margin_end(18);
                    btn.set_margin_top(18);
                    // Légende AppStream en tooltip (comme GNOME), sinon « Agrandir ».
                    btn.set_tooltip_text(Some(caption.as_deref().unwrap_or("Agrandir")));
                    let s = sender.clone();
                    btn.connect_clicked(move |_| s.input(DetailMsg::ShowShot(idx)));
                    self.carousel.append(&btn);
                }
            }
            DetailMsg::ShowShot(idx) => {
                self.lightbox_tex = self.shots.get(idx).cloned();
                // Zoom : ×2 la largeur native de CETTE image (fenêtre et marges
                // bornent de toute façon l'agrandissement réel).
                if let Some(w) = self.shot_widths.get(idx) {
                    self.lightbox_clamp.set_maximum_size(w.saturating_mul(2));
                }
                self.lightbox_visible = true;
            }
            DetailMsg::DismissShot => {
                self.lightbox_visible = false;
            }
            DetailMsg::Run => {
                let name = meta_name(&self.meta, &self.package.name);
                if self.meta.as_ref().map(|m| m.broken).unwrap_or(false) {
                    // On n'empêche pas l'essai (l'utilisateur reste maître), mais on
                    // prévient : nix refusera très probablement d'évaluer ce paquet.
                    crate::util::toast(
                        &self.toast_overlay,
                        &format!("{name} est marqué cassé dans nixpkgs — tentative de lancement quand même…"),
                    );
                } else {
                    crate::util::toast(&self.toast_overlay, &format!("Lancement de {name}…"));
                }
                if let Err(e) = onyx_core::run(&self.package) {
                    crate::util::toast(&self.toast_overlay, &format!("Échec du lancement : {e}"));
                }
            }
            DetailMsg::ToggleKeep => {
                let next = if self.kept { DetailMsg::Unkeep } else { DetailMsg::Keep };
                sender.input(next);
            }
            DetailMsg::Shell => match onyx_core::shell(&self.package) {
                Ok(onyx_core::ShellOutcome::Spawned) => crate::util::toast(
                    &self.toast_overlay,
                    &format!("{} ouvert dans un terminal…", self.package.name),
                ),
                // Le core ne touche pas au presse-papier : c'est la façade qui décide
                // quoi faire quand aucun terminal n'existe.
                Ok(onyx_core::ShellOutcome::NoTerminal) => {
                    if let Some(display) = gtk::gdk::Display::default() {
                        display
                            .clipboard()
                            .set_text(&onyx_core::shell_command(&self.package));
                    }
                    crate::util::toast(
                        &self.toast_overlay,
                        "Aucun terminal trouvé — commande copiée.",
                    );
                }
                Err(e) => crate::util::toast(&self.toast_overlay, &format!("Échec du shell : {e}")),
            },
            DetailMsg::Keep => match onyx_core::keep(&self.package) {
                Ok(()) => {
                    self.kept = true;
                    self.prompt_apply_or_attach(&sender);
                }
                Err(e) => crate::util::toast(&self.toast_overlay, &format!("Échec : {e}")),
            },
            DetailMsg::Unkeep => match onyx_core::unkeep(&self.package.attr) {
                Ok(()) => {
                    self.kept = false;
                    let toast = adw::Toast::new(
                        "Retiré de ta config. Applique, puis libère l'espace si tu veux.",
                    );
                    toast.set_button_label(Some("Libérer l'espace"));
                    let s = sender.input_sender().clone();
                    toast.connect_button_clicked(move |_| {
                        let _ = s.send(DetailMsg::CollectGarbage);
                    });
                    self.toast_overlay.add_toast(toast);
                }
                Err(e) => crate::util::toast(&self.toast_overlay, &format!("Échec : {e}")),
            },
            DetailMsg::CollectGarbage => {
                crate::util::toast(&self.toast_overlay, "Nettoyage du store…");
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let res = onyx_core::garbage_collect().map_err(|e| e.to_string());
                    let _ = s.send(DetailMsg::GarbageCollected(res));
                });
            }
            DetailMsg::GarbageCollected(res) => crate::util::toast(
                &self.toast_overlay,
                &match res {
                    Ok(summary) if summary.is_empty() => "Rien à libérer.".to_string(),
                    Ok(summary) => summary,
                    Err(e) => format!("Nettoyage impossible : {e}"),
                },
            ),
            DetailMsg::AdvancedEdit => {
                // Blocs Nix actuels de l'app (le cas échéant), pour préremplir l'éditeur.
                let m = onyx_core::load_manifest();
                let (home, system) = m
                    .kept
                    .iter()
                    .find(|e| e.package.attr == self.package.attr)
                    .map(|e| (e.home_nix.clone(), e.system_nix.clone()))
                    .unwrap_or((None, None));
                let system_available = onyx_core::detect_layout().system.is_some();
                let pkg = self.package.clone();
                let s = sender.input_sender().clone();
                nix_editor::present(
                    &self.toast_overlay,
                    &self.package,
                    home,
                    system,
                    system_available,
                    move |blocks| {
                        let res = onyx_core::set_blocks(&pkg, blocks.home, blocks.system)
                            .map_err(|e| e.to_string());
                        let _ = s.send(DetailMsg::BlocksSaved(res));
                    },
                );
            }
            DetailMsg::BlocksSaved(res) => match res {
                Ok(()) => {
                    // set_blocks garde l'app si besoin : l'éditer, c'est vouloir l'avoir.
                    self.kept = true;
                    self.prompt_apply_or_attach(&sender);
                }
                Err(e) => crate::util::toast(&self.toast_overlay, &format!("Échec : {e}")),
            },
            DetailMsg::ApplyConfig => {
                crate::util::toast(&self.toast_overlay, "Application de la config…");
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let res = onyx_core::apply_config().map_err(|e| e.to_string());
                    let _ = s.send(DetailMsg::Applied(res));
                });
            }
            DetailMsg::Applied(res) => {
                crate::util::toast(&self.toast_overlay, &crate::util::describe_apply(res));
            }
            DetailMsg::CopyCommand => {
                if let Some(display) = gtk::gdk::Display::default() {
                    display.clipboard().set_text(&onyx_core::run_command(&self.package));
                }
                crate::util::toast(&self.toast_overlay, "Commande copiée.");
            }
        }
    }
}

impl DetailModel {
    /// Monte les trois pages du `GtkStack` : spinner / ready / failed.
    fn build_stack(&self, ready: &gtk::Box) {
        let spinner = adw::StatusPage::new();
        spinner.set_paintable(Some(&adw::SpinnerPaintable::new(Some(&self.stack))));
        spinner.set_title("Chargement des détails de l'application");
        self.stack.add_named(&spinner, Some("spinner"));

        self.stack.add_named(ready, Some("ready"));

        let failed = adw::StatusPage::new();
        failed.set_icon_name(Some("dialog-error-symbolic"));
        failed.set_title("Impossible d'afficher les détails de l'application");
        failed.set_description(Some(
            "Aucune métadonnée n'a pu être récupérée pour ce paquet.",
        ));
        self.stack.add_named(&failed, Some("failed"));

        self.stack.set_visible_child_name("spinner");
    }

    fn rebuild_context_bar(&self, meta: &AppMeta) {
        context_bar::fill(
            &self.ctx_bar,
            build_context_tiles(
                meta,
                self.size_loaded,
                self.closure_size,
                self.safety_loaded,
                self.safety.as_ref(),
            ),
        );
    }

    fn rebuild_problems(&self, meta: &AppMeta) {
        while let Some(c) = self.problems_box.first_child() {
            self.problems_box.remove(&c);
        }
        match infobox::problems(meta.broken, meta.insecure, &meta.known_vulnerabilities) {
            Some(bar) => {
                self.problems_box.append(&bar);
                self.problems_box.set_visible(true);
            }
            None => self.problems_box.set_visible(false),
        }
    }

    /// Section « Évaluations ». Note et avis arrivent indépendamment : la colonne
    /// gauche s'affiche dès la note, la droite se remplit quand les avis arrivent.
    fn rebuild_reviews(&self, reviews: Option<&[Review]>) {
        match self.rating.filter(|r| r.total > 0) {
            Some(rating) => build_reviews(&self.reviews_box, &rating, reviews),
            None => self.reviews_box.set_visible(false),
        }
    }
}

// ── Sections ──────────────────────────────────────────────────────────────────

fn boxed_list() -> gtk::ListBox {
    let l = gtk::ListBox::new();
    l.set_selection_mode(gtk::SelectionMode::None);
    l.set_valign(gtk::Align::Start);
    l.add_css_class("boxed-list");
    l
}

/// Note d'en-tête : cinq étoiles + nombre d'avis (`label_review_count`).
fn build_stars(container: &gtk::Box, rating: &Rating) {
    while let Some(c) = container.first_child() {
        container.remove(&c);
    }
    let stars = stars_widget(rating.stars, 16);
    stars.set_halign(gtk::Align::Start);
    stars.set_valign(gtk::Align::Center);
    container.append(&stars);

    let count = gtk::Label::new(Some(&format!("({})", rating.total)));
    count.set_margin_start(5);
    count.set_halign(gtk::Align::Start);
    count.set_valign(gtk::Align::Center);
    count.add_css_class("dim-label");
    container.append(&count);

    container.set_visible(true);
}

/// `box_reviews` : titre `.title-2` puis deux `boxed-list` côte à côte
/// (`spacing 12`, homogeneous).
fn build_reviews(container: &gtk::Box, rating: &Rating, reviews: Option<&[Review]>) {
    while let Some(c) = container.first_child() {
        container.remove(&c);
    }

    let title = gtk::Label::new(Some("Évaluations"));
    title.set_xalign(0.0);
    title.set_halign(gtk::Align::Start);
    title.set_valign(gtk::Align::Start);
    title.set_hexpand(true);
    title.add_css_class("title-2");
    container.append(&title);

    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    inner.set_homogeneous(true);

    // Colonne gauche : histogramme + « Écrire un avis ».
    let left = boxed_list();
    let hist_row = gtk::ListBoxRow::new();
    hist_row.set_activatable(false);
    hist_row.set_selectable(false);
    let hist_clamp = adw::Clamp::new();
    hist_clamp.set_halign(gtk::Align::Start);
    hist_clamp.set_maximum_size(500);
    hist_clamp.set_tightening_threshold(500);
    hist_clamp.set_valign(gtk::Align::Center);
    hist_clamp.set_child(Some(&review_histogram::histogram(rating)));
    hist_row.set_child(Some(&hist_clamp));
    left.append(&hist_row);

    let write = adw::ButtonRow::new();
    write.set_title("Écrire un a_vis");
    write.set_use_underline(true);
    write.set_end_icon_name(Some("go-next-symbolic"));
    // GNOME grise ce bouton hors ligne ou si l'app n'est pas installée. Onyx ne
    // peut pas publier du tout : pas de compte ODRS, lecture seule (spec §8).
    write.set_sensitive(false);
    write.set_tooltip_text(Some("Publier un avis nécessite un compte ODRS"));
    left.append(&write);
    inner.append(&left);

    // Colonne droite : avis vedette + « Tous les avis ».
    let right = boxed_list();
    if let Some(featured) = reviews.and_then(onyx_core::featured_review) {
        right.append(&review_row::row(featured));
    }
    let all = adw::ButtonRow::new();
    all.set_title("Tous les avis");
    all.set_end_icon_name(Some("go-next-symbolic"));
    match reviews.filter(|r| !r.is_empty()) {
        Some(list) => {
            let list = list.to_vec();
            let parent = container.clone();
            all.connect_activated(move |_| show_reviews_dialog(&list, &parent));
        }
        None => all.set_sensitive(false),
    }
    right.append(&all);
    inner.append(&right);

    container.append(&inner);
    container.set_visible(true);
}

/// Dialogue « Tous les avis » : une `GsReviewRow` par avis, du mieux classé au moins.
fn show_reviews_dialog(reviews: &[Review], parent: &impl IsA<gtk::Widget>) {
    let list = boxed_list();
    let mut sorted: Vec<&Review> = reviews.iter().collect();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.score));
    for r in sorted {
        list.append(&review_row::row(r));
    }
    present_dialog("Tous les avis", &list, 560, 640, parent);
}

/// Un dialogue plein écran : liste clampée dans un scroll, sous une HeaderBar.
fn present_dialog(
    title: &str,
    child: &impl IsA<gtk::Widget>,
    width: i32,
    height: i32,
    parent: &impl IsA<gtk::Widget>,
) {
    let c = adw::Clamp::new();
    c.set_maximum_size(600);
    c.set_margin_top(12);
    c.set_margin_bottom(12);
    c.set_margin_start(12);
    c.set_margin_end(12);
    c.set_child(Some(child));

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_child(Some(&c));

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroller));

    let dialog = adw::Dialog::new();
    dialog.set_title(title);
    dialog.set_content_width(width);
    dialog.set_content_height(height);
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

/// Les quatre tuiles de la context bar, dans l'ordre du `.ui`.
///
/// - **Stockage** : taille de téléchargement de la fermeture (`closure_size`).
/// - **Sécurité** : `broken`/`insecure` (nix) d'abord, puis les permissions
///   Flatpak (comme GNOME), sinon la licence en repli.
/// - **Matériel** : générique bureau (pas de `<relations>` exploitable).
/// - **Âge** : classification OARS, comme GNOME.
fn build_context_tiles(
    meta: &AppMeta,
    size_loaded: bool,
    closure_size: Option<u64>,
    safety_loaded: bool,
    safety: Option<&onyx_core::Safety>,
) -> [gtk::Button; 4] {
    let (storage_val, storage_desc): (String, &str) = if !size_loaded {
        ("…".into(), "Calcul de la taille…")
    } else if let Some(bytes) = closure_size {
        (format_size(bytes), "Téléchargement total avec dépendances")
    } else {
        ("?".into(), "Nécessite une compilation locale")
    };
    let storage = context_bar::tile(
        Content::Text(&storage_val),
        "grey",
        "Taille du téléchargement",
        storage_desc,
    );

    let safety_tile = if meta.broken {
        context_bar::tile(
            Content::Icon("app-safety-unsafe-symbolic"),
            "red",
            "Cassé",
            "Marqué broken dans nixpkgs : le lancement échouera probablement",
        )
    } else if meta.insecure || !meta.known_vulnerabilities.is_empty() {
        let d = if meta.known_vulnerabilities.is_empty() {
            "Marqué non sécurisé dans nixpkgs".to_string()
        } else {
            format!("Vulnérabilités connues : {}", meta.known_vulnerabilities.join(", "))
        };
        context_bar::tile(Content::Icon("app-safety-unsafe-symbolic"), "red", "Vulnérable", &d)
    } else if let Some(s) = safety {
        context_bar::tile(Content::Icon(s.icon), s.color, s.title, &s.descriptions.join(" ; "))
    } else if !safety_loaded {
        context_bar::tile(
            Content::Icon("app-safety-unknown-symbolic"),
            "grey",
            "Analyse…",
            "Vérification des permissions…",
        )
    } else {
        match meta.is_free_license {
            Some(true) => context_bar::tile(
                Content::Icon("app-safety-ok-symbolic"),
                "green",
                "Logiciel libre",
                "Code source public, licence libre",
            ),
            Some(false) => context_bar::tile(
                Content::Icon("app-safety-unsafe-symbolic"),
                "orange",
                "Propriétaire",
                "Code source non public",
            ),
            None => context_bar::tile(
                Content::Icon("app-safety-unknown-symbolic"),
                "grey",
                "Inconnu",
                "Licence non renseignée",
            ),
        }
    };

    let hardware = context_bar::tile(
        Content::Icon("video-display-symbolic"),
        "grey",
        "Ordinateur uniquement",
        "Fonctionne sur les ordinateurs de bureau et les ordinateurs portables",
    );

    let age = match &meta.age_label {
        Some(label) => {
            let desc = if meta.age_descriptions.is_empty() {
                "Aucun contenu inapproprié".to_string()
            } else {
                meta.age_descriptions.join(" ; ")
            };
            context_bar::tile(Content::Text(label), age_color(label), "Évaluation par âge", &desc)
        }
        None => context_bar::tile(
            Content::Text("?"),
            "grey",
            "Évaluation par âge",
            "Aucune information de classification d'âge",
        ),
    };

    [storage, safety_tile, hardware, age]
}

/// Classe de teinte du lozenge d'âge — alias `details-rating-N` du CSS GNOME.
fn age_color(label: &str) -> &'static str {
    let n: u32 = label.trim_end_matches('+').parse().unwrap_or(0);
    if n >= 18 {
        "details-rating-18"
    } else if n >= 15 {
        "details-rating-15"
    } else if n >= 12 {
        "details-rating-12"
    } else if n >= 5 {
        "details-rating-5"
    } else {
        "details-rating-0"
    }
}

/// Taille en octets façon FR (« 206,0 Mo »), unités décimales (1000, pas 1024 —
/// comme `gs_utils_format_size` et `nix path-info`).
fn format_size(bytes: u64) -> String {
    const K: f64 = 1000.0;
    let b = bytes as f64;
    if b < K {
        return format!("{bytes} o");
    }
    let (val, unit) = if b < K * K {
        (b / K, "Ko")
    } else if b < K * K * K {
        (b / (K * K), "Mo")
    } else {
        (b / (K * K * K), "Go")
    };
    format!("{val:.1} {unit}").replace('.', ",")
}

/// `box_addons` : titre `.title-2` (`margin-bottom 12`) + grille de tuiles.
/// Purement informative — les addons Flatpak n'ont pas d'équivalent nixpkgs à
/// ouvrir, donc les tuiles sont non navigables (`info_tile`).
fn build_addons(container: &gtk::Box, meta: &AppMeta) {
    while let Some(c) = container.first_child() {
        container.remove(&c);
    }
    if meta.addons.is_empty() {
        container.set_visible(false);
        return;
    }

    let title = gtk::Label::new(Some("Extensions"));
    title.set_xalign(0.0);
    title.set_halign(gtk::Align::Start);
    title.set_valign(gtk::Align::Start);
    title.set_hexpand(true);
    title.set_margin_bottom(12);
    title.add_css_class("title-2");
    container.append(&title);

    let flow = tile_grid();
    for a in meta.addons.iter().take(12) {
        flow.append(&info_tile(
            &a.name,
            a.summary.as_deref().filter(|s| !s.trim().is_empty()),
            a.icon_url.clone(),
        ));
    }
    container.append(&flow);
    container.set_visible(true);
}

/// `list_box_version_history` : la dernière release, puis un `AdwButtonRow` qui
/// ouvre le dialogue plein historique (calque de `GsAppVersionHistoryDialog`).
fn build_version_history(
    container: &gtk::Box,
    meta: &AppMeta,
    installed_version: Option<&str>,
    parent: gtk::Widget,
) {
    while let Some(c) = container.first_child() {
        container.remove(&c);
    }
    if meta.releases.is_empty() {
        container.set_visible(false);
        return;
    }
    let latest = &meta.releases[0];
    let list = boxed_list();
    list.append(&version_history_row::row(
        &latest.version,
        latest.timestamp,
        latest.description.as_deref(),
        installed_version == Some(latest.version.as_str()),
    ));

    if meta.releases.len() > 1 {
        let more = adw::ButtonRow::new();
        more.set_title("Historique des versions");
        more.set_end_icon_name(Some("go-next-symbolic"));
        let releases = meta.releases.clone();
        let installed = installed_version.map(str::to_string);
        more.connect_activated(move |_| {
            let list = boxed_list();
            for r in &releases {
                list.append(&version_history_row::row(
                    &r.version,
                    r.timestamp,
                    r.description.as_deref(),
                    installed.as_deref() == Some(r.version.as_str()),
                ));
            }
            present_dialog("Historique des versions", &list, 480, 560, &parent);
        });
        list.append(&more);
    }

    container.append(&list);
    container.set_visible(true);
}

/// `box_license` : tuile licence à gauche, `links_stack` à droite.
fn build_license_box(container: &gtk::Box, meta: &AppMeta) {
    while let Some(c) = container.first_child() {
        container.remove(&c);
    }
    container.set_homogeneous(true);

    let get_involved = meta.help.clone().or_else(|| meta.homepage.clone());
    let tile = if meta.is_free_license == Some(true) {
        license_tile::community(meta.license.as_deref(), get_involved)
    } else {
        license_tile::proprietary(meta.license.as_deref(), meta.is_free_license)
    };
    container.append(&tile);

    // Ordre et icônes exacts de `gs-details-page.ui`. Pas de ligne « Contact » :
    // ni AppStream Flathub ni nixpkgs n'exposent d'URL de contact développeur.
    let links = [
        ("Site web du _projet", "webpage-symbolic", meta.homepage.clone()),
        ("Faire un _don", "donate-symbolic", meta.donation.clone()),
        ("Contribuer aux _traductions", "translations-symbolic", meta.translate.clone()),
        ("_Signaler un problème", "computer-fail-symbolic", meta.bugtracker.clone()),
        ("_Aide", "help-link-symbolic", meta.help.clone()),
    ];
    let present: Vec<_> = links
        .into_iter()
        .filter_map(|(label, icon, url)| {
            url.filter(|u| !u.trim().is_empty()).map(|u| (label, icon, u))
        })
        .collect();

    // Le `GtkStack` du `.ui` : page « links » ou page « empty ».
    let stack = gtk::Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    if present.is_empty() {
        stack.add_named(&no_metadata_card(), Some("empty"));
    } else {
        let list = boxed_list();
        for (label, icon, url) in present {
            list.append(&link_row(label, icon, &url));
        }
        stack.add_named(&list, Some("links"));
    }
    container.append(&stack);
}

/// Page « empty » du `links_stack` : grosse icône `?` et explication centrée.
fn no_metadata_card() -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    card.set_hexpand_set(true);
    card.add_css_class("card");

    let inner = gtk::Box::new(gtk::Orientation::Vertical, 8);
    inner.set_margin_top(14);
    inner.set_margin_bottom(14);
    inner.set_margin_start(14);
    inner.set_margin_end(14);
    inner.set_valign(gtk::Align::Center);
    inner.set_hexpand(true);
    inner.add_css_class("dim-label");

    let img = gtk::Image::from_icon_name("dialog-question-symbolic");
    img.set_pixel_size(96);
    img.set_margin_bottom(8);
    inner.append(&img);

    let title = gtk::Label::new(Some("Aucune métadonnée"));
    title.add_css_class("heading");
    inner.append(&title);

    let desc = gtk::Label::new(Some(
        "Ce paquet ne fournit aucun lien vers un site web, un dépôt de code ou un \
         suivi de bugs.",
    ));
    desc.set_justify(gtk::Justification::Center);
    desc.set_wrap(true);
    desc.set_xalign(0.5);
    inner.append(&desc);

    card.append(&inner);
    card
}

fn link_row(label: &str, icon: &str, url: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(label);
    row.set_use_underline(true);
    row.set_subtitle(&esc(url));
    row.set_subtitle_lines(1);
    row.set_subtitle_selectable(true);
    row.set_activatable(true);
    row.add_prefix(&gtk::Image::from_icon_name(icon));
    row.add_suffix(&gtk::Image::from_icon_name("external-link-symbolic"));
    let url = url.to_string();
    row.connect_activated(move |_| {
        let _ = gtk::gio::AppInfo::launch_default_for_uri(&url, None::<&gtk::gio::AppLaunchContext>);
    });
    row
}

/// Tuile-carte informative : même gabarit visuel qu'`AppTile` (icône 64 chargée
/// en asynchrone, vignette colorée de repli, nom + résumé 2 lignes) mais non
/// navigable — les extensions n'ont pas d'équivalent nixpkgs vers lequel ouvrir
/// une page détail.
fn info_tile(name: &str, summary: Option<&str>, icon_url: Option<String>) -> gtk::Widget {
    let icon_slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    icon_slot.set_width_request(64);
    icon_slot.set_height_request(64);
    icon_slot.set_valign(gtk::Align::Center);

    let fallback = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    fallback.set_width_request(64);
    fallback.set_height_request(64);
    fallback.set_halign(gtk::Align::Center);
    fallback.set_valign(gtk::Align::Center);
    fallback.set_hexpand(false);
    fallback.set_vexpand(false);
    fallback.add_css_class("fallback-tile");
    fallback.add_css_class(fallback_class(name));
    let glyph = gtk::Image::from_icon_name(FALLBACK_GLYPH);
    glyph.set_pixel_size(48);
    glyph.set_hexpand(true);
    glyph.set_vexpand(true);
    glyph.set_halign(gtk::Align::Center);
    glyph.set_valign(gtk::Align::Center);
    glyph.add_css_class("fallback-glyph");
    fallback.append(&glyph);
    icon_slot.append(&fallback);

    // gtk::Image (≠ Picture) : `pixel_size` borne le layout à 64 tout en
    // rendant la texture 128 en pleine résolution → net en HiDPI (cf. AppTile).
    let img = gtk::Image::new();
    img.set_pixel_size(64);
    img.set_visible(false);
    icon_slot.append(&img);

    if let Some(url) = icon_url {
        let img = img.clone();
        let fallback = fallback.clone();
        gtk::glib::spawn_future_local(async move {
            let bytes = gtk::gio::spawn_blocking(move || onyx_core::cached_fetch_bytes(&url))
                .await
                .ok()
                .flatten();
            if let Some(tex) = bytes.as_deref().and_then(|b| texture_from_bytes_sized(b, 128)) {
                img.set_paintable(Some(&tex));
                img.set_visible(true);
                fallback.set_visible(false);
            }
        });
    }

    let text = gtk::Box::new(gtk::Orientation::Vertical, 6);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);

    let name_label = gtk::Label::new(Some(name));
    name_label.set_xalign(0.0);
    name_label.set_max_width_chars(12);
    name_label.set_width_chars(0);
    name_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name_label.add_css_class("app-tile-name");
    text.append(&name_label);

    if let Some(summary) = summary {
        let s = gtk::Label::new(Some(summary));
        s.set_xalign(0.0);
        s.set_yalign(0.0);
        s.set_wrap(true);
        s.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        s.set_lines(2);
        s.set_max_width_chars(16);
        s.set_width_chars(0);
        s.set_ellipsize(gtk::pango::EllipsizeMode::End);
        s.add_css_class("app-tile-summary");
        text.append(&s);
    }

    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    inner.set_margin_top(17);
    inner.set_margin_bottom(17);
    inner.set_margin_start(17);
    inner.set_margin_end(17);
    inner.append(&icon_slot);
    inner.append(&text);

    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("card");
    card.set_width_request(190);
    card.set_hexpand(true);
    card.set_halign(gtk::Align::Fill);
    card.set_valign(gtk::Align::Start);
    card.append(&inner);
    card.upcast()
}

/// La grille des sections Extensions / Autres applications : même config de
/// `FlowBox` que les grilles de l'accueil (3 colonnes max, spacing 14).
fn tile_grid() -> gtk::FlowBox {
    let flow = gtk::FlowBox::new();
    flow.set_homogeneous(true);
    flow.set_column_spacing(14);
    flow.set_row_spacing(14);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(3);
    flow.set_hexpand(true);
    flow.set_valign(gtk::Align::Start);
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow
}

// ── Helpers de texte ──────────────────────────────────────────────────────────

fn meta_name(meta: &Option<AppMeta>, fallback: &str) -> String {
    meta.as_ref()
        .map(|x| x.name.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn meta_description(meta: &Option<AppMeta>, fallback: &str) -> String {
    meta.as_ref()
        .and_then(|x| x.description.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if fallback.is_empty() {
                "Pas de description.".to_string()
            } else {
                fallback.to_string()
            }
        })
}

/// Longueur au-delà de laquelle on replie la description (≈ 4 lignes, façon
/// `GsDescriptionBox::MAX_COLLAPSED_LINES`).
const DESC_COLLAPSE_CHARS: usize = 300;

fn desc_needs_toggle(meta: &Option<AppMeta>, fallback: &str) -> bool {
    meta_description(meta, fallback).chars().count() > DESC_COLLAPSE_CHARS
}

/// Description à afficher : tronquée sur une frontière de mot (+ « … ») si
/// repliée et trop longue, complète sinon.
fn desc_display(meta: &Option<AppMeta>, fallback: &str, expanded: bool) -> String {
    let full = meta_description(meta, fallback);
    if expanded || full.chars().count() <= DESC_COLLAPSE_CHARS {
        return full;
    }
    let mut end = full
        .char_indices()
        .nth(DESC_COLLAPSE_CHARS)
        .map(|(i, _)| i)
        .unwrap_or(full.len());
    if let Some(sp) = full[..end].rfind(char::is_whitespace) {
        end = sp;
    }
    format!("{}…", full[..end].trim_end())
}

/// Une fiche sans aucun contenu exploitable → page « failed », comme GNOME.
fn is_empty_meta(meta: &AppMeta) -> bool {
    meta.summary.is_none() && meta.description.is_none() && meta.version.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_bytes_and_kilo() {
        assert_eq!(format_size(42), "42 o");
        assert_eq!(format_size(1_500), "1,5 Ko");
    }

    #[test]
    fn format_size_mega_and_giga() {
        assert_eq!(format_size(206_039_799), "206,0 Mo");
        assert_eq!(format_size(1_640_000_000), "1,6 Go");
    }

    #[test]
    fn age_color_matches_gnome_rating_classes() {
        assert_eq!(age_color("18+"), "details-rating-18");
        assert_eq!(age_color("15"), "details-rating-15");
        assert_eq!(age_color("12"), "details-rating-12");
        assert_eq!(age_color("5"), "details-rating-5");
        assert_eq!(age_color("0"), "details-rating-0");
    }

    #[test]
    fn desc_display_truncates_on_word_boundary() {
        let long = "mot ".repeat(200);
        let out = desc_display(&None, &long, false);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= DESC_COLLAPSE_CHARS + 1);
        // Pas de coupe en plein milieu d'un mot.
        assert!(out.trim_end_matches('…').ends_with("mot"));
    }

    #[test]
    fn desc_display_expanded_returns_full_text() {
        let long = "mot ".repeat(200);
        assert_eq!(desc_display(&None, &long, true), long);
    }

    #[test]
    fn desc_display_leaves_short_text_alone() {
        assert_eq!(desc_display(&None, "court", false), "court");
        assert_eq!(desc_display(&None, "", false), "Pas de description.");
    }

    #[test]
    fn desc_needs_toggle_only_past_threshold() {
        assert!(!desc_needs_toggle(&None, &"a".repeat(DESC_COLLAPSE_CHARS)));
        assert!(desc_needs_toggle(&None, &"a".repeat(DESC_COLLAPSE_CHARS + 1)));
    }

    /// Vérifie l'arbre GTK réel contre `gs-details-page.ui`. Exige un display,
    /// donc `#[ignore]` : `cargo test -p onyx-gui -- --ignored`.
    #[test]
    #[ignore = "nécessite un display GTK"]
    fn widget_tree_matches_gnome_ui() {
        gtk::init().expect("gtk::init");

        let pkg = Package {
            attr: "hello".into(),
            name: "hello".into(),
            version: "2.12".into(),
            description: "Programme de démonstration".into(),
        };
        let ctrl = <DetailModel as relm4::Component>::builder().launch(pkg);
        let widgets = ctrl.widgets();

        // `box_details_header` n'a PAS de spacing dans le `.ui` : c'est
        // `box_details_header_not_icon` (spacing 6) qui écarte.
        assert_eq!(widgets.header_box.spacing(), 0, "box_details_header");
        // `application_details` : résumé ↔ description.
        assert_eq!(widgets.application_details.spacing(), 12);
        // `box_install_remove` : spacing 9, centré.
        assert_eq!(widgets.box_install_remove.spacing(), 9);
        assert_eq!(widgets.box_install_remove.halign(), gtk::Align::Center);

        // Le GtkStack porte les trois états du `.ui` et démarre sur « spinner ».
        let stack = &ctrl.model().stack;
        for name in ["spinner", "ready", "failed"] {
            assert!(stack.child_by_name(name).is_some(), "page « {name} » absente");
        }
        assert_eq!(stack.visible_child_name().as_deref(), Some("spinner"));

        // Le carousel démarre sur sa page de repli (aucune capture chargée).
        assert_eq!(
            ctrl.model().carousel.widget().visible_child_name().as_deref(),
            Some("fallback")
        );

        // `box_details` : spacing 18, valign start, classe `.details-page`.
        let body = widgets
            .scroller
            .child()
            .and_downcast::<gtk::Viewport>()
            .and_then(|v| v.child())
            .and_downcast::<gtk::Box>()
            .expect("body");
        assert!(body.has_css_class("details-page"));
        assert_eq!(body.spacing(), 18);
        assert_eq!(body.valign(), gtk::Align::Start);

        // Chaque section est clampée 860/576 marges 12 — sauf le carousel, qui
        // déborde en pleine largeur (donc hors clamp).
        let mut clamps = 0;
        let mut child = body.first_child();
        while let Some(w) = child {
            if let Some(c) = w.downcast_ref::<adw::Clamp>() {
                assert_eq!(c.maximum_size(), CLAMP_MAX);
                assert_eq!(c.tightening_threshold(), CLAMP_TIGHTEN);
                assert_eq!(c.margin_start(), 12);
                assert_eq!(c.margin_end(), 12);
                clamps += 1;
            }
            child = w.next_sibling();
        }
        // en-tête, résumé, extensions, context bar, versions, infobar, licence,
        // avis, autres applis — le carousel est le seul enfant non clampé.
        assert_eq!(clamps, 9, "9 sections clampées attendues");

        // Un SEUL #[test] GTK : le main context GLib reste acquis par le thread
        // du premier test GTK, même en --test-threads=1 — un second #[test]
        // échouerait sur « Failed to acquire default main context ». Les
        // assertions supplémentaires vivent donc en helpers appelés ici.
        assert_addons_render_as_tile_grid();
    }

    /// Les extensions se rendent en **grille de tuiles-cartes** (`info_tile`),
    /// même gabarit qu'`AppTile` mais non navigable (les addons Flatpak n'ont
    /// pas d'équivalent nixpkgs à ouvrir).
    fn assert_addons_render_as_tile_grid() {
        let meta = AppMeta {
            addons: vec![onyx_core::Addon {
                name: "Highlighter".into(),
                summary: Some("Coloration syntaxique".into()),
                icon_url: None,
            }],
            ..Default::default()
        };

        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        build_addons(&container, &meta);
        assert!(container.is_visible());

        let flow = container
            .last_child()
            .and_downcast::<gtk::FlowBox>()
            .expect("la section doit se terminer par une grille FlowBox");
        assert_eq!(flow.max_children_per_line(), 3);

        let tile = flow
            .first_child()
            .and_downcast::<gtk::FlowBoxChild>()
            .and_then(|c| c.child())
            .expect("une tuile");
        assert!(tile.has_css_class("card"), "la tuile doit être une carte");
    }
}
