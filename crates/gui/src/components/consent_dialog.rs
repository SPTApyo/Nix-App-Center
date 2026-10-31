//! Asks before keeping an app nixpkgs refuses without the user's consent:
//! a proprietary app, then an app marked insecure.

use adw::prelude::*;
use nix_app_center_core::Allowance;

use crate::i18n::t;

/// Asks about the first thing `missing` needs; `on_allow` gets what the
/// user allowed. Nothing happens when the user cancels.
pub fn present(
    parent: &impl IsA<gtk::Widget>,
    name: &str,
    missing: Allowance,
    vulnerabilities: &str,
    on_allow: impl FnOnce(Allowance) + 'static,
) {
    let dialog = if missing.unfree {
        let d = adw::AlertDialog::new(
            Some(&t("Allow a Proprietary App?")),
            Some(&t(
                "{} is proprietary software: its source code is not public. Nix installs it only with your permission.",
            )
            .replace("{}", name)),
        );
        d.add_response("cancel", &t("_Cancel"));
        d.add_response("all", &t("Allow All _Proprietary Apps"));
        d.add_response("one", &t("_Allow This App"));
        d.set_response_appearance("one", adw::ResponseAppearance::Suggested);
        d.set_default_response(Some("one"));
        d
    } else {
        let mut body = t("nixpkgs marks {} as insecure. Install it only if you know the risk.")
            .replace("{}", name);
        if !vulnerabilities.is_empty() {
            body.push_str("\n\n");
            body.push_str(&t("Known vulnerabilities: {}").replace("{}", vulnerabilities));
        }
        let d = adw::AlertDialog::new(Some(&t("Install an Insecure App?")), Some(&body));
        d.add_response("cancel", &t("_Cancel"));
        d.add_response("insecure", &t("_Install Anyway"));
        d.set_response_appearance("insecure", adw::ResponseAppearance::Destructive);
        d.set_default_response(Some("cancel"));
        d
    };
    dialog.set_close_response("cancel");
    let on_allow = std::cell::Cell::new(Some(on_allow));
    dialog.connect_response(None, move |_, response| {
        let allow = match response {
            "all" => {
                let mut s = nix_app_center_core::load_settings();
                s.allow_all_unfree = true;
                let _ = nix_app_center_core::save_settings(&s);
                Allowance {
                    unfree: true,
                    insecure: false,
                }
            }
            "one" => Allowance {
                unfree: true,
                insecure: false,
            },
            "insecure" => Allowance {
                unfree: false,
                insecure: true,
            },
            _ => return,
        };
        if let Some(f) = on_allow.take() {
            f(allow);
        }
    });
    dialog.present(Some(parent));
}
