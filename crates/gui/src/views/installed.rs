//! Installed tab: every app on the machine in one list tagged by origin,
//! the command-line tools kept, and the apps tried (removable from the store).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use adw::prelude::*;
use nix_app_center_core::{Package, ProfileApp};
use relm4::prelude::*;

use crate::i18n::t;
use crate::util::{self, esc};

/// A kept or tried package and what its row needs.
#[derive(Debug, Clone)]
pub struct Item {
    package: Package,
    icon: Option<Vec<u8>>,
    /// The AppStream summary, for apps that have one.
    summary: Option<String>,
    /// The package's own desktop file.
    desktop_file: Option<PathBuf>,
    tried_at: Option<u64>,
    /// Store paths the package fetched.
    paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct Data {
    kept: Vec<Item>,
    tried: Vec<Item>,
    profile: Vec<ProfileApp>,
    sizes: std::collections::HashMap<PathBuf, u64>,
    elements: Vec<nix_app_center_core::ProfileElement>,
    /// What is installed, and how: for the status icons.
    index: Vec<nix_app_center_core::Installed>,
    /// The configuration changed since the last apply.
    pending: bool,
}

impl Data {
    /// Whether the rebuild already installed a kept package.
    /// Without pending changes everything kept was applied; with them, the
    /// index tells, though an attribute named unlike its package is missed.
    fn applied(&self, item: &Item) -> bool {
        !self.pending
            || nix_app_center_core::find_installed(&self.index, &item.package.attr, "", None)
                .is_some()
    }

    /// The kept packages no profile app stands for: command-line tools,
    /// plus apps the next apply installs.
    fn unmatched(&self) -> Vec<&Item> {
        self.kept
            .iter()
            .filter(|i| !self.profile.iter().any(|a| stands_for(a, i)))
            .collect()
    }

    /// The `nix profile` element holding `store_path`, never Home Manager's.
    fn removable(&self, store_path: &Path) -> Option<&str> {
        let path = store_path.to_string_lossy();
        self.elements
            .iter()
            .find(|e| !e.is_home_manager() && e.store_paths.iter().any(|p| *p == path))
            .map(|e| e.name.as_str())
    }

    /// " · 120 MB" for `paths`, empty when unknown.
    fn size_suffix<'a>(&self, paths: impl IntoIterator<Item = &'a PathBuf>) -> String {
        let total: u64 = paths.into_iter().filter_map(|p| self.sizes.get(p)).sum();
        if total == 0 {
            String::new()
        } else {
            format!(" · {}", gtk::glib::format_size(total))
        }
    }
}

/// The page the switcher shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Apps,
    Tools,
    Tried,
}

pub struct InstalledModel {
    page: Page,
    query: String,
    tabs: Vec<(Page, gtk::ToggleButton)>,
    data: Option<Data>,
    removing: HashSet<String>,
    applying: bool,
    gc_running: bool,
    status: Option<String>,
    banner: adw::Banner,
    content: gtk::Box,
}

#[derive(Debug)]
pub enum InstalledMsg {
    Reload,
    DataReady(Data),
    Open(Package),
    Unkeep(String),
    RemoveTried(String),
    ProfileRemove(String),
    Removed(String, Result<u64, String>),
    Apply,
    Applied(Result<nix_app_center_core::ApplyOutcome, String>),
    Gc,
    GcDone(String),
    Show(Page),
    Query(String),
    /// Explains what keeps these store paths.
    Why(String, Vec<String>),
}

#[relm4::component(pub)]
impl SimpleComponent for InstalledModel {
    type Init = ();
    type Input = InstalledMsg;
    type Output = Package;

