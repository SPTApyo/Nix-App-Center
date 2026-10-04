//! Review row, a copy of `gs-review-row.ui`.
//!
//! `GtkListBoxRow .review-row` holding a vertical `GtkBox` (spacing 6):
//!  1. `GtkBox spacing 10`: stars, `.heading` summary (ellipsize, hexpand),
//!     `.dim-label` date aligned right;
//!  2. `.dim-label` author, `ellipsize end`;
//!  3. review text (`wrap`, `max-width-chars 80`, margins 10/8).
//!
//! The vote / report / delete buttons of the `.ui` are out of scope: they
//! need an ODRS account, and Onyx is read only.

use gtk::prelude::*;
use onyx_core::Review;

use crate::util::{format_relative, stars_widget};

/// The featured review, non activatable (like the right column `GsReviewRow`).
pub fn row(review: &Review) -> gtk::ListBoxRow {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 6);

    // Line 1: stars + summary + date.
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

    // Line 2: author.
    if let Some(author) = review.author.as_deref() {
        let a = gtk::Label::new(Some(author));
        a.set_xalign(0.0);
        a.set_ellipsize(gtk::pango::EllipsizeMode::End);
        a.add_css_class("dim-label");
        vb.append(&a);
    }

    // Line 3: the text.
    let text = gtk::Label::new(Some(&review.description));
    text.set_xalign(0.0);
    text.set_wrap(true);
    text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    text.set_max_width_chars(80);
    text.set_margin_top(10);
    text.set_margin_bottom(8);
    vb.append(&text);

    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    row.add_css_class("review-row");
    row.set_child(Some(&vb));
    row
}
