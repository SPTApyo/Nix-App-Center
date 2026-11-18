//! System Settings: the `my.*` options of the configuration, read from Nix
//! each time, written to Nix App Center's own settings file, then applied.
//! One sidebar entry per option group; what another file sets is listed
//! apart, since changing it here would only fail the rebuild.

use adw::prelude::*;
use nix_app_center_core::{LayoutKind, Setting, SettingKind, SettingValue};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::components::sidebar::{self, Section};
use crate::i18n::t;
use crate::util::esc;

type Values = Rc<RefCell<BTreeMap<String, SettingValue>>>;
type Setter = Rc<dyn Fn(Option<SettingValue>)>;
/// A row, and how to show a value in it.
type Editor = (adw::ActionRow, Box<dyn Fn(&SettingValue)>);
type RowSlot = Rc<RefCell<Option<adw::ActionRow>>>;

/// Enums with this many choices or fewer show as buttons.
const CHOICE_BUTTONS: usize = 6;

/// What the dialog shares between loads.
#[derive(Clone)]
struct Page {
    overlay: adw::ToastOverlay,
    bar: gtk::ActionBar,
    bar_title: gtk::Label,
    bar_detail: gtk::Label,
    /// The settings file as the page would write it.
    values: Values,
    /// The settings file as Nix read it.
    saved: Values,
    settings: Rc<RefCell<Vec<Setting>>>,
    /// The changed-dot of each sidebar entry, by section.
    dots: Rc<RefCell<Vec<(String, gtk::Widget)>>>,
}

impl Page {
    /// Names whose value differs from the file.
    fn pending(&self) -> Vec<String> {
        let (values, saved) = (self.values.borrow(), self.saved.borrow());
        let mut names: Vec<String> = values
            .keys()
            .chain(saved.keys())
            .filter(|k| values.get(*k) != saved.get(*k))
            .cloned()
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn refresh(&self) {
        let pending = self.pending();
        let settings = self.settings.borrow();
        let titles: Vec<String> = pending
            .iter()
            .filter_map(|n| settings.iter().find(|s| s.name == *n))
            .map(|s| title_and_body(s).0)
            .collect();
        self.bar.set_revealed(!pending.is_empty());
        self.bar_title.set_text(&crate::util::plural(
            "{} pending change",
            "{} pending changes",
            pending.len() as i64,
        ));
        self.bar_detail.set_text(&titles.join(" · "));
        let root = root_of(&settings);
        for (section, dot) in self.dots.borrow().iter() {
            dot.set_visible(pending.iter().any(|n| section_of(n, &root) == *section));
        }
    }
}

pub fn present(parent: &impl IsA<gtk::Widget>) {
    let dialog = adw::Dialog::builder()
        .title(t("System Settings"))
        .content_width(1040)
        .content_height(760)
        .build();
    let bar_title = gtk::Label::new(None);
    bar_title.add_css_class("heading");
    bar_title.set_xalign(0.0);
    let bar_detail = crate::util::dim_label("");
    bar_detail.set_wrap(false);
    bar_detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
    texts.append(&bar_title);
    texts.append(&bar_detail);
    texts.set_margin_start(6);
    let discard = gtk::Button::with_mnemonic(&t("_Discard"));
    let apply = gtk::Button::with_mnemonic(&t("_Apply and Rebuild"));
    apply.add_css_class("suggested-action");
    apply.set_tooltip_text(Some(&t(
        "Write the changed settings to nix-app-center-settings.nix and rebuild",
    )));
    let bar = gtk::ActionBar::new();
    bar.pack_start(&texts);
    bar.pack_end(&apply);
    bar.pack_end(&discard);
    bar.set_revealed(false);
    let page = Page {
        overlay: adw::ToastOverlay::new(),
        bar: bar.clone(),
        bar_title,
        bar_detail,
        values: Rc::default(),
        saved: Rc::default(),
        settings: Rc::default(),
        dots: Rc::default(),
    };
    apply.connect_clicked({
        let page = page.clone();
        move |_| save_and_apply(&page)
    });
    discard.connect_clicked({
        let page = page.clone();
        move |_| {
            let settings = page.settings.borrow().clone();
            show(&page, &settings);
        }
    });
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&page.overlay));
    toolbar.add_bottom_bar(&bar);
    dialog.set_child(Some(&toolbar));
    load(&page);
    dialog.present(Some(parent));
}