    view! {
        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            #[local_ref]
            banner -> adw::Banner {},
            gtk::ScrolledWindow {
                set_hscrollbar_policy: gtk::PolicyType::Never,
                set_vexpand: true,
                #[name(clamp)]
                adw::Clamp {
                    set_maximum_size: 860,
                    set_tightening_threshold: 576,
                    set_margin_top: 24,
                    set_margin_bottom: 24,
                    set_margin_start: 12,
                    set_margin_end: 12,
                }
            }
        }
    }

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let banner = adw::Banner::new(&t("Your configuration changed. Apply it to uninstall."));
        banner.set_button_label(Some(&t("_Apply")));
        banner.connect_button_clicked({
            let s = sender.input_sender().clone();
            move |_| {
                let _ = s.send(InstalledMsg::Apply);
            }
        });
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .build();

        let search = gtk::SearchEntry::builder()
            .placeholder_text(t("Filter installed apps"))
            .hexpand(true)
            .build();
        search.connect_search_changed({
            let s = sender.input_sender().clone();
            move |e| {
                let _ = s.send(InstalledMsg::Query(e.text().to_string()));
            }
        });
        let switcher = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        switcher.add_css_class("linked");
        let mut tabs: Vec<(Page, gtk::ToggleButton)> = Vec::new();
        for page in [Page::Apps, Page::Tools, Page::Tried] {
            let button = gtk::ToggleButton::new();
            button.set_active(page == Page::Apps);
            if let Some((_, first)) = tabs.first() {
                button.set_group(Some(first));
            }
            let s = sender.input_sender().clone();
            button.connect_toggled(move |b| {
                if b.is_active() {
                    let _ = s.send(InstalledMsg::Show(page));
                }
            });
            switcher.append(&button);
            tabs.push((page, button));
        }
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        controls.append(&switcher);
        controls.append(&search);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 24);
        column.append(&controls);
        column.append(&content);

        let model = InstalledModel {
            page: Page::Apps,
            query: String::new(),
            tabs,
            data: None,
            removing: HashSet::new(),
            applying: false,
            gc_running: false,
            status: None,
            banner: banner.clone(),
            content: content.clone(),
        };
        let widgets = view_output!();
        widgets.clamp.set_child(Some(&column));
        if let Some(scroller) = widgets.clamp.parent().and_downcast::<gtk::ScrolledWindow>() {
            util::smooth_scroll(&scroller);
        }

        model.rebuild(&sender);
        sender.input(InstalledMsg::Reload);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            InstalledMsg::Reload => {
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let _ = s.send(InstalledMsg::DataReady(load_data()));
                    // List again once icons are known.
                    if !nix_app_center_core::appstream_settled() {
                        nix_app_center_core::wait_appstream(std::time::Duration::from_secs(180));
                        let _ = s.send(InstalledMsg::DataReady(load_data()));
                    }
                });
            }
            InstalledMsg::DataReady(data) => {
                if self.data.is_some() {
                    self.content.add_css_class("settled");
                }
                self.data = Some(data);
                self.rebuild(&sender);
            }
            InstalledMsg::Open(pkg) => {
                let _ = sender.output(pkg);
            }
            InstalledMsg::Unkeep(attr) => {
                match nix_app_center_core::unkeep(&attr) {
                    Ok(()) => {
                        if let Some(d) = self.data.as_mut() {
                            d.kept.retain(|i| i.package.attr != attr);
                        }
                        self.status = None;
                        self.banner.set_revealed(true);
                    }
                    Err(e) => self.status = Some(crate::util::failed(&e)),
                }
                self.rebuild(&sender);
            }
            InstalledMsg::RemoveTried(attr) => {
                if !self.removing.insert(attr.clone()) {
                    return;
                }
                self.rebuild(&sender);
                let name = self
                    .data
                    .as_ref()
                    .and_then(|d| d.tried.iter().find(|i| i.package.attr == attr))
                    .map(|i| display_name(&i.package))
                    .unwrap_or_else(|| attr.clone());
                let s = sender.input_sender().clone();
                let task_attr = attr.clone();
                crate::components::apply_dialog::present_task(
                    &self.content,
                    &t("Removing {}").replace("{}", &name),
                    move || {
                        nix_app_center_core::remove_tried(&task_attr).map_err(|e| e.to_string())
                    },
                    move |bytes| freed_text(&name, *bytes),
                    move |res| {
                        let _ = s.send(InstalledMsg::Removed(attr.clone(), res));
                    },
                );
            }
            InstalledMsg::ProfileRemove(name) => {
                self.status = Some(match nix_app_center_core::profile_remove(&name) {
                    Ok(()) => t("{} uninstalled.").replace("{}", &name),
                    Err(e) => crate::util::failed(&e),
                });
                sender.input(InstalledMsg::Reload);
            }
            InstalledMsg::Removed(attr, res) => {
                self.removing.remove(&attr);
                let name = self
                    .data
                    .as_ref()
                    .and_then(|d| d.tried.iter().find(|i| i.package.attr == attr))
                    .map(|i| display_name(&i.package))
                    .unwrap_or_else(|| attr.clone());
                if res.is_ok() {
                    if let Some(d) = self.data.as_mut() {
                        d.tried.retain(|i| i.package.attr != attr);
                    }
                }
                self.status = Some(match res {
                    Ok(bytes) => freed_text(&name, bytes),
                    Err(e) => crate::util::failed(&e),
                });
                self.rebuild(&sender);
            }
            InstalledMsg::Apply => {
                if self.applying {
                    return;
                }
                self.banner.set_revealed(false);
                let Some(argv) = nix_app_center_core::apply_argv() else {
                    sender.input(InstalledMsg::Applied(Ok(
                        nix_app_center_core::ApplyOutcome::UnknownLayout,
                    )));
                    return;
                };
                self.applying = true;
                let s = sender.input_sender().clone();
                crate::components::apply_dialog::present(
                    &self.content,
                    &t("Applying Configuration"),
                    vec![argv],
                    move |res| {
                        let _ = s.send(InstalledMsg::Applied(res));
                    },
                );
            }
            InstalledMsg::Applied(res) => {
                self.applying = false;
                self.status = Some(util::describe_apply(res));
                self.rebuild(&sender);
                sender.input(InstalledMsg::Reload);
            }
            InstalledMsg::Gc => {
                if self.gc_running {
                    return;
                }
                self.gc_running = true;
                self.rebuild(&sender);
                let s = sender.input_sender().clone();
                let said = |out: &String| {
                    if out.is_empty() {
                        t("Done.")
                    } else {
                        out.clone()
                    }
                };
                crate::components::apply_dialog::present_task(
                    &self.content,
                    &t("Cleaning Up the Store"),
                    || nix_app_center_core::garbage_collect().map_err(|e| e.to_string()),
                    said,
                    move |res| {
                        let msg = match res {
                            Ok(out) => said(&out),
                            Err(e) => crate::util::failed(&e),
                        };
                        let _ = s.send(InstalledMsg::GcDone(msg));
                    },
                );
            }
            InstalledMsg::GcDone(status) => {
                self.gc_running = false;
                self.status = Some(status);
                self.rebuild(&sender);
            }
            InstalledMsg::Show(page) => {
                self.page = page;
                self.rebuild(&sender);
            }
            InstalledMsg::Query(query) => {
                self.query = query;
                self.rebuild(&sender);
            }
            InstalledMsg::Why(name, paths) => {
                crate::views::storage::explain(&self.content, name, paths);
            }
        }
    }
}

