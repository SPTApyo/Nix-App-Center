//! A search result row; a click opens the details page, where the actions
//! are.

use adw::prelude::*;
use nix_software_core::{Package, Rating};
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};

use crate::util::{fallback_icon, texture_from_bytes_sized, AppItem, IconState};

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
    type Output = Package; // activation opens the details page
    type CommandOutput = ();
    type ParentWidget = gtk::ListBox;

    view! {
        // GNOME Software's GsAppRow without its button.
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
                    // Keep the image from stretching the row.
                    set_hexpand: false,
                    set_vexpand: false,
                    add_css_class: "fallback-tile",
                    add_css_class: fallback_icon(&self.package).1,

                    gtk::Image {
                        set_icon_name: Some(fallback_icon(&self.package).0),
                        set_pixel_size: 48,
                        // Needed to center the glyph.
                        set_hexpand: true,
                        set_vexpand: true,
                        set_halign: gtk::Align::Center,
                        set_valign: gtk::Align::Center,
                        add_css_class: "fallback-glyph",
                    },
                },
                // Image: 64 px layout, full resolution texture.
                gtk::Image {
                    #[watch]
                    set_visible: self.icon_state == IconState::Real,
                    #[watch]
                    set_paintable: self.icon_tex.as_ref(),
                    set_pixel_size: 64,
                },
            },

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
        let AppItem {
            package,
            icon_url,
            rating,
            ..
        } = item;
        let icon_state = if icon_url.is_some() {
            IconState::Loading
        } else {
            IconState::Generic
        };
        if let Some(url) = icon_url {
            nix_software_core::remember_icon_url(&package.attr, &url);
            // Safe after the row is destroyed.
            let s = sender.input_sender().clone();
            std::thread::spawn(move || {
                let _ = s.send(RowMsg::IconLoaded(nix_software_core::cached_fetch_bytes(
                    &url,
                )));
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
                self.icon_tex = bytes
                    .as_deref()
                    .and_then(|b| texture_from_bytes_sized(b, 128));
                self.icon_state = if self.icon_tex.is_some() {
                    IconState::Real
                } else {
                    IconState::Generic
                };
            }
        }
    }
}

/// Icon of star `pos` (1 to 5): full, half or empty.
fn star_icon(rating: Option<Rating>, pos: i32) -> &'static str {
    match rating {
        Some(r) if r.stars >= pos as f32 => "starred-symbolic",
        Some(r) if r.stars >= pos as f32 - 0.5 => "semi-starred-symbolic",
        _ => "non-starred-symbolic",
    }
}

/// CSS class of star `pos`: lit or dimmed.
fn star_class(rating: Option<Rating>, pos: i32) -> &'static str {
    match rating {
        Some(r) if r.stars >= pos as f32 - 0.5 => "star-on",
        _ => "star-off",
    }
}

/// "(count)", empty without a rating.
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

/// `s` cut to `max` characters with "…".
fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    format!("{}…", s[..end].trim_end())
}