/// Reads the settings from Nix and shows them.
fn load(page: &Page) {
    page.bar.set_revealed(false);
    let spinner = adw::StatusPage::builder()
        .title(t("Reading Your Configuration…"))
        .paintable(&adw::SpinnerPaintable::new(Some(&page.overlay)))
        .build();
    page.overlay.set_child(Some(&spinner));
    let page = page.clone();
    gtk::glib::spawn_future_local(async move {
        let res = gtk::gio::spawn_blocking(nix_app_center_core::load_system_settings)
            .await
            .unwrap_or_else(|_| Err(String::new()));
        match res {
            Ok(settings) => show(&page, &settings),
            Err(e) => show_error(&page, &e),
        }
    });
}

fn show_error(page: &Page, error: &str) {
    let text = gtk::Label::new(Some(error));
    text.set_wrap(true);
    text.set_selectable(true);
    text.set_xalign(0.0);
    text.add_css_class("monospace");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    buttons.set_halign(gtk::Align::Center);
    let retry = gtk::Button::with_mnemonic(&t("_Try Again"));
    retry.add_css_class("pill");
    retry.connect_clicked({
        let page = page.clone();
        move |_| load(&page)
    });
    buttons.append(&retry);
    if let Some(steps) = nix_app_center_core::update_inputs_steps() {
        let update = gtk::Button::with_mnemonic(&t("_Update Inputs"));
        update.add_css_class("pill");
        update.add_css_class("suggested-action");
        update.set_tooltip_text(Some(&t(
            "An option may exist in your repository but not in the locked version",
        )));
        update.connect_clicked({
            let page = page.clone();
            move |btn| {
                let page = page.clone();
                crate::components::apply_dialog::present(
                    btn,
                    &t("Updating Inputs"),
                    steps.clone(),
                    move |_| load(&page),
                );
            }
        });
        buttons.append(&update);
    }
    let body = gtk::Box::new(gtk::Orientation::Vertical, 18);
    body.append(&text);
    body.append(&buttons);
    let status = adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title(t("Could Not Read the Settings"))
        .child(&body)
        .build();
    page.overlay.set_child(Some(&status));
}

/// The namespace most settings share, such as `my`.
fn root_of(settings: &[Setting]) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for s in settings {
        *counts
            .entry(s.name.split('.').next().unwrap_or(""))
            .or_default() += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(r, _)| r.to_string())
        .unwrap_or_default()
}

/// Under `root` (`my`): `boot` for `my.boot.loader`, empty for
/// `my.desktop`. Elsewhere its namespace: `programs` for
/// `programs.niri.enable`.
fn section_of(name: &str, root: &str) -> String {
    let parts: Vec<&str> = name.split('.').collect();
    match parts.as_slice() {
        [first, section, _, ..] if *first == root => section.to_string(),
        [first, _] if *first == root => String::new(),
        [first, ..] => first.to_string(),
        [] => String::new(),
    }
}

/// `Wake on LAN` for `wakeOnLan`, `AI` for `ai`.
fn humanize(ident: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for c in ident.chars() {
        if c.is_uppercase() || words.is_empty() {
            words.push(String::new());
        }
        if let Some(w) = words.last_mut() {
            w.push(c);
        }
    }
    let words: Vec<String> = words
        .into_iter()
        .enumerate()
        .map(|(i, w)| {
            let lower = w.to_lowercase();
            if ["ai", "rgb", "vpn", "lan", "efi", "cpu", "gpu", "uid"].contains(&lower.as_str()) {
                lower.to_uppercase()
            } else if i == 0 {
                let mut c = lower.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            } else {
                lower
            }
        })
        .collect();
    words.join(" ")
}

