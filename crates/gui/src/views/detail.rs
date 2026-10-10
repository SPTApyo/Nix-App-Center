//! The details page, laid out like GNOME Software's. The sections are
//! widgets from `components::details`; this file assembles and loads them.

use adw::prelude::*;
use nix_software_core::{AppMeta, Package, Rating, Review};
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;

use crate::components::details::{
    attach_dialog, context_bar, infobox, license_tile, lozenge::Content, nix_editor,
    review_histogram, review_row, screenshot_carousel::ScreenshotCarousel, version_history_row,
};
use crate::components::AppTile;
use crate::i18n::t;
use crate::util::{
    esc, fallback_icon, stars_widget, texture_from_bytes, texture_from_bytes_sized, AppItem,
};

/// Section width.
const CLAMP_MAX: i32 = 860;
const CLAMP_TIGHTEN: i32 = 576;

/// Icon of the origin button.
const ORIGIN_ICON: &str = "nix-snowflake";

pub struct DetailModel {
    package: Package,
    meta: Option<AppMeta>,
    loading: bool,
    /// The app as a Nix profile already holds it.
    installed: Option<nix_software_core::ProfileApp>,
    /// True when Nix Software writes the config; else `nix profile`.
    managed: bool,
    /// The `nix profile` element of this package.
    profile_name: Option<String>,
    profile_busy: bool,
    kept: bool,
    /// The NixOS module that sets the app up.
    module: Option<nix_software_core::NixOption>,
    /// The revision the app is held at.
    pin: Option<nix_software_core::Pin>,
    holding: bool,
    /// The app has its own user block, which a hold cannot rewrite.
    custom_blocks: bool,
    icon_tex: Option<gtk::gdk::Texture>,
    // Loaded apart; the flag tells pending from unknown.
    size_loaded: bool,
    closure_size: Option<u64>,
    desc_expanded: bool,
    desc_needs_toggle: bool,
    // Screen scale, so images never upscale.
    scale: i32,
    // Feeds the header stars and the histogram.
    rating: Option<Rating>,
    shots: Vec<gtk::gdk::Texture>,
    shot_widths: Vec<i32>,
    lightbox_tex: Option<gtk::gdk::Texture>,
    lightbox_clamp: adw::Clamp,
    lightbox_visible: bool,
    stack: gtk::Stack,
    carousel: ScreenshotCarousel,
    star_box: gtk::Box,
    installs_label: gtk::Label,
    ctx_bar: gtk::Box,
    addons_box: gtk::Box,
    version_box: gtk::Box,
    problems_box: gtk::Box,
    license_box: gtk::Box,
    build_box: gtk::Box,
    /// One switch per recipe flag, with the recipe default.
    flag_rows: Vec<(String, bool, adw::SwitchRow)>,
    reviews_box: gtk::Box,
    dev_apps_box: gtk::Box,
    dev_apps_heading: gtk::Label,
    dev_tiles: FactoryVecDeque<AppTile>,
    toast_overlay: adw::ToastOverlay,
}

#[derive(Debug)]
pub enum DetailMsg {
    Loaded(Box<AppMeta>),
    LoadFailed,
    RatingLoaded(Rating),
    InstallsLoaded(u64),
    ReviewsLoaded(Vec<Review>),
    IconLoaded(Vec<u8>),
    ScreenshotLoaded(Vec<u8>, Option<String>),
    ShowShot(usize),
    DismissShot,
    /// Main button: run an app, open a shell for a CLI tool.
    Primary,
    /// Launches the installed copy.
    Open,
    /// The Flathub app shown is not this package.
    ReportMatch,
    Unmatch,
    /// `nix profile` add or remove, without a managed config.
    ProfileToggle,
    ProfileDone(Result<Option<String>, String>),
    /// The launch mode the primary button did not take.
    AltLaunch,
    Run,
    /// Keeps, or stops keeping.
    ToggleKeep,
    Keep,
    Unkeep,
    /// `nix shell` in a terminal, the try-first path for CLI tools.
    Shell,
    CopyCommand,
    /// Opens the Nix editor.
    AdvancedEdit,
    /// Blocks saved; Ok means the app is kept.
    BlocksSaved(Result<(), String>),
    /// Developer apps found in nixpkgs.
    DevAppsResolved(Vec<(Package, Option<String>)>),
    /// An "Other Apps" tile clicked.
    OpenApp(Package),
    /// `nix run` failed, with its last error line.
    RunFailed(String),
    SizeLoaded(Option<u64>),
    ModuleLoaded(Option<nix_software_core::NixOption>),
    /// Keeps the app with the switched build options.
    SaveBuildOptions,
    /// Holds the app at the revision in use, or releases it.
    ToggleHold,
    /// Holds the app at an earlier version.
    HoldAt(nix_software_core::VersionChoice),
    ChooseVersion,
    ChooseMode,
    CopyName,
    Remove,
    TriedRemoved(Result<u64, String>),
    SetMode(nix_software_core::InstallMode),
    VersionsLoaded(Vec<nix_software_core::VersionChoice>),
    Held(Result<Option<nix_software_core::Pin>, String>),
    /// Keeps the app through its NixOS module.
    UseModule,
    ShowModule,
    ToggleDescription,
    /// Applies the configuration.
    ApplyConfig,
    Applied(Result<nix_software_core::ApplyOutcome, String>),
}

impl DetailModel {
    /// After keeping: an apply toast when attached, else the attach dialog.
    fn prompt_apply_or_attach(&self, sender: &ComponentSender<Self>) {
        let layout = nix_software_core::detect_layout();
        if nix_software_core::is_attached(&layout) {
            apply_toast(
                &self.toast_overlay,
                sender,
                &t("Added to your configuration. Apply it to install."),
            );
        } else {
            let overlay = self.toast_overlay.clone();
            let sender = sender.clone();
            attach_dialog::present(
                &self.toast_overlay,
                nix_software_core::attach_plan(&layout),
                move |res| match res {
                    Ok(()) => apply_toast(
                        &overlay,
                        &sender,
                        &t("Connected to your configuration. Apply it to install."),
                    ),
                    Err(e) => crate::util::toast(
                        &overlay,
                        &t("Could not connect: {}").replace("{}", &crate::util::tr_err(&e)),
                    ),
                },
            );
        }
    }
}

/// A toast with an Apply button.
fn apply_toast(overlay: &adw::ToastOverlay, sender: &ComponentSender<DetailModel>, text: &str) {
    let toast = adw::Toast::new(text);
    toast.set_button_label(Some(&t("Apply")));
    let s = sender.input_sender().clone();
    toast.connect_button_clicked(move |_| {
        let _ = s.send(DetailMsg::ApplyConfig);
    });
    overlay.add_toast(toast);
}

#[relm4::component(pub)]
impl SimpleComponent for DetailModel {
    type Init = Package;
    type Input = DetailMsg;
    /// A package to open in a new page.
    type Output = Package;

