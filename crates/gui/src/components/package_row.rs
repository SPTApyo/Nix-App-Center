//! `PackageRow` — la ligne d'app de la **recherche** (seul endroit en liste).
//! Purement navigationnelle : cliquer n'importe où ouvre le détail. Les actions
//! (lancer / garder / shell / copier) vivent toutes sur la page détail, dans un
//! split-button unique — une ligne, un seul geste, une seule cible.

use adw::prelude::*;
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};
use onyx_core::{Package, Rating};

use crate::util::{fallback_class, texture_from_bytes_sized, AppItem, IconState, FALLBACK_GLYPH};

pub struct PackageRow {
    package: Package,
    rating: Option<Rating>,
    icon_tex: Option<gtk::gdk::Texture>,
    icon_state: IconState,
}

#[derive(Debug)]
pub enum RowMsg {
    Open,
    IconLoaded(Option<Vec<u8>>),
}

#[relm4::factory(pub)]
impl FactoryComponent for PackageRow {
    type Init = AppItem;
    type Input = RowMsg;
    type Output = Package; // activation → ouvre la page détail
    type CommandOutput = ();
    type ParentWidget = gtk::ListBox;

    view! {
        // `GsAppRow` (gs-app-row.ui) sans son bouton : [icône | vbox(nom, résumé,
        // étoiles)]. Row activable (clic → détail) via un geste.
        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Horizontal,
            set_spacing: 14,
            set_margin_top: 10,
            set_margin_bottom: 10,
            set_margin_start: 12,
            set_margin_end: 12,

            add_controller = gtk::GestureClick {
                connect_released[sender] => move |_, _, _, _| {
                    sender.input(RowMsg::Open);
                }
            },

            // Icône 64 : shimmer (chargement) → vignette générique → icône réelle.
            gtk::Box {
                set_width_request: 64,
                set_height_request: 64,
                set_valign: gtk::Align::Center,

                gtk::Box {
                    #[watch]
                    set_visible: self.icon_state == IconState::Loading,
                    set_width_request: 64,
                    set_height_request: 64,
                    add_css_class: "icon-shimmer",
                },
                gtk::Box {
                    #[watch]
                    set_visible: self.icon_state == IconState::Generic,
                    set_width_request: 64,
                    set_height_request: 64,
                    set_halign: gtk::Align::Center,
                    set_valign: gtk::Align::Center,
                    // Bloque la remontée du hexpand de l'image interne : sinon la
                    // colonne d'icônes s'étire et désaligne toute la ligne.
                    set_hexpand: false,
                    set_vexpand: false,
                    add_css_class: "fallback-tile",
                    add_css_class: fallback_class(&self.package.name),

                    gtk::Image {
                        set_icon_name: Some(FALLBACK_GLYPH),
                        set_pixel_size: 48,
                        // hexpand/vexpand : sans ça halign Center est ignoré sur
                        // l'axe principal de la Box → glyphe collé à gauche.
                        set_hexpand: true,
                        set_vexpand: true,
                        set_halign: gtk::Align::Center,
                        set_valign: gtk::Align::Center,
                        add_css_class: "fallback-glyph",
                    },
                },
                // gtk::Image : `pixel_size` borne le layout à 64 tout en rendant
                // la texture 128 en pleine résolution → net en HiDPI.
                gtk::Image {
                    #[watch]
                    set_visible: self.icon_state == IconState::Real,
                    #[watch]
                    set_paintable: self.icon_tex.as_ref(),
                    set_pixel_size: 64,
                },
            },

            // Nom · résumé · étoiles (empilés, façon GsAppRow).
            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_valign: gtk::Align::Center,
                set_hexpand: true,
                set_spacing: 2,

                gtk::Label {
                    set_label: &self.package.name,
                    set_xalign: 0.0,
                    set_ellipsize: gtk::pango::EllipsizeMode::End,
                    add_css_class: "heading",
                },
                gtk::Label {
                    set_label: &row_subtitle(&self.package),
                    set_xalign: 0.0,
                    set_ellipsize: gtk::pango::EllipsizeMode::End,
                    add_css_class: "dim-label",
                },

                // Note ODRS (GsStarWidget) : 5 étoiles + nombre d'avis, SOUS le résumé.
                gtk::Box {
                    set_visible: self.rating.is_some(),
                    set_margin_top: 3,
                    set_spacing: 1,
                    add_css_class: "app-stars",

                    gtk::Image { set_icon_name: Some(star_icon(self.rating, 1)), set_pixel_size: 14, add_css_class: star_class(self.rating, 1) },
                    gtk::Image { set_icon_name: Some(star_icon(self.rating, 2)), set_pixel_size: 14, add_css_class: star_class(self.rating, 2) },
                    gtk::Image { set_icon_name: Some(star_icon(self.rating, 3)), set_pixel_size: 14, add_css_class: star_class(self.rating, 3) },
                    gtk::Image { set_icon_name: Some(star_icon(self.rating, 4)), set_pixel_size: 14, add_css_class: star_class(self.rating, 4) },
                    gtk::Image { set_icon_name: Some(star_icon(self.rating, 5)), set_pixel_size: 14, add_css_class: star_class(self.rating, 5) },

                    gtk::Label {
                        set_label: &review_count(self.rating),
                        set_margin_start: 5,
                        add_css_class: "dim-label",
                        add_css_class: "caption",
                    },
                },
            },

        }
    }

    fn init_model(item: Self::Init, _idx: &DynamicIndex, sender: FactorySender<Self>) -> Self {
        let AppItem { package, icon_url, rating, .. } = item;
        let icon_state = if icon_url.is_some() {
            IconState::Loading
        } else {
            IconState::Generic
        };
        if let Some(url) = icon_url {
            onyx_core::remember_icon_url(&package.attr, &url);
            // input_sender().send() (≠ input()) ne panique pas si la ligne a été
            // détruite entre-temps (clear lors d'une recherche/filtre).
            let s = sender.input_sender().clone();
            std::thread::spawn(move || {
                let _ = s.send(RowMsg::IconLoaded(onyx_core::cached_fetch_bytes(&url)));
            });
        }
        Self {
            package,
            rating,
            icon_tex: None,
            icon_state,
        }
    }

    fn update(&mut self, msg: Self::Input, sender: FactorySender<Self>) {
        match msg {
            RowMsg::Open => {
                let _ = sender.output(self.package.clone());
            }
            RowMsg::IconLoaded(bytes) => {
                self.icon_tex = bytes.as_deref().and_then(|b| texture_from_bytes_sized(b, 128));
                self.icon_state = if self.icon_tex.is_some() {
                    IconState::Real
                } else {
                    IconState::Generic
                };
            }
        }
    }
}

