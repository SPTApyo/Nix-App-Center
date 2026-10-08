//! Search results: AppStream apps, other packages, and a package set filter.

use std::collections::HashMap;

use adw::prelude::*;
use nix_software_core::Package;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;

use crate::components::package_row::PackageRow;
use crate::i18n::t;
use crate::util::{skeleton_list, AppItem};

#[derive(Clone)]
enum Filter {
    All,
    NoSet,
    Set(String),
}

pub struct AppList {
    apps: FactoryVecDeque<PackageRow>,
    others: FactoryVecDeque<PackageRow>,
    stack: gtk::Stack,
    heading_apps: gtk::Label,
    heading_others: gtk::Label,
    heading_locate: gtk::Label,
    locate_list: gtk::ListBox,
    heading_options: gtk::Label,
    options_list: gtk::ListBox,
    heading_types: gtk::Label,
    types_list: gtk::ListBox,
    filter_dropdown: gtk::DropDown,
    items: Vec<AppItem>,
    filters: Vec<Filter>, // DropDown index to filter
}

#[derive(Debug)]
pub enum AppListInput {
    SetLoading,
    Set(Vec<AppItem>),
    /// Packages providing the queried command.
    SetLocate(String, Vec<Package>),
    /// Apps that open files of the queried type.
    SetFileTypes(String, Vec<Package>),
    /// NixOS options matching the query.
    SetOptions(Vec<nix_software_core::NixOption>),
    Filter,
    Open(Package),
}

#[relm4::component(pub)]
impl SimpleComponent for AppList {
    type Init = ();
    type Input = AppListInput;
    type Output = Package;

    view! {
        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_vexpand: true,
        }
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let apps = FactoryVecDeque::builder()
            .launch(gtk::ListBox::new())
            .forward(sender.input_sender(), AppListInput::Open);
        let others = FactoryVecDeque::builder()
            .launch(gtk::ListBox::new())
            .forward(sender.input_sender(), AppListInput::Open);

        let lists: [&gtk::ListBox; 2] = [apps.widget(), others.widget()];
        for lb in lists {
            lb.add_css_class("boxed-list");
            lb.set_selection_mode(gtk::SelectionMode::None);
            lb.set_valign(gtk::Align::Start);
        }

        let filter_dropdown = gtk::DropDown::from_strings(&[&t("All")]);
        filter_dropdown.set_halign(gtk::Align::Start);
        filter_dropdown.set_visible(false);
        filter_dropdown.connect_selected_notify({
            let sender = sender.clone();
            move |_| sender.input(AppListInput::Filter)
        });

        let heading_apps = section_heading(&t("Applications"));
        let heading_others = section_heading(&t("Other Packages"));
        let heading_locate = section_heading("");
        heading_locate.set_visible(false);

        let locate_list = crate::util::boxed_list();
        locate_list.set_visible(false);

        let heading_types = section_heading("");
        heading_types.set_visible(false);
        let types_list = crate::util::boxed_list();
        types_list.set_visible(false);

        let heading_options = section_heading(&t("NixOS Options"));
        heading_options.set_visible(false);
        let options_list = crate::util::boxed_list();
        options_list.set_visible(false);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        content.append(&filter_dropdown);
        content.append(&heading_apps);
        content.append(apps.widget());
        content.append(&heading_types);
        content.append(&types_list);
        content.append(&heading_others);
        content.append(others.widget());
        content.append(&heading_locate);
        content.append(&locate_list);
        content.append(&heading_options);
        content.append(&options_list);

        let clamp = adw::Clamp::new();
        clamp.set_maximum_size(820);
        clamp.set_margin_top(16);
        clamp.set_margin_bottom(16);
        clamp.set_margin_start(12);
        clamp.set_margin_end(12);
        clamp.set_child(Some(&content));

        let scroll = gtk::ScrolledWindow::builder().vexpand(true).build();
        scroll.set_child(Some(&clamp));
        crate::util::smooth_scroll(&scroll);

        // Placeholder shaped like the real results.
        let loading_heading = gtk::Box::builder()
            .width_request(120)
            .height_request(18)
            .margin_top(6)
            .margin_bottom(6)
            .halign(gtk::Align::Start)
            .build();
        loading_heading.add_css_class("skeleton");
        let loading_content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        loading_content.append(&loading_heading);
        loading_content.append(&skeleton_list(7));

        let loading_clamp = adw::Clamp::new();
        loading_clamp.set_maximum_size(820);
        loading_clamp.set_margin_top(16);
        loading_clamp.set_margin_start(12);
        loading_clamp.set_margin_end(12);
        loading_clamp.set_valign(gtk::Align::Start);
        loading_clamp.set_child(Some(&loading_content));

        let empty = adw::StatusPage::builder()
            .icon_name("edit-find-symbolic")
            .title(t("No Results"))
            .description(t("Try different keywords"))
            .vexpand(true)
            .build();