    view! {
        adw::NavigationPage {
            set_title: &model.package.name,

            #[wrap(Some)]
            set_child = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {},

                #[wrap(Some)]
                #[name(ready_page)]
                set_content = &gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,

                    // Never revealed: nixpkgs has no translation data.
                    adw::Banner {
                        set_button_label: Some(&t("Help _Translate")),
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

                                // 1. Header
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[name(header_box)]
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Horizontal,
                                        set_spacing: 24,

                                        // Colored fallback when there is no icon.
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
                                                set_hexpand: false,
                                                set_vexpand: false,
                                                add_css_class: "fallback-tile",
                                                add_css_class: fallback_icon(&model.package).1,

                                                gtk::Image {
                                                    set_icon_name: Some(fallback_icon(&model.package).0),
                                                    set_pixel_size: 96,
                                                    // Needed to center the glyph.
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

                                        gtk::Box {
                                            set_orientation: gtk::Orientation::Horizontal,
                                            set_margin_top: 6,
                                            set_spacing: 6,
                                            set_hexpand: true,

                                            gtk::Box {
                                                set_orientation: gtk::Orientation::Vertical,
                                                set_halign: gtk::Align::Fill,
                                                set_valign: gtk::Align::Center,
                                                set_hexpand: true,
                                                set_spacing: 6,

                                                gtk::Box {
                                                    set_spacing: 6,

                                                    gtk::Label {
                                                        #[watch]
                                                        set_label: &meta_name(&model.meta, &model.package.name),
                                                        set_halign: gtk::Align::Fill,
                                                        set_valign: gtk::Align::Start,
                                                        set_hexpand: false,
                                                        set_xalign: 0.0,
                                                        set_selectable: true,
                                                        // Else it takes the initial focus.
                                                        set_focusable: false,
                                                        set_wrap: true,
                                                        set_max_width_chars: 20,
                                                        add_css_class: "app-title",
                                                        add_css_class: "title-1",
                                                    },
                                                    gtk::Button {
                                                        set_icon_name: "edit-copy-symbolic",
                                                        set_valign: gtk::Align::Center,
                                                        #[watch]
                                                        set_tooltip_text: Some(&t("Copy the package name to write in a configuration: pkgs.{}").replace("{}", &model.package.attr)),
                                                        add_css_class: "flat",
                                                        add_css_class: "circular",
                                                        connect_clicked => DetailMsg::CopyName,
                                                    },
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

                                                // Verified badge, in the Nix color.
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
                                                            &t("The developer identity is verified")),
                                                        add_css_class: "nix-packaging-color",
                                                    },
                                                    gtk::Label {
                                                        set_label: &t("Verified"),
                                                        set_xalign: 0.0,
                                                        set_yalign: 0.5,
                                                        set_hexpand: false,
                                                        add_css_class: "nix-packaging-color",
                                                    },
                                                },

                                                #[local_ref]
                                                star_box -> gtk::Box {
                                                    set_orientation: gtk::Orientation::Horizontal,
                                                    set_valign: gtk::Align::Start,
                                                    set_visible: false,
                                                },

                                                #[local_ref]
                                                installs_label -> gtk::Label {
                                                    set_xalign: 0.0,
                                                    set_visible: false,
                                                    add_css_class: "dim-label",
                                                    add_css_class: "caption",
                                                },
                                            },

                                            gtk::Box {
                                                set_orientation: gtk::Orientation::Vertical,
                                                set_halign: gtk::Align::End,
                                                set_hexpand: false,
                                                set_valign: gtk::Align::Center,
                                                set_margin_top: 6,
                                                set_spacing: 6,

                                                // Primary: try. Menu: everything else.
                                                #[name(box_install_remove)]
                                                gtk::Box {
                                                    add_css_class: "linked",
                                                    add_css_class: "action-group",
                                                    set_orientation: gtk::Orientation::Horizontal,
                                                    set_halign: gtk::Align::Center,

                                                    gtk::Button {
                                                        #[watch]
                                                        set_label: &if model.installed.is_some() {
                                                            t("_Open")
                                                        } else if model.cli_first() {
                                                            t("Try in a _Shell")
                                                        } else {
                                                            t("_Try")
                                                        },
                                                        set_use_underline: true,
                                                        #[watch]
                                                        set_tooltip_text: Some(&if model.is_self() {
                                                            t("This is the app you are using.")
                                                        } else if model.installed.is_some() {
                                                            t("Open the installed app")
                                                        } else if model.cli_first() {
                                                            t("Open a terminal where this package can be used. Nothing is added to your system.")
                                                        } else {
                                                            t("Start the app without installing it. Nothing is added to your system.")
                                                        }),
                                                        set_valign: gtk::Align::Center,
                                                        add_css_class: "suggested-action",
                                                        connect_clicked => DetailMsg::Primary,
                                                    },
                                                    gtk::Button {
                                                        set_icon_name: "user-trash-symbolic",
                                                        #[watch]
                                                        set_visible: model.removable(),
                                                        #[watch]
                                                        set_tooltip_text: Some(&model.remove_tooltip()),
                                                        add_css_class: "suggested-action",
                                                        connect_clicked => DetailMsg::Remove,
                                                    },
                                                    gtk::MenuButton {
                                                        set_icon_name: "pan-down-symbolic",
                                                        set_tooltip_text: Some(&t("More Actions")),
                                                        set_valign: gtk::Align::Center,
                                                        add_css_class: "suggested-action",

                                                        #[wrap(Some)]
                                                        #[name(actions_popover)]
                                                        set_popover = &gtk::Popover {
                                                            gtk::Box {
                                                                set_orientation: gtk::Orientation::Vertical,
                                                                set_spacing: 4,

                                                                gtk::Button {
                                                                    #[watch]
                                                                    set_label: &model.keep_label(),
                                                                    #[watch]
                                                                    set_sensitive: !model.profile_busy && !model.installed_elsewhere(),
                                                                    set_tooltip_text: Some(&model.keep_tooltip()),
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::ToggleKeep,
                                                                },
                                                                gtk::Button {
                                                                    #[watch]
                                                                    set_label: &if model.cli_first() { t("Try the App") } else { t("Try in a Shell") },
                                                                    #[watch]
                                                                    set_visible: model.installed.is_none() && !model.is_self(),
                                                                    set_tooltip_text: Some(&if model.cli_first() { t("Start the app now, without installing it.") } else { t("Open a terminal where this package can be used, without installing it.") }),
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::AltLaunch,
                                                                },
                                                                gtk::Button {
                                                                    set_label: &t("Copy Command"),
                                                                    set_tooltip_text: Some(&t("Copy the command that tries this package, to paste in a terminal.")),
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::CopyCommand,
                                                                },
                                                                gtk::Button {
                                                                    set_label: &t("Advanced Options…"),
                                                                    #[watch]
                                                                    set_sensitive: !model.installed_elsewhere(),
                                                                    set_tooltip_text: Some(&t("Write your own Nix for this app: services, options, overrides.")),
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::AdvancedEdit,
                                                                },
                                                                gtk::Button {
                                                                    set_label: &t("Version…"),
                                                                    #[watch]
                                                                    set_visible: model.can_hold(),
                                                                    #[watch]
                                                                    set_sensitive: !model.holding,
                                                                    set_tooltip_text: Some(&t("Keep the installed version, or pick an earlier one.")),
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::ChooseVersion,
                                                                },
                                                                gtk::Button {
                                                                    set_label: &t("Install As…"),
                                                                    #[watch]
                                                                    set_visible: model.kept && model.mode().is_some(),
                                                                    set_tooltip_text: Some(&t("Install this app another way: in your home, for every user, or through its module.")),
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::ChooseMode,
                                                                },
                                                                gtk::Separator {
                                                                    set_orientation: gtk::Orientation::Horizontal,
                                                                },
                                                                gtk::Button {
                                                                    set_label: &t("Apply Configuration"),
                                                                    set_tooltip_text: Some(&t("Rebuild your system now with the apps you added or removed.")),
                                                                    add_css_class: "flat",
                                                                    connect_clicked => DetailMsg::ApplyConfig,
                                                                },
                                                            },
                                                        },
                                                    },
                                                },

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
                                                                    set_subtitle: &t("Nix package collection"),
                                                                },
                                                            },
                                                        },
                                                    },
                                                },
                                            },
                                        },
                                    },
                                },

                                // 2. Screenshots, full width
                                #[local_ref]
                                carousel_widget -> gtk::Stack {},

                                // 3. Summary and description
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

                                        gtk::Box {
                                            #[watch]
                                            set_visible: model.loading,
                                            set_orientation: gtk::Orientation::Vertical,
                                            set_spacing: 8,

                                            gtk::Box { add_css_class: "skeleton", set_height_request: 13, set_hexpand: true },
                                            gtk::Box { add_css_class: "skeleton", set_height_request: 13, set_hexpand: true },
                                            gtk::Box { add_css_class: "skeleton", set_height_request: 13, set_width_request: 320, set_halign: gtk::Align::Start },
                                        },
                                        gtk::Stack {
                                            #[watch]
                                            set_visible: !model.loading,
                                            set_transition_type: gtk::StackTransitionType::Crossfade,
                                            set_transition_duration: 280,
                                            set_interpolate_size: true,
                                            set_vhomogeneous: false,
                                            set_hexpand: true,

                                            add_named[Some("short")] = &gtk::Label {
                                                #[watch]
                                                set_label: &desc_display(&model.meta, &model.package.description, false),
                                                set_halign: gtk::Align::Fill,
                                                set_valign: gtk::Align::Start,
                                                set_wrap: true,
                                                set_xalign: 0.0,
                                            },
                                            add_named[Some("full")] = &gtk::Label {
                                                #[watch]
                                                set_label: &desc_display(&model.meta, &model.package.description, true),
                                                set_halign: gtk::Align::Fill,
                                                set_valign: gtk::Align::Start,
                                                set_wrap: true,
                                                set_xalign: 0.0,
                                            },
                                            #[watch]
                                            set_visible_child_name: if model.desc_expanded { "full" } else { "short" },
                                        },
                                        gtk::Button {
                                            #[watch]
                                            set_visible: !model.loading && model.desc_needs_toggle,
                                            #[watch]
                                            set_label: &if model.desc_expanded { t("Show Less") } else { t("Show More") },
                                            add_css_class: "pill",
                                            set_halign: gtk::Align::Center,
                                            set_margin_bottom: 14,
                                            connect_clicked => DetailMsg::ToggleDescription,
                                        },
                                    },
                                },

                                // 4. Add-ons
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

                                // 5. Context tiles
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[local_ref]
                                    ctx_bar -> gtk::Box {},
                                },

                                // 6. Version history
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

                                // 7. Infobars
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

                                // 8. License and links
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[local_ref]
                                    license_box -> gtk::Box {},
                                },

                                // 9. Build options
                                adw::Clamp {
                                    set_maximum_size: CLAMP_MAX,
                                    set_tightening_threshold: CLAMP_TIGHTEN,
                                    set_margin_start: 12,
                                    set_margin_end: 12,

                                    #[local_ref]
                                    build_box -> gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_visible: false,
                                    },
                                },

                                // 10. Ratings and reviews
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

                                // 11. Other apps by the developer
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

                        // Screenshot lightbox
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

                                // Caps the image at its native width.
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

    fn init(
        pkg: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let entry = nix_software_core::load_manifest().entry(&pkg.attr).cloned();
        let kept = entry.is_some();
        let pin = entry.as_ref().and_then(|e| e.pin.clone());
        let custom_blocks = entry.as_ref().is_some_and(|e| e.home_nix.is_some());

        let toast_overlay = adw::ToastOverlay::new();
        let stack = gtk::Stack::new();
        let carousel = ScreenshotCarousel::new();
        let carousel_widget = carousel.widget().clone();
        let star_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let installs_label = gtk::Label::new(None);
        let ctx_bar = context_bar::container();
        let addons_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let version_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let problems_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let license_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let build_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let reviews_box = gtk::Box::new(gtk::Orientation::Vertical, 18);
        let dev_apps_box = gtk::Box::new(gtk::Orientation::Vertical, 6);

        // Shown once a developer app is found.
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

        // Screenshot width: physical screen width, bounded.
        let (target_dev_w, scale) = crate::util::screen_metrics();
        let target_w = target_dev_w.clamp(1280, 3840) as u32;

        // Reuse the icon the list already loaded.
        let icon_tex = nix_software_core::known_icon_url(&pkg.attr)
            .and_then(|url| nix_software_core::cached_bytes_if_present(&url))
            .and_then(|bytes| texture_from_bytes_sized(&bytes, 128));

        let managed = nix_software_core::detect_layout().is_usable();
        let model = DetailModel {
            package: pkg.clone(),
            meta: None,
            loading: true,
            installed: nix_software_core::installed_app(&pkg.attr, None),
            managed,
            profile_name: if managed {
                None
            } else {
                nix_software_core::profile_elements()
                    .into_iter()
                    .find(|e| e.is_attr(&pkg.attr))
                    .map(|e| e.name)
            },
            profile_busy: false,
            kept,
            pin,
            holding: false,
            custom_blocks,
            module: None,
            icon_tex,
            size_loaded: false,
            closure_size: None,
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
            installs_label: installs_label.clone(),
            ctx_bar: ctx_bar.clone(),
            addons_box: addons_box.clone(),
            version_box: version_box.clone(),
            problems_box: problems_box.clone(),
            license_box: license_box.clone(),
            build_box: build_box.clone(),
            flag_rows: Vec::new(),
            reviews_box: reviews_box.clone(),
            dev_apps_box: dev_apps_box.clone(),
            dev_apps_heading: dev_apps_heading.clone(),
            dev_tiles,
            toast_overlay: toast_overlay.clone(),
        };
        let widgets = view_output!();

        let mut entry = widgets
            .actions_popover
            .child()
            .and_then(|b| b.first_child());
        while let Some(w) = entry {
            if let Some(btn) = w.downcast_ref::<gtk::Button>() {
                if let Some(label) = btn.child().and_downcast::<gtk::Label>() {
                    label.set_xalign(0.0);
                }
                let popover = widgets.actions_popover.clone();
                btn.connect_clicked(move |_| popover.popdown());
            }
            entry = w.next_sibling();
        }

        crate::util::smooth_scroll(&widgets.scroller);

        // One width for every section.
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
        let sg_origin = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        sg_origin.add_widget(&widgets.box_install_remove);
        sg_origin.add_widget(&widgets.origin_btn);

        let attrs = gtk::pango::AttrList::new();
        attrs.insert(gtk::pango::AttrFloat::new_scale(0.85));
        attrs.insert(gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Normal));
        widgets.origin_label.set_attributes(Some(&attrs));

        // The ready box becomes a stack page.
        let toolbar = root.child().and_downcast::<adw::ToolbarView>();
        if let Some(toolbar) = &toolbar {
            toolbar.set_content(None::<&gtk::Widget>);
            model.build_stack(&widgets.ready_page);
            toolbar.set_content(Some(&stack));
        }

        // Toast overlay between the page and its content.
        if let Some(toolbar) = root.child() {
            root.set_child(None::<&gtk::Widget>);
            model.toast_overlay.set_child(Some(&toolbar));
            root.set_child(Some(&model.toast_overlay));
        }

        let s = sender.input_sender().clone();
        let pkg_meta = pkg.clone();
        std::thread::spawn(move || {
            let meta = nix_software_core::cached_app_meta(&pkg_meta, target_w);
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
                if self.installed.is_none() {
                    let desktop_id = meta
                        .launchable
                        .as_deref()
                        .map(|l| l.trim_end_matches(".desktop"))
                        .or(meta.app_id.as_deref());
                    self.installed =
                        nix_software_core::installed_app(&self.package.attr, desktop_id);
                }

                self.rebuild_context_bar(&meta);
                self.rebuild_problems(&meta, &sender);
                if std::path::Path::new("/etc/NIXOS").exists() {
                    let attr = self.package.attr.clone();
                    let s = sender.input_sender().clone();
                    std::thread::spawn(move || {
                        let _ = s.send(DetailMsg::ModuleLoaded(nix_software_core::module_for(
                            &attr,
                        )));
                    });
                }
                build_addons(&self.addons_box, &meta);
                build_version_history(
                    &self.version_box,
                    &meta,
                    meta.version.as_deref(),
                    self.toast_overlay.clone().upcast(),
                );
                let s = sender.input_sender().clone();
                build_license_box(&self.license_box, &meta, move || {
                    let _ = s.send(DetailMsg::ReportMatch);
                });
                self.rebuild_build_options(&meta, &sender);

                // Shown only when nixpkgs has one of them.
                if !meta.developer_apps.is_empty() {
                    let apps = meta.developer_apps.clone();
                    let attr = self.package.attr.clone();
                    let s = sender.input_sender().clone();
                    std::thread::spawn(move || {
                        // Fuzzy matches may land on this attr.
                        let resolved = nix_software_core::resolve_dev_apps(&apps, &attr);
                        if !resolved.is_empty() {
                            let _ = s.send(DetailMsg::DevAppsResolved(resolved));
                        }
                    });
                }

                if meta.screenshot_urls.is_empty() {
                    self.carousel.show_fallback();
                }
                self.carousel.widget().set_visible(meta.app_id.is_some());

                let attr = self.package.attr.clone();
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let _ = s.send(DetailMsg::SizeLoaded(nix_software_core::closure_size(
                        &attr,
                    )));
                });

                if let Some(app_id) = meta.app_id.clone() {
                    let s = sender.input_sender().clone();
                    let id = app_id.clone();
                    std::thread::spawn(move || {
                        if let Some(r) = nix_software_core::rating_for(&id) {
                            let _ = s.send(DetailMsg::RatingLoaded(r));
                        }
                    });
                    let s = sender.input_sender().clone();
                    let id = app_id.clone();
                    std::thread::spawn(move || {
                        if let Some(n) = nix_software_core::flathub_installs(&id).filter(|n| *n > 0)
                        {
                            let _ = s.send(DetailMsg::InstallsLoaded(n));
                        }
                    });
                    let s = sender.input_sender().clone();
                    let version = meta.version.clone().unwrap_or_default();
                    std::thread::spawn(move || {
                        let reviews = nix_software_core::reviews_for(&app_id, &version);
                        if !reviews.is_empty() {
                            let _ = s.send(DetailMsg::ReviewsLoaded(reviews));
                        }
                    });
                }

                if let Some(url) = meta.icon_url.clone() {
                    let s = sender.input_sender().clone();
                    std::thread::spawn(move || {
                        if let Some(b) = nix_software_core::cached_fetch_bytes(&url) {
                            let _ = s.send(DetailMsg::IconLoaded(b));
                        }
                    });
                }
                for (i, url) in meta.screenshot_urls.iter().take(4).cloned().enumerate() {
                    let caption = meta.screenshot_captions.get(i).cloned().flatten();
                    let s = sender.input_sender().clone();
                    std::thread::spawn(move || {
                        if let Some(b) = nix_software_core::cached_fetch_bytes(&url) {
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
                    t("Other Apps")
                } else {
                    t("Other Apps by {}").replace("{}", &dev)
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
            DetailMsg::ModuleLoaded(module) => {
                self.module = module;
                if let Some(meta) = self.meta.clone() {
                    self.rebuild_problems(&meta, &sender);
                }
            }
            DetailMsg::SaveBuildOptions => {
                let changes: Vec<(String, bool)> = self
                    .flag_rows
                    .iter()
                    .filter(|(_, default, row)| row.is_active() != *default)
                    .map(|(name, _, row)| (name.clone(), row.is_active()))
                    .collect();
                let res =
                    nix_software_core::set_flags(&self.package, changes).map_err(|e| e.to_string());
                sender.input(DetailMsg::BlocksSaved(res));
            }
            DetailMsg::ToggleHold => {
                if self.pin.is_some() {
                    let res = nix_software_core::set_pin(&self.package, None)
                        .map(|()| None)
                        .map_err(|e| e.to_string());
                    sender.input(DetailMsg::Held(res));
                    return;
                }
                self.start_hold(None, &sender);
            }
            DetailMsg::HoldAt(choice) => self.start_hold(Some(choice), &sender),
            DetailMsg::Remove => {
                if self.kept || self.profile_name.is_some() {
                    sender.input(DetailMsg::ToggleKeep);
                    return;
                }
                let attr = self.package.attr.clone();
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let res = nix_software_core::remove_tried(&attr).map_err(|e| e.to_string());
                    let _ = s.send(DetailMsg::TriedRemoved(res));
                });
            }
            DetailMsg::TriedRemoved(res) => {
                let text = match res {
                    Ok(_) => t("{} removed.").replace("{}", &self.package.name),
                    Err(e) => crate::util::failed(&e),
                };
                crate::util::toast(&self.toast_overlay, &text);
            }
            DetailMsg::ChooseMode => {
                use nix_software_core::InstallMode;
                let layout = nix_software_core::detect_layout();
                let module = self
                    .module
                    .as_ref()
                    .filter(|_| layout.system.is_some())
                    .map(|o| InstallMode::Module(o.name.clone()));
                let choices = [
                    (
                        "home",
                        t("In _Your Home"),
                        layout.home.as_ref().map(|_| InstallMode::Home),
                    ),
                    (
                        "system",
                        t("For _Everyone"),
                        layout.system.as_ref().map(|_| InstallMode::System),
                    ),
                    ("module", t("Through the _Module"), module),
                ];
                let dialog = adw::AlertDialog::new(
                    Some(&t("Install As")),
                    Some(&t("In your home, only you get the app. For everyone, it goes to the system packages. Its module also sets up what the app needs around it, such as services or groups.")),
                );
                dialog.add_response("cancel", &t("_Cancel"));
                let current = self.mode();
                let mut modes = Vec::new();
                for (id, label, mode) in choices {
                    let Some(mode) = mode else { continue };
                    dialog.add_response(id, &label);
                    dialog.set_response_enabled(id, current.as_ref() != Some(&mode));
                    modes.push((id, mode));
                }
                let s = sender.input_sender().clone();
                dialog.connect_response(None, move |_, id| {
                    if let Some((_, mode)) = modes.iter().find(|(i, _)| *i == id) {
                        let _ = s.send(DetailMsg::SetMode(mode.clone()));
                    }
                });
                dialog.present(Some(&self.toast_overlay));
            }
            DetailMsg::SetMode(mode) => {
                let res =
                    nix_software_core::set_mode(&self.package, &mode).map_err(|e| e.to_string());
                sender.input(DetailMsg::BlocksSaved(res));
            }
            DetailMsg::ChooseVersion => {
                crate::util::toast(&self.toast_overlay, &t("Looking for versions…"));
                let attr = self.package.attr.clone();
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let _ = s.send(DetailMsg::VersionsLoaded(nix_software_core::versions(
                        &attr,
                    )));
                });
            }
            DetailMsg::VersionsLoaded(choices) => {
                let s = sender.input_sender().clone();
                crate::components::version_dialog::present(
                    &self.toast_overlay,
                    choices,
                    self.pin.as_ref().map(|p| p.rev.clone()),
                    move |pick| {
                        let _ = s.send(match pick {
                            crate::components::version_dialog::Pick::Version(c) => {
                                DetailMsg::HoldAt(c)
                            }
                            crate::components::version_dialog::Pick::Follow
                            | crate::components::version_dialog::Pick::Installed => {
                                DetailMsg::ToggleHold
                            }
                        });
                    },
                );
            }
            DetailMsg::Held(res) => {
                self.holding = false;
                match res {
                    Ok(pin) => {
                        self.pin = pin;
                        sender.input(DetailMsg::BlocksSaved(Ok(())));
                    }
                    Err(e) => crate::util::toast(&self.toast_overlay, &crate::util::failed(&e)),
                }
            }
            DetailMsg::UseModule => {
                let Some(option) = &self.module else {
                    return;
                };
                let res = nix_software_core::set_blocks(
                    &self.package,
                    Some(String::new()),
                    Some(format!("{} = true;", option.name)),
                )
                .map_err(|e| e.to_string());
                sender.input(DetailMsg::BlocksSaved(res));
            }
            DetailMsg::ShowModule => {
                if let Some(option) = &self.module {
                    crate::components::option_dialog::present(&self.toast_overlay, option);
                }
            }
            DetailMsg::ToggleDescription => {
                self.desc_expanded = !self.desc_expanded;
            }
            DetailMsg::InstallsLoaded(n) => {
                self.installs_label.set_label(
                    &gettextrs::ngettext(
                        "{} install on Flathub last month",
                        "{} installs on Flathub last month",
                        n.min(u64::from(u32::MAX)) as u32,
                    )
                    .replace("{}", &compact(n)),
                );
                self.installs_label.set_visible(true);
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
                    let max_logical = (tex.width() / self.scale).max(1);
                    self.shots.push(tex.clone());
                    self.shot_widths.push(max_logical);

                    // Margins keep a gap; the clamp caps width.
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
                    btn.set_tooltip_text(Some(&caption.unwrap_or_else(|| t("Enlarge"))));
                    let s = sender.clone();
                    btn.connect_clicked(move |_| s.input(DetailMsg::ShowShot(idx)));
                    self.carousel.append(&btn);
                }
            }
            DetailMsg::ShowShot(idx) => {
                self.lightbox_tex = self.shots.get(idx).cloned();
                // Zoom to twice the native width.
                if let Some(w) = self.shot_widths.get(idx) {
                    self.lightbox_clamp.set_maximum_size(w.saturating_mul(2));
                }
                self.lightbox_visible = true;
            }
            DetailMsg::DismissShot => {
                self.lightbox_visible = false;
            }
            DetailMsg::ReportMatch => {
                let name = self
                    .meta
                    .as_ref()
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                let dialog = adw::AlertDialog::new(
                    Some(&t("Not the Same App?")),
                    Some(&t("Nix Software matched the nixpkgs package “{attr}” with the Flathub app “{name}”. If they are different apps, Nix Software stops showing Flathub data (icon, screenshots, reviews) for this package.")
                        .replace("{attr}", &self.package.attr)
                        .replace("{name}", &name)),
                );
                dialog.add_response("cancel", &t("_Cancel"));
                dialog.add_response("unmatch", &t("_Not the Same App"));
                dialog.set_response_appearance("unmatch", adw::ResponseAppearance::Destructive);
                let s = sender.input_sender().clone();
                dialog.connect_response(None, move |_, r| {
                    if r == "unmatch" {
                        let _ = s.send(DetailMsg::Unmatch);
                    }
                });
                dialog.present(Some(&self.toast_overlay));
            }
            DetailMsg::Unmatch => {
                let text = match nix_software_core::report_mapping(&self.package.attr, "") {
                    Ok(()) => {
                        nix_software_core::forget_app_meta(&self.package.attr);
                        t("Saved. Open the page again to see the package alone.")
                    }
                    Err(e) => crate::util::failed(&e),
                };
                crate::util::toast(&self.toast_overlay, &text);
            }
            DetailMsg::Open => {
                use gtk::gio::prelude::AppInfoExt;
                let launched = self
                    .installed
                    .as_ref()
                    .and_then(|a| gtk::gio::DesktopAppInfo::from_filename(&a.desktop_file))
                    .is_some_and(|app| {
                        app.launch(&[], None::<&gtk::gio::AppLaunchContext>).is_ok()
                    });
                if !launched {
                    crate::util::toast(&self.toast_overlay, &t("Could not open the app."));
                }
            }
            DetailMsg::Primary if self.is_self() => {
                crate::util::toast(&self.toast_overlay, &t("This is the app you are using."));
            }
            DetailMsg::Primary => {
                sender.input(if self.installed.is_some() {
                    DetailMsg::Open
                } else if self.cli_first() {
                    DetailMsg::Shell
                } else {
                    DetailMsg::Run
                });
            }
            DetailMsg::AltLaunch => {
                sender.input(if self.cli_first() {
                    DetailMsg::Run
                } else {
                    DetailMsg::Shell
                });
            }
            DetailMsg::Run => {
                let name = meta_name(&self.meta, &self.package.name);
                if self.meta.as_ref().map(|m| m.broken).unwrap_or(false) {
                    crate::util::toast(
                        &self.toast_overlay,
                        &t("{} is marked broken in nixpkgs. Trying anyway…").replace("{}", &name),
                    );
                } else {
                    crate::util::toast(
                        &self.toast_overlay,
                        &t("Starting {}…").replace("{}", &name),
                    );
                }
                match nix_software_core::run(&self.package) {
                    Ok(child) => {
                        let s = sender.input_sender().clone();
                        std::thread::spawn(move || {
                            if let Some(err) = nix_software_core::run_failure(child) {
                                let _ = s.send(DetailMsg::RunFailed(err));
                            }
                        });
                    }
                    Err(e) => {
                        crate::util::toast(
                            &self.toast_overlay,
                            &t("Could not start: {}").replace("{}", &crate::util::tr_err(&e)),
                        );
                    }
                }
            }
            DetailMsg::RunFailed(err) => {
                let name = meta_name(&self.meta, &self.package.name);
                crate::util::toast(
                    &self.toast_overlay,
                    &t("{name} stopped with an error: {err}")
                        .replace("{name}", &name)
                        .replace("{err}", &err),
                );
            }
            DetailMsg::ToggleKeep if self.installed_elsewhere() => {}
            DetailMsg::ToggleKeep if !self.managed => sender.input(DetailMsg::ProfileToggle),
            DetailMsg::ProfileToggle => {
                if self.profile_busy {
                    return;
                }
                self.profile_busy = true;
                let (attr, name) = (self.package.attr.clone(), self.profile_name.clone());
                crate::util::toast(
                    &self.toast_overlay,
                    &if name.is_some() {
                        t("Uninstalling…")
                    } else {
                        t("Installing…")
                    },
                );
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let res = match &name {
                        Some(n) => nix_software_core::profile_remove(n).map(|()| None),
                        None => nix_software_core::profile_add(&attr).map(|()| {
                            nix_software_core::profile_elements()
                                .into_iter()
                                .find(|e| e.is_attr(&attr))
                                .map(|e| e.name)
                        }),
                    };
                    let _ = s.send(DetailMsg::ProfileDone(res.map_err(|e| e.to_string())));
                });
            }
            DetailMsg::ProfileDone(res) => {
                self.profile_busy = false;
                let text = match res {
                    Ok(Some(name)) => {
                        self.profile_name = Some(name);
                        t("Installed for your user.")
                    }
                    Ok(None) => {
                        self.profile_name = None;
                        t("Uninstalled.")
                    }
                    Err(e) => crate::util::failed(&e),
                };
                self.installed = nix_software_core::installed_app(
                    &self.package.attr,
                    self.meta.as_ref().and_then(|m| m.app_id.as_deref()),
                );
                crate::util::toast(&self.toast_overlay, &text);
            }
            DetailMsg::ToggleKeep => {
                let next = if self.kept {
                    DetailMsg::Unkeep
                } else {
                    DetailMsg::Keep
                };
                sender.input(next);
            }
            DetailMsg::Shell => match nix_software_core::shell(&self.package) {
                Ok(nix_software_core::ShellOutcome::Spawned) => crate::util::toast(
                    &self.toast_overlay,
                    &t("{} opened in a terminal…").replace("{}", &self.package.name),
                ),
                Ok(nix_software_core::ShellOutcome::NoTerminal) => {
                    if let Some(display) = gtk::gdk::Display::default() {
                        display
                            .clipboard()
                            .set_text(&nix_software_core::shell_command(
                                &self.package,
                                self.unfree(),
                            ));
                    }
                    crate::util::toast(
                        &self.toast_overlay,
                        &t("No terminal found. Command copied."),
                    );
                }
                Err(e) => crate::util::toast(
                    &self.toast_overlay,
                    &t("Could not open a shell: {}").replace("{}", &crate::util::tr_err(&e)),
                ),
            },
            DetailMsg::Keep => match nix_software_core::keep(&self.package) {
                Ok(()) => {
                    self.kept = true;
                    self.prompt_apply_or_attach(&sender);
                }
                Err(e) => crate::util::toast(&self.toast_overlay, &crate::util::failed(&e)),
            },
            DetailMsg::Unkeep => match nix_software_core::unkeep(&self.package.attr) {
                Ok(()) => {
                    self.kept = false;
                    self.pin = None;
                    apply_toast(
                        &self.toast_overlay,
                        &sender,
                        &t("Removed from your configuration. Apply it to uninstall."),
                    );
                }
                Err(e) => crate::util::toast(&self.toast_overlay, &crate::util::failed(&e)),
            },
            DetailMsg::AdvancedEdit if self.installed_elsewhere() => {}
            DetailMsg::AdvancedEdit => {
                let m = nix_software_core::load_manifest();
                let (home, system) = m
                    .kept
                    .iter()
                    .find(|e| e.package.attr == self.package.attr)
                    .map(|e| (e.home_nix.clone(), e.system_nix.clone()))
                    .unwrap_or((None, None));
                let layout = nix_software_core::detect_layout();
                let system_available = layout.system.is_some();
                // Start from the plain entry.
                let home = home.or_else(|| {
                    Some(nix_software_core::default_home_block(
                        &layout,
                        &self.package.attr,
                    ))
                });
                let pkg = self.package.clone();
                let s = sender.input_sender().clone();
                nix_editor::present(
                    &self.toast_overlay,
                    &self.package,
                    home,
                    system,
                    system_available,
                    move |blocks| {
                        let res = nix_software_core::set_blocks(&pkg, blocks.home, blocks.system)
                            .map_err(|e| e.to_string());
                        let _ = s.send(DetailMsg::BlocksSaved(res));
                    },
                );
            }
            DetailMsg::BlocksSaved(res) => match res {
                Ok(()) => {
                    // Editing an app keeps it.
                    self.kept = true;
                    self.custom_blocks = nix_software_core::load_manifest()
                        .entry(&self.package.attr)
                        .is_some_and(|e| e.home_nix.is_some());
                    if let Some(meta) = self.meta.clone() {
                        self.rebuild_problems(&meta, &sender);
                    }
                    self.prompt_apply_or_attach(&sender);
                }
                Err(e) => crate::util::toast(&self.toast_overlay, &crate::util::failed(&e)),
            },
            DetailMsg::ApplyConfig => match nix_software_core::apply_argv() {
                Some(argv) => {
                    let s = sender.input_sender().clone();
                    crate::components::apply_dialog::present(
                        &self.toast_overlay,
                        &t("Applying Configuration"),
                        vec![argv],
                        move |res| {
                            let _ = s.send(DetailMsg::Applied(res));
                        },
                    );
                }
                None => sender.input(DetailMsg::Applied(Ok(
                    nix_software_core::ApplyOutcome::UnknownLayout,
                ))),
            },
            DetailMsg::Applied(res) => {
                crate::util::toast(&self.toast_overlay, &crate::util::describe_apply(res));
            }
            DetailMsg::CopyName => {
                if let Some(display) = gtk::gdk::Display::default() {
                    display.clipboard().set_text(&self.package.attr);
                }
                crate::util::toast(
                    &self.toast_overlay,
                    &t("{} copied.").replace("{}", &self.package.attr),
                );
            }
            DetailMsg::CopyCommand => {
                if let Some(display) = gtk::gdk::Display::default() {
                    display
                        .clipboard()
                        .set_text(&nix_software_core::run_command(
                            &self.package,
                            self.unfree(),
                        ));
                }
                crate::util::toast(&self.toast_overlay, &t("Command copied."));
            }
        }
    }
}

