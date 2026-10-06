//! `PackageRow`: the app row of **search** (the only list in the app).
//! Purely navigational: clicking anywhere opens the details page. Actions
//! (run / keep / shell / copy) all live on the details page, in a single
//! split button: one row, one gesture, one target.

use adw::prelude::*;
use onyx_core::{Package, Rating};
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};

use crate::util::{fallback_icon, texture_from_bytes_sized, AppItem, IconState};

pub struct PackageRow {
    package: Package,
    rating: Option<Rating>,
    icon_tex: Option<gtk::gdk::Texture>,
    icon_state: IconState,
    /// Position when created, for the cascade.
    appear: usize,
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
        // `GsAppRow` (gs-app-row.ui) without its button: [icon | vbox(name,
        // summary, stars)]. The row is activated (click to details) by a gesture.
        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Horizontal,
            set_spacing: 14,
            // Fades in after the items before it.
            add_css_class: "appear",
            add_css_class: crate::util::appear_classes(self.appear)[1],
            set_margin_top: 10,
            set_margin_bottom: 10,
            set_margin_start: 12,
            set_margin_end: 12,

            add_controller = gtk::GestureClick {
                connect_released[sender] => move |_, _, _, _| {
                    sender.input(RowMsg::Open);
                }
            },

            // 64 px icon: shimmer (loading), generic thumbnail, real icon.
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
                    // Stops the inner image hexpand from propagating, else the
                    // icon column stretches and misaligns the whole row.
                    set_hexpand: false,
                    set_vexpand: false,
                    add_css_class: "fallback-tile",
                    add_css_class: fallback_icon(&self.package).1,

                    gtk::Image {
                        set_icon_name: Some(fallback_icon(&self.package).0),
                        set_pixel_size: 48,
                        // hexpand/vexpand: without them halign Center is ignored
                        // on the Box main axis and the glyph sticks to the left.
                        set_hexpand: true,
                        set_vexpand: true,
                        set_halign: gtk::Align::Center,
                        set_valign: gtk::Align::Center,
                        add_css_class: "fallback-glyph",
                    },
                },
                // gtk::Image: `pixel_size` bounds the layout to 64 while the
                // 128 texture renders at full resolution, sharp on HiDPI.
                gtk::Image {
                    #[watch]
                    set_visible: self.icon_state == IconState::Real,
                    #[watch]
                    set_paintable: self.icon_tex.as_ref(),
                    set_pixel_size: 64,
                },
            },

            // Name · summary · stars (stacked, like GsAppRow).
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

                // ODRS rating (GsStarWidget): 5 stars + review count, BELOW the summary.
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

    fn init_model(item: Self::Init, idx: &DynamicIndex, sender: FactorySender<Self>) -> Self {
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
            onyx_core::remember_icon_url(&package.attr, &url);
            // input_sender().send() (unlike input()) does not panic if the row
            // was destroyed meanwhile (cleared by a new search or filter).
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
            appear: idx.current_index(),
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

/// Icon of the `pos`-th star (1..5) for a rating, like `GsStarWidget`:
/// full if `rating ≥ pos`, half if `rating ≥ pos-0.5`, empty otherwise.
fn star_icon(rating: Option<Rating>, pos: i32) -> &'static str {
    match rating {
        Some(r) if r.stars >= pos as f32 => "starred-symbolic",
        Some(r) if r.stars >= pos as f32 - 0.5 => "semi-starred-symbolic",
        _ => "non-starred-symbolic",
    }
}

/// CSS class of the `pos`-th star: golden (`star-on`) when full or half,
/// dimmed (`star-off`) otherwise. Colors are defined in `APP_CSS`.
fn star_class(rating: Option<Rating>, pos: i32) -> &'static str {
    match rating {
        Some(r) if r.stars >= pos as f32 - 0.5 => "star-on",
        _ => "star-off",
    }
}

/// Review count in parentheses (empty without a rating).
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

/// Truncates to `max` characters with "…" when longer (see tiles).
fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    format!("{}…", s[..end].trim_end())
}
