//! Infobar, a copy of the `.card .infobox` blocks of `gs-details-page.ui`.
//!
//! `GtkBox vertical .card .infobox .warning`: centered content (`justify center`,
//! `wrap`), first label in `.heading`. GNOME puts no icon there, only centered
//! text.
//!
//! GNOME has six of them (EOL, problems, repo included/missing, base system
//! package, clear data). Only one has a nixpkgs data source: `problems`
//! (from `broken` / `insecure` / `known_vulnerabilities`). The others are not
//! built, for lack of a Nix equivalent.

use crate::i18n::t;
use gtk::prelude::*;

/// A warning infobar. `lines` = the title (`.heading`) then the following
/// paragraphs, like the stacked `GtkLabel`s of the `.ui`.
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

/// The "problems" infobar. `None` when nixpkgs reports nothing for this package.
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