impl DetailModel {
    /// True when the user's own configuration installs it.
    fn installed_elsewhere(&self) -> bool {
        !self.kept && self.profile_name.is_none() && self.installed.is_some()
    }

    /// Holds the app at `choice`, else at the revision in use. Downloads in
    /// a thread.
    fn start_hold(
        &mut self,
        choice: Option<nix_software_core::VersionChoice>,
        sender: &ComponentSender<Self>,
    ) {
        self.holding = true;
        crate::util::toast(
            &self.toast_overlay,
            &t("Downloading nixpkgs to hold this version…"),
        );
        let pkg = self.package.clone();
        let version = self.meta.as_ref().and_then(|m| m.version.clone());
        let s = sender.input_sender().clone();
        std::thread::spawn(move || {
            let pin = match choice {
                Some(c) => nix_software_core::pin_at(&c.rev, Some(c.version)),
                None => nix_software_core::pin_current(version),
            };
            let res = pin
                .and_then(|p| nix_software_core::set_pin(&pkg, Some(p.clone())).map(|()| Some(p)))
                .map_err(|e| e.to_string());
            let _ = s.send(DetailMsg::Held(res));
        });
    }

    /// True when the menu offers to hold or release the app.
    fn can_hold(&self) -> bool {
        self.managed
            && !self.installed_elsewhere()
            && !self.custom_blocks
            && (self.pin.is_some() || nix_software_core::pinned_rev().is_some())
    }

