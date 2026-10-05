//! Development gallery (`dev-gallery` feature): a tab listing labeled real
//! scenarios that open the real search/details views, plus the real home
//! embedded. A visual harness to see each kind of app side by side
//! (clickable or not, with or without icon, conditional sections) and catch
//! careless mistakes. Never compiled in release.

use adw::prelude::*;
use onyx_core::Package;
use relm4::prelude::*;

use crate::views::home::{HomeModel, HomeOutput};

/// (label, real query, expected behavior to check).
const SEARCH_SCENARIOS: &[(&str, &str, &str)] = &[
    (
        "App with icon + rating",
        "firefox",
        "“Applications” section, icon, stars, clickable",
    ),
    (
        "Recognized / unrecognized mix",
        "editor",
        "“Applications” (with icons) AND “Other Packages”",
    ),
    (
        "Package set filter",
        "aeson",
        "A “haskellPackages” dropdown shows up",
    ),
    (
        "Provides a command",
        "rg",
        "“Provides the rg command” section (nix-locate)",
    ),
    ("No results", "zzqxnonexistent", "“No Results” page"),
];

/// (label, real attr, expected behavior to check).
const DETAIL_SCENARIOS: &[(&str, &str, &str)] = &[
    ("Full listing", "steam",
     "Screenshots; Add-ons = NON clickable grid; Other apps = CLICKABLE grid; age tile; proprietary license"),
    ("Free software + reviews", "firefox",
     "“Free Software” tile; review histogram; version history"),
    ("Plain runnable Nix", "ripgrep",
     "Little metadata; Run button; no screenshots"),
    ("Keep-only / no metadata", "zlib",
     "No mainProgram; “No metadata” card"),
    // python27 is checked meta.insecure=true; same rendering (red tile +
    // problems banner) as broken. Refresh if it stops being insecure.
    ("Vulnerable (insecure)", "python27",
     "Red “Vulnerable” tile + problems banner"),
];

pub struct GalleryModel {
    // Kept alive: the real home is embedded in the gallery.
    #[allow(dead_code)]
    home: Controller<HomeModel>,
}

#[derive(Debug)]
pub enum GalleryMsg {
    /// Tile click in the embedded home, re-emitted to the parent.
    HomeOpen(Package),
    /// Category card click in the embedded home, re-emitted to the parent.
    HomeCategory(&'static [&'static str], &'static str),
}

#[derive(Debug)]
pub enum GalleryOutput {
    /// Runs a real prefilled search (AppMsg::Search).
    Search(String),
    /// Opens the real details page of an attr (AppMsg::OpenDetail).
    OpenDetail(Package),
    /// Opens a category page (AppMsg::OpenCategory) from the embedded home.
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

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // The real home, embedded: its outputs are routed to the parent.
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

/// Builds the index: three sections (Search / Details / Home).
/// `home_widget` is the root of `HomeModel`, a `gtk::ScrolledWindow`.
fn build_index(
    sender: &ComponentSender<GalleryModel>,
    home_widget: &gtk::ScrolledWindow,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);

    // ── Search ────────────────────────────────────────────────────────────
    content.append(&heading("Search"));
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

    // ── Details ───────────────────────────────────────────────────────────
    content.append(&heading("Details"));
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

    // ── Home (embedded) ───────────────────────────────────────────────────
    content.append(&heading("Home"));
    let note = gtk::Label::new(Some(
        "Icon states to check: loading (shimmer), colored fallback, real icon.",
    ));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    content.append(&note);
    // The home is itself a ScrolledWindow (vexpand): bounded in height and
    // without vexpand it becomes a fixed height panel in the gallery scroll
    // (otherwise two nested scrollers fight over the height).
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

/// A scenario row: title + subtitle = expected behavior.
fn scenario_row(label: &str, expected: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(label)
        .subtitle(expected)
        .activatable(true)
        .build();
    row.set_subtitle_lines(0); // do not truncate the expected behavior
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    row
}
