//! Storage tab: a pie chart of the store by category, the generations, the
//! apps by size, home folders, and the clean-ups. Refreshed at most once a
//! minute.

use std::time::{Duration, Instant};

use adw::prelude::*;
use nix_app_center_core::{AppOrigin, Category, GenProfile, HomeDir, HomeKind, StorageReport};
use relm4::prelude::*;

use crate::i18n::t;
use crate::util::{self, esc};

const RELOAD_AFTER: Duration = Duration::from_secs(60);

pub struct StorageModel {
    report: Option<Result<StorageReport, String>>,
    /// Home folders, measured separately.
    home: Option<Vec<HomeDir>>,
    /// Outcome of the last cache clean-up.
    status: Option<String>,
    loaded_at: Option<Instant>,
    loading: bool,
    content: gtk::Box,
    home_slot: gtk::Box,
}

#[derive(Debug)]
pub enum StorageMsg {
    /// Tab entered.
    Reload,
    Refresh,
    Loaded(Result<StorageReport, String>),
    /// Runs a clean-up command in the build log dialog.
    Clean(Vec<String>),
    Cleaned,
    HomeLoaded(Vec<HomeDir>),
    /// Deletes this cache folder, once confirmed.
    ClearCache(HomeDir),
    CacheCleared(Result<(), String>),
}

#[relm4::component(pub)]
impl SimpleComponent for StorageModel {
    type Init = ();
    type Input = StorageMsg;
    type Output = ();

    view! {
        #[root]
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

    fn init(
        _: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .build();
        let model = StorageModel {
            report: None,
            home: None,
            status: None,
            loaded_at: None,
            loading: false,
            content: content.clone(),
            home_slot: gtk::Box::new(gtk::Orientation::Vertical, 0),
        };
        let widgets = view_output!();
        widgets.clamp.set_child(Some(&content));
        model.rebuild(&sender);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            StorageMsg::Reload => {
                if self.loaded_at.is_none_or(|at| at.elapsed() > RELOAD_AFTER) {
                    sender.input(StorageMsg::Refresh);
                }
            }
            StorageMsg::Refresh => {
                if self.loading {
                    return;
                }
                self.loading = true;
                self.rebuild(&sender);
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let res = nix_app_center_core::storage_report().map_err(|e| e.to_string());
                    let _ = s.send(StorageMsg::Loaded(res));
                });
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let _ = s.send(StorageMsg::HomeLoaded(nix_app_center_core::home_usage()));
                });
            }
            StorageMsg::HomeLoaded(dirs) => {
                self.home = Some(dirs);
                self.fill_home(&sender);
            }
            StorageMsg::ClearCache(dir) => {
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let res = nix_app_center_core::clear_cache(&dir).map_err(|e| e.to_string());
                    let _ = s.send(StorageMsg::CacheCleared(res));
                });
            }
            StorageMsg::CacheCleared(res) => {
                self.status = Some(match res {
                    Ok(()) => t("Cache cleared."),
                    Err(e) => crate::util::failed(&e),
                });
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let _ = s.send(StorageMsg::HomeLoaded(nix_app_center_core::home_usage()));
                });
            }
            StorageMsg::Loaded(res) => {
                self.loading = false;
                self.loaded_at = Some(Instant::now());
                if self.report.is_some() {
                    self.content.add_css_class("settled");
                }
                self.report = Some(res);
                self.rebuild(&sender);
            }
            StorageMsg::Clean(argv) => {
                let s = sender.input_sender().clone();
                crate::components::apply_dialog::present(
                    &self.content,
                    &t("Cleaning Up"),
                    vec![argv],
                    move |_| {
                        let _ = s.send(StorageMsg::Cleaned);
                    },
                );
            }
            StorageMsg::Cleaned => {
                self.loaded_at = None;
                sender.input(StorageMsg::Refresh);
            }
        }
    }
}