    /// True when the app can be taken off: kept, installed with nix
    /// profile, or tried.
    fn removable(&self) -> bool {
        self.kept || self.profile_name.is_some() || self.tried()
    }

    fn tried(&self) -> bool {
        nix_software_core::load_tried()
            .tried
            .iter()
            .any(|e| e.package.attr == self.package.attr)
    }

    fn remove_tooltip(&self) -> String {
        if self.kept {
            t("Take the app out of your configuration. It is uninstalled when you apply.")
        } else if self.profile_name.is_some() {
            t("Uninstall the app from your user profile.")
        } else {
            t("Delete the tried app from the store.")
        }
    }

    fn keep_tooltip(&self) -> String {
        if self.installed_elsewhere() {
            return t("Your own configuration installs this app; Nix Software leaves it alone.");
        }
        match (self.managed, self.kept, self.profile_name.is_some()) {
            (true, true, _) => {
                t("Take the app out of your configuration. It is uninstalled when you apply.")
            }
            (true, false, _) => {
                t("Add the app to your configuration. It is installed when you apply.")
            }
            (false, _, _) => t("Install or remove the app for your user only, with nix profile."),
        }
    }

    fn keep_label(&self) -> String {
        if self.installed_elsewhere() {
            return match self.installed.as_ref().map(|a| a.scope) {
                Some(nix_software_core::Scope::System) => t("In Your System Configuration"),
                _ => t("In Your User Configuration"),
            };
        }
        match (self.managed, self.kept, self.profile_name.is_some()) {
            (true, true, _) => t("Remove from Configuration"),
            (true, false, _) => t("Add to Configuration"),
            (false, _, true) => t("Uninstall"),
            (false, _, false) => t("Install for This User"),
        }
    }

