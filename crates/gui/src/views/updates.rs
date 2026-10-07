//! "Updates" tab. Every package follows the nixpkgs revision the config pins,
//! so an update is that pin moving to the head of its branch. The tab shows
//! the current pin, the branch head, the installed apps that change, and runs
//! the update (move the pin, then apply) when Onyx knows how.
//!
//! The check downloads nixpkgs once per hour at most: it only runs when the
//! tab is entered and the last check is older than ten minutes.

use std::time::{Duration, Instant};

use adw::prelude::*;
use onyx_core::{AppUpdate, Latest, Source, UpdateMethod};
use relm4::prelude::*;

use crate::i18n::t;
use crate::util::{self, esc};

const RECHECK_AFTER: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct Check {
    source: Source,
    latest: Option<Latest>,
    apps: Vec<AppUpdate>,
    command: Option<Vec<String>>,
    new_release: Option<String>,
}

impl Check {
    /// Number shown on the tab badge: 0 when up to date.
    fn pending(&self) -> usize {
        match &self.latest {
            Some(l) if self.source.rev.as_deref() != Some(l.rev.as_str()) => self.apps.len().max(1),
            _ => 0,
        }
    }
}

#[derive(Debug)]
pub enum UpdatesOutput {
    /// Updates pending after a check; `notify` for background checks.
    Pending { count: usize, notify: bool },
}

pub struct UpdatesModel {
    check: Option<Check>,
    checked_at: Option<Instant>,
    checking: bool,
    background: bool,
    updating: bool,
    status: Option<String>,
    content: gtk::Box,
}

#[derive(Debug)]
pub enum UpdatesMsg {
    /// Tab entered: checks again when the last check is old.
    Reload,
    /// "Check Again".
    Refresh,
    /// Daily check started by the app, notifies when updates are pending.
    Background,
    Checked(Check),
    Update,
    Updated(Result<onyx_core::ApplyOutcome, String>),
}

#[relm4::component(pub)]
impl SimpleComponent for UpdatesModel {
    type Init = ();
    type Input = UpdatesMsg;
    type Output = UpdatesOutput;

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
            .spacing(18)
            .vexpand(true)
            .build();
        let model = UpdatesModel {
            check: None,
            checked_at: None,
            checking: false,
            background: false,
            updating: false,
            status: None,
            content: content.clone(),
        };
        let widgets = view_output!();
        widgets.clamp.set_child(Some(&content));
        model.rebuild(&sender);
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            UpdatesMsg::Reload => {
                if self
                    .checked_at
                    .is_none_or(|at| at.elapsed() > RECHECK_AFTER)
                {
                    sender.input(UpdatesMsg::Refresh);
                }
            }
            UpdatesMsg::Refresh => {
                if self.checking {
                    return;
                }
                self.checking = true;
                self.rebuild(&sender);
                let s = sender.input_sender().clone();
                std::thread::spawn(move || {
                    let _ = s.send(UpdatesMsg::Checked(check()));
                });
            }
            UpdatesMsg::Background => {
                self.background = true;
                sender.input(UpdatesMsg::Refresh);
            }
            UpdatesMsg::Checked(c) => {
                self.checking = false;
                self.checked_at = Some(Instant::now());
                let notify = std::mem::take(&mut self.background);
                let _ = sender.output(UpdatesOutput::Pending {
                    count: c.pending(),
                    notify,
                });
                self.check = Some(c);
                self.rebuild(&sender);
            }
            UpdatesMsg::Update => {
                let Some(argv) = self.check.as_ref().and_then(|c| c.command.clone()) else {
                    return;
                };
                if self.updating {
                    return;
                }
                self.updating = true;
                self.rebuild(&sender);
                let s = sender.input_sender().clone();
                crate::components::apply_dialog::present(
                    &self.content,
                    &t("Updating"),
                    argv,
                    move |res| {
                        let _ = s.send(UpdatesMsg::Updated(res));
                    },
                );
            }
            UpdatesMsg::Updated(res) => {
                self.updating = false;
                let ok = res.is_ok();
                self.status = Some(util::describe_apply(res));
                self.checked_at = None;
                self.rebuild(&sender);
                if ok {
                    sender.input(UpdatesMsg::Refresh);
                }
            }
        }
    }
}

/// Blocking: reads the pin, asks for the branch head, compares the apps.
fn check() -> Check {
    let source = onyx_core::current_source();
    let latest = source.branch.as_deref().and_then(onyx_core::latest);
    let apps = match &latest {
        Some(l) if source.rev.as_deref() != Some(l.rev.as_str()) => {
            onyx_core::app_updates(&l.rev, &onyx_core::profile_apps())
        }
        _ => Vec::new(),
    };
    let command = onyx_core::update_command(&source, &onyx_core::detect_layout());
    let new_release = source.branch.as_deref().and_then(onyx_core::newer_release);
    Check {
        source,
        latest,
        apps,
        command,
        new_release,
    }
}

fn short(rev: &str) -> &str {
    rev.get(..7).unwrap_or(rev)
}

fn revision_line(rev: Option<&str>, ts: Option<u64>) -> String {
    match (rev, ts) {
        (Some(r), Some(ts)) => t("Revision {rev}, {age}")
            .replace("{rev}", short(r))
            .replace("{age}", &util::format_relative(ts as i64).to_lowercase()),
        (Some(r), None) => t("Revision {rev}").replace("{rev}", short(r)),
        _ => t("Revision unknown"),
    }
}