/// Color, title and explanation of each category.
fn category_info(c: Category) -> (&'static str, String, String) {
    match c {
        Category::System => (
            "#3584e4",
            t("System"),
            t("What your system configuration installs"),
        ),
        Category::User => (
            "#2ec27e",
            t("User"),
            t("Your user packages and Home Manager"),
        ),
        Category::Running => (
            "#ff7800",
            t("In Use"),
            t("Kept by programs running right now"),
        ),
        Category::OldGenerations => (
            "#9141ac",
            t("Old Generations"),
            t("Kept so you can roll back"),
        ),
        Category::Development => (
            "#f6d32d",
            t("Development"),
            t("Dev shells, build results, and build inputs Nix keeps"),
        ),
        Category::TriedApps => (
            "#33c7de",
            t("Tried Apps"),
            t("Apps you ran with Nix App Center"),
        ),
        Category::Garbage => ("#9a9996", t("Garbage"), t("Nothing uses it anymore")),
    }
}

/// Loading placeholder shaped like the page.
fn storage_skeleton() -> gtk::Box {
    let bar = |w: i32, h: i32| {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        b.set_size_request(w, h);
        b.set_halign(gtk::Align::Start);
        b.add_css_class("skeleton");
        b
    };
    let page = gtk::Box::new(gtk::Orientation::Vertical, 24);

    let head = gtk::Box::new(gtk::Orientation::Vertical, 8);
    head.append(&bar(140, 24));
    head.append(&bar(460, 14));
    page.append(&head);

    let disc = bar(200, 200);
    disc.add_css_class("round");
    disc.set_valign(gtk::Align::Center);
    let legend = gtk::Box::new(gtk::Orientation::Vertical, 14);
    legend.set_hexpand(true);
    legend.set_valign(gtk::Align::Center);
    for w in [220, 180, 200, 240, 160, 190] {
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let dot = bar(14, 14);
        dot.add_css_class("round");
        line.append(&dot);
        line.append(&bar(w, 14));
        legend.append(&line);
    }
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    inner.set_margin_top(18);
    inner.set_margin_bottom(18);
    inner.set_margin_start(18);
    inner.set_margin_end(18);
    inner.append(&disc);
    inner.append(&legend);
    let card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    card.add_css_class("card");
    card.append(&inner);
    page.append(&card);

    for rows in [2, 1] {
        let sec = gtk::Box::new(gtk::Orientation::Vertical, 12);
        sec.append(&bar(180, 24));
        sec.append(&util::skeleton_list(rows));
        page.append(&sec);
    }
    page
}

fn rgb(hex: &str) -> (f64, f64, f64) {
    let c = |i: usize| f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0)) / 255.0;
    (c(1), c(3), c(5))
}

fn size(bytes: u64) -> String {
    gtk::glib::format_size(bytes).to_string()
}

impl StorageModel {
    fn rebuild(&self, sender: &ComponentSender<Self>) {
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
        let report = match (&self.report, self.loading) {
            (_, true) | (None, _) => {
                self.content.append(&storage_skeleton());
                return;
            }
            (Some(Err(e)), _) => {
                self.content.append(&util::dim_label(&util::failed(e)));
                return;
            }
            (Some(Ok(r)), _) => r,
        };
        if let Some(status) = &self.status {
            self.content.append(&util::dim_label(status));
        }
        let sections = [
            overview(report, sender),
            self.cleanups(report, sender),
            generations(report),
            apps(report),
        ];
        for (i, sec) in sections.iter().enumerate() {
            util::appear(sec, i * 2);
            self.content.append(sec);
        }
        self.content.append(&self.home_slot);
        self.fill_home(sender);
    }

    /// Fills the home section alone, so the rest keeps its animations.
    fn fill_home(&self, sender: &ComponentSender<Self>) {
        while let Some(child) = self.home_slot.first_child() {
            self.home_slot.remove(&child);
        }
        let (Some(dirs), Some(Ok(report))) = (&self.home, &self.report) else {
            return;
        };
        let sec = home_files(dirs, report, sender);
        util::appear(&sec, 0);
        self.home_slot.append(&sec);
    }