    /// True when the license is known to be unfree.
    fn unfree(&self) -> bool {
        self.meta.as_ref().and_then(|m| m.is_free_license) == Some(false)
    }

    /// True on the page of Nix Software itself: running it only
    /// raises this window.
    fn is_self(&self) -> bool {
        self.package.attr == crate::util::SELF_ATTR
    }

    /// How Nix Software installs this app; `None` when custom blocks
    /// say more.
    fn mode(&self) -> Option<nix_software_core::InstallMode> {
        nix_software_core::load_manifest()
            .entry(&self.package.attr)
            .and_then(|e| e.mode())
    }

    fn cli_first(&self) -> bool {
        self.meta.as_ref().is_some_and(|m| m.app_id.is_none())
    }

    /// Mounts the spinner, ready and failed pages.
    fn build_stack(&self, ready: &gtk::Box) {
        let spinner = adw::StatusPage::new();
        spinner.set_paintable(Some(&adw::SpinnerPaintable::new(Some(&self.stack))));
        spinner.set_title(&t("Loading App Details"));
        self.stack.add_named(&spinner, Some("spinner"));

        self.stack.add_named(ready, Some("ready"));

        let failed = adw::StatusPage::new();
        failed.set_icon_name(Some("dialog-error-symbolic"));
        failed.set_title(&t("Unable to Show App Details"));
        failed.set_description(Some(&t("No metadata could be retrieved for this package.")));
        self.stack.add_named(&failed, Some("failed"));

        self.stack.set_visible_child_name("spinner");
    }

