//! "Installed" tab, a list per origin like GNOME Software:
//!
//! • "In Your Configuration": packages Onyx keeps (`kept.json`), removable.
//! • "Recently Tried": `nix run` history, each one removable from the store.
//! • "System" / "User": apps the Nix profiles already hold, read only and
//!   dimmed, drawn from their desktop files with the local icons.
//!
//! Reloaded when the tab is activated, through `InstalledMsg::Reload`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use adw::prelude::*;
use onyx_core::{Package, ProfileApp, Scope};
use relm4::prelude::*;

use crate::i18n::t;
use crate::util::{self, esc};

/// A kept or tried package, with what its row needs.
#[derive(Debug, Clone)]
pub struct Item {
    package: Package,
    icon: Option<Vec<u8>>,
    /// Desktop file shipped by the package itself (tried apps).
    desktop_file: Option<PathBuf>,
    tried_at: Option<u64>,
    /// Store paths the package fetched (tried apps).
    paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct Data {
    kept: Vec<Item>,
    tried: Vec<Item>,
    profile: Vec<ProfileApp>,
    sizes: std::collections::HashMap<PathBuf, u64>,
}

impl Data {
    /// " · 120 MB" for the given paths, empty when unknown.
    fn size_suffix<'a>(&self, paths: impl IntoIterator<Item = &'a PathBuf>) -> String {
        let total: u64 = paths.into_iter().filter_map(|p| self.sizes.get(p)).sum();
        if total == 0 {
            String::new()
        } else {
            format!(" · {}", gtk::glib::format_size(total))
        }
    }
}

pub struct InstalledModel {
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
    Removed(String, Result<u64, String>),
    Apply,
    Applied(Result<onyx_core::ApplyOutcome, String>),
    Gc,
    GcDone(String),
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

        let model = InstalledModel {
            data: None,
            removing: HashSet::new(),
            applying: false,
            gc_running: false,
            status: None,
            banner: banner.clone(),
            content: content.clone(),
        };
        let widgets = view_output!();
        widgets.clamp.set_child(Some(&content));
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
                    // Icons come from AppStream: list again once it is loaded.
                    if !onyx_core::appstream_settled() {
                        onyx_core::wait_appstream(std::time::Duration::from_secs(180));
                        let _ = s.send(InstalledMsg::DataReady(load_data()));
                    }
                });
            }
            InstalledMsg::DataReady(data) => {
                self.data = Some(data);
                self.rebuild(&sender);
            }
            InstalledMsg::Open(pkg) => {
                let _ = sender.output(pkg);
            }
            InstalledMsg::Unkeep(attr) => {
                match onyx_core::unkeep(&attr) {
                    Ok(()) => {
                        if let Some(d) = self.data.as_mut() {
                            d.kept.retain(|i| i.package.attr != attr);
                        }
                        self.status = None;
                        self.banner.set_revealed(true);
                    }
                    Err(e) => self.status = Some(t("Failed: {}").replace("{}", &e.to_string())),
                }
                self.rebuild(&sender);
            }
            InstalledMsg::RemoveTried(attr) => {
                if !self.removing.insert(attr.clone()) {
                    return;
                }
                self.rebuild(&sender);
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let res = onyx_core::remove_tried(&attr).map_err(|e| e.to_string());
                    let _ = s.send(InstalledMsg::Removed(attr, res));
                });
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
                    Ok(0) => t("{} removed. Nothing to free: your system still uses it.")
                        .replace("{}", &name),
                    Ok(bytes) => t("{name} removed, {size} freed.")
                        .replace("{name}", &name)
                        .replace("{size}", &gtk::glib::format_size(bytes)),
                    Err(e) => t("Failed: {}").replace("{}", &e),
                });
                self.rebuild(&sender);
            }
            InstalledMsg::Apply => {
                if self.applying {
                    return;
                }
                self.banner.set_revealed(false);
                let Some(argv) = onyx_core::apply_argv() else {
                    sender.input(InstalledMsg::Applied(Ok(
                        onyx_core::ApplyOutcome::UnknownLayout,
                    )));
                    return;
                };
                self.applying = true;
                let s = sender.input_sender().clone();
                crate::components::apply_dialog::present(
                    &self.content,
                    &t("Applying Configuration"),
                    argv,
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
                self.status = Some(t("Cleaning up the store…"));
                self.rebuild(&sender);
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let msg = match onyx_core::garbage_collect() {
                        Ok(s) if !s.is_empty() => s,
                        Ok(_) => t("Done."),
                        Err(e) => t("Failed: {}").replace("{}", &e.to_string()),
                    };
                    let _ = s.send(InstalledMsg::GcDone(msg));
                });
            }
            InstalledMsg::GcDone(status) => {
                self.gc_running = false;
                self.status = Some(status);
                self.rebuild(&sender);
            }
        }
    }
}

impl InstalledModel {
    /// Clears `content` and fills it again from the current state.
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