fn section_title(section: &str) -> String {
    if section.is_empty() {
        t("General")
    } else {
        humanize(section)
    }
}

/// An icon for a known group; any other gets a generic one.
fn section_icon(section: &str) -> &'static str {
    match section {
        "" | "system" => "computer-symbolic",
        "desktop" => "video-display-symbolic",
        "features" => "application-x-addon-symbolic",
        "network" => "network-wired-symbolic",
        "boot" => "system-reboot-symbolic",
        "keyboard" => "input-keyboard-symbolic",
        "monitor" => "video-display-symbolic",
        "hardware" => "drive-harddisk-symbolic",
        "gaming" => "input-gaming-symbolic",
        "dev" => "utilities-terminal-symbolic",
        "user" => "avatar-default-symbolic",
        "power" => "battery-symbolic",
        "laptop" => "computer-laptop-symbolic",
        "vpn" | "zerotier" | "wakeOnLan" | "share" => "network-vpn-symbolic",
        "streaming" | "replay" => "media-record-symbolic",
        _ => "preferences-system-symbolic",
    }
}

/// The row title and the rest of the help: the first paragraph titles it
/// unless too long, then the option's own name does.
fn title_and_body(setting: &Setting) -> (String, String) {
    let help = setting.help();
    let (first, rest) = help.split_once("\n\n").unwrap_or((help.as_str(), ""));
    let first = first.trim();
    if !first.is_empty() && first.chars().count() <= 70 && !first.contains('\n') {
        let title = first.strip_suffix('.').unwrap_or(first).to_string();
        return (title, unwrap_lines(rest));
    }
    let mut parts = setting.name.rsplit('.');
    let last = parts.next().unwrap_or_default();
    let ident = if last == "enable" {
        parts.next().unwrap_or(last)
    } else {
        last
    };
    (humanize(ident), unwrap_lines(&help))
}

/// Paragraphs kept, the line breaks inside them joined.
fn unwrap_lines(text: &str) -> String {
    text.trim()
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Markup under a title: help, state, notes, then the option's name.
fn subtitle(setting: &Setting, body: &str, changed: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    if !body.is_empty() {
        lines.push(esc(body));
    }
    if changed {
        let note = if setting.overridden().is_empty() {
            let default = setting.default.as_deref().unwrap_or("–");
            t("Set here · default: {}").replace("{}", default)
        } else {
            t("Set here · replaces {}").replace("{}", &setting.overridden().join(", "))
        };
        lines.push(format!("<b>{}</b>", esc(&note)));
    }
    lines.push(format!(
        "<span font_family=\"monospace\" size=\"small\">{}</span>",
        esc(&setting.name)
    ));
    lines.join("\n")
}

fn show(page: &Page, settings: &[Setting]) {
    *page.settings.borrow_mut() = settings.to_vec();
    let own: BTreeMap<String, SettingValue> = settings
        .iter()
        .filter_map(|s| Some((s.name.clone(), s.own.clone()?)))
        .collect();
    *page.saved.borrow_mut() = own.clone();
    *page.values.borrow_mut() = own;
    page.dots.borrow_mut().clear();
    if settings.is_empty() {
        let empty = adw::StatusPage::builder()
            .icon_name("preferences-system-symbolic")
            .title(t("No System Settings"))
            .description(t(
                "Your configuration declares no options of its own. Declare one in your files (options.<name> = lib.mkOption …) and it shows here.",
            ))
            .build();
        page.overlay.set_child(Some(&empty));
        page.refresh();
        return;
    }
    let mut sections: Vec<(String, Vec<&Setting>)> = Vec::new();
    let root = root_of(settings);
    for setting in settings {
        let key = section_of(&setting.name, &root);
        match sections.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(setting),
            None => sections.push((key, vec![setting])),
        }
    }
    sections.sort_by_key(|(k, _)| !k.is_empty());

    let mut entries = Vec::new();
    for (key, list) in &sections {
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("changed-dot");
        dot.set_valign(gtk::Align::Center);
        dot.set_tooltip_text(Some(&t("Pending changes")));
        page.dots
            .borrow_mut()
            .push((key.clone(), dot.clone().upcast()));
        entries.push(Section {
            key: key.clone(),
            title: section_title(key),
            icon: section_icon(key),
            page: section_page(page, key, list).upcast(),
            suffix: Some(dot.upcast()),
        });
    }
    let source = match nix_app_center_core::detect_layout().kind {
        LayoutKind::NixosFlake { dir, host } => Some(format!("{}#{host}", dir.display())),
        LayoutKind::NixosLegacy { dir } => {
            Some(dir.join("configuration.nix").display().to_string())
        }
        _ => None,
    };
    let footer = source.map(|source| {
        sidebar::footer(
            &t("Read from {source}. Changes go to nix-app-center-settings.nix.")
                .replace("{source}", &source),
        )
    });
    let body = sidebar::body(entries, footer);
    page.overlay.set_child(Some(&body));
    page.refresh();
}