    fn cleanups(&self, r: &StorageReport, sender: &ComponentSender<Self>) -> gtk::Box {
        let of = |c: Category| {
            r.categories
                .iter()
                .find(|(k, _)| *k == c)
                .map_or(0, |x| x.1)
        };
        let list = util::boxed_list();
        let garbage = adw::ActionRow::builder()
            .title(t("Empty Garbage"))
            .subtitle(esc(&t("{} that nothing uses anymore. Safe to delete.")
                .replace(
                    "{}",
                    &size(of(Category::Garbage) + of(Category::TriedApps)),
                )))
            .build();
        garbage.set_tooltip_text(Some(&t(
            "Runs the Nix garbage collector: deletes store paths nothing uses.",
        )));
        garbage.add_suffix(&button(&t("_Clean"), false, {
            let s = sender.input_sender().clone();
            move || {
                let _ = s.send(StorageMsg::Clean(vec!["nix-store".into(), "--gc".into()]));
            }
        }));
        list.append(&garbage);

        let old = adw::ActionRow::builder()
            .title(t("Delete Old Generations"))
            .subtitle(esc(&t("About {}, and you can no longer roll back to them. The boot menu follows at the next rebuild.").replace("{}", &size(of(Category::OldGenerations)))))
            .build();
        old.set_tooltip_text(Some(&t(
            "Deletes the old generations: you can no longer boot or roll back to them.",
        )));
        old.add_suffix(&button(&t("_Delete…"), true, {
            let s = sender.input_sender().clone();
            let parent = self.content.clone();
            move || confirm_delete_generations(&parent, s.clone())
        }));
        list.append(&old);
        util::section(&t("Free Up Space"), None, &list)
    }
}

