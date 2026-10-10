//! Infobars of the details page: nixpkgs problems, and the NixOS module
//! that sets an app up.

use crate::i18n::t;
use gtk::prelude::*;

/// A warning infobar: a heading line, then paragraphs.
fn warning(lines: &[&str]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.add_css_class("card");
    b.add_css_class("infobox");
    b.add_css_class("warning");

    for (i, text) in lines.iter().enumerate() {
        let l = gtk::Label::new(Some(text));
        l.set_justify(gtk::Justification::Center);
        l.set_wrap(true);
        if i == 0 {
            l.add_css_class("heading");
        }
        b.append(&l);
    }
    b
}

/// The broken or insecure infobar; `None` when nixpkgs reports nothing.
pub fn problems(broken: bool, insecure: bool, vulnerabilities: &[String]) -> Option<gtk::Box> {
    if broken {
        return Some(warning(&[
            &t("Broken Package"),
            &t("nixpkgs marks this package as broken: running it will probably fail."),
        ]));
    }
    if insecure || !vulnerabilities.is_empty() {
        let detail = if vulnerabilities.is_empty() {
            t("This package is marked insecure in nixpkgs.")
        } else {
            t("This package is marked insecure in nixpkgs ({}).")
                .replace("{}", &vulnerabilities.join(", "))
        };
        return Some(warning(&[&t("Known Vulnerabilities"), &detail]));
    }
    None
}

/// Days after which a hold is called out more strongly.
const LONG_HOLD_DAYS: u64 = 30;

/// The held version infobar, with a button releasing the hold.
pub fn held(pin: &nix_software_core::Pin, on_release: impl Fn() + 'static) -> gtk::Box {
    let days = nix_software_core::now_unix().saturating_sub(pin.since) / 86_400;
    let heading = if days >= LONG_HOLD_DAYS {
        crate::util::plural("Held for {} day", "Held for {} days", days as i64)
    } else {
        t("Held Version")
    };
    let template = match pin.version {
        Some(_) => t("This app stays at version {version} from nixpkgs {rev}, held {age}. It gets no security updates until you release it."),
        None => t("This app stays at nixpkgs {rev}, held {age}. It gets no security updates until you release it."),
    };
    let text = template
        .replace("{version}", pin.version.as_deref().unwrap_or_default())
        .replace("{rev}", pin.rev.get(..7).unwrap_or(&pin.rev))
        .replace(
            "{age}",
            &crate::util::format_relative(pin.since as i64).to_lowercase(),
        );
    let b = warning(&[&heading, &text]);
    let release = gtk::Button::with_mnemonic(&t("_Release"));
    release.add_css_class("pill");
    release.set_tooltip_text(Some(&t("Let the app follow your configuration again.")));
    release.set_halign(gtk::Align::Center);
    release.set_margin_top(6);
    release.connect_clicked(move |_| on_release());
    b.append(&release);
    b
}

/// The app comes from the user's own files: where, and how to hand it over.
pub fn declared(
    file: &std::path::Path,
    line: usize,
    on_take_over: impl Fn() + 'static,
) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.add_css_class("card");
    b.add_css_class("infobox");
    b.add_css_class("info");
    let heading = gtk::Label::new(Some(&t("In Your Configuration")));
    heading.add_css_class("heading");
    b.append(&heading);
    let text = gtk::Label::new(Some(
        &t("{file}, line {line}. Nix Software leaves your files alone. Take the app over to manage it here, then delete that line.")
            .replace("{file}", &file.display().to_string())
            .replace("{line}", &line.to_string()),
    ));
    text.set_justify(gtk::Justification::Center);
    text.set_wrap(true);
    text.set_selectable(true);
    b.append(&text);
    let take = gtk::Button::with_mnemonic(&t("_Take Over"));
    take.add_css_class("pill");
    take.set_tooltip_text(Some(&t(
        "Add the app to the files of Nix Software. Your own line stays until you delete it.",
    )));
    take.set_halign(gtk::Align::Center);
    take.set_margin_top(6);
    take.connect_clicked(move |_| on_take_over());
    b.append(&take);
    b
}

/// The installed version is not the one of nixpkgs: an overlay or another
/// source changed it, so updates leave it alone.
pub fn changed(installed: &str, nixpkgs: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.add_css_class("card");
    b.add_css_class("infobox");
    b.add_css_class("info");
    for (i, text) in [
        t("Changed by Your Configuration"),
        t("Version {installed} is installed, nixpkgs has {nixpkgs}. Your configuration changes this app, for example with an overlay, so Updates leaves it alone.")
            .replace("{installed}", installed)
            .replace("{nixpkgs}", nixpkgs),
    ]
    .iter()
    .enumerate()
    {
        let l = gtk::Label::new(Some(text));
        l.set_justify(gtk::Justification::Center);
        l.set_wrap(true);
        if i == 0 {
            l.add_css_class("heading");
        }
        b.append(&l);
    }
    b
}
