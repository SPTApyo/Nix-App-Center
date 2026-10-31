//! Preferences: the detected configuration and its overrides, installing,
//! updates and search. Saved on change.

use adw::prelude::*;
use nix_app_center_core::{Layout, LayoutKind};

use crate::i18n::t;
use crate::util::esc;

pub fn present(parent: &impl IsA<gtk::Widget>) {
    let settings = nix_app_center_core::load_settings();
    let dialog = adw::PreferencesDialog::new();
    let page = adw::PreferencesPage::new();

    let config = adw::PreferencesGroup::builder()
        .title(t("Configuration"))
        .description(t(
            "Nix App Center finds your configuration by reading its files. Correct it here when it guesses wrong.",
        ))
        .build();
    let detected = adw::ActionRow::builder()
        .title(t("Detected"))
        .subtitle(esc(&describe(&nix_app_center_core::detect_layout())))
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
        move |edit: &dyn Fn(&mut nix_app_center_core::Settings)| {
            let mut s = nix_app_center_core::load_settings();
            edit(&mut s);
            let _ = nix_app_center_core::save_settings(&s);
            detected.set_subtitle(&esc(&describe(&nix_app_center_core::detect_layout())));
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

    let (check, verdict) = nix_check_rows();
    for w in [
        detected.upcast_ref::<gtk::Widget>(),
        dir.upcast_ref(),
        host.upcast_ref(),
        check.upcast_ref(),
        verdict.upcast_ref(),
    ] {
        config.add(w);
    }

    let installing = adw::PreferencesGroup::builder()
        .title(t("Installing"))
        .build();
    let everyone = adw::SwitchRow::builder()
        .title(t("Install for Everyone"))
        .subtitle(t(
            "Apps you add go to the system packages, for every user of this computer",
        ))
        .active(settings.install_for_everyone)
        .build();
    everyone.connect_active_notify(move |row| {
        let mut s = nix_app_center_core::load_settings();
        s.install_for_everyone = row.is_active();
        let _ = nix_app_center_core::save_settings(&s);
    });
    installing.add(&everyone);
    everyone.set_visible(nix_app_center_core::detect_layout().system.is_some());
    let unfree = adw::SwitchRow::builder()
        .title(t("Allow Proprietary Apps"))
        .subtitle(t(
            "Without asking for each one. Only the apps you add are allowed.",
        ))
        .active(settings.allow_all_unfree)
        .build();
    unfree.connect_active_notify(move |row| {
        let mut s = nix_app_center_core::load_settings();
        s.allow_all_unfree = row.is_active();
        let _ = nix_app_center_core::save_settings(&s);
    });
    installing.add(&unfree);

    let review = adw::SwitchRow::builder()
        .title(t("Show Changes Before Applying"))
        .subtitle(t(
            "What gets updated, added or removed, before the system switches",
        ))
        .active(settings.review_changes)
        .build();
    review.connect_active_notify(move |row| {
        let mut s = nix_app_center_core::load_settings();
        s.review_changes = row.is_active();
        let _ = nix_app_center_core::save_settings(&s);
    });

    let updates = adw::PreferencesGroup::builder().title(t("Updates")).build();
    let auto = adw::SwitchRow::builder()
        .title(t("Check for Updates Automatically"))
        .subtitle(t(
            "Once a day, with a notification when apps can be updated",
        ))
        .active(settings.auto_check_updates)
        .build();
    auto.connect_active_notify(move |row| {
        let mut s = nix_app_center_core::load_settings();
        s.auto_check_updates = row.is_active();
        let _ = nix_app_center_core::save_settings(&s);
    });
    updates.add(&auto);
    updates.add(&review);

    let search = adw::PreferencesGroup::builder().title(t("Search")).build();
    let by_command = adw::ActionRow::builder()
        .title(t("Search by Command"))
        .subtitle(esc(&if nix_app_center_core::locate_ready() {
            t("Ready: searching a command also lists the packages that provide it.")
        } else {
            t("Lists the packages that provide a command. Needs the nix-index database, about 100 MB.")
        }))
        .build();
    if !nix_app_center_core::locate_ready() {
        let download = gtk::Button::with_mnemonic(&t("_Download"));
        download.set_tooltip_text(Some(&t("Download the nix-index database, about 100 MB.")));
        download.set_valign(gtk::Align::Center);
        download.connect_clicked({
            let row = by_command.clone();
            move |btn| {
                btn.set_sensitive(false);
                row.set_subtitle(&esc(&t("Downloading the nix-index database…")));
                let (btn, row) = (btn.clone(), row.clone());
                gtk::glib::spawn_future_local(async move {
                    let res = gtk::gio::spawn_blocking(nix_app_center_core::download_locate_db).await;
                    match res {
                        Ok(Ok(())) => {
                            row.set_subtitle(&esc(&t("Ready: searching a command also lists the packages that provide it.")));
                            btn.set_visible(false);
                        }
                        Ok(Err(e)) => {
                            row.set_subtitle(&esc(&crate::util::failed(&e)));
                            btn.set_sensitive(true);
                        }
                        Err(_) => btn.set_sensitive(true),
                    }
                });
            }
        });
        by_command.add_suffix(&download);
    }
    search.add(&by_command);

    let advanced = adw::PreferencesGroup::builder()
        .title(t("Advanced"))
        .description(t(
            "For unusual setups only, such as a configuration kept in another folder and built with your own script. Leave it empty if Apply already works.",
        ))
        .build();
    let command = adw::EntryRow::builder()
        .title(t("Apply command (empty: found by itself)"))
        .show_apply_button(true)
        .build();
    if let Some(c) = &settings.apply_command {
        command.set_text(c);
    }
    command.connect_apply(move |row| {
        let text = row.text().trim().to_string();
        save(&|s| s.apply_command = (!text.is_empty()).then(|| text.clone()));
    });
    let warning = adw::ActionRow::builder()
        .title(t("How It Runs"))
        .subtitle(esc(&t(
            "Run by the shell. Start it with sudo to run it as administrator: Nix App Center asks for the password itself, so tools that call sudo on their own, such as nh, do not work here. The progress bar follows nixos-rebuild and plain Nix logs.",
        )))
        .build();
    warning.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
    let flakes = adw::EntryRow::builder()
        .title(t(
            "Other flakes to search (github:owner/repo, comma separated)",
        ))
        .show_apply_button(true)
        .text(settings.extra_flakes.join(", "))
        .build();
    flakes.connect_apply(|row| {
        let list: Vec<String> = row
            .text()
            .split(',')
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .collect();
        let mut s = nix_app_center_core::load_settings();
        let added: Vec<String> = list
            .iter()
            .filter(|f| !s.extra_flakes.contains(f))
            .cloned()
            .collect();
        s.extra_flakes = list;
        let _ = nix_app_center_core::save_settings(&s);
        for flake in added {
            offer_cache(row, flake);
        }
    });
    let suggested = adw::ExpanderRow::builder()
        .title(t("Flakes Suggested by search.nixos.org"))
        .subtitle(t("Search their packages too"))
        .build();
    suggested.connect_expanded_notify({
        let loaded = std::cell::Cell::new(false);
        let flakes = flakes.clone();
        move |row| {
            if !row.is_expanded() || loaded.replace(true) {
                return;
            }
            let (row, flakes) = (row.clone(), flakes.clone());
            gtk::glib::spawn_future_local(async move {
                let list = gtk::gio::spawn_blocking(nix_app_center_core::suggested_flakes)
                    .await
                    .unwrap_or_default();
                if list.is_empty() {
                    row.add_row(
                        &adw::ActionRow::builder()
                            .title(t("The list could not be read."))
                            .build(),
                    );
                }
                for flake in list {
                    let chosen = nix_app_center_core::load_settings()
                        .extra_flakes
                        .contains(&flake);
                    let switch = adw::SwitchRow::builder()
                        .title(esc(flake.trim_start_matches("github:")))
                        .active(chosen)
                        .build();
                    let flakes = flakes.clone();
                    switch.connect_active_notify(move |sw| {
                        let mut s = nix_app_center_core::load_settings();
                        s.extra_flakes.retain(|f| *f != flake);
                        if sw.is_active() {
                            s.extra_flakes.push(flake.clone());
                        }
                        flakes.set_text(&s.extra_flakes.join(", "));
                        let _ = nix_app_center_core::save_settings(&s);
                        if sw.is_active() {
                            offer_cache(sw, flake.clone());
                        }
                    });
                    row.add_row(&switch);
                }
            });
        }
    });
    advanced.add(&command);
    advanced.add(&flakes);
    advanced.add(&suggested);
    advanced.add(&warning);

    page.add(&config);
    page.add(&installing);
    let storage = adw::PreferencesGroup::builder().title(t("Storage")).build();
    let cleanup = adw::SwitchRow::builder()
        .title(t("Clean Up Automatically"))
        .subtitle(t("Every week, deletes what nothing uses and generations older than 30 days, which then leave History. Applies with your configuration."))
        .build();
    match nix_app_center_core::auto_cleanup() {
        nix_app_center_core::AutoCleanup::ByConfig => {
            cleanup.set_active(true);
            cleanup.set_sensitive(false);
            cleanup.set_subtitle(&t("Set by your own configuration."));
        }
        state => cleanup.set_active(state == nix_app_center_core::AutoCleanup::On),
    }
    cleanup.connect_active_notify(|row| {
        if let Err(e) = nix_app_center_core::set_auto_cleanup(row.is_active()) {
            row.set_subtitle(&crate::util::failed(&e));
        }
    });
    storage.add(&cleanup);
    storage.set_visible(nix_app_center_core::detect_layout().is_usable());

    page.add(&updates);
    page.add(&storage);
    page.add(&search);
    page.add(&advanced);
    if let Some(caches) = caches_group() {
        page.add(&caches);
    }
    dialog.add(&page);
    dialog.present(Some(parent));
}

/// Offers the binary cache `flake` declares, once it is added.
fn offer_cache(widget: &impl IsA<gtk::Widget>, flake: String) {
    let widget = widget.clone().upcast::<gtk::Widget>();
    gtk::glib::spawn_future_local(async move {
        let name = flake.clone();
        let cache = gtk::gio::spawn_blocking(move || {
            nix_app_center_core::flake_cache(&name)
                .filter(|c| !nix_app_center_core::cache_trusted(&c.url))
        })
        .await
        .ok()
        .flatten();
        let Some(cache) = cache else {
            return;
        };
        let dialog = widget.ancestor(adw::PreferencesDialog::static_type());
        let shown = flake.trim_start_matches("github:").to_string();
        crate::components::cache_dialog::present(&widget, &shown, cache, move |saved| {
            let text = match saved {
                Ok(()) => t("Saved. Apply your configuration to use it."),
                Err(e) => crate::util::failed(&e),
            };
            if let Some(d) = dialog
                .as_ref()
                .and_then(|d| d.downcast_ref::<adw::PreferencesDialog>())
            {
                d.add_toast(adw::Toast::new(&text));
            }
        });
    });
}

/// The caches Nix App Center makes the system use, each removable.
fn caches_group() -> Option<adw::PreferencesGroup> {
    let urls = nix_app_center_core::trusted_caches();
    if urls.is_empty() {
        return None;
    }
    let group = adw::PreferencesGroup::builder()
        .title(t("Binary Caches"))
        .description(t(
            "Servers your system downloads apps from, besides cache.nixos.org.",
        ))
        .build();
    for url in urls {
        let row = adw::ActionRow::builder().title(esc(&url)).build();
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text(t("Stop Using It"))
            .valign(gtk::Align::Center)
            .build();
        remove.add_css_class("flat");
        let (group2, row2) = (group.clone(), row.clone());
        remove.connect_clicked(move |_| {
            if nix_app_center_core::untrust_cache(&url).is_ok() {
                group2.remove(&row2);
                group2.set_visible(!nix_app_center_core::trusted_caches().is_empty());
                if let Some(d) = group2
                    .ancestor(adw::PreferencesDialog::static_type())
                    .and_downcast::<adw::PreferencesDialog>()
                {
                    d.add_toast(adw::Toast::new(&t(
                        "Removed. Apply your configuration to undo it.",
                    )));
                }
            }
        });
        row.add_suffix(&remove);
        group.add(&row);
    }
    Some(group)
}

pub(crate) fn describe(layout: &Layout) -> String {
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
    let mut lines = vec![base];
    if let Some(u) = &layout.hm_user {
        lines.push(t("Home Manager module for {}").replace("{}", u));
    }
    if !layout.apply.is_empty() {
        lines.push(t("Applied with: {}").replace("{}", &layout.apply.join(" ")));
    }
    lines.join("\n")
}

fn verified_text(v: &nix_app_center_core::Verified) -> String {
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

/// "Check with Nix" and the row showing what Nix answered.
pub(crate) fn nix_check_rows() -> (adw::ButtonRow, adw::ActionRow) {
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
                let layout = nix_app_center_core::detect_layout();
                let res =
                    gtk::gio::spawn_blocking(move || nix_app_center_core::verify_layout(&layout))
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
    (check, verdict)
}
