//! Search results: AppStream apps, other packages, and a filter sidebar.

use std::collections::HashMap;

use adw::prelude::*;
use nix_software_core::Package;
use relm4::factory::FactoryVecDeque;
use relm4::prelude::*;

use crate::components::package_row::PackageRow;
use crate::i18n::t;
use crate::util::{skeleton_list, AppItem};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum License {
    #[default]
    All,
    Free,
    Unfree,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Nixpkgs,
    /// Packages of another flake.
    Flake(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PackageSet {
    None,
    Named(String),
}

/// The sidebar choices; groups combine.
#[derive(Debug, Clone, Default, PartialEq)]
struct Filters {
    by_name: bool,
    installed: bool,
    license: License,
    source: Option<Source>,
    set: Option<PackageSet>,
}

/// One radio button of the sidebar.
#[derive(Debug, Clone)]
pub enum Choice {
    ByName(bool),
    Installed(bool),
    License(License),
    Source(Option<Source>),
    Set(Option<PackageSet>),
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
    sidebar: gtk::Box,
    reset: gtk::Button,
    items: Vec<AppItem>,
    meta: HashMap<String, nix_software_core::ResultMeta>,
    filters: Filters,
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
    /// License and platform per attribute, found after the results.
    SetMeta(HashMap<String, nix_software_core::ResultMeta>),
    Choose(Choice),
    Reset,
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

        // Shown when the sidebar folds away.
        let filters_button = gtk::ToggleButton::builder()
            .label(t("Filters"))
            .halign(gtk::Align::Start)
            .visible(false)
            .build();

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

        let heading_options = section_heading(&t("Options"));
        heading_options.set_visible(false);
        let options_list = crate::util::boxed_list();
        options_list.set_visible(false);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        content.append(&filters_button);
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

        let reset = gtk::Button::builder()
            .label(t("Reset Filters"))
            .sensitive(false)
            .build();
        reset.connect_clicked({
            let sender = sender.clone();
            move |_| sender.input(AppListInput::Reset)
        });
        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 18);
        let side_content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(16)
            .margin_bottom(16)
            .margin_start(12)
            .margin_end(12)
            .build();
        side_content.append(&reset);
        side_content.append(&sidebar);
        let side_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&side_content)
            .build();

        let split = adw::OverlaySplitView::builder()
            .sidebar(&side_scroll)
            .content(&scroll)
            .min_sidebar_width(220.0)
            .max_sidebar_width(280.0)
            .build();
        filters_button
            .bind_property("active", &split, "show-sidebar")
            .bidirectional()
            .build();
        let bin = adw::BreakpointBin::builder()
            .width_request(360)
            .height_request(200)
            .child(&split)
            .build();
        if let Ok(narrow) = adw::BreakpointCondition::parse("max-width: 860sp") {
            let bp = adw::Breakpoint::new(narrow);
            bp.add_setter(&split, "collapsed", Some(&true.to_value()));
            bp.add_setter(&filters_button, "visible", Some(&true.to_value()));
            bin.add_breakpoint(bp);
        }

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
        stack.add_named(&bin, Some("filled"));
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
            sidebar,
            reset,
            items: Vec::new(),
            filters: Filters::default(),
            meta: HashMap::new(),
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
            AppListInput::SetMeta(meta) => {
                self.meta = meta;
                self.rebuild_sidebar(&sender);
                self.apply();
            }
            AppListInput::Set(items) => {
                self.items = items;
                self.meta.clear();
                // A new search keeps only the sort.
                self.filters = Filters {
                    by_name: self.filters.by_name,
                    ..Filters::default()
                };
                self.rebuild_sidebar(&sender);
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
            AppListInput::Choose(choice) => {
                let f = &mut self.filters;
                match choice {
                    Choice::ByName(v) => f.by_name = v,
                    Choice::Installed(v) => f.installed = v,
                    Choice::License(v) => f.license = v,
                    Choice::Source(v) => f.source = v,
                    Choice::Set(v) => f.set = v,
                }
                self.apply();
            }
            AppListInput::Reset => {
                self.filters = Filters::default();
                self.rebuild_sidebar(&sender);
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
            if o.home {
                let tag = gtk::Label::new(Some("Home Manager"));
                tag.add_css_class("dim-label");
                tag.add_css_class("caption");
                row.add_suffix(&tag);
            }
            if nix_software_core::load_manifest()
                .option_value(&o.name)
                .is_some()
            {
                let set = gtk::Image::from_icon_name("installed-check-symbolic");
                set.set_tooltip_text(Some(&t("Set by Nix Software")));
                row.add_suffix(&set);
            }
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

    /// Refills the sidebar with the groups these results have, with counts.
    fn rebuild_sidebar(&mut self, sender: &ComponentSender<Self>) {
        while let Some(child) = self.sidebar.first_child() {
            self.sidebar.remove(&child);
        }
        let mut counts: HashMap<&str, usize> = HashMap::new();
        let mut sources: HashMap<&str, usize> = HashMap::new();
        let mut no_set = 0usize;
        let mut installed = 0usize;
        let runs = |it: &&AppItem| self.meta.get(&it.package.attr).is_none_or(|m| m.available);
        let items: Vec<&AppItem> = self.items.iter().filter(runs).collect();
        for it in &items {
            if crate::util::installed(&it.package).is_some() {
                installed += 1;
            }
            if let Some(src) = it.package.source.as_deref() {
                *sources.entry(src).or_insert(0) += 1;
                continue;
            }
            match it.package.package_set() {
                Some(s) => *counts.entry(s).or_insert(0) += 1,
                None => no_set += 1,
            }
        }
        let total = items.len();
        let nixpkgs = total - sources.values().sum::<usize>();
        let free = self.meta.values().filter(|m| m.available && m.free).count();
        let unfree = self
            .meta
            .values()
            .filter(|m| m.available && !m.free)
            .count();
        let mut sets: Vec<(&str, usize)> = counts.into_iter().collect();
        sets.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let mut flakes: Vec<(&str, usize)> = sources.into_iter().collect();
        flakes.sort();

        let f = &self.filters;
        let mut groups: Vec<(String, Vec<Opt>)> = vec![(
            t("Sort"),
            vec![
                (t("Best Match"), None, !f.by_name, Choice::ByName(false)),
                (t("Name"), None, f.by_name, Choice::ByName(true)),
            ],
        )];
        if installed > 0 {
            groups.push((
                t("Show"),
                vec![
                    (
                        t("All"),
                        Some(total),
                        !f.installed,
                        Choice::Installed(false),
                    ),
                    (
                        t("Installed"),
                        Some(installed),
                        f.installed,
                        Choice::Installed(true),
                    ),
                ],
            ));
        }
        if free > 0 && unfree > 0 {
            let l = f.license;
            groups.push((
                t("License"),
                vec![
                    (
                        t("All"),
                        Some(free + unfree),
                        l == License::All,
                        Choice::License(License::All),
                    ),
                    (
                        t("Free Software"),
                        Some(free),
                        l == License::Free,
                        Choice::License(License::Free),
                    ),
                    (
                        t("Proprietary"),
                        Some(unfree),
                        l == License::Unfree,
                        Choice::License(License::Unfree),
                    ),
                ],
            ));
        }
        if !flakes.is_empty() {
            let mut opts = vec![
                (
                    t("All"),
                    Some(total),
                    f.source.is_none(),
                    Choice::Source(None),
                ),
                (
                    "nixpkgs".to_string(),
                    Some(nixpkgs),
                    f.source == Some(Source::Nixpkgs),
                    Choice::Source(Some(Source::Nixpkgs)),
                ),
            ];
            for (src, n) in flakes {
                let chosen = Source::Flake(src.to_string());
                opts.push((
                    src.split(':').next_back().unwrap_or(src).to_string(),
                    Some(n),
                    f.source.as_ref() == Some(&chosen),
                    Choice::Source(Some(chosen)),
                ));
            }
            groups.push((t("Source"), opts));
        }
        if !sets.is_empty() {
            let mut opts = vec![
                (t("All"), Some(nixpkgs), f.set.is_none(), Choice::Set(None)),
                (
                    t("No Package Set"),
                    Some(no_set),
                    f.set == Some(PackageSet::None),
                    Choice::Set(Some(PackageSet::None)),
                ),
            ];
            for (name, n) in sets {
                let chosen = PackageSet::Named(name.to_string());
                opts.push((
                    name.to_string(),
                    Some(n),
                    f.set.as_ref() == Some(&chosen),
                    Choice::Set(Some(chosen)),
                ));
            }
            groups.push((t("Package Set"), opts));
        }
        for (title, opts) in groups {
            self.sidebar.append(&radio_group(&title, opts, sender));
        }
        self.reset.set_sensitive(self.filters != Filters::default());
    }

    /// Filters the items and fills both sections.
    fn apply(&mut self) {
        self.reset.set_sensitive(self.filters != Filters::default());
        let f = &self.filters;
        let free = |it: &AppItem| self.meta.get(&it.package.attr).map(|m| m.free);
        let keep = |it: &AppItem| {
            let p = &it.package;
            (!f.installed || crate::util::installed(p).is_some())
                && match f.license {
                    License::All => true,
                    License::Free => free(it) == Some(true),
                    License::Unfree => free(it) == Some(false),
                }
                && match &f.source {
                    None => true,
                    Some(Source::Nixpkgs) => p.source.is_none(),
                    Some(Source::Flake(s)) => p.source.as_deref() == Some(s.as_str()),
                }
                && match &f.set {
                    None => true,
                    Some(PackageSet::None) => p.source.is_none() && p.package_set().is_none(),
                    Some(PackageSet::Named(s)) => {
                        p.source.is_none() && p.package_set() == Some(s.as_str())
                    }
                }
        };
        // What cannot run on this machine is never shown.
        let runs = |it: &AppItem| self.meta.get(&it.package.attr).is_none_or(|m| m.available);
        let mut shown: Vec<&AppItem> = self.items.iter().filter(|i| runs(i) && keep(i)).collect();
        if f.by_name {
            shown.sort_by_key(|i| i.package.name.to_lowercase());
        }

        let mut apps = self.apps.guard();
        let mut others = self.others.guard();
        apps.clear();
        others.clear();
        for it in shown {
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

        // Options or commands may have arrived first.
        let has_extra = self.options_list.is_visible()
            || self.locate_list.is_visible()
            || self.types_list.is_visible();
        self.stack
            .set_visible_child_name(if has_apps || has_others || has_extra {
                "filled"
            } else {
                "empty"
            });
    }
}

/// A radio option: label, count, chosen, what choosing it does.
type Opt = (String, Option<usize>, bool, Choice);

/// A titled list of radio rows sending their choice.
fn radio_group(title: &str, opts: Vec<Opt>, sender: &ComponentSender<AppList>) -> gtk::Box {
    let group = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let heading = section_heading(title);
    heading.set_margin_top(0);
    group.append(&heading);
    let list = crate::util::boxed_list();
    let mut first: Option<gtk::CheckButton> = None;
    for (label, count, chosen, choice) in opts {
        let check = gtk::CheckButton::new();
        check.set_group(first.as_ref());
        check.set_active(chosen);
        first.get_or_insert_with(|| check.clone());
        let row = adw::ActionRow::builder()
            .title(crate::util::esc(&label))
            .activatable_widget(&check)
            .build();
        row.add_prefix(&check);
        if let Some(n) = count {
            let l = gtk::Label::new(Some(&n.to_string()));
            l.add_css_class("dim-label");
            row.add_suffix(&l);
        }
        let s = sender.input_sender().clone();
        check.connect_toggled(move |c| {
            if c.is_active() {
                let _ = s.send(AppListInput::Choose(choice.clone()));
            }
        });
        list.append(&row);
    }
    group.append(&list);
    group
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