/// One sidebar entry's page: what can be changed, then what another file sets.
fn section_page(page: &Page, key: &str, settings: &[&Setting]) -> adw::PreferencesPage {
    let prefs = adw::PreferencesPage::new();
    let elsewhere: Vec<&Setting> = settings
        .iter()
        .copied()
        .filter(|s| !s.overridden().is_empty())
        .collect();
    let mut summary = crate::util::plural("{} setting", "{} settings", settings.len() as i64);
    if !elsewhere.is_empty() {
        summary.push_str(" · ");
        summary.push_str(&crate::util::plural(
            "{} set in another file",
            "{} set in other files",
            elsewhere.len() as i64,
        ));
    }
    prefs.add(&sidebar::header(&section_title(key), &summary));

    let mut main: Option<adw::PreferencesGroup> = None;
    for setting in settings.iter().filter(|s| s.overridden().is_empty()) {
        if setting.kind == SettingKind::Enum
            && setting.values.len() <= CHOICE_BUTTONS
            && setting.editable()
        {
            prefs.add(&choice_group(page, setting));
            main = None;
            continue;
        }
        let group = main.get_or_insert_with(|| {
            let g = adw::PreferencesGroup::new();
            prefs.add(&g);
            g
        });
        group.add(&row(page, setting));
    }
    if !elsewhere.is_empty() {
        let mut files: Vec<&str> = elsewhere
            .iter()
            .flat_map(|s| s.overridden().iter().map(String::as_str))
            .collect();
        files.sort_unstable();
        files.dedup();
        let group = adw::PreferencesGroup::builder()
            .title(esc(&t("Set in {}").replace("{}", &files.join(", "))))
            .description(esc(&t(
                "Your own files set these. A value changed here replaces theirs, and your files stay as they are. Locked ones are forced there: edit them in the file.",
            )))
            .build();
        for setting in elsewhere {
            if setting.editable() {
                group.add(&row(page, setting));
            } else {
                group.add(&locked_row(setting));
            }
        }
        prefs.add(&group);
    }
    prefs
}

/// A value as a person reads it.
fn shown(value: &SettingValue) -> String {
    match value {
        SettingValue::Bool(true) => t("On"),
        SettingValue::Bool(false) => t("Off"),
        SettingValue::String(s) => s.split_whitespace().collect::<Vec<_>>().join(" "),
        SettingValue::Array(items) if items.is_empty() => t("None"),
        SettingValue::Array(items) => items
            .iter()
            .map(|v| v.as_str().map_or_else(|| v.to_string(), String::from))
            .collect::<Vec<_>>()
            .join(", "),
        SettingValue::Null => "–".to_string(),
        other => other.to_string(),
    }
}

/// A read-only row: the value, and the file setting it.
fn locked_row(setting: &Setting) -> gtk::Widget {
    let (title, body) = title_and_body(setting);
    let row = adw::ActionRow::builder()
        .title(esc(&title))
        .subtitle(subtitle(setting, &body, false))
        .build();
    row.add_suffix(&file_chip(&setting.elsewhere, true));
    row.add_suffix(&value_label(setting));
    row.upcast()
}