    fn rebuild_context_bar(&self, meta: &AppMeta) {
        context_bar::fill(
            &self.ctx_bar,
            build_context_tiles(meta, self.size_loaded, self.closure_size),
        );
    }

    fn rebuild_problems(&self, meta: &AppMeta, sender: &ComponentSender<Self>) {
        while let Some(c) = self.problems_box.first_child() {
            self.problems_box.remove(&c);
        }
        if let Some(bar) =
            infobox::problems(meta.broken, meta.insecure, &meta.known_vulnerabilities)
        {
            self.problems_box.append(&bar);
        }
        if let Some(pin) = &self.pin {
            let s = sender.input_sender().clone();
            self.problems_box.append(&infobox::held(pin, move || {
                let _ = s.send(DetailMsg::ToggleHold);
            }));
        }
        if let Some(option) = &self.module {
            let in_use = nix_software_core::load_manifest().kept.iter().any(|e| {
                e.system_nix
                    .as_deref()
                    .is_some_and(|b| b.contains(&option.name))
            });
            let can_use = !in_use
                && !self.installed_elsewhere()
                && nix_software_core::detect_layout().system.is_some();
            let on_use = can_use.then(|| {
                let s = sender.input_sender().clone();
                Box::new(move || {
                    let _ = s.send(DetailMsg::UseModule);
                }) as Box<dyn Fn()>
            });
            let s = sender.input_sender().clone();
            let bar = infobox::module(&option.name, on_use, move || {
                let _ = s.send(DetailMsg::ShowModule);
            });
            self.problems_box.append(&bar);
        }
        self.problems_box
            .set_visible(self.problems_box.first_child().is_some());
    }

    /// The build options section: a switch per recipe flag. Only where Nix
    /// Software writes the configuration.
    fn rebuild_build_options(&mut self, meta: &AppMeta, sender: &ComponentSender<Self>) {
        while let Some(c) = self.build_box.first_child() {
            self.build_box.remove(&c);
        }
        self.flag_rows.clear();
        let entry = nix_software_core::load_manifest()
            .kept
            .into_iter()
            .find(|e| e.package.attr == self.package.attr);
        let editable = self.managed
            && !self.installed_elsewhere()
            && entry.as_ref().is_none_or(|e| e.home_nix.is_none());
        if meta.build_flags.is_empty() || !editable {
            self.build_box.set_visible(false);
            return;
        }
        let saved = entry.map(|e| e.flags).unwrap_or_default();

        let expander = adw::ExpanderRow::builder()
            .title(esc(&t("Build Options")))
            .subtitle(esc(&t(
                "Changing an option builds the app on your computer, which can take a long time.",
            )))
            .build();
        expander.add_prefix(&gtk::Image::from_icon_name("emblem-system-symbolic"));
        for flag in &meta.build_flags {
            let active = saved
                .iter()
                .find(|(n, _)| *n == flag.name)
                .map_or(flag.default, |(_, v)| *v);
            let row = adw::SwitchRow::builder()
                .title(esc(&flag.name))
                .subtitle(esc(&if flag.default {
                    t("On by default")
                } else {
                    t("Off by default")
                }))
                .active(active)
                .build();
            expander.add_row(&row);
            self.flag_rows.push((flag.name.clone(), flag.default, row));
        }
        let save = adw::ButtonRow::builder()
            .title(t("_Save Build Options"))
            .use_underline(true)
            .build();
        save.connect_activated({
            let s = sender.input_sender().clone();
            move |_| {
                let _ = s.send(DetailMsg::SaveBuildOptions);
            }
        });
        expander.add_row(&save);

        let list = crate::util::boxed_list();
        list.append(&expander);
        self.build_box.append(&list);
        self.build_box.set_visible(true);
    }