impl InstalledModel {
    /// Fills `content` from the current state.
    fn rebuild(&self, sender: &ComponentSender<Self>) {
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
        if let Some(status) = &self.status {
            let lbl = gtk::Label::new(Some(status));
            lbl.add_css_class("dim-label");
            lbl.set_wrap(true);
            lbl.set_xalign(0.0);
            self.content.append(&lbl);
        }
        let Some(data) = &self.data else {
            self.content.append(&util::skeleton_list(4));
            return;
        };

        let apps = self.app_rows(data, sender);
        let tools: Vec<&Item> = data
            .unmatched()
            .into_iter()
            .filter(|i| !is_app(i) || data.applied(i))
            .collect();
        for (page, button) in &self.tabs {
            let (label, count) = match page {
                Page::Apps => (t("Applications"), apps.len()),
                Page::Tools => (t("Tools"), tools.len()),
                Page::Tried => (t("Tried"), data.tried.len()),
            };
            button.set_label(&format!("{label}  {count}"));
        }
        let query = self.query.to_lowercase();
        let wanted = |text: &str| query.is_empty() || text.to_lowercase().contains(&query);
        match self.page {
            Page::Apps => {
                let rows: Vec<gtk::Widget> = apps
                    .into_iter()
                    .filter(|(name, _)| wanted(name))
                    .map(|(_, row)| row.upcast())
                    .collect();
                self.content.append(&section(
                    &t("Applications"),
                    Some(&t("Every app on this computer, whoever installed it.")),
                    rows,
                    &t("No app matches."),
                    None,
                ));
            }
            Page::Tools => {
                let tools: Vec<&Item> = tools
                    .into_iter()
                    .filter(|i| wanted(&i.package.attr))
                    .collect();
                let body: gtk::Widget = if tools.is_empty() {
                    util::dim_label(&t(
                        "No command-line tools kept yet. Open a package and choose “Add to Configuration”.",
                    ))
                    .upcast()
                } else {
                    self.tools_grid(&tools, data, sender).upcast()
                };
                self.content.append(&util::section(
                    &t("Command-Line Tools"),
                    Some(&t(
                        "Packages without an app window. Open one to see its page or remove it.",
                    )),
                    &body,
                ));
            }
            Page::Tried => self.tried_page(data, &wanted, sender),
        }
    }

