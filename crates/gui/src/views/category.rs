//! A category page: New & Updated, then Other Software.

use adw::prelude::*;
use nix_software_core::Package;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;

use crate::components::AppTile;
use crate::i18n::t;
use crate::util::{skeleton_grid, AppItem};

type Section = Vec<(Package, Option<String>)>;

pub struct CategoryModel {
    label: String,
    recent: FactoryVecDeque<AppTile>,
    others: FactoryVecDeque<AppTile>,
    loading: bool,
    has_recent: bool,
    has_others: bool,
}

#[derive(Debug)]
pub enum CategoryMsg {
    Loaded((Section, Section)),
    Open(Package),
}

#[relm4::component(pub)]
impl SimpleComponent for CategoryModel {
    type Init = (&'static [&'static str], String); // (slug union, label)
    type Input = CategoryMsg;
    type Output = Package;

    view! {
        adw::NavigationPage {
            set_title: &model.label,

            #[wrap(Some)]
            set_child = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {},

                #[wrap(Some)]
                #[name(scroller)]
                set_content = &gtk::ScrolledWindow {
                    set_vexpand: true,

                    adw::Clamp {
                        set_maximum_size: 1000,
                        set_tightening_threshold: 600,
                        set_margin_top: 24,
                        set_margin_bottom: 24,
                        set_margin_start: 12,
                        set_margin_end: 12,

                        gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 14,

                            #[local_ref]
                            skeleton -> gtk::FlowBox {
                                #[watch]
                                set_visible: model.loading,
                            },

                            adw::StatusPage {
                                #[watch]
                                set_visible: !model.loading && !model.has_recent && !model.has_others,
                                set_vexpand: true,
                                set_icon_name: Some("edit-find-symbolic"),
                                set_title: &t("Nothing to Show"),
                                set_description: Some(&t("None of these apps are in nixpkgs")),
                            },

                            gtk::Label {
                                #[watch]
                                set_visible: model.has_recent,
                                set_label: &t("New & Updated"),
                                set_xalign: 0.0,
                                add_css_class: "heading",
                            },
                            #[local_ref]
                            recent_flow -> gtk::FlowBox {
                                #[watch]
                                set_visible: model.has_recent,
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
                                set_visible: model.has_others,
                                set_label: &t("Other Software"),
                                set_xalign: 0.0,
                                set_margin_top: 12,
                                add_css_class: "heading",
                            },
                            #[local_ref]
                            others_flow -> gtk::FlowBox {
                                #[watch]
                                set_visible: model.has_others,
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
        }
    }

    fn init(
        (slugs, label): Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let recent = FactoryVecDeque::builder()
            .launch(gtk::FlowBox::new())
            .forward(sender.input_sender(), CategoryMsg::Open);
        let others = FactoryVecDeque::builder()
            .launch(gtk::FlowBox::new())
            .forward(sender.input_sender(), CategoryMsg::Open);

        let model = CategoryModel {
            label: t(&label),
            recent,
            others,
            loading: true,
            has_recent: false,
            has_others: false,
        };
        let recent_flow: &gtk::FlowBox = model.recent.widget();
        let others_flow: &gtk::FlowBox = model.others.widget();
        let skeleton = skeleton_grid(9);
        let widgets = view_output!();
        crate::util::smooth_scroll(&widgets.scroller);

        let s = sender.input_sender().clone();
        std::thread::spawn(move || {
            let _ = s.send(CategoryMsg::Loaded(
                nix_software_core::category_sections_union(slugs, 200),
            ));
        });

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            CategoryMsg::Loaded((recent_items, others_items)) => {
                self.has_recent = !recent_items.is_empty();
                self.has_others = !others_items.is_empty();

                let mut g = self.recent.guard();
                for (p, icon) in recent_items {
                    g.push_back(AppItem::new(p, icon));
                }
                drop(g);

                let mut g = self.others.guard();
                for (p, icon) in others_items {
                    g.push_back(AppItem::new(p, icon));
                }
                drop(g);

                self.loading = false;
            }
            CategoryMsg::Open(pkg) => {
                let _ = sender.output(pkg);
            }
        }
    }
}