/// Icône de la `pos`-ième étoile (1..5) pour une note, façon `GsStarWidget` :
/// pleine si `note ≥ pos`, demi si `note ≥ pos-0.5`, vide sinon.
fn star_icon(rating: Option<Rating>, pos: i32) -> &'static str {
    match rating {
        Some(r) if r.stars >= pos as f32 => "starred-symbolic",
        Some(r) if r.stars >= pos as f32 - 0.5 => "semi-starred-symbolic",
        _ => "non-starred-symbolic",
    }
}

/// Classe CSS de la `pos`-ième étoile : dorée (`star-on`) si pleine/demi, sinon
/// atténuée (`star-off`). Couleurs définies dans `APP_CSS`.
fn star_class(rating: Option<Rating>, pos: i32) -> &'static str {
    match rating {
        Some(r) if r.stars >= pos as f32 - 0.5 => "star-on",
        _ => "star-off",
    }
}

/// Nombre d'avis entre parenthèses (vide si pas de note).
fn review_count(rating: Option<Rating>) -> String {
    rating.map(|r| format!("({})", r.total)).unwrap_or_default()
}

fn row_subtitle(pkg: &Package) -> String {
    if !pkg.description.is_empty() {
        truncate(&pkg.description, 100)
    } else if pkg.version.is_empty() {
        pkg.attr.clone()
    } else {
        format!("{} · {}", pkg.version, pkg.attr)
    }
}

/// Tronque à `max` caractères avec « … » si dépassement (cf. tuiles).
fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    format!("{}…", s[..end].trim_end())
}