/// Totals, the pie chart and its legend.
fn overview(r: &StorageReport, sender: &ComponentSender<StorageModel>) -> gtk::Box {
    let slices: Vec<(Category, u64)> = r
        .categories
        .iter()
        .copied()
        .filter(|(_, s)| *s > 0)
        .collect();
    let total: u64 = slices.iter().map(|s| s.1).sum();

    let pie = gtk::DrawingArea::new();
    pie.set_content_width(200);
    pie.set_content_height(200);
    pie.set_valign(gtk::Align::Center);
    let parts = slices.clone();
    // Share of the circle drawn, animated once.
    let progress = std::rc::Rc::new(std::cell::Cell::new(0.0_f64));
    let drawn = progress.clone();
    pie.set_draw_func(move |_, cr, w, h| {
        let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
        let radius = cx.min(cy) - 2.0;
        let mut angle = -std::f64::consts::FRAC_PI_2;
        for (cat, bytes) in &parts {
            let sweep = *bytes as f64 / total.max(1) as f64 * std::f64::consts::TAU * drawn.get();
            let (r, g, b) = rgb(category_info(*cat).0);
            cr.set_source_rgb(r, g, b);
            cr.move_to(cx, cy);
            cr.arc(cx, cy, radius, angle, angle + sweep);
            cr.close_path();
            let _ = cr.fill();
            angle += sweep;
        }
        cr.set_operator(gtk::cairo::Operator::Clear);
        cr.arc(cx, cy, radius * 0.55, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    });

    let target = adw::CallbackAnimationTarget::new({
        let (pie, progress) = (pie.clone(), progress.clone());
        move |v| {
            progress.set(v);
            pie.queue_draw();
        }
    });
    let sweep = adw::TimedAnimation::new(&pie, 0.0, 1.0, 900, target);
    sweep.set_easing(adw::Easing::EaseOutCubic);
    // Unmapped widgets skip animations.
    pie.connect_map(move |_| sweep.play());

    let legend = gtk::Box::new(gtk::Orientation::Vertical, 8);
    legend.set_hexpand(true);
    legend.set_valign(gtk::Align::Center);
    for (i, (cat, bytes)) in slices.iter().enumerate() {
        let (color, title, desc) = category_info(*cat);
        let swatch = gtk::DrawingArea::new();
        swatch.set_content_width(14);
        swatch.set_content_height(14);
        swatch.set_valign(gtk::Align::Center);
        let (r, g, b) = rgb(color);
        swatch.set_draw_func(move |_, cr, w, h| {
            cr.set_source_rgb(r, g, b);
            cr.arc(
                f64::from(w) / 2.0,
                f64::from(h) / 2.0,
                f64::from(w.min(h)) / 2.0,
                0.0,
                std::f64::consts::TAU,
            );
            let _ = cr.fill();
        });
        let name = gtk::Label::new(Some(&title));
        name.add_css_class("heading");
        name.set_xalign(0.0);
        let amount = gtk::Label::new(Some(&format!(
            "{} · {:.0} %",
            size(*bytes),
            *bytes as f64 * 100.0 / total.max(1) as f64
        )));
        amount.add_css_class("dim-label");
        amount.add_css_class("numeric");
        amount.set_hexpand(true);
        amount.set_xalign(1.0);
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        top.append(&swatch);
        top.append(&name);
        top.append(&amount);
        let d = gtk::Label::new(Some(&desc));
        d.add_css_class("caption");
        d.add_css_class("dim-label");
        d.set_xalign(0.0);
        d.set_margin_start(22);
        let entry = gtk::Box::new(gtk::Orientation::Vertical, 0);
        entry.append(&top);
        entry.append(&d);
        util::appear(&entry, i + 2);
        legend.append(&entry);
    }

    let chart = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    chart.add_css_class("card");
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    inner.set_margin_top(18);
    inner.set_margin_bottom(18);
    inner.set_margin_start(18);
    inner.set_margin_end(18);
    inner.append(&pie);
    inner.append(&legend);
    chart.append(&inner);
    inner.set_hexpand(true);

    let summary = t("{store} in {paths} store paths, before hard link sharing. The disk has {free} free of {disk}.")
        .replace("{store}", &size(r.store_size))
        .replace("{paths}", &r.store_paths.to_string())
        .replace("{free}", &size(r.disk_free))
        .replace("{disk}", &size(r.disk_size));
    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.add_css_class("flat");
    refresh.set_tooltip_text(Some(&t("Measure Again")));
    refresh.connect_clicked({
        let s = sender.input_sender().clone();
        move |_| {
            let _ = s.send(StorageMsg::Refresh);
        }
    });
    let sec = util::section(&t("Nix Store"), Some(&summary), &chart);
    if let Some(head) = sec.first_child().and_downcast::<gtk::Box>() {
        head.append(&refresh);
    }
    sec
}

fn generations(r: &StorageReport) -> gtk::Box {
    let list = util::boxed_list();
    for (profile, title) in [
        (GenProfile::System, t("System")),
        (GenProfile::HomeManager, t("Home Manager")),
        (GenProfile::User, t("User Profile")),
    ] {
        let gens: Vec<_> = r
            .generations
            .iter()
            .filter(|g| g.profile == profile)
            .collect();
        if gens.is_empty() {
            continue;
        }
        let old: u64 = gens.iter().filter(|g| !g.current && !g.booted).count() as u64;
        let exp = adw::ExpanderRow::builder()
            .title(esc(&title))
            .subtitle(esc(&format!(
                "{} · {}",
                util::plural("{} generation", "{} generations", gens.len() as i64),
                util::plural("{} old", "{} old", old as i64)
            )))
            .build();
        for g in gens {
            let mut tags = Vec::new();
            if g.current {
                tags.push(t("current"));
            }
            if g.same_as_current {
                tags.push(t("same as current"));
            }
            if g.booted {
                tags.push(t("booted"));
            }
            let title = if tags.is_empty() {
                t("Generation {}").replace("{}", &g.number.to_string())
            } else {
                format!(
                    "{} · {}",
                    t("Generation {}").replace("{}", &g.number.to_string()),
                    tags.join(", ")
                )
            };
            let mut sub = vec![util::format_relative(g.date as i64)];
            if let Some(v) = &g.version {
                sub.push(format!("NixOS {v}"));
            }
            if !g.current && !g.booted && !g.same_as_current {
                sub.push(t("frees {}").replace("{}", &size(g.exclusive)));
            }
            let row = adw::ActionRow::builder()
                .title(esc(&title))
                .subtitle(esc(&sub.join(" · ")))
                .build();
            exp.add_row(&row);
        }
        list.append(&exp);
    }
    util::section(
        &t("Generations"),
        Some(&t("Each rebuild keeps the previous state so you can roll back. Old ones share most of their files: deleting one frees only what it alone keeps.")),
        &list,
    )
}

fn apps(r: &StorageReport) -> gtk::Box {
    let list = util::boxed_list();
    for (origins, title) in [
        (vec![AppOrigin::System], t("System Apps")),
        (vec![AppOrigin::User], t("User Apps")),
        (
            vec![AppOrigin::Kept, AppOrigin::Tried],
            t("Nix App Center Apps"),
        ),
    ] {
        let apps: Vec<_> = r
            .apps
            .iter()
            .filter(|a| origins.contains(&a.origin))
            .collect();
        if apps.is_empty() {
            continue;
        }
        let exp = adw::ExpanderRow::builder()
            .title(esc(&title))
            .subtitle(esc(&util::plural("{} app", "{} apps", apps.len() as i64)))
            .build();
        for a in apps {
            let own = if a.own > 0 && a.own < a.closure {
                format!(" · {}", t("{} itself").replace("{}", &size(a.own)))
            } else {
                String::new()
            };
            let detail = t("{} with its dependencies").replace("{}", &size(a.closure))
                + &own
                + &match a.origin {
                    AppOrigin::Kept => format!(" · {}", t("kept by Nix App Center")),
                    AppOrigin::Tried => format!(" · {}", t("tried")),
                    _ => String::new(),
                };
            let row = a
                .desktop_file
                .as_deref()
                .and_then(|f| util::desktop_row(f, &detail))
                .unwrap_or_else(|| {
                    adw::ActionRow::builder()
                        .title(esc(&a.name))
                        .subtitle(esc(&detail))
                        .build()
                });
            exp.add_row(&row);
        }
        list.append(&exp);
    }
    util::section(
        &t("Apps"),
        Some(&t("Sizes include every dependency, so apps sharing libraries add up to more than the store.")),
        &list,
    )
}

/// The biggest home folders; caches can be cleared.
fn home_files(
    dirs: &[HomeDir],
    report: &StorageReport,
    sender: &ComponentSender<StorageModel>,
) -> gtk::Box {
    let list = util::boxed_list();
    let total: u64 = dirs.iter().map(|d| d.size).sum();
    for d in dirs.iter().take(15) {
        let kind = match d.kind {
            HomeKind::Cache => t("Cache"),
            HomeKind::Data => t("Data"),
            HomeKind::Config => t("Settings"),
        };
        let owner = report
            .apps
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(&d.name))
            .map(|a| format!(" · {}", t("used by {}").replace("{}", &a.name)))
            .unwrap_or_default();
        let row = adw::ActionRow::builder()
            .title(esc(&d.name))
            .subtitle(esc(&format!("{kind} · {}{owner}", size(d.size))))
            .tooltip_text(d.path.display().to_string())
            .build();
        if d.kind == HomeKind::Cache {
            let dir = d.clone();
            let s = sender.input_sender().clone();
            let parent = list.clone();
            row.add_suffix(&button(&t("Clear…"), false, move || {
                confirm_clear(&parent, dir.clone(), s.clone())
            }));
        }
        list.append(&row);
    }
    util::section(
        &t("Your Files"),
        Some(&t("Folders apps keep in your home: {} in all. A cache is rebuilt when needed; data and settings are yours, Nix App Center never deletes them.").replace("{}", &size(total))),
        &list,
    )
}

