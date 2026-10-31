//! Development gallery (`dev-gallery` feature): real searches and details
//! pages for every kind of app, and the home page.

use adw::prelude::*;
use nix_app_center_core::Package;
use relm4::prelude::*;

use crate::views::home::{HomeModel, HomeOutput};

/// (label, query, what to check).
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

/// (label, attr, what to check).
const DETAIL_SCENARIOS: &[(&str, &str, &str)] = &[
    ("Full listing", "steam",
     "Screenshots; Add-ons = NON clickable grid; Other apps = CLICKABLE grid; age tile; proprietary license"),
    ("Free software + reviews", "firefox",
     "“Free Software” tile; review histogram; version history"),
    ("Plain runnable Nix", "ripgrep",
     "Little metadata; Run button; no screenshots"),
    ("Keep-only / no metadata", "zlib",
     "No mainProgram; “No metadata” card"),
    // python27 is marked insecure.
    ("Vulnerable (insecure)", "python27",
     "Red “Vulnerable” tile + problems banner"),
];

pub struct GalleryModel {
    _home: Controller<HomeModel>,
}

#[derive(Debug)]
pub enum GalleryMsg {
    /// A tile of the embedded home, for the parent.
    HomeOpen(Package),
    /// A category card of the embedded home, for the parent.
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

        let model = GalleryModel { _home: home };
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

/// The gallery: search, details and home sections.
fn build_index(
    sender: &ComponentSender<GalleryModel>,
    home_widget: &gtk::ScrolledWindow,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);

    content.append(&heading("Search"));
    let search_list = crate::util::boxed_list();
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

    content.append(&heading("Details"));
    let detail_list = crate::util::boxed_list();
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

    content.append(&heading("Dialogs"));
    let dialog_list = crate::util::boxed_list();
    let row = scenario_row(
        "Binary cache consent",
        "Names the flake, its server and key; Use Cache is suggested",
    );
    row.connect_activated(|row| {
        let cache = nix_app_center_core::Cache {
            url: "https://nix-gaming.cachix.org".into(),
            keys: vec![
                "nix-gaming.cachix.org-1:nbjlureqMbRAxR1gJ/f3hxemL9svXaZF/Ees8vCUUs4=".into(),
            ],
        };
        crate::components::cache_dialog::present(row, "fufexan/nix-gaming", cache, |_| {});
    });
    dialog_list.append(&row);
    content.append(&dialog_list);

    content.append(&heading("Apply"));
    let apply_list = crate::util::boxed_list();
    let row = scenario_row(
        "Review before applying",
        "Summary, kernel and every-user banners, kept apps first, sizes; nothing runs",
    );
    row.connect_activated(|row| {
        let set = nix_app_center_core::ChangeSet {
            changes: nix_app_center_core::parse_changes(
                "firefox: 131.0 → 132.0, 5.2 MiB\nsteam: ∅ → 1.0.0.85, 3.1 MiB\n\
                 vlc: 3.0.21 → ∅, -45.0 MiB\nglibc: 2.40-36 → 2.40-66, 1.0 MiB\n\
                 libfoo: -12.0 KiB\nlinux: 6.12.1 → 6.12.4, 2.0 MiB\n",
            ),
            reboot: true,
            system: true,
        };
        crate::components::apply_dialog::present_review(row, &set);
    });
    apply_list.append(&row);
    content.append(&apply_list);

    content.append(&heading("Updates"));
    let updates_list = crate::util::boxed_list();
    let row = scenario_row(
        "Switch to the next NixOS release",
        "flake.nix diff, stateVersion warning, release notes link; Switch does nothing here",
    );
    row.connect_activated(|row| {
        let original = "nixpkgs.url = \"github:NixOS/nixpkgs/nixos-26.05\";\nhome-manager.url = \"github:nix-community/home-manager/release-26.05\";\n";
        let plan = nix_app_center_core::ReleasePlan {
            release: "26.11".into(),
            edit: Some(nix_app_center_core::ReleaseEdit {
                file: "/etc/nixos/flake.nix".into(),
                original: original.into(),
                patched: original.replace("26.05", "26.11"),
                lock: Vec::new(),
            }),
            diff: "nixpkgs.url: nixos-26.05 → nixos-26.11\nhome-manager.url: release-26.05 → release-26.11\n".into(),
            steps: Vec::new(),
        };
        crate::views::updates::release_dialog(row, plan, |_| {});
    });
    updates_list.append(&row);
    content.append(&updates_list);

    content.append(&heading("Home"));
    let note = gtk::Label::new(Some(
        "Icon states to check: loading (shimmer), colored fallback, real icon.",
    ));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    content.append(&note);
    // Fixed height: avoids two nested scrollers.
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
/// A scenario: its label and what to check.
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