/// The files setting a value too; a lock when they force it.
fn file_chip(files: &[String], locked: bool) -> gtk::Box {
    let chip = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    chip.add_css_class("lock-chip");
    chip.set_valign(gtk::Align::Center);
    let icon = if locked {
        "changes-prevent-symbolic"
    } else {
        "document-edit-symbolic"
    };
    chip.append(&gtk::Image::from_icon_name(icon));
    chip.append(&gtk::Label::new(Some(&files.join(", "))));
    chip
}

fn value_label(setting: &Setting) -> gtk::Label {
    let value = setting.value.clone().unwrap_or(SettingValue::Null);
    let label = gtk::Label::new(Some(&shown(&value)));
    label.add_css_class("heading");
    label.set_selectable(true);
    label.set_max_width_chars(28);
    let text = shown(&value);
    // Short values stay whole; long ones end in an ellipsis.
    if text.chars().count() > 14 {
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_width_chars(14);
    }
    label.set_tooltip_text(Some(&text));
    label.set_xalign(1.0);
    label
}

/// The reset button and the setter shared by every editor.
fn setter(page: &Page, setting: &Setting, row: RowSlot) -> (gtk::Button, Setter) {
    let reset = gtk::Button::from_icon_name("edit-undo-symbolic");
    reset.add_css_class("flat");
    reset.set_valign(gtk::Align::Center);
    reset.set_tooltip_text(Some(&t("Reset to Default")));
    reset.set_visible(setting.own.is_some());
    let set: Setter = {
        let (page, reset, setting) = (page.clone(), reset.clone(), setting.clone());
        let body = title_and_body(&setting).1;
        Rc::new(move |v: Option<SettingValue>| {
            let changed = {
                let mut values = page.values.borrow_mut();
                match v {
                    Some(v) => values.insert(setting.name.clone(), v),
                    None => values.remove(&setting.name),
                };
                values.contains_key(&setting.name)
            };
            reset.set_visible(changed);
            if let Some(row) = row.borrow().as_ref() {
                row.set_subtitle(&subtitle(&setting, &body, changed));
            }
            page.refresh();
        })
    };
    (reset, set)
}

/// The row of one setting nothing else defines.
fn row(page: &Page, setting: &Setting) -> gtk::Widget {
    let value = setting.value.clone().unwrap_or(SettingValue::Null);
    if !setting.editable() {
        let (title, body) = title_and_body(setting);
        let row = adw::ActionRow::builder()
            .title(esc(&title))
            .subtitle(subtitle(setting, &body, false))
            .build();
        row.add_suffix(&value_label(setting));
        return row.upcast();
    }
    let slot: RowSlot = Rc::default();
    let (reset, set) = setter(page, setting, slot.clone());
    let (row, show_value) = editor(setting, &value, &set);
    *slot.borrow_mut() = Some(row.clone());
    // Reset gives the other file's value back, when known.
    let default = if setting.overridden().is_empty() {
        default_value(setting)
    } else if !setting.elsewhere.is_empty() {
        setting.value.clone()
    } else {
        None
    };
    if !setting.overridden().is_empty() {
        row.add_suffix(&file_chip(setting.overridden(), false));
    }
    reset.connect_clicked(move |_| {
        if let Some(d) = &default {
            show_value(d);
        }
        set(None);
    });
    row.add_suffix(&reset);
    row.upcast()
}