        let stack = gtk::Stack::builder().vexpand(true).build();
        stack.add_named(&loading_clamp, Some("loading"));
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&scroll, Some("filled"));
        stack.set_visible_child_name("empty");

        let model = AppList {
            apps,
            others,
            stack: stack.clone(),
            heading_apps,
            heading_others,
            heading_locate: heading_locate.clone(),
            locate_list: locate_list.clone(),
            heading_options: heading_options.clone(),
            heading_types: heading_types.clone(),
            types_list: types_list.clone(),
            options_list: options_list.clone(),
            filter_dropdown,
            items: Vec::new(),
            filters: vec![Filter::All],
        };
        let widgets = view_output!();
        root.append(&stack);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AppListInput::SetLoading => {
                self.clear_locate();
                clear(&self.heading_types, &self.types_list);
                self.set_options(&[]);
                self.stack.set_visible_child_name("loading");
            }
            AppListInput::Set(items) => {
                self.items = items;
                self.rebuild_filter();
                self.apply();
            }
            AppListInput::SetLocate(query, packages) => {
                let title = t("Provides the “{}” command").replace("{}", &query);
                self.fill(
                    &self.heading_locate,
                    &self.locate_list,
                    &title,
                    packages,
                    &sender,
                );
            }
            AppListInput::SetFileTypes(query, packages) => {
                let title = t("Opens “{}” files").replace("{}", &query);
                self.fill(
                    &self.heading_types,
                    &self.types_list,
                    &title,
                    packages,
                    &sender,
                );
            }
            AppListInput::SetOptions(options) => {
                self.set_options(&options);
                if !options.is_empty()
                    && self.stack.visible_child_name().as_deref() == Some("empty")
                {
                    self.stack.set_visible_child_name("filled");
                }
            }
            AppListInput::Filter => {
                self.apply();
            }
            AppListInput::Open(pkg) => {
                let _ = sender.output(pkg);
            }
        }
    }
}

impl AppList {
    fn set_options(&self, options: &[nix_software_core::NixOption]) {
        while let Some(child) = self.options_list.first_child() {
            self.options_list.remove(&child);
        }
        for o in options {
            let first = o
                .description
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let row = adw::ActionRow::builder()
                .title(crate::util::esc(&o.name))
                .subtitle(crate::util::esc(&first))
                .subtitle_lines(1)
                .activatable(true)
                .build();
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            let opt = o.clone();
            row.connect_activated(move |row| crate::components::option_dialog::present(row, &opt));
            self.options_list.append(&row);
        }
        self.heading_options.set_visible(!options.is_empty());
        self.options_list.set_visible(!options.is_empty());
    }

    fn clear_locate(&self) {
        clear(&self.heading_locate, &self.locate_list);
    }

    /// Fills a side section with package rows.
    fn fill(
        &self,
        heading: &gtk::Label,
        list: &gtk::ListBox,
        title: &str,
        packages: Vec<Package>,
        sender: &ComponentSender<Self>,
    ) {
        clear(heading, list);
        if packages.is_empty() {
            return;
        }
        heading.set_label(title);
        heading.set_visible(true);
        list.set_visible(true);
        for pkg in packages {
            let row = adw::ActionRow::builder()
                .title(crate::util::esc(if pkg.name.is_empty() {
                    &pkg.attr
                } else {
                    &pkg.name
                }))
                .subtitle(crate::util::esc(&pkg.description))
                .activatable(true)
                .build();
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            let s = sender.input_sender().clone();
            row.connect_activated(move |_| {
                let _ = s.send(AppListInput::Open(pkg.clone()));
            });
            list.append(&row);
        }
        if self.stack.visible_child_name().as_deref() == Some("empty") {
            self.stack.set_visible_child_name("filled");
        }
    }

    /// Refills the package set filter, with counts.
    fn rebuild_filter(&mut self) {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        let mut no_set = 0usize;
        for it in &self.items {
            match it.package.package_set() {
                Some(s) => *counts.entry(s).or_insert(0) += 1,
                None => no_set += 1,
            }
        }

        let mut sets: Vec<(&str, usize)> = counts.into_iter().collect();
        sets.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

        let mut labels: Vec<String> =
            vec![t("All ({})").replace("{}", &self.items.len().to_string())];
        let mut filters: Vec<Filter> = vec![Filter::All];
        if no_set > 0 {
            labels.push(t("No package set ({})").replace("{}", &no_set.to_string()));
            filters.push(Filter::NoSet);
        }
        for (name, n) in &sets {
            labels.push(format!("{name} ({n})"));
            filters.push(Filter::Set((*name).to_string()));
        }

        self.filters = filters;
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.filter_dropdown
            .set_model(Some(&gtk::StringList::new(&refs)));
        self.filter_dropdown.set_selected(0);
        self.filter_dropdown.set_visible(!sets.is_empty());
    }

    /// Filters the items and fills both sections.
    fn apply(&mut self) {
        let idx = self.filter_dropdown.selected() as usize;
        let filter = self.filters.get(idx).cloned().unwrap_or(Filter::All);
        let keep = |it: &AppItem| match &filter {
            Filter::All => true,
            Filter::NoSet => it.package.package_set().is_none(),
            Filter::Set(s) => it.package.package_set() == Some(s.as_str()),
        };

        let mut apps = self.apps.guard();
        let mut others = self.others.guard();
        apps.clear();
        others.clear();
        for it in self.items.iter().filter(|i| keep(i)) {
            // Recognized apps, even without an icon.
            if it.recognized {
                apps.push_back(it.clone());
            } else {
                others.push_back(it.clone());
            }
        }
        drop(apps);
        drop(others);

        let has_apps = !self.apps.is_empty();
        let has_others = !self.others.is_empty();
        self.heading_apps.set_visible(has_apps);
        self.apps.widget().set_visible(has_apps);
        self.others.widget().set_visible(has_others);
        self.heading_others.set_visible(has_others && has_apps);

        self.stack
            .set_visible_child_name(if has_apps || has_others {
                "filled"
            } else {
                "empty"
            });
    }
}

fn clear(heading: &gtk::Label, list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    heading.set_visible(false);
    list.set_visible(false);
}

fn section_heading(text: &str) -> gtk::Label {
    let l = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .margin_top(6)
        .build();
    l.add_css_class("heading");
    l
}