    /// Every app, kept or not, by name, each tagged with where it comes from.
    fn app_rows(
        &self,
        data: &Data,
        sender: &ComponentSender<Self>,
    ) -> Vec<(String, adw::ActionRow)> {
        use gtk::gio::prelude::AppInfoExt;
        let mut rows: Vec<(String, adw::ActionRow)> = Vec::new();
        let mut placed: HashSet<String> = HashSet::new();
        for a in &data.profile {
            let Some(info) = gtk::gio::DesktopAppInfo::from_filename(&a.desktop_file)
                .filter(|i| i.should_show())
            else {
                continue;
            };
            let kept = data.kept.iter().find(|i| stands_for(a, i));
            // The desktop file describes what really runs.
            let subtitle = info
                .description()
                .map(|d| d.to_string())
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| a.version.clone() + &data.size_suffix([&a.store_path]));
            let Some(row) = util::desktop_row(&a.desktop_file, &subtitle) else {
                continue;
            };
            if kept.is_some() {
                add_tags(&row, &[t("Nix App Center")]);
            }
            let stem = a
                .desktop_file
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned());
            let status =
                nix_app_center_core::find_installed(&data.index, &a.pname, "", stem.as_deref());
            row.add_suffix(&util::installed_pill(status, 16));
            row.add_suffix(&open_button(a.desktop_file.clone()));
            let mut actions: Vec<Action> = Vec::new();
            if let Some(i) = kept {
                placed.insert(i.package.attr.clone());
                actions.push((
                    t("Show _Details"),
                    false,
                    InstalledMsg::Open(i.package.clone()),
                ));
            }
            actions.push((
                t("_Why Is It Here?"),
                false,
                InstalledMsg::Why(
                    info.name().to_string(),
                    vec![a.store_path.to_string_lossy().to_string()],
                ),
            ));
            if let Some(i) = kept {
                actions.push((
                    t("_Remove from Configuration"),
                    true,
                    InstalledMsg::Unkeep(i.package.attr.clone()),
                ));
            } else if let Some(name) = data.removable(&a.store_path) {
                actions.push((
                    t("_Uninstall"),
                    true,
                    InstalledMsg::ProfileRemove(name.to_string()),
                ));
            }
            row.add_suffix(&menu(actions, sender));
            rows.push((format!("{} {}", info.name(), a.pname), row));
        }
        for item in data
            .unmatched()
            .into_iter()
            .filter(|i| is_app(i) && !data.applied(i) && !placed.contains(&i.package.attr))
        {
            let subtitle = item.summary.as_deref().unwrap_or(&item.package.version);
            let row = self.item_row(item, subtitle, sender);
            add_tags(&row, &[t("Nix App Center"), t("Not Applied Yet")]);
            row.add_suffix(&menu(
                vec![
                    (
                        t("Show _Details"),
                        false,
                        InstalledMsg::Open(item.package.clone()),
                    ),
                    (
                        t("_Remove from Configuration"),
                        true,
                        InstalledMsg::Unkeep(item.package.attr.clone()),
                    ),
                ],
                sender,
            ));
            rows.push((
                format!("{} {}", display_name(&item.package), item.package.attr),
                row,
            ));
        }
        rows.sort_by_key(|(name, _)| name.to_lowercase());
        rows
    }

    /// Apps run with Try, each removable from the store.
    fn tried_page(
        &self,
        data: &Data,
        wanted: &dyn Fn(&str) -> bool,
        sender: &ComponentSender<Self>,
    ) {
        let rows: Vec<gtk::Widget> = data
            .tried
            .iter()
            .filter(|i| wanted(&display_name(&i.package)) || wanted(&i.package.attr))
            .map(|i| {
                let when = i
                    .tried_at
                    .map(|ts| util::format_relative(ts as i64))
                    .unwrap_or_default();
                let row = self.item_row(i, &(when + &data.size_suffix(&i.paths)), sender);
                let busy = self.removing.contains(&i.package.attr);
                if busy {
                    let spinner = gtk::Spinner::new();
                    spinner.start();
                    row.add_suffix(&spinner);
                }
                row.add_suffix(&self.action_button(
                    &t("Remove"),
                    busy,
                    InstalledMsg::RemoveTried(i.package.attr.clone()),
                    sender,
                ));
                row.set_tooltip_text(Some(&t(
                    "Remove deletes what this try downloaded, keeping what other apps need.",
                )));
                row.upcast()
            })
            .collect();
        let gc = (!data.tried.is_empty()).then(|| {
            let b = self.action_button(
                &t("_Free Up Space"),
                self.gc_running,
                InstalledMsg::Gc,
                sender,
            );
            b.set_tooltip_text(Some(&t(
                "Runs nix-collect-garbage: removes everything nothing uses from the store",
            )));
            b
        });
        self.content.append(&section(
            &t("Recently Tried"),
            Some(&t(
                "Removing a tried app deletes it from the store and keeps what other apps need.",
            )),
            rows,
            &t("Nothing tried yet. Run a package from search and it will show up here."),
            gc,
        ));
    }

    /// Kept packages without an app identity, as compact buttons.
    fn tools_grid(
        &self,
        tools: &[&Item],
        data: &Data,
        sender: &ComponentSender<Self>,
    ) -> gtk::FlowBox {
        let flow = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(2)
            .max_children_per_line(3)
            .column_spacing(4)
            .row_spacing(4)
            .build();
        flow.add_css_class("card");
        flow.add_css_class("tools-grid");
        for item in tools {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            line.append(&gtk::Image::from_icon_name("utilities-terminal-symbolic"));
            let label = gtk::Label::new(Some(&item.package.attr));
            label.add_css_class("monospace");
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_xalign(0.0);
            line.append(&label);
            let button = gtk::Button::builder()
                .child(&line)
                .tooltip_text(t("Show Details"))
                .build();
            button.add_css_class("flat");
            if !data.applied(item) {
                line.add_css_class("dim-label");
                button.set_tooltip_text(Some(&t("Not Applied Yet")));
            }
            let (s, pkg) = (sender.input_sender().clone(), item.package.clone());
            button.connect_clicked(move |_| {
                let _ = s.send(InstalledMsg::Open(pkg.clone()));
            });
            flow.append(&button);
        }
        flow
    }

    fn item_row(
        &self,
        item: &Item,
        subtitle: &str,
        sender: &ComponentSender<Self>,
    ) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(esc(&display_name(&item.package)))
            .subtitle(esc(subtitle))
            .activatable(true)
            .build();
        row.add_prefix(&item_icon(item));
        let s = sender.input_sender().clone();
        let pkg = item.package.clone();
        row.connect_activated(move |_| {
            let _ = s.send(InstalledMsg::Open(pkg.clone()));
        });
        row
    }

    fn action_button(
        &self,
        label: &str,
        busy: bool,
        msg: InstalledMsg,
        sender: &ComponentSender<Self>,
    ) -> gtk::Button {
        let b = gtk::Button::with_mnemonic(label);
        b.set_valign(gtk::Align::Center);
        b.set_sensitive(!busy);
        let s = sender.input_sender().clone();
        let msg = std::cell::Cell::new(Some(msg));
        b.connect_clicked(move |_| {
            if let Some(m) = msg.take() {
                let _ = s.send(m);
            }
        });
        b
    }
}