/// A small enum as a group of buttons, one per choice.
fn choice_group(page: &Page, setting: &Setting) -> adw::PreferencesGroup {
    let (title, body) = title_and_body(setting);
    let group = adw::PreferencesGroup::builder()
        .title(esc(&title))
        .description(subtitle(setting, &body, setting.own.is_some()))
        .build();
    let (reset, set) = setter(page, setting, Rc::default());
    group.set_header_suffix(Some(&reset));
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(CHOICE_BUTTONS as u32)
        .column_spacing(12)
        .row_spacing(12)
        .build();
    let current = setting.value.clone().unwrap_or(SettingValue::Null);
    let mut buttons: Vec<(SettingValue, gtk::ToggleButton)> = Vec::new();
    for (label, choice) in choices(setting) {
        let button = gtk::ToggleButton::with_label(&label);
        button.add_css_class("choice");
        button.set_active(current == choice);
        if let Some((_, first)) = buttons.first() {
            button.set_group(Some(first));
        }
        flow.append(&button);
        buttons.push((choice, button));
    }
    for (choice, button) in &buttons {
        let (set, choice) = (set.clone(), choice.clone());
        button.connect_toggled(move |b| {
            if b.is_active() {
                set(Some(choice.clone()));
            }
        });
    }
    let default = default_value(setting);
    reset.connect_clicked(move |_| {
        if let Some((_, b)) = buttons.iter().find(|(c, _)| Some(c) == default.as_ref()) {
            b.set_active(true);
        }
        set(None);
    });
    group.add(&flow);
    group
}

/// An enum's choices as (label, value), "None" first when null is allowed.
fn choices(setting: &Setting) -> Vec<(String, SettingValue)> {
    let none = setting.nullable.then(|| (t("None"), SettingValue::Null));
    none.into_iter()
        .chain(
            setting
                .values
                .iter()
                .map(|v| (v.clone(), SettingValue::String(v.clone()))),
        )
        .collect()
}

/// The default, when its Nix text reads as a plain value.
fn default_value(setting: &Setting) -> Option<SettingValue> {
    let text = setting.default.as_deref()?.trim();
    if text == "[ ]" {
        return Some(SettingValue::Array(Vec::new()));
    }
    // Nix strings, numbers and booleans read as JSON.
    text.parse::<SettingValue>()
        .ok()
        .filter(|v| !v.is_object() && !v.is_array())
}

/// The row editing `setting`, and how to show a value in it.
fn editor(setting: &Setting, value: &SettingValue, set: &Setter) -> Editor {
    let (title, body) = title_and_body(setting);
    let title = esc(&title);
    let sub = subtitle(setting, &body, setting.own.is_some());
    match setting.kind {
        SettingKind::Bool => {
            let row = adw::SwitchRow::builder()
                .title(title)
                .subtitle(sub)
                .active(value.as_bool().unwrap_or(false))
                .build();
            let set = set.clone();
            row.connect_active_notify(move |r| set(Some(SettingValue::Bool(r.is_active()))));
            let shown = row.clone();
            let show = move |v: &SettingValue| shown.set_active(v.as_bool().unwrap_or(false));
            (row.upcast(), Box::new(show))
        }
        SettingKind::Enum => {
            let all = choices(setting);
            let labels: Vec<&str> = all.iter().map(|(l, _)| l.as_str()).collect();
            let row = adw::ComboRow::builder()
                .title(title)
                .subtitle(sub)
                .model(&gtk::StringList::new(&labels))
                .build();
            let values: Vec<SettingValue> = all.into_iter().map(|(_, v)| v).collect();
            let index = |values: &[SettingValue], v: &SettingValue| {
                values
                    .iter()
                    .position(|c| c == v)
                    .and_then(|i| u32::try_from(i).ok())
            };
            row.set_selected(index(&values, value).unwrap_or(gtk::INVALID_LIST_POSITION));
            let set = set.clone();
            let picked = values.clone();
            row.connect_selected_notify(move |r| {
                if let Some(choice) = usize::try_from(r.selected())
                    .ok()
                    .and_then(|i| picked.get(i))
                {
                    set(Some(choice.clone()));
                }
            });
            let shown = row.clone();
            let show = move |v: &SettingValue| {
                if let Some(i) = index(&values, v) {
                    shown.set_selected(i);
                }
            };
            (row.upcast(), Box::new(show))
        }
        SettingKind::List => list_editor(title, sub, value, set),
        kind => text_editor(title, sub, kind, setting.nullable, value, set),
    }
}

