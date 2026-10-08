//! An app tile of the category grids; a click opens its page.

use adw::prelude::*;
use nix_software_core::Package;
use relm4::factory::{DynamicIndex, FactoryComponent, FactorySender};

use crate::util::{fallback_icon, texture_from_bytes_sized, AppItem, IconState};

pub struct AppTile {
    package: Package,
    icon_tex: Option<gtk::gdk::Texture>,
    icon_state: IconState,
}

#[derive(Debug)]
pub enum TileMsg {
    Open,
    IconLoaded(Option<Vec<u8>>),
}

#[relm4::factory(pub)]
impl FactoryComponent for AppTile {
    type Init = AppItem;
    type Input = TileMsg;
    type Output = Package; // click opens the details page
    type CommandOutput = ();
    type ParentWidget = gtk::FlowBox;

    view! {
        // Sized like GNOME Software's GsSummaryTile.
        #[root]
        gtk::Button {
            add_css_class: "card",
            set_width_request: 190,
            set_hexpand: true,
            set_halign: gtk::Align::Fill,
            set_valign: gtk::Align::Start,
            connect_clicked => TileMsg::Open,

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,
                set_spacing: 14,
                set_margin_top: 17,
                set_margin_bottom: 17,
                set_margin_start: 17,
                set_margin_end: 17,

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
                        // Keep the image from stretching the tile.
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
                    set_spacing: 6,

                    gtk::Label {
                        set_label: &self.package.name,
                        set_xalign: 0.0,
                        // Three columns fit the clamp.
                        set_max_width_chars: 12,
                        set_width_chars: 0,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        add_css_class: "app-tile-name",
                    },
                    gtk::Label {
                        set_label: &truncate_desc(&self.package.description),
                        set_xalign: 0.0,
                        set_yalign: 0.0,
                        set_wrap: true,
                        // Long words must not widen the tile.
                        set_wrap_mode: gtk::pango::WrapMode::WordChar,
                        set_lines: 2,
                        set_max_width_chars: 16,
                        set_width_chars: 0,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        add_css_class: "app-tile-summary",
                    },
                },
            },
        }
    }

    fn init_model(init: Self::Init, _idx: &DynamicIndex, sender: FactorySender<Self>) -> Self {
        let AppItem {
            package, icon_url, ..
        } = init;
        let icon_state = if icon_url.is_some() {
            IconState::Loading
        } else {
            IconState::Generic
        };
        if let Some(url) = icon_url {
            nix_software_core::remember_icon_url(&package.attr, &url);
            // Safe after the tile is destroyed.
            let s = sender.input_sender().clone();
            std::thread::spawn(move || {
                let _ = s.send(TileMsg::IconLoaded(nix_software_core::cached_fetch_bytes(
                    &url,
                )));
            });
        }
        Self {
            package,
            icon_tex: None,
            icon_state,
        }
    }

    fn update(&mut self, msg: Self::Input, sender: FactorySender<Self>) {
        match msg {
            TileMsg::Open => {
                let _ = sender.output(self.package.clone());
            }
            TileMsg::IconLoaded(bytes) => {
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

/// `s` cut with "…" when too long.
fn truncate_desc(s: &str) -> String {
    const MAX: usize = 64;
    let s = s.trim();
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let end = s.char_indices().nth(MAX).map(|(i, _)| i).unwrap_or(s.len());
    format!("{}…", s[..end].trim_end())
}
