//! The welcome tour shown on first launch: how trying, keeping and applying
//! work, what configuration was found, and the main settings.

use adw::prelude::*;

use crate::i18n::t;

/// One page: an icon, a title, a text, and an optional extra widget.
fn page(icon: &str, title: &str, body: &str, extra: Option<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 18);
    b.add_css_class("welcome-page");
    b.set_valign(gtk::Align::Center);
    b.set_hexpand(true);
    b.set_margin_start(36);
    b.set_margin_end(36);
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(112);
    image.add_css_class("welcome-icon");
    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("title-1");
    heading.add_css_class("welcome-title");
    heading.set_wrap(true);
    heading.set_justify(gtk::Justification::Center);
    let text = gtk::Label::new(Some(body));
    text.add_css_class("welcome-body");
    text.set_wrap(true);
    text.set_justify(gtk::Justification::Center);
    text.set_max_width_chars(54);
    b.append(&image);
    b.append(&heading);
    b.append(&text);
    if let Some(extra) = extra {
        extra.add_css_class("welcome-extra");
        b.append(&extra);
    }
    b
}

/// What Nix Software found, with a check by Nix and a way to correct it.
fn config_extra(parent: &gtk::Widget) -> gtk::Widget {
    let layout = nix_software_core::detect_layout();
    let detected = adw::ActionRow::builder()
        .title(t("Detected"))
        .subtitle(crate::util::esc(&crate::views::preferences::describe(
            &layout,
        )))
        .subtitle_selectable(true)
        .build();
    let (check, verdict) = crate::views::preferences::nix_check_rows();
    let change = adw::ButtonRow::builder()
        .title(t("Change in Preferences…"))
        .build();
    change.connect_activated({
        let parent = parent.clone();
        move |_| crate::views::preferences::present(&parent)
    });
    let list = crate::util::boxed_list();
    if layout.kind == nix_software_core::LayoutKind::Unknown {
        let hint = adw::ActionRow::builder()
            .title(t("No Configuration Yet"))
            .subtitle(crate::util::esc(&t(
                "Apps you install go to your user profile with nix profile. To keep them in a configuration instead, set up Home Manager, then open Preferences.",
            )))
            .build();
        let link = gtk::LinkButton::with_label(
            "https://nix-community.github.io/home-manager/",
            &t("Home Manager Manual"),
        );
        link.set_valign(gtk::Align::Center);
        hint.add_suffix(&link);
        list.append(&hint);
    }
    for w in [
        detected.upcast_ref::<gtk::Widget>(),
        check.upcast_ref(),
        verdict.upcast_ref(),
        change.upcast_ref(),
    ] {
        list.append(w);
    }
    list.upcast()
}

/// The two settings most people want to decide at once.
fn settings_extra() -> gtk::Widget {
    let settings = nix_software_core::load_settings();
    let list = crate::util::boxed_list();
    if nix_software_core::detect_layout().system.is_some() {
        let everyone = adw::SwitchRow::builder()
            .title(t("Install for Everyone"))
            .subtitle(t(
                "Apps you add go to the system packages, for every user of this computer",
            ))
            .active(settings.install_for_everyone)
            .build();
        everyone.connect_active_notify(|row| {
            let mut s = nix_software_core::load_settings();
            s.install_for_everyone = row.is_active();
            let _ = nix_software_core::save_settings(&s);
        });
        list.append(&everyone);
    }
    let auto = adw::SwitchRow::builder()
        .title(t("Check for Updates Automatically"))
        .subtitle(t(
            "Once a day, with a notification when apps can be updated",
        ))
        .active(settings.auto_check_updates)
        .build();
    auto.connect_active_notify(|row| {
        let mut s = nix_software_core::load_settings();
        s.auto_check_updates = row.is_active();
        let _ = nix_software_core::save_settings(&s);
    });
    list.append(&auto);
    list.upcast()
}

/// Remembers that the tour was seen.
fn mark_seen() {
    let mut s = nix_software_core::load_settings();
    if !s.onboarded {
        s.onboarded = true;
        let _ = nix_software_core::save_settings(&s);
    }
}