fn confirm_clear(parent: &gtk::ListBox, dir: HomeDir, s: relm4::Sender<StorageMsg>) {
    let dialog = adw::AlertDialog::new(
        Some(&t("Clear the “{}” Cache?").replace("{}", &dir.name)),
        Some(
            &t("This frees {}. The app rebuilds what it needs; close it first if it is running.")
                .replace("{}", &size(dir.size)),
        ),
    );
    dialog.add_response("cancel", &t("_Cancel"));
    dialog.add_response("clear", &t("C_lear"));
    dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
    dialog.connect_response(None, move |_, r| {
        if r == "clear" {
            let _ = s.send(StorageMsg::ClearCache(dir.clone()));
        }
    });
    dialog.present(Some(parent));
}

fn confirm_delete_generations(parent: &gtk::Box, s: relm4::Sender<StorageMsg>) {
    let dialog = adw::AlertDialog::new(
        Some(&t("Delete Old Generations?")),
        Some(&t("You will not be able to roll back to them. The current system and your current user setup stay.")),
    );
    dialog.add_response("cancel", &t("_Cancel"));
    dialog.add_response("delete", &t("_Delete"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.connect_response(None, move |_, response| {
        if response == "delete" {
            // The system profile is root's.
            let _ = s.send(StorageMsg::Clean(vec![
                "sudo".into(),
                "nix-collect-garbage".into(),
                "--delete-old".into(),
            ]));
        }
    });
    dialog.present(Some(parent));
}

fn button(label: &str, destructive: bool, on_click: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::with_mnemonic(label);
    b.set_valign(gtk::Align::Center);
    if destructive {
        b.add_css_class("destructive-action");
    }
    b.connect_clicked(move |_| on_click());
    crate::components::apply_dialog::guard(&b);
    b
}