        // In your configuration.
        let kept_attrs: HashSet<String> = data
            .kept
            .iter()
            .map(|i| i.package.attr.to_lowercase())
            .collect();
        let rows: Vec<gtk::Widget> = data
            .kept
            .iter()
            .map(|i| {
                let row = self.item_row(i, &i.package.version, sender);
                row.add_suffix(&self.action_button(
                    &t("Remove"),
                    false,
                    InstalledMsg::Unkeep(i.package.attr.clone()),
                    sender,
                ));
                row.upcast()
            })
            .collect();
        self.content.append(&section(
            &t("In Your Configuration"),
            None,
            rows,
            &t("No kept packages yet. Open an app and choose “Add to Configuration”."),
            None,
        ));

        // Recently tried.
        let rows: Vec<gtk::Widget> = data
            .tried
            .iter()
            .map(|i| {
                let when = i
                    .tried_at
                    .map(|ts| util::format_relative(ts as i64))
                    .unwrap_or_default();
                let row = self.item_row(i, &when, sender);
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
                row.upcast()
            })
            .collect();
        let gc = (!data.tried.is_empty()).then(|| {
            let b = self.action_button(
                &t("Free Up Space"),
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

        // Read-only: what the profiles already hold.
        for (scope, title, subtitle) in [
            (
                Scope::User,
                t("User"),
                t("Declared in your user configuration."),
            ),
            (
                Scope::System,
                t("System"),
                t("Declared in your system configuration."),
            ),
        ] {
            let rows: Vec<gtk::Widget> = data
                .profile
                .iter()
                .filter(|a| a.scope == scope && !kept_attrs.contains(&a.pname.to_lowercase()))
                .filter_map(|a| {
                    let sub = a.version.clone() + &data.size_suffix([&a.store_path]);
                    util::desktop_row(&a.desktop_file, &sub)
                })
                .map(|row| {
                    row.add_css_class("dim-label");
                    row.upcast()
                })
                .collect();
            if !rows.is_empty() {
                self.content
                    .append(&section(&title, Some(&subtitle), rows, "", None));
            }
        }
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
        let b = gtk::Button::with_label(label);
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

fn display_name(p: &Package) -> String {
    if p.name.is_empty() {
        p.attr.clone()
    } else {
        p.name.clone()
    }
}

/// Flathub icon, else the icon the package ships, else a family badge.
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
        // The package dir is not in the theme path.
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

/// A titled boxed list, or the `empty` text when there are no rows.
fn section(
    title: &str,
    subtitle: Option<&str>,
    rows: Vec<gtk::Widget>,
    empty: &str,
    header_suffix: Option<gtk::Button>,
) -> gtk::Box {
    let sec = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
    titles.set_hexpand(true);
    let h = gtk::Label::new(Some(title));
    h.add_css_class("title-2");
    h.set_xalign(0.0);
    titles.append(&h);
    if let Some(sub) = subtitle {
        let l = gtk::Label::new(Some(sub));
        l.add_css_class("dim-label");
        l.set_xalign(0.0);
        l.set_wrap(true);
        titles.append(&l);
    }
    head.append(&titles);
    if let Some(b) = header_suffix {
        b.add_css_class("flat");
        head.append(&b);
    }
    sec.append(&head);

    if rows.is_empty() {
        let l = gtk::Label::new(Some(empty));
        l.add_css_class("dim-label");
        l.set_xalign(0.0);
        l.set_wrap(true);
        sec.append(&l);
    } else {
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::None);
        for r in rows {
            list.append(&r);
        }
        sec.append(&list);
    }
    sec
}

/// Reads the manifest, the history and the profiles, with icons. Blocking:
/// call it from a thread.
fn load_data() -> Data {
    let tried = onyx_core::load_tried().tried;
    let kept: Vec<(Package, Option<u64>, Vec<String>)> = onyx_core::load_manifest()
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
    let profile = onyx_core::profile_apps();
    let paths: Vec<PathBuf> = profile
        .iter()
        .map(|a| a.store_path.clone())
        .chain(tried.iter().flat_map(|i| i.paths.clone()))
        .collect();
    Data {
        sizes: onyx_core::store_sizes(&paths),
        kept,
        tried,
        profile,
    }
}

/// Resolves icons with bounded parallelism (waves of `MAX_PAR` threads).
fn with_icons(entries: Vec<(Package, Option<u64>, Vec<String>)>) -> Vec<Item> {
    const MAX_PAR: usize = 16;
    let mut out = Vec::with_capacity(entries.len());
    for chunk in entries.chunks(MAX_PAR) {
        let handles: Vec<_> = chunk
            .iter()
            .cloned()
            .map(|(package, tried_at, out_paths)| {
                std::thread::spawn(move || {
                    let meta = onyx_core::cached_app_meta(&package, 1280);
                    Item {
                        icon: meta
                            .icon_url
                            .as_deref()
                            .and_then(onyx_core::cached_fetch_bytes),
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
