//! Home page: featured carousel, category cards, Editor's Choice, and
//! New & Updated.

use adw::prelude::*;
use nix_software_core::Package;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;

use crate::components::{AppTile, FeaturedCarousel, FeaturedInput};
use crate::i18n::{n_, t};
use crate::util::{skeleton_grid, AppItem};

struct CategoryInfo {
    label: &'static str,
    icon_name: &'static str,
    css_class: &'static str,
    /// Freedesktop categories merged into this card.
    cats: &'static [&'static str],
}

// GNOME Software's categories.
const CATEGORIES: &[CategoryInfo] = &[
    CategoryInfo {
        label: n_("Create"),
        icon_name: "org.gnome.Software.Create",
        css_class: "category-create",
        cats: &["graphics", "audiovideo"],
    },
    CategoryInfo {
        label: n_("Work"),
        icon_name: "org.gnome.Software.Work",
        css_class: "category-work",
        cats: &["office", "utility"],
    },
    CategoryInfo {
        label: n_("Play"),
        icon_name: "org.gnome.Software.Play",
        css_class: "category-play",
        cats: &["game"],
    },
    CategoryInfo {
        label: n_("Socialize"),
        icon_name: "org.gnome.Software.Socialize",
        css_class: "category-socialize",
        cats: &["network"],
    },
    CategoryInfo {
        label: n_("Learn"),
        icon_name: "org.gnome.Software.Learn",
        css_class: "category-learn",
        cats: &["education", "science"],
    },
    CategoryInfo {
        label: n_("Develop"),
        icon_name: "org.gnome.Software.Develop",
        css_class: "category-develop",
        cats: &["development"],
    },
];

type Items = Vec<(Package, Option<String>)>;

pub struct HomeModel {
    carousel: Controller<FeaturedCarousel>,
    editor: FactoryVecDeque<AppTile>,
    recent: FactoryVecDeque<AppTile>,
    has_editor: bool,
    has_recent: bool,
    editor_loaded: bool,
    recent_loaded: bool,
}

#[derive(Debug)]
pub enum HomeMsg {
    Open(Package),
    PopularLoaded(Items),
    RecentLoaded(Items),
}

#[derive(Debug)]
pub enum HomeOutput {
    Category(&'static [&'static str], &'static str), // (slug union, label)
    Open(Package),
}

#[relm4::component(pub)]
impl SimpleComponent for HomeModel {
    type Init = ();
    type Input = HomeMsg;
    type Output = HomeOutput;

    view! {
        #[name(scroller)]
        gtk::ScrolledWindow {
            set_vexpand: true,
            set_hexpand: true,
            set_hscrollbar_policy: gtk::PolicyType::Never,

            adw::Clamp {
                set_hexpand: true,
                set_maximum_size: 1000,
                set_tightening_threshold: 600,

                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_hexpand: true,
                    set_margin_top: 24,
                    set_margin_bottom: 36,
                    set_margin_start: 12,
                    set_margin_end: 12,
                    set_valign: gtk::Align::Start,
                    set_spacing: 6,

                    #[local_ref]
                    carousel_skeleton -> gtk::Box {
                        #[watch]
                        set_visible: !model.editor_loaded,
                    },
                    #[local_ref]
                    carousel_widget -> gtk::Box {
                        #[watch]
                        set_visible: model.editor_loaded,
                    },

                    #[name(cat_grid)]
                    gtk::Grid {
                        set_margin_top: 21,
                        set_margin_bottom: 6,
                        set_row_spacing: 14,
                        set_column_spacing: 14,
                        set_column_homogeneous: true,
                    },

                    gtk::Label {
                        #[watch]
                        set_visible: !model.editor_loaded || model.has_editor,
                        set_label: &t("Editor's Choice"),
                        set_xalign: 0.0,
                        set_margin_top: 21,
                        set_margin_bottom: 6,
                        add_css_class: "heading",
                    },
                    #[local_ref]
                    editor_skeleton -> gtk::FlowBox {
                        #[watch]
                        set_visible: !model.editor_loaded,
                    },
                    #[local_ref]
                    editor_flow -> gtk::FlowBox {
                        #[watch]
                        set_visible: model.editor_loaded && model.has_editor,
                        set_homogeneous: true,
                        set_column_spacing: 14,
                        set_row_spacing: 14,
                        set_min_children_per_line: 1,
                        set_max_children_per_line: 3,
                        set_hexpand: true,
                        set_valign: gtk::Align::Start,
                        set_selection_mode: gtk::SelectionMode::None,
                    },

                    gtk::Label {
                        #[watch]
                        set_visible: !model.recent_loaded || model.has_recent,
                        set_label: &t("New & Updated"),
                        set_xalign: 0.0,
                        set_margin_top: 21,
                        set_margin_bottom: 6,
                        add_css_class: "heading",
                    },
                    #[local_ref]
                    recent_skeleton -> gtk::FlowBox {
                        #[watch]
                        set_visible: !model.recent_loaded,
                    },
                    #[local_ref]
                    recent_flow -> gtk::FlowBox {
                        #[watch]
                        set_visible: model.recent_loaded && model.has_recent,
                        set_homogeneous: true,
                        set_column_spacing: 14,
                        set_row_spacing: 14,
                        set_min_children_per_line: 1,
                        set_max_children_per_line: 3,
                        set_hexpand: true,
                        set_valign: gtk::Align::Start,
                        set_selection_mode: gtk::SelectionMode::None,
                    },
                }
            }
        }
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let carousel = FeaturedCarousel::builder()
            .launch(())
            .forward(sender.input_sender(), HomeMsg::Open);
        let editor = FactoryVecDeque::builder()
            .launch(gtk::FlowBox::new())
            .forward(sender.input_sender(), HomeMsg::Open);
        let recent = FactoryVecDeque::builder()
            .launch(gtk::FlowBox::new())
            .forward(sender.input_sender(), HomeMsg::Open);