    /// The ratings section; the rating and the reviews arrive apart.
    fn rebuild_reviews(&self, reviews: Option<&[Review]>) {
        match self.rating.filter(|r| r.total > 0) {
            Some(rating) => build_reviews(
                &self.reviews_box,
                &rating,
                reviews,
                self.meta
                    .as_ref()
                    .and_then(|m| Some((m.app_id.clone()?, m.version.clone().unwrap_or_default()))),
                &self.toast_overlay,
            ),
            None => self.reviews_box.set_visible(false),
        }
    }
}

/// Header stars and review count.
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

/// The ratings section: histogram and write button, featured review and
/// the full list.
fn build_reviews(
    container: &gtk::Box,
    rating: &Rating,
    reviews: Option<&[Review]>,
    app: Option<(String, String)>,
    overlay: &adw::ToastOverlay,
) {
    while let Some(c) = container.first_child() {
        container.remove(&c);
    }

    let title = gtk::Label::new(Some(&t("Ratings & Reviews")));
    title.set_xalign(0.0);
    title.set_halign(gtk::Align::Start);
    title.set_valign(gtk::Align::Start);
    title.set_hexpand(true);
    title.add_css_class("title-2");
    container.append(&title);

    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    inner.set_homogeneous(true);

    let left = crate::util::boxed_list();
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
    write.set_title(&t("_Write a Review"));
    write.set_use_underline(true);
    write.set_end_icon_name(Some("go-next-symbolic"));
    match app {
        Some((app_id, version)) => {
            let overlay = overlay.clone();
            write.connect_activated(move |row| {
                let overlay = overlay.clone();
                crate::components::details::review_dialog::present(
                    row,
                    app_id.clone(),
                    version.clone(),
                    move |res| {
                        let text = match res {
                            Ok(()) => t("Thanks, your review was sent."),
                            Err(e) => crate::util::failed(&e),
                        };
                        crate::util::toast(&overlay, &text);
                    },
                );
            });
        }
        None => write.set_sensitive(false),
    }
    left.append(&write);
    inner.append(&left);

    let right = crate::util::boxed_list();
    if let Some(featured) = reviews.and_then(nix_software_core::featured_review) {
        right.append(&review_row::row(featured));
    }
    let all = adw::ButtonRow::new();
    all.set_title(&t("All Reviews"));
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

/// Dialog listing every review, best first.
fn show_reviews_dialog(reviews: &[Review], parent: &impl IsA<gtk::Widget>) {
    let list = crate::util::boxed_list();
    let mut sorted: Vec<&Review> = reviews.iter().collect();
    sorted.sort_by_key(|r| std::cmp::Reverse(r.score));
    for r in sorted {
        list.append(&review_row::row(r));
    }
    present_dialog(&t("All Reviews"), &list, 560, 640, parent);
}

/// A dialog with a header bar and a scrolled, clamped `content`.
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

/// The four context tiles: download size, safety (nixpkgs problems, else
/// not sandboxed), hardware, age rating.
fn build_context_tiles(
    meta: &AppMeta,
    size_loaded: bool,
    closure_size: Option<u64>,
) -> [gtk::Button; 4] {
    let (storage_val, storage_desc) = if !size_loaded {
        ("…".into(), t("Computing size…"))
    } else if let Some(bytes) = closure_size {
        (
            gtk::glib::format_size(bytes).to_string(),
            t("Total download including dependencies"),
        )
    } else {
        ("?".into(), t("Needs a local build"))
    };
    let storage = context_bar::tile(
        Content::Text(&storage_val),
        "grey",
        &t("Download Size"),
        &storage_desc,
    );

    let safety_tile = if meta.broken {
        context_bar::tile(
            Content::Icon("app-safety-unsafe-symbolic"),
            "red",
            &t("Broken"),
            &t("Marked broken in nixpkgs: running it will probably fail"),
        )
    } else if meta.insecure || !meta.known_vulnerabilities.is_empty() {
        let d = if meta.known_vulnerabilities.is_empty() {
            t("Marked insecure in nixpkgs")
        } else {
            t("Known vulnerabilities: {}").replace("{}", &meta.known_vulnerabilities.join(", "))
        };
        context_bar::tile(
            Content::Icon("app-safety-unsafe-symbolic"),
            "red",
            &t("Vulnerable"),
            &d,
        )
    } else {
        let (color, desc) = match meta.is_free_license {
            Some(true) => ("grey", t("Free software with your full user rights")),
            Some(false) => ("orange", t("Proprietary, with your full user rights")),
            None => ("grey", t("Runs with your full user rights")),
        };
        context_bar::tile(
            Content::Icon("app-safety-unknown-symbolic"),
            color,
            &t("Not Sandboxed"),
            &desc,
        )
    };

    let hardware = context_bar::tile(
        Content::Icon("video-display-symbolic"),
        "grey",
        &t("Desktop Only"),
        &t("Works on desktops and laptops"),
    );

    let age = match &meta.age_label {
        Some(label) => {
            let desc = if meta.age_descriptions.is_empty() {
                t("No inappropriate content")
            } else {
                let d: Vec<String> = meta
                    .age_descriptions
                    .iter()
                    .map(|d| t(d.as_str()))
                    .collect();
                d.join("; ")
            };
            context_bar::tile(
                Content::Text(label),
                age_color(label),
                &t("Age Rating"),
                &desc,
            )
        }
        None => context_bar::tile(
            Content::Text("?"),
            "grey",
            &t("Age Rating"),
            &t("No age rating information"),
        ),
    };

    [storage, safety_tile, hardware, age]
}

/// CSS tint of an age lozenge.
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

/// The add-ons section; its tiles do not open pages.
fn build_addons(container: &gtk::Box, meta: &AppMeta) {
    while let Some(c) = container.first_child() {
        container.remove(&c);
    }
    if meta.addons.is_empty() {
        container.set_visible(false);
        return;
    }

    let title = gtk::Label::new(Some(&t("Add-ons")));
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

/// The nixpkgs release (else the latest) and the full history button.
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
    let latest = meta
        .releases
        .iter()
        .find(|r| installed_version == Some(r.version.as_str()))
        .unwrap_or(&meta.releases[0]);
    let list = crate::util::boxed_list();
    list.append(&version_history_row::row(
        &latest.version,
        latest.timestamp,
        latest.description.as_deref(),
        installed_version == Some(latest.version.as_str()),
        latest.url.as_deref(),
    ));

    if meta.releases.len() > 1 {
        let more = adw::ButtonRow::new();
        more.set_title(&t("Version History"));
        more.set_end_icon_name(Some("go-next-symbolic"));
        let releases = meta.releases.clone();
        let installed = installed_version.map(str::to_string);
        more.connect_activated(move |_| {
            let list = crate::util::boxed_list();
            for r in &releases {
                list.append(&version_history_row::row(
                    &r.version,
                    r.timestamp,
                    r.description.as_deref(),
                    installed.as_deref() == Some(r.version.as_str()),
                    r.url.as_deref(),
                ));
            }
            present_dialog(&t("Version History"), &list, 480, 560, &parent);
        });
        list.append(&more);
    }

    container.append(&list);
    container.set_visible(true);
}

/// License card and links, side by side.
fn build_license_box(container: &gtk::Box, meta: &AppMeta, on_wrong_app: impl Fn() + 'static) {
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

    let links = [
        (
            t("_Project Website"),
            "webpage-symbolic",
            meta.homepage.clone(),
        ),
        (t("_Donate"), "donate-symbolic", meta.donation.clone()),
        (
            t("Contribute _Translations"),
            "translations-symbolic",
            meta.translate.clone(),
        ),
        (
            t("_Report an Issue"),
            "computer-fail-symbolic",
            meta.bugtracker.clone(),
        ),
        (t("_Help"), "help-link-symbolic", meta.help.clone()),
        (
            t("Package _Recipe"),
            "document-edit-symbolic",
            meta.source_url.clone(),
        ),
        (
            t("Download _Page"),
            "folder-download-symbolic",
            meta.download_page.clone(),
        ),
    ];
    let present: Vec<_> = links
        .into_iter()
        .filter_map(|(label, icon, url)| {
            url.filter(|u| !u.trim().is_empty())
                .map(|u| (label, icon, u))
        })
        .collect();

    let stack = gtk::Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    if present.is_empty() {
        stack.add_named(&no_metadata_card(), Some("empty"));
    } else {
        let list = crate::util::boxed_list();
        for (label, icon, url) in present {
            list.append(&link_row(&label, icon, &url));
        }
        // A recipe position means maintainers are known.
        if meta.source_url.is_some() {
            list.append(&maintainers_row(meta));
        }
        for row in [commands_row(meta), opens_row(meta)].into_iter().flatten() {
            list.append(&row);
        }
        if meta.app_id.is_some() {
            let row = adw::ActionRow::builder()
                .title(esc(&t("Wrong App?")))
                .subtitle(esc(&t(
                    "This page shows another app's name, icon or pictures",
                )))
                .activatable(true)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name("dialog-question-symbolic"));
            row.connect_activated(move |_| on_wrong_app());
            list.append(&row);
        }
        stack.add_named(&list, Some("links"));
    }
    container.append(&stack);
}