/// A menu entry: label, destructive, message sent.
type Action = (String, bool, InstalledMsg);

/// True when the profile app `a` is the kept package `item`: by pname, or
/// by desktop file name (steam ships steam.desktop from steam-unwrapped).
fn stands_for(a: &ProfileApp, item: &Item) -> bool {
    let attr = item.package.attr.rsplit('.').next().unwrap_or_default();
    let stem = a
        .desktop_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    a.pname.eq_ignore_ascii_case(attr) || stem.eq_ignore_ascii_case(attr)
}

/// True for a kept package with an app identity.
fn is_app(item: &Item) -> bool {
    item.icon.is_some() || item.desktop_file.is_some()
}

/// Small tags under a row's title, such as where the app comes from.
fn add_tags(row: &adw::ActionRow, tags: &[String]) {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    line.set_valign(gtk::Align::Center);
    for tag in tags {
        let label = gtk::Label::new(Some(tag));
        label.add_css_class("chip");
        line.append(&label);
    }
    row.add_suffix(&line);
}

/// The "more" button, holding `actions`.
fn menu(actions: Vec<Action>, sender: &ComponentSender<InstalledModel>) -> gtk::MenuButton {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let popover = gtk::Popover::builder().child(&list).build();
    for (label, destructive, msg) in actions {
        let button = gtk::Button::with_mnemonic(&label);
        button.add_css_class("flat");
        if destructive {
            button.add_css_class("error");
        }
        if let Some(l) = button.child().and_downcast::<gtk::Label>() {
            l.set_xalign(0.0);
        }
        let (s, p) = (sender.input_sender().clone(), popover.clone());
        let msg = std::cell::Cell::new(Some(msg));
        button.connect_clicked(move |_| {
            p.popdown();
            if let Some(m) = msg.take() {
                let _ = s.send(m);
            }
        });
        list.append(&button);
    }
    let more = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(t("More Actions"))
        .popover(&popover)
        .build();
    more.add_css_class("flat");
    more
}