/// Shows the tour over `parent`.
pub fn present(parent: &impl IsA<gtk::Widget>) {
    let parent = parent.clone().upcast::<gtk::Widget>();
    let pages = [
        page(
            "io.github.sptapyo.NixSoftware",
            &t("Welcome to Nix Software"),
            &t("An app store for NixOS and Home Manager. In one minute, see how it works and check that it found your configuration."),
            None,
        ),
        page(
            "media-playback-start-symbolic",
            &t("Try Apps Without Installing"),
            &t("Try starts any app right away. Nothing is installed and nothing changes on your system: close the app and it is gone."),
            None,
        ),
        page(
            "bookmark-new-symbolic",
            &t("Keep the Ones You Like"),
            &t("Add to Configuration writes the app into two small files that belong to Nix Software. Your own files are touched once, to import them, and only after you approve the exact change."),
            None,
        ),
        page(
            "computer-symbolic",
            &t("Your Configuration"),
            &t("This is where Nix Software found your system. If it is wrong, correct it now: apps would otherwise be added to the wrong place."),
            Some(config_extra(&parent)),
        ),
        page(
            "view-refresh-symbolic",
            &t("Apply When You Are Ready"),
            &t("Apply rebuilds your system with the apps you added. A bar shows how far it is; you can close it and keep browsing."),
            Some(settings_extra()),
        ),
        page(
            "io.github.sptapyo.NixSoftware",
            &t("You Are All Set"),
            &t("Explore apps, look at Updates for new versions and at Storage to free space. This tour stays in the main menu."),
            None,
        ),
    ];

    let carousel = adw::Carousel::builder()
        .vexpand(true)
        .allow_scroll_wheel(false)
        .build();
    for p in &pages {
        carousel.append(p);
    }
    let dots = adw::CarouselIndicatorDots::builder()
        .carousel(&carousel)
        .build();
    let skip = gtk::Button::with_mnemonic(&t("_Skip"));
    skip.add_css_class("flat");
    skip.set_tooltip_text(Some(&t("Close the tour. It stays in the main menu.")));
    let back = gtk::Button::with_mnemonic(&t("_Back"));
    let next = gtk::Button::with_mnemonic(&t("_Next"));
    next.add_css_class("suggested-action");
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.append(&back);
    actions.append(&next);
    let bottom = gtk::CenterBox::new();
    bottom.set_margin_top(12);
    bottom.set_margin_bottom(18);
    bottom.set_margin_start(18);
    bottom.set_margin_end(18);
    bottom.set_start_widget(Some(&skip));
    bottom.set_center_widget(Some(&dots));
    bottom.set_end_widget(Some(&actions));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&carousel);
    content.append(&bottom);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::builder().show_title(false).build());
    toolbar.set_content(Some(&content));
    let dialog = adw::Dialog::builder()
        .title(t("Welcome"))
        .content_width(720)
        .content_height(620)
        .child(&toolbar)
        .build();

    let last = pages.len() as u32 - 1;
    let show = {
        let (pages, back, next, skip) = (pages.clone(), back.clone(), next.clone(), skip.clone());
        move |index: u32| {
            for (i, p) in pages.iter().enumerate() {
                if i as u32 == index {
                    p.add_css_class("welcome-active");
                } else {
                    p.remove_css_class("welcome-active");
                }
            }
            back.set_sensitive(index > 0);
            skip.set_visible(index < last);
            next.set_label(&if index == last {
                t("_Start Exploring")
            } else {
                t("_Next")
            });
            next.set_use_underline(true);
        }
    };
    show(0);
    carousel.connect_page_changed({
        let show = show.clone();
        move |_, index| show(index)
    });
    let go = {
        let (carousel, pages) = (carousel.clone(), pages.clone());
        move |index: u32| {
            pages[index as usize].add_css_class("welcome-active");
            carousel.scroll_to(&carousel.nth_page(index), true);
        }
    };
    back.connect_clicked({
        let (carousel, go) = (carousel.clone(), go.clone());
        move |_| {
            let i = carousel.position().round() as u32;
            if i > 0 {
                go(i - 1);
            }
        }
    });
    next.connect_clicked({
        let (carousel, dialog) = (carousel.clone(), dialog.clone());
        move |_| {
            let i = carousel.position().round() as u32;
            if i >= last {
                dialog.close();
            } else {
                go(i + 1);
            }
        }
    });
    skip.connect_clicked({
        let dialog = dialog.clone();
        move |_| {
            dialog.close();
        }
    });
    dialog.connect_closed(|_| mark_seen());
    dialog.present(Some(&parent));
}
