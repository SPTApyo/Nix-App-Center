//! Infobar — calque des blocs `.card .infobox` de `gs-details-page.ui`.
//!
//! `GtkBox vertical .card .infobox .warning` : contenu centré (`justify center`,
//! `wrap`), premier label en `.heading`. GNOME n'y met pas d'icône — ce sont du
//! texte centré, rien d'autre.
//!
//! GNOME en a six (EOL, problèmes, dépôt inclus/absent, paquet du système de
//! base, effacer les données). Une seule a une source de données nixpkgs :
//! `problems` (← `broken` / `insecure` / `known_vulnerabilities`). Les autres ne
//! sont pas construites, faute d'équivalent Nix (cf. spec §3.4).

use gtk::prelude::*;

/// Une infobar d'avertissement. `lines` = le titre (`.heading`) puis les
/// paragraphes suivants, comme les `GtkLabel` empilés du `.ui`.
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

/// L'infobar « problèmes ». `None` si nixpkgs ne signale rien sur ce paquet.
pub fn problems(broken: bool, insecure: bool, vulnerabilities: &[String]) -> Option<gtk::Box> {
    if broken {
        return Some(warning(&[
            "Paquet cassé",
            "nixpkgs marque ce paquet comme broken : le lancement échouera probablement.",
        ]));
    }
    if insecure || !vulnerabilities.is_empty() {
        let detail = if vulnerabilities.is_empty() {
            "Ce paquet est marqué non sécurisé dans nixpkgs.".to_string()
        } else {
            format!(
                "Ce paquet est marqué non sécurisé dans nixpkgs ({}).",
                vulnerabilities.join(", ")
            )
        };
        return Some(warning(&["Vulnérabilités connues", &detail]));
    }
    None
}
