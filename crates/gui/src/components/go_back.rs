//! Going back to an earlier generation: the switch with its progress, then
//! the configuration Nix App Center recorded for it.

use adw::prelude::*;
use nix_app_center_core::{HistoryPoint, HistoryScope};

use crate::i18n::t;

/// "Today, 14:02".
pub fn when(point: &HistoryPoint) -> String {
    let time = gtk::glib::DateTime::from_unix_local(point.date as i64)
        .ok()
        .and_then(|d| d.format("%H:%M").ok())
        .map(|s| s.to_string())
        .unwrap_or_default();
    format!(
        "{}, {time}",
        crate::util::format_relative(point.date as i64)
    )
}

/// Asks, then goes back to `point`; `done` gets the message to show.
pub fn confirm(
    parent: &impl IsA<gtk::Widget>,
    point: HistoryPoint,
    done: impl Fn(String) + 'static,
) {
    let mut body = match &point.kept {
        Some(_) => t("Your system switches to this version, and the apps you added with Nix App Center go back to what they were then."),
        None => t("Your system switches to this version. It was not made by Nix App Center, so your configuration stays as it is: the next apply brings the newer version back."),
    };
    if point.scope == HistoryScope::System {
        body.push_str("\n\n");
        body.push_str(&t("This affects every user of this computer."));
        if nix_app_center_core::needs_restart(&point) {
            body.push(' ');
            body.push_str(&t("It uses another kernel: restart to finish."));
        }
        if let Some(name) = nix_app_center_core::running_specialisation() {
            body.push_str("\n\n");
            body.push_str(
                &t("You are running the {} specialisation: going back starts the default configuration of that version.")
                    .replace("{}", &name),
            );
        }
    }
    let dialog = adw::AlertDialog::new(
        Some(&t("Go Back to {}?").replace("{}", &when(&point))),
        Some(&body),
    );
    dialog.add_response("cancel", &t("_Cancel"));
    dialog.add_response("back", &t("_Go Back"));
    dialog.set_response_appearance("back", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("back"));
    dialog.set_close_response("cancel");
    let parent_widget = parent.clone().upcast::<gtk::Widget>();
    let done = std::rc::Rc::new(done);
    dialog.connect_response(None, move |_, response| {
        if response == "back" {
            let done = done.clone();
            run(&parent_widget, point.clone(), move |m| done(m));
        }
    });
    dialog.present(Some(parent));
}

/// Goes back to `point` without asking: the user chose Undo.
pub fn run(parent: &impl IsA<gtk::Widget>, point: HistoryPoint, done: impl Fn(String) + 'static) {
    let Some(steps) = nix_app_center_core::go_back_steps(&point) else {
        done(t("This version no longer exists."));
        return;
    };
    let message = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let after = {
        let message = message.clone();
        move |res: &Result<nix_app_center_core::ApplyOutcome, String>| {
            if !matches!(res, Ok(nix_app_center_core::ApplyOutcome::Done(_))) {
                return None;
            }
            let restored = nix_app_center_core::restore_history(&point);
            nix_app_center_core::record_history();
            let text = match restored {
                Ok(true) => t("Back to the version of {}.").replace("{}", &when(&point)),
                Ok(false) => t("Back to the version of {}. Your configuration is unchanged.")
                    .replace("{}", &when(&point)),
                Err(e) => crate::util::failed(&e),
            };
            message.replace(text.clone());
            Some(text)
        }
    };
    crate::components::apply_dialog::present_then(
        parent,
        &t("Going Back"),
        steps,
        after,
        move |res| {
            let text = message.borrow().clone();
            done(if text.is_empty() {
                crate::util::describe_apply(res)
            } else {
                text
            });
        },
    );
}
