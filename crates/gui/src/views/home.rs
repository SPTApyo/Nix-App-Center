//! Vue accueil — copie de `gs-overview-page.ui` : carousel vedette → cartes de
//! catégories → « Notre sélection » (Editor's Choice) → « Nouveautés et mises à
//! jour ». Grilles de tuiles dans un seul scroll/clamp (max 1000, tightening 600).

use adw::prelude::*;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;
use onyx_core::Package;

use crate::components::{AppTile, FeaturedCarousel, FeaturedInput};
use crate::util::{skeleton_grid, AppItem};

struct CategoryInfo {
    label: &'static str,
    icon_name: &'static str,
    css_class: &'static str,
    /// Union de catégories freedesktop (cf. GNOME gs-desktop-data.c, groupe « all »).
    cats: &'static [&'static str],
}

// Schéma exact de GNOME Software (ordre + libellés FR + unions).
const CATEGORIES: &[CategoryInfo] = &[
    CategoryInfo { label: "Créer",      icon_name: "org.gnome.Software.Create",    css_class: "category-create",    cats: &["graphics", "audiovideo"] },
    CategoryInfo { label: "Travailler", icon_name: "org.gnome.Software.Work",      css_class: "category-work",      cats: &["office", "utility"] },
    CategoryInfo { label: "Jouer",      icon_name: "org.gnome.Software.Play",      css_class: "category-play",      cats: &["game"] },
    CategoryInfo { label: "Socialiser", icon_name: "org.gnome.Software.Socialize", css_class: "category-socialize", cats: &["network"] },
    CategoryInfo { label: "Apprendre",  icon_name: "org.gnome.Software.Learn",     css_class: "category-learn",     cats: &["education", "science"] },
    CategoryInfo { label: "Développer", icon_name: "org.gnome.Software.Develop",   css_class: "category-develop",   cats: &["development"] },
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
    Category(&'static [&'static str], &'static str), // (slugs union, libellé)
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

                    // Carousel vedette (skeleton tant que pas chargé)
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

                    // Cartes de catégories (3 colonnes), remplies en init
                    #[name(cat_grid)]
                    gtk::Grid {
                        set_margin_top: 21,
                        set_margin_bottom: 6,
                        set_row_spacing: 14,
                        set_column_spacing: 14,
                        set_column_homogeneous: true,
                    },

                    // Notre sélection (Editor's Choice)
                    gtk::Label {
                        #[watch]
                        set_visible: !model.editor_loaded || model.has_editor,
                        set_label: "Notre sélection",
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

                    // Nouveautés et mises à jour
                    gtk::Label {
                        #[watch]
                        set_visible: !model.recent_loaded || model.has_recent,
                        set_label: "Nouveautés et mises à jour",
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

    fn init(_: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
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

        // Skeletons de chargement.
        // Le carousel réel = Box(spacing 12, height 318) { carte vexpand .card, points }.
        // On reproduit la même structure pour un placeholder à l'identique.
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

        // Cartes de catégories.
        for (i, cat) in CATEGORIES.iter().enumerate() {
            let btn = build_category_button(cat, &sender);
            widgets
                .cat_grid
                .attach(&btn, (i % 3) as i32, (i / 3) as i32, 1, 1);
        }

        // Chargements en arrière-plan.
        let s1 = sender.input_sender().clone();
        std::thread::spawn(move || {
            let _ = s1.send(HomeMsg::PopularLoaded(onyx_core::popular_packages(30)));
        });
        let s2 = sender.input_sender().clone();
        std::thread::spawn(move || {
            let _ = s2.send(HomeMsg::RecentLoaded(onyx_core::recently_updated_packages(20)));
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            HomeMsg::Open(pkg) => {
                let _ = sender.output(HomeOutput::Open(pkg));
            }
            HomeMsg::PopularLoaded(items) => {
                // 6 premières → carousel ; 12 suivantes → « Notre sélection ».
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
    let img = gtk::Image::builder().icon_name(cat.icon_name).pixel_size(32).build();
    img.add_css_class("icon-dropshadow");
    let lbl = gtk::Label::builder()
        .label(cat.label)
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