/// Card shown when the package has no links.
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

    let title = gtk::Label::new(Some(&t("No Metadata")));
    title.add_css_class("heading");
    inner.append(&title);

    let desc = gtk::Label::new(Some(&t(
        "This package provides no link to a website, code repository or issue tracker.",
    )));
    desc.set_justify(gtk::Justification::Center);
    desc.set_wrap(true);
    desc.set_xalign(0.5);
    inner.append(&desc);

    card.append(&inner);
    card
}

/// `105082` as `105K`.
fn compact(n: u64) -> String {
    match n {
        0..=9_999 => n.to_string(),
        10_000..=999_999 => format!("{}K", n / 1000),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

fn info_row(title: &str, subtitle: &str, icon: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(esc(title))
        .subtitle(esc(subtitle))
        .subtitle_selectable(true)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name(icon));
    row
}

/// The commands the package puts in `PATH`.
fn commands_row(meta: &AppMeta) -> Option<adw::ActionRow> {
    (!meta.binaries.is_empty()).then(|| {
        info_row(
            &t("Commands"),
            &meta.binaries.join(", "),
            "utilities-terminal-symbolic",
        )
    })
}

/// File types the app opens: names when few, else families.
fn opens_row(meta: &AppMeta) -> Option<adw::ActionRow> {
    let types = &meta.mediatypes;
    if types.is_empty() {
        return None;
    }
    let text = if types.len() <= 6 {
        types
            .iter()
            .map(|m| gtk::gio::content_type_get_description(m).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        let mut families: Vec<String> = Vec::new();
        for m in types {
            let family = match m.split('/').next().unwrap_or("") {
                "video" => t("Video"),
                "audio" => t("Audio"),
                "image" => t("Images"),
                "text" => t("Text"),
                _ => t("Other files"),
            };
            if !families.contains(&family) {
                families.push(family);
            }
        }
        format!(
            "{} · {}",
            families.join(", "),
            crate::util::plural("{} file type", "{} file types", types.len() as i64)
        )
    };
    Some(info_row(&t("Opens"), &text, "document-open-symbolic"))
}

/// The package's nixpkgs maintainers; an orphan is shown as such.
fn maintainers_row(meta: &AppMeta) -> adw::ActionRow {
    let people: Vec<String> = meta
        .maintainers
        .iter()
        .cloned()
        .chain(meta.teams.iter().map(|t| format!("{t} team")))
        .collect();
    let row = adw::ActionRow::builder()
        .title(t("nixpkgs Maintainers"))
        .subtitle(esc(&if people.is_empty() {
            t("Nobody maintains this package in nixpkgs.")
        } else {
            people.join(", ")
        }))
        .subtitle_selectable(true)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name(if people.is_empty() {
        "dialog-warning-symbolic"
    } else {
        "system-users-symbolic"
    }));
    row
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
        crate::util::open_uri(&url);
    });
    row
}

/// A tile like `AppTile` that opens nothing, for add-ons.
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
    let (icon, class) = fallback_icon(&Package {
        description: summary.unwrap_or_default().to_string(),
        ..Package::from_attr(name)
    });
    fallback.add_css_class(class);
    let glyph = gtk::Image::from_icon_name(icon);
    glyph.set_pixel_size(48);
    glyph.set_hexpand(true);
    glyph.set_vexpand(true);
    glyph.set_halign(gtk::Align::Center);
    glyph.set_valign(gtk::Align::Center);
    glyph.add_css_class("fallback-glyph");
    fallback.append(&glyph);
    icon_slot.append(&fallback);

    // Image: 64 px layout, full resolution texture.
    let img = gtk::Image::new();
    img.set_pixel_size(64);
    img.set_visible(false);
    icon_slot.append(&img);

    if let Some(url) = icon_url {
        let img = img.clone();
        let fallback = fallback.clone();
        gtk::glib::spawn_future_local(async move {
            let bytes =
                gtk::gio::spawn_blocking(move || nix_software_core::cached_fetch_bytes(&url))
                    .await
                    .ok()
                    .flatten();
            if let Some(tex) = bytes
                .as_deref()
                .and_then(|b| texture_from_bytes_sized(b, 128))
            {
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

/// The grid of the add-ons and other apps sections.
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
                t("No description.")
            } else {
                fallback.to_string()
            }
        })
}

/// Length past which the description collapses.
const DESC_COLLAPSE_CHARS: usize = 300;

fn desc_needs_toggle(meta: &Option<AppMeta>, fallback: &str) -> bool {
    meta_description(meta, fallback).chars().count() > DESC_COLLAPSE_CHARS
}

/// The description, cut on a word when collapsed.
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

/// True when the listing has nothing to show.
fn is_empty_meta(meta: &AppMeta) -> bool {
    meta.summary.is_none() && meta.description.is_none() && meta.version.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let long = "word ".repeat(200);
        let out = desc_display(&None, &long, false);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= DESC_COLLAPSE_CHARS + 1);
        // Never cut in the middle of a word.
        assert!(out.trim_end_matches('…').ends_with("word"));
    }

    #[test]
    fn desc_display_expanded_returns_full_text() {
        let long = "word ".repeat(200);
        assert_eq!(desc_display(&None, &long, true), long);
    }

    #[test]
    fn desc_display_leaves_short_text_alone() {
        assert_eq!(desc_display(&None, "short", false), "short");
        assert_eq!(desc_display(&None, "", false), "No description.");
    }

    #[test]
    fn desc_needs_toggle_only_past_threshold() {
        assert!(!desc_needs_toggle(&None, &"a".repeat(DESC_COLLAPSE_CHARS)));
        assert!(desc_needs_toggle(
            &None,
            &"a".repeat(DESC_COLLAPSE_CHARS + 1)
        ));
    }

    /// Checks the page's widget tree. Needs a display: run with `--ignored`.
    #[test]
    #[ignore = "needs a GTK display"]
    fn widget_tree_matches_gnome_ui() {
        gtk::init().expect("gtk::init");

        let pkg = Package {
            attr: "hello".into(),
            name: "hello".into(),
            version: "2.12".into(),
            description: "Demo program".into(),
        };
        let ctrl = <DetailModel as relm4::Component>::builder().launch(pkg);
        let widgets = ctrl.widgets();

        // GNOME's wide header spacing.
        assert_eq!(widgets.header_box.spacing(), 24, "box_details_header");
        // `application_details`: summary ↔ description.
        assert_eq!(widgets.application_details.spacing(), 12);
        // The split button, centered.
        assert_eq!(widgets.box_install_remove.halign(), gtk::Align::Center);

        // Starts on the spinner page.
        let stack = &ctrl.model().stack;
        for name in ["spinner", "ready", "failed"] {
            assert!(
                stack.child_by_name(name).is_some(),
                "page \"{name}\" missing"
            );
        }
        assert_eq!(stack.visible_child_name().as_deref(), Some("spinner"));

        // No screenshot yet: fallback page.
        assert_eq!(
            ctrl.model()
                .carousel
                .widget()
                .visible_child_name()
                .as_deref(),
            Some("fallback")
        );

        // `box_details`: spacing 18, valign start, `.details-page` class.
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

        // Every section is clamped except the carousel.
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
        assert_eq!(clamps, 10, "10 clamped sections expected");

        // One GTK test only: GLib keeps the first thread.
        assert_addons_render_as_tile_grid();
    }

    /// Add-ons render as a grid of tiles that open nothing.
    fn assert_addons_render_as_tile_grid() {
        let meta = AppMeta {
            addons: vec![nix_software_core::Addon {
                name: "Highlighter".into(),
                summary: Some("Syntax highlighting".into()),
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
            .expect("the section must end with a FlowBox grid");
        assert_eq!(flow.max_children_per_line(), 3);

        let tile = flow
            .first_child()
            .and_downcast::<gtk::FlowBoxChild>()
            .and_then(|c| c.child())
            .expect("a tile");
        assert!(tile.has_css_class("card"), "the tile must be a card");
    }
}