        let model = HomeModel {
            carousel,
            editor,
            recent,
            has_editor: false,
            has_recent: false,
            editor_loaded: false,
            recent_loaded: false,
        };
        let carousel_widget = model.carousel.widget();
        let editor_flow: &gtk::FlowBox = model.editor.widget();
        let recent_flow: &gtk::FlowBox = model.recent.widget();

        // Placeholder shaped like the carousel.
        let carousel_skeleton = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .height_request(318)
            .build();
        let carousel_card = gtk::Box::builder().vexpand(true).build();
        carousel_card.add_css_class("skeleton");
        carousel_card.add_css_class("large");
        let carousel_dots = gtk::Box::builder()
            .height_request(12)
            .width_request(72)
            .halign(gtk::Align::Center)
            .build();
        carousel_dots.add_css_class("skeleton");
        carousel_skeleton.append(&carousel_card);
        carousel_skeleton.append(&carousel_dots);
        let editor_skeleton = skeleton_grid(6);
        let recent_skeleton = skeleton_grid(6);

        let widgets = view_output!();
        crate::util::smooth_scroll(&widgets.scroller);

        for (i, cat) in CATEGORIES.iter().enumerate() {
            let btn = build_category_button(cat, &sender);
            widgets
                .cat_grid
                .attach(&btn, (i % 3) as i32, (i / 3) as i32, 1, 1);
        }

        let s1 = sender.input_sender().clone();
        std::thread::spawn(move || {
            let _ = s1.send(HomeMsg::PopularLoaded(nix_software_core::popular_packages(
                30,
            )));
        });
        let s2 = sender.input_sender().clone();
        std::thread::spawn(move || {
            let _ = s2.send(HomeMsg::RecentLoaded(
                nix_software_core::recently_updated_packages(20),
            ));
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            HomeMsg::Open(pkg) => {
                let _ = sender.output(HomeOutput::Open(pkg));
            }
            HomeMsg::PopularLoaded(items) => {
                // Six for the carousel, twelve for Editor's Choice.
                let carousel_items: Vec<AppItem> = items
                    .iter()
                    .take(6)
                    .map(|(p, icon)| AppItem::new(p.clone(), icon.clone()))
                    .collect();
                self.carousel.emit(FeaturedInput::Set(carousel_items));

                let mut g = self.editor.guard();
                for (p, icon) in items.into_iter().skip(6).take(12) {
                    g.push_back(AppItem::new(p, icon));
                }
                drop(g);
                self.has_editor = !self.editor.is_empty();
                self.editor_loaded = true;
            }
            HomeMsg::RecentLoaded(items) => {
                let mut g = self.recent.guard();
                for (p, icon) in items.into_iter().take(12) {
                    g.push_back(AppItem::new(p, icon));
                }
                drop(g);
                self.has_recent = !self.recent.is_empty();
                self.recent_loaded = true;
            }
        }
    }
}

fn build_category_button(cat: &CategoryInfo, sender: &ComponentSender<HomeModel>) -> gtk::Button {
    let img = gtk::Image::builder()
        .icon_name(cat.icon_name)
        .pixel_size(32)
        .build();
    img.add_css_class("icon-dropshadow");
    let lbl = gtk::Label::builder()
        .label(t(cat.label))
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();

    let bx = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(10)
        .build();
    bx.append(&img);
    bx.append(&lbl);

    let btn = gtk::Button::builder().child(&bx).hexpand(true).build();
    btn.add_css_class("category-card");
    btn.add_css_class(cat.css_class);

    let cats = cat.cats;
    let label = cat.label;
    let sender = sender.clone();
    btn.connect_clicked(move |_| {
        let _ = sender.output(HomeOutput::Category(cats, label));
    });
    btn
}
