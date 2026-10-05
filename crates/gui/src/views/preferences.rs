//! Preferences dialog: what Onyx detected of the Nix configuration, overrides
//! when the text scan guesses wrong, a check by Nix itself, and the update
//! check preference. Settings are saved as soon as they change.

use adw::prelude::*;
use onyx_core::{Layout, LayoutKind};

use crate::i18n::t;
use crate::util::esc;

pub fn present(parent: &impl IsA<gtk::Widget>) {
    let settings = onyx_core::load_settings();
    let dialog = adw::PreferencesDialog::new();
    let page = adw::PreferencesPage::new();

    // ── Configuration ─────────────────────────────────────────────────────────
    let config = adw::PreferencesGroup::builder()
        .title(t("Configuration"))
        .description(t(
            "Onyx finds your configuration by reading its files. Correct it here when it guesses wrong.",
        ))
        .build();
    let detected = adw::ActionRow::builder()
        .title(t("Detected"))
        .subtitle(esc(&describe(&onyx_core::detect_layout())))
        .subtitle_selectable(true)
        .build();
    let dir = adw::EntryRow::builder()
        .title(t("NixOS configuration folder (empty: /etc/nixos)"))
        .show_apply_button(true)
        .build();
    if let Some(d) = &settings.config_dir {
        dir.set_text(&d.to_string_lossy());
    }
    let host = adw::EntryRow::builder()
        .title(t("Host (empty: this machine)"))
        .show_apply_button(true)
        .build();
    if let Some(h) = &settings.host {
        host.set_text(h);
    }
    let save = {
        let detected = detected.clone();
        move |edit: &dyn Fn(&mut onyx_core::Settings)| {
            let mut s = onyx_core::load_settings();
            edit(&mut s);
            let _ = onyx_core::save_settings(&s);
            detected.set_subtitle(&esc(&describe(&onyx_core::detect_layout())));
        }
    };
    dir.connect_apply({
        let save = save.clone();
        move |row| {
            let text = row.text().trim().to_string();
            save(&|s| s.config_dir = (!text.is_empty()).then(|| text.clone().into()));
        }
    });
    host.connect_apply({
        let save = save.clone();
        move |row| {
            let text = row.text().trim().to_string();
            save(&|s| s.host = (!text.is_empty()).then(|| text.clone()));
        }
    });

    let check = adw::ButtonRow::builder().title(t("Check with Nix")).build();
    let verdict = adw::ActionRow::builder()
        .title(t("Nix says"))
        .subtitle(esc(&t("Not checked yet.")))
        .subtitle_selectable(true)
        .build();
    check.connect_activated({
        let verdict = verdict.clone();
        move |row| {
            row.set_sensitive(false);
            verdict.set_subtitle(&esc(&t("Evaluating your configuration…")));
            let (row, verdict) = (row.clone(), verdict.clone());
            gtk::glib::spawn_future_local(async move {
                let layout = onyx_core::detect_layout();
                let res = gtk::gio::spawn_blocking(move || onyx_core::verify_layout(&layout))
                    .await
                    .unwrap_or_else(|_| Err(String::new()));
                verdict.set_subtitle(&esc(&match res {
                    Ok(v) => verified_text(&v),
                    Err(e) => t("Nix could not evaluate it: {}").replace("{}", &e),
                }));
                row.set_sensitive(true);
            });
        }
    });
    for w in [
        detected.upcast_ref::<gtk::Widget>(),
        dir.upcast_ref(),
        host.upcast_ref(),
        check.upcast_ref(),
        verdict.upcast_ref(),
    ] {
        config.add(w);
    }

    // ── Updates ───────────────────────────────────────────────────────────────
    let updates = adw::PreferencesGroup::builder().title(t("Updates")).build();
    let auto = adw::SwitchRow::builder()
        .title(t("Check for Updates Automatically"))
        .subtitle(t(
            "Once a day, with a notification when apps can be updated",
        ))
        .active(settings.auto_check_updates)
        .build();
    auto.connect_active_notify(move |row| {
        let mut s = onyx_core::load_settings();
        s.auto_check_updates = row.is_active();
        let _ = onyx_core::save_settings(&s);
    });
    updates.add(&auto);

    page.add(&config);
    page.add(&updates);
    dialog.add(&page);
    dialog.present(Some(parent));
}

fn describe(layout: &Layout) -> String {
    let base = match &layout.kind {
        LayoutKind::NixosFlake { dir, host } => t("NixOS flake in {dir}, host {host}")
            .replace("{dir}", &dir.display().to_string())
            .replace("{host}", host),
        LayoutKind::NixosLegacy { dir } => {
            t("NixOS with channels in {}").replace("{}", &dir.display().to_string())
        }
        LayoutKind::HmFlake { dir, .. } => {
            t("Home Manager flake in {}").replace("{}", &dir.display().to_string())
        }
        LayoutKind::HmStandalone { dir } => {
            t("Home Manager in {}").replace("{}", &dir.display().to_string())
        }
        LayoutKind::Unknown => return t("No configuration found."),
    };
    match &layout.hm_user {
        Some(u) => format!(
            "{base}\n{}",
            t("Home Manager module for {}").replace("{}", u)
        ),
        None => base,
    }
}

fn verified_text(v: &onyx_core::Verified) -> String {
    let mut lines = Vec::new();
    if !v.hosts.is_empty() {
        lines.push(t("Configurations: {}").replace("{}", &v.hosts.join(", ")));
    }
    lines.push(match &v.hm_users {
        Ok(users) if !users.is_empty() => {
            t("Home Manager users: {}").replace("{}", &users.join(", "))
        }
        Ok(_) => t("No Home Manager module."),
        Err(e) => t("This host does not evaluate: {}").replace("{}", e),
    });
    lines.join("\n")
}