/// What removing a tried app freed.
fn freed_text(name: &str, bytes: u64) -> String {
    if bytes == 0 {
        t("{} removed. Nothing to free: your system still uses it.").replace("{}", name)
    } else {
        t("{name} removed, {size} freed.")
            .replace("{name}", name)
            .replace("{size}", &gtk::glib::format_size(bytes))
    }
}

/// Starts the app of a desktop file.
fn open_button(file: PathBuf) -> gtk::Button {
    let open = gtk::Button::with_mnemonic(&t("_Open"));
    open.set_valign(gtk::Align::Center);
    open.connect_clicked(move |_| {
        use gtk::gio::prelude::AppInfoExt;
        let context = gtk::gdk::Display::default().map(|d| d.app_launch_context());
        if let Some(app) = gtk::gio::DesktopAppInfo::from_filename(&file) {
            let _ = app.launch(&[], context.as_ref());
        }
    });
    open
}

fn display_name(p: &Package) -> String {
    if p.name.is_empty() {
        p.attr.clone()
    } else {
        p.name.clone()
    }
}

/// Flathub icon, else the package's own, else a family badge.
fn item_icon(item: &Item) -> gtk::Widget {
    if let Some(tex) = item
        .icon
        .as_deref()
        .and_then(|b| util::texture_from_bytes_sized(b, 64))
    {
        let img = gtk::Image::from_paintable(Some(&tex));
        img.set_pixel_size(32);
        return img.upcast();
    }
    let local = item.desktop_file.as_deref().and_then(|f| {
        use gtk::gio::prelude::AppInfoExt;
        if let Some(display) = gtk::gdk::Display::default() {
            if let Some(share) = f.parent().and_then(Path::parent) {
                gtk::IconTheme::for_display(&display).add_search_path(share.join("icons"));
            }
        }
        gtk::gio::DesktopAppInfo::from_filename(f)?.icon()
    });
    match local {
        Some(icon) => util::row_icon(Some(&icon)).upcast(),
        None => util::fallback_badge(&item.package, 32),
    }
}

