//! A review: stars, summary, date, author, text, and votes.

use adw::prelude::*;
use nix_software_core::{Review, ReviewAction};

use crate::i18n::t;
use crate::util::{format_relative, stars_widget};

/// The row of `review`.
pub fn row(review: &Review) -> gtk::ListBoxRow {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 6);

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    head.append(&stars_widget(review.stars, 16));

    let summary = gtk::Label::new(Some(&review.summary));
    summary.set_xalign(0.0);
    summary.set_hexpand(true);
    summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
    summary.add_css_class("heading");
    head.append(&summary);

    let date = gtk::Label::new(Some(&format_relative(review.timestamp)));
    date.set_halign(gtk::Align::End);
    date.add_css_class("dim-label");
    head.append(&date);
    vb.append(&head);

    let used = match (review.distro.is_empty(), review.version.is_empty()) {
        (false, false) => t("on {distro}, version {version}")
            .replace("{distro}", &review.distro)
            .replace("{version}", &review.version),
        (false, true) => t("on {}").replace("{}", &review.distro),
        _ => String::new(),
    };
    let byline = [review.author.clone().unwrap_or_default(), used]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    if !byline.is_empty() {
        let a = gtk::Label::new(Some(&byline));
        a.set_xalign(0.0);
        a.set_ellipsize(gtk::pango::EllipsizeMode::End);
        a.add_css_class("dim-label");
        vb.append(&a);
    }

    let text = gtk::Label::new(Some(&review.description));
    text.set_xalign(0.0);
    text.set_wrap(true);
    text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    text.set_max_width_chars(80);
    text.set_margin_top(10);
    text.set_margin_bottom(8);
    vb.append(&text);
    vb.append(&feedback(review));

    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.add_css_class("review-row");
    row.set_child(Some(&vb));
    row
}

/// "Was this review useful?" with votes, Yes, No and Report.
fn feedback(review: &Review) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let question = gtk::Label::new(Some(&if review.karma_up > 0 {
        crate::util::plural(
            "{} person found this useful. Was it useful to you?",
            "{} people found this useful. Was it useful to you?",
            review.karma_up,
        )
    } else {
        t("Was this review useful to you?")
    }));
    question.add_css_class("dim-label");
    question.add_css_class("caption");
    question.set_xalign(0.0);
    question.set_hexpand(true);
    question.set_wrap(true);
    bar.append(&question);
    for (label, action) in [
        (t("Yes"), ReviewAction::Useful),
        (t("No"), ReviewAction::NotUseful),
        (t("Report…"), ReviewAction::Report),
    ] {
        let b = gtk::Button::with_label(&label);
        b.add_css_class("flat");
        b.add_css_class("caption");
        let (review, bar_ref, question) = (review.clone(), bar.clone(), question.clone());
        b.connect_clicked(move |btn| {
            if action == ReviewAction::Report {
                confirm_report(btn, review.clone(), bar_ref.clone(), question.clone());
            } else {
                send(review.clone(), action, bar_ref.clone(), question.clone());
            }
        });
        bar.append(&b);
    }
    bar
}

fn confirm_report(parent: &gtk::Button, review: Review, bar: gtk::Box, question: gtk::Label) {
    let dialog = adw::AlertDialog::new(
        Some(&t("Report This Review?")),
        Some(&t(
            "Report reviews that are abusive, rude or discriminatory. Moderators will check it.",
        )),
    );
    dialog.add_response("cancel", &t("_Cancel"));
    dialog.add_response("report", &t("_Report"));
    dialog.set_response_appearance("report", adw::ResponseAppearance::Destructive);
    dialog.connect_response(None, move |_, r| {
        if r == "report" {
            send(
                review.clone(),
                ReviewAction::Report,
                bar.clone(),
                question.clone(),
            );
        }
    });
    dialog.present(Some(parent));
}

/// Sends `action`, then shows its outcome instead of the buttons.
fn send(review: Review, action: ReviewAction, bar: gtk::Box, question: gtk::Label) {
    let mut child = question.next_sibling();
    while let Some(w) = child {
        child = w.next_sibling();
        w.set_sensitive(false);
    }
    gtk::glib::spawn_future_local(async move {
        let res =
            gtk::gio::spawn_blocking(move || nix_software_core::act_on_review(&review, action))
                .await
                .unwrap_or_else(|_| Err(String::new()));
        let mut child = question.next_sibling();
        while let Some(w) = child {
            child = w.next_sibling();
            bar.remove(&w);
        }
        question.set_label(&match res {
            Ok(()) if action == ReviewAction::Report => t("Thanks, the review was reported."),
            Ok(()) => t("Thanks for your feedback."),
            Err(e) => crate::util::failed(&e),
        });
    });
}