impl UpdatesModel {
    fn rebuild(&self, sender: &ComponentSender<Self>) {
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
        if let Some(status) = &self.status {
            self.content.append(&util::dim_label(status));
        }
        let Some(c) = self.check.as_ref().filter(|_| !self.checking) else {
            self.content
                .append(&util::dim_label(&t("Checking for updates…")));
            self.content.append(&util::skeleton_list(3));
            return;
        };

        if let Some(release) = &c.new_release {
            self.content.append(&release_banner(release));
        }
        self.content.append(&self.source_list(c, sender));

        let Some(latest) = &c.latest else {
            let text = if c.source.branch.is_none() {
                t("Nix Software cannot tell which nixpkgs branch your configuration follows.")
            } else {
                t("Nix Software could not reach nixpkgs to check for updates.")
            };
            self.content.append(&util::dim_label(&text));
            return;
        };
        if c.source.rev.as_deref() == Some(latest.rev.as_str()) {
            self.content.append(&up_to_date_view());
            return;
        }

        self.content.append(&self.header(c, latest, sender));
        if let Some(note) = method_note(&c.source.method).filter(|_| c.command.is_none()) {
            self.content.append(&util::dim_label(&note));
        }
        if !c.apps.is_empty() {
            let list = util::boxed_list();
            for u in &c.apps {
                let change = format!("{} → {}", u.app.version, u.new_version);
                let row = util::desktop_row(&u.app.desktop_file, &change).unwrap_or_else(|| {
                    adw::ActionRow::builder()
                        .title(esc(&u.app.pname))
                        .subtitle(esc(&change))
                        .build()
                });
                list.append(&row);
            }
            self.content.append(&list);
        }
    }

    fn source_list(&self, c: &Check, sender: &ComponentSender<Self>) -> gtk::ListBox {
        let row = adw::ActionRow::builder()
            .title(esc(&match &c.source.branch {
                Some(b) => t("nixpkgs {}").replace("{}", b),
                None => "nixpkgs".to_string(),
            }))
            .subtitle(esc(&t("Installed: {}").replace(
                "{}",
                &revision_line(c.source.rev.as_deref(), c.source.last_modified),
            )))
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("nix-snowflake-symbolic"));
        let again = gtk::Button::from_icon_name("view-refresh-symbolic");
        again.set_tooltip_text(Some(&t("Check Again")));
        again.add_css_class("flat");
        again.set_valign(gtk::Align::Center);
        again.connect_clicked({
            let s = sender.input_sender().clone();
            move |_| {
                let _ = s.send(UpdatesMsg::Refresh);
            }
        });
        row.add_suffix(&again);
        let list = util::boxed_list();
        list.append(&row);
        list
    }

    fn header(&self, c: &Check, latest: &Latest, sender: &ComponentSender<Self>) -> gtk::Box {
        let n = c.apps.len() as i64;
        let title = gtk::Label::new(Some(&if n > 0 {
            util::plural("{} app update", "{} app updates", n)
        } else {
            t("New nixpkgs revision")
        }));
        title.add_css_class("title-2");
        title.set_xalign(0.0);
        let sub = util::dim_label(&t("Available: {}").replace(
            "{}",
            &revision_line(Some(&latest.rev), Some(latest.last_modified)),
        ));
        let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
        titles.set_hexpand(true);
        titles.append(&title);
        titles.append(&sub);

        let hb = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        hb.append(&titles);
        if c.command.is_some() {
            let btn = gtk::Button::with_mnemonic(&t("_Update"));
            btn.add_css_class("suggested-action");
            btn.set_valign(gtk::Align::Center);
            btn.set_sensitive(!self.updating);
            btn.set_tooltip_text(c.command.as_ref().map(|a| a.join(" ")).as_deref());
            btn.connect_clicked({
                let s = sender.input_sender().clone();
                move |_| {
                    let _ = s.send(UpdatesMsg::Update);
                }
            });
            hb.append(&btn);
        }
        hb
    }
}

/// Why Onyx cannot run the update itself.
fn method_note(method: &UpdateMethod) -> Option<String> {
    match method {
        UpdateMethod::Pinned { by } => Some(
            t("nixpkgs comes from the “{}” input of your configuration. Update that flake, then apply.")
                .replace("{}", by),
        ),
        UpdateMethod::Unknown => Some(t("Nix Software cannot update this configuration by itself.")),
        _ => None,
    }
}

/// A newer NixOS release: moving to it means editing the branch in the
/// config (flake input or channel), which the user does on purpose.
fn release_banner(release: &str) -> adw::Banner {
    let banner = adw::Banner::new(
        &t("NixOS {0} is available. Switch your configuration to nixos-{0} when you are ready.")
            .replace("{0}", release),
    );
    banner.set_button_label(Some(&t("Release _Notes")));
    banner.set_revealed(true);
    banner.connect_button_clicked(|_| {
        crate::util::open_uri("https://nixos.org/manual/nixos/unstable/release-notes");
    });
    banner
}
/// "Everything is up to date" illustration (borrowed from GNOME Software), centered.
fn up_to_date_view() -> gtk::Box {
    // Embedded: the source tree does not exist once installed.
    const SVG: &[u8] = include_bytes!("../../data/assets/up-to-date.svg");
    let pic = gtk::Picture::new();
    if let Ok(tex) = gtk::gdk::Texture::from_bytes(&gtk::glib::Bytes::from_static(SVG)) {
        pic.set_paintable(Some(&tex));
    }
    pic.set_size_request(256, 192);
    pic.add_css_class("icon-dropshadow");
    let title = gtk::Label::new(Some(&t("Everything is up to date")));
    title.add_css_class("title-1");

    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .spacing(18)
        .build();
    b.append(&pic);
    b.append(&title);
    b
}