/// A titled list, or `empty` without rows.
fn section(
    title: &str,
    subtitle: Option<&str>,
    rows: Vec<gtk::Widget>,
    empty: &str,
    header_suffix: Option<gtk::Button>,
) -> gtk::Box {
    let sec = if rows.is_empty() {
        util::section(title, subtitle, &util::dim_label(empty))
    } else {
        let list = util::boxed_list();
        for r in rows {
            list.append(&r);
        }
        util::section(title, subtitle, &list)
    };
    if let (Some(b), Some(head)) = (header_suffix, sec.first_child().and_downcast::<gtk::Box>()) {
        b.add_css_class("flat");
        head.append(&b);
    }
    sec
}

/// The manifest, history and profiles, with icons. Blocking.
fn load_data() -> Data {
    let tried = nix_app_center_core::load_tried().tried;
    let kept: Vec<(Package, Option<u64>, Vec<String>)> = nix_app_center_core::load_manifest()
        .packages()
        .into_iter()
        .map(|p| (p, None, Vec::new()))
        .collect();
    let tried: Vec<(Package, Option<u64>, Vec<String>)> = tried
        .into_iter()
        .map(|e| (e.package, Some(e.last_tried), e.out_paths))
        .collect();
    let kept = with_icons(kept);
    let tried = with_icons(tried);
    let profile = nix_app_center_core::profile_apps();
    let paths: Vec<PathBuf> = profile
        .iter()
        .map(|a| a.store_path.clone())
        .chain(tried.iter().flat_map(|i| i.paths.clone()))
        .collect();
    Data {
        sizes: nix_app_center_core::store_sizes(&paths),
        elements: nix_app_center_core::profile_elements(),
        index: nix_app_center_core::installed_index(),
        pending: nix_app_center_core::apply_pending(),
        kept,
        tried,
        profile,
    }
}

/// Resolves icons, `MAX_PAR` threads at a time.
fn with_icons(entries: Vec<(Package, Option<u64>, Vec<String>)>) -> Vec<Item> {
    const MAX_PAR: usize = 16;
    let mut out = Vec::with_capacity(entries.len());
    for chunk in entries.chunks(MAX_PAR) {
        let handles: Vec<_> = chunk
            .iter()
            .cloned()
            .map(|(package, tried_at, out_paths)| {
                std::thread::spawn(move || {
                    let meta = nix_app_center_core::cached_app_meta(&package, 1280);
                    Item {
                        summary: meta.summary.clone(),
                        icon: meta
                            .icon_url
                            .as_deref()
                            .and_then(nix_app_center_core::cached_fetch_bytes),
                        desktop_file: out_paths.iter().find_map(|p| first_desktop(Path::new(p))),
                        paths: out_paths.iter().map(PathBuf::from).collect(),
                        package,
                        tried_at,
                    }
                })
            })
            .collect();
        out.extend(handles.into_iter().filter_map(|h| h.join().ok()));
    }
    out
}

fn first_desktop(out: &Path) -> Option<PathBuf> {
    std::fs::read_dir(out.join("share/applications"))
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "desktop"))
}
