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

/// The NixOS module infobar. Without `on_use`, only "Details" shows.
pub fn module(
    option: &str,
    on_use: Option<Box<dyn Fn()>>,
    on_details: impl Fn() + 'static,
) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.add_css_class("card");
    b.add_css_class("infobox");
    b.add_css_class("info");
    let heading = gtk::Label::new(Some(&t("NixOS Module Available")));
    heading.add_css_class("heading");
    b.append(&heading);
    let text = gtk::Label::new(Some(
        &t("{} sets up this app with what it needs around it, such as services, groups or firewall rules. Prefer it to the plain package.")
            .replace("{}", option),
    ));
    text.set_justify(gtk::Justification::Center);
    text.set_wrap(true);
    b.append(&text);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    buttons.set_halign(gtk::Align::Center);
    buttons.set_margin_top(6);
    if let Some(on_use) = on_use {
        let use_btn = gtk::Button::with_mnemonic(&t("_Use the Module"));
        use_btn.add_css_class("suggested-action");
        use_btn.add_css_class("pill");
        use_btn.connect_clicked(move |_| on_use());
        buttons.append(&use_btn);
    }
    let details = gtk::Button::with_mnemonic(&t("_Details"));
    details.add_css_class("pill");
    details.connect_clicked(move |_| on_details());
    buttons.append(&details);
    b.append(&buttons);
    b
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
    release.set_halign(gtk::Align::Center);
    release.set_margin_top(6);
    release.connect_clicked(move |_| on_release());
    b.append(&release);
    b
}