/// A text field for a string or a number; red while it does not parse.
fn text_editor(
    title: String,
    sub: String,
    kind: SettingKind,
    nullable: bool,
    value: &SettingValue,
    set: &Setter,
) -> Editor {
    let row = adw::ActionRow::builder().title(title).subtitle(sub).build();
    let entry = gtk::Entry::new();
    entry.set_valign(gtk::Align::Center);
    if kind == SettingKind::Str {
        entry.add_css_class("monospace");
    } else {
        entry.set_input_purpose(gtk::InputPurpose::Number);
        entry.set_width_chars(8);
    }
    let as_text = |v: &SettingValue| match v {
        SettingValue::String(s) => s.clone(),
        SettingValue::Null => String::new(),
        other => other.to_string(),
    };
    entry.set_text(&as_text(value));
    if nullable {
        entry.set_placeholder_text(Some(&t("None")));
    }
    let set = set.clone();
    entry.connect_changed(move |e| {
        let text = e.text().to_string();
        let parsed = match kind {
            _ if nullable && text.trim().is_empty() => Some(SettingValue::Null),
            SettingKind::Int => text.trim().parse::<i64>().ok().map(SettingValue::from),
            SettingKind::Float => text
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|f| f.is_finite())
                .map(SettingValue::from),
            _ => Some(SettingValue::String(text)),
        };
        if parsed.is_some() {
            e.remove_css_class("error");
            set(parsed);
        } else {
            e.add_css_class("error");
        }
    });
    row.add_suffix(&entry);
    row.set_activatable_widget(Some(&entry));
    let show = move |v: &SettingValue| entry.set_text(&as_text(v));
    (row, Box::new(show))
}

/// The strings in a popover, each removable, with a field to add one.
fn list_editor(title: String, sub: String, value: &SettingValue, set: &Setter) -> Editor {
    let row = adw::ActionRow::builder().title(title).subtitle(sub).build();
    let items: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(strings(value)));
    let summary = crate::util::dim_label("");
    summary.set_wrap(true);
    summary.set_max_width_chars(30);
    let edit = gtk::MenuButton::builder()
        .icon_name("document-edit-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(t("Edit List"))
        .build();
    edit.add_css_class("flat");
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    let add = adw::EntryRow::builder()
        .title(t("Add"))
        .show_apply_button(true)
        .build();
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 6);
    box_.set_size_request(320, -1);
    box_.append(&list);
    let popover = gtk::Popover::builder().child(&box_).build();
    edit.set_popover(Some(&popover));

    type Redraw = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
    let redraw: Redraw = Rc::default();
    let draw: Rc<dyn Fn()> = {
        let (items, list, add, summary, redraw) = (
            items.clone(),
            list.clone(),
            add.clone(),
            summary.clone(),
            redraw.clone(),
        );
        Rc::new(move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            for (i, item) in items.borrow().iter().enumerate() {
                let line = adw::ActionRow::builder()
                    .use_markup(false)
                    .title(item.as_str())
                    .build();
                let remove = gtk::Button::from_icon_name("list-remove-symbolic");
                remove.add_css_class("flat");
                remove.set_valign(gtk::Align::Center);
                remove.set_tooltip_text(Some(&t("Remove")));
                let (items, redraw) = (items.clone(), redraw.clone());
                remove.connect_clicked(move |_| {
                    items.borrow_mut().remove(i);
                    if let Some(f) = redraw.borrow().clone() {
                        f();
                    }
                });
                line.add_suffix(&remove);
                list.append(&line);
            }
            list.append(&add);
            summary.set_text(&items.borrow().join(", "));
        })
    };
    *redraw.borrow_mut() = Some({
        let (draw, items, set) = (draw.clone(), items.clone(), set.clone());
        Rc::new(move || {
            draw();
            let all: Vec<SettingValue> = items
                .borrow()
                .iter()
                .cloned()
                .map(SettingValue::String)
                .collect();
            set(Some(SettingValue::Array(all)));
        })
    });
    add.connect_apply({
        let (items, redraw) = (items.clone(), redraw.clone());
        move |entry| {
            let text = entry.text().trim().to_string();
            if text.is_empty() {
                return;
            }
            items.borrow_mut().push(text);
            entry.set_text("");
            if let Some(f) = redraw.borrow().clone() {
                f();
            }
        }
    });
    draw();
    row.add_suffix(&summary);
    row.add_suffix(&edit);
    let show = move |v: &SettingValue| {
        *items.borrow_mut() = strings(v);
        draw();
    };
    (row, Box::new(show))
}

fn strings(value: &SettingValue) -> Vec<String> {
    value
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Writes the settings file, connects it once, then applies.
fn save_and_apply(page: &Page) {
    page.bar.set_sensitive(false);
    let snapshot = page.values.borrow().clone();
    let replaces: BTreeMap<String, Vec<String>> = page
        .settings
        .borrow()
        .iter()
        .filter(|s| snapshot.contains_key(&s.name) && !s.overridden().is_empty())
        .map(|s| (s.name.clone(), s.overridden().to_vec()))
        .collect();
    let page = page.clone();
    gtk::glib::spawn_future_local(async move {
        let res = gtk::gio::spawn_blocking(move || {
            nix_app_center_core::write_system_settings(&snapshot, &replaces)
                .map_err(|e| e.to_string())
        })
        .await
        .unwrap_or_else(|_| Err(String::new()));
        page.bar.set_sensitive(true);
        if let Err(e) = res {
            crate::util::toast(&page.overlay, &crate::util::failed(&e));
            return;
        }
        *page.saved.borrow_mut() = page.values.borrow().clone();
        page.refresh();
        let layout = nix_app_center_core::detect_layout();
        match nix_app_center_core::attach_plan(&layout) {
            nix_app_center_core::AttachPlan::AlreadyAttached => run_apply(&page),
            plan => {
                let p = page.clone();
                crate::components::details::attach_dialog::present(
                    &page.overlay,
                    plan,
                    move |res| match res {
                        Ok(()) => run_apply(&p),
                        Err(e) => crate::util::toast(&p.overlay, &crate::util::failed(&e)),
                    },
                );
            }
        }
    });
}

/// Rebuilds, then reads the settings again.
fn run_apply(page: &Page) {
    let Some(argv) = nix_app_center_core::apply_argv() else {
        return;
    };
    let p = page.clone();
    crate::components::apply_dialog::present(
        &page.overlay,
        &t("Applying System Settings"),
        vec![argv],
        move |res| {
            if matches!(res, Ok(nix_app_center_core::ApplyOutcome::Done(_))) {
                load(&p);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_as_words() {
        assert_eq!(humanize("wakeOnLan"), "Wake on LAN");
        assert_eq!(humanize("ai"), "AI");
        assert_eq!(humanize("efiMountPoint"), "EFI mount point");
        assert_eq!(section_of("my.boot.loader", "my"), "boot");
        assert_eq!(section_of("my.desktop", "my"), "");
        assert_eq!(section_of("programs.niri.enable", "my"), "programs");
        assert_eq!(section_of("custom.gaming.enable", "custom"), "gaming");
    }

    #[test]
    fn the_first_paragraph_titles_the_row() {
        let setting = |name: &str, description: &str| Setting {
            name: name.into(),
            description: Some(description.into()),
            kind: SettingKind::Bool,
            value: None,
            default: None,
            values: Vec::new(),
            own: None,
            elsewhere: Vec::new(),
            nullable: false,
            priority: 1500,
            replaces: Vec::new(),
        };
        let s = setting(
            "my.boot.osProber",
            "Dual boot detection.\n\nos-prober adds other systems.",
        );
        assert_eq!(
            title_and_body(&s),
            (
                "Dual boot detection".to_string(),
                "os-prober adds other systems.".to_string()
            )
        );
        let s = setting("my.gaming.enable", "Whether to enable gaming stack.");
        assert_eq!(title_and_body(&s).0, "Gaming stack");
        let long = "A".repeat(90);
        let s = setting("my.user.uid", &long);
        assert_eq!(title_and_body(&s), ("UID".to_string(), long));
    }
}
