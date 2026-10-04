//! Histogramme des avis — calque de `gs-review-histogram.ui`.
//!
//! `GtkGrid` (row/column-spacing 6, marges 12) :
//! ```text
//!  col 0                     col 1
//! ┌─────────────┬──────────────────────────┐
//! │             │  ★ ★ ★ ★ ☆   (24px)      │  row 0
//! │    4,2      │  sur 5 étoiles (scale .8)│
//! ├─────────────┴──────────────────────────┤
//! │  grid2 .review-histogram (spacing 2)   │  row 1, colspan 2
//! │   ▬▬▬▬▬▬▬▬▬▬▬▬ ★★★★★                   │
//! │   ▬▬▬▬▬▬▬     ★★★★                     │
//! │   ▬▬▬         ★★★                      │
//! │   ▬           ★★                       │
//! │   ▬           ★                        │
//! ├────────────────────────────────────────┤
//! │  128 appréciations au total            │  row 2, colspan 2
//! └────────────────────────────────────────┘
//! ```
//! Dans `grid2`, la barre est en colonne 0 (hexpand, `margin-end 5`,
//! `height-request 6`) et les étoiles occupent les colonnes 1..niveau — le
//! nombre d'étoiles décroît donc de ligne en ligne, barres alignées à gauche.
//!
//! `GsReviewBar` est un widget de dessin custom ; on le rend avec une
//! `GtkProgressBar` stylée (`.review-histogram-bar`), rendu identique.

use gtk::prelude::*;
use onyx_core::Rating;

use crate::util::stars_widget;

/// L'histogramme complet, prêt à être posé dans une `GtkListBoxRow`.
pub fn histogram(rating: &Rating) -> gtk::Grid {
    let grid = gtk::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(6);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);

    // (0,0) — la moyenne, en très gros.
    let value = gtk::Label::new(Some(&format!("{:.1}", rating.stars).replace('.', ",")));
    value.set_halign(gtk::Align::Start);
    value.set_valign(gtk::Align::Center);
    value.add_css_class("review-average");
    grid.attach(&value, 0, 0, 1, 1);

    // (1,0) — cinq étoiles 24px + « sur 5 étoiles ».
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
    vbox.set_halign(gtk::Align::Start);
    vbox.set_valign(gtk::Align::Center);
    vbox.set_hexpand(true);
    vbox.set_vexpand(true);
    vbox.set_margin_start(6);
    vbox.set_margin_top(8);

    let stars = stars_widget(rating.stars, 24);
    stars.set_spacing(6);
    stars.add_css_class("reviewstarvalue");
    vbox.append(&stars);

    let out_of = gtk::Label::new(Some("sur 5 étoiles"));
    out_of.set_halign(gtk::Align::Start);
    out_of.set_valign(gtk::Align::Center);
    out_of.add_css_class("caption");
    vbox.append(&out_of);
    grid.attach(&vbox, 1, 0, 1, 1);

    // (0,1) sur deux colonnes — les cinq barres.
    grid.attach(&bars(rating), 0, 1, 2, 1);

    // (0,2) sur deux colonnes — le total.
    let total = gtk::Label::new(Some(&format!("{} appréciations au total", rating.total)));
    total.set_halign(gtk::Align::Start);
    total.set_valign(gtk::Align::Center);
    total.set_hexpand(true);
    total.set_margin_top(6);
    grid.attach(&total, 0, 2, 2, 1);

    grid
}

fn bars(rating: &Rating) -> gtk::Grid {
    let g = gtk::Grid::new();
    g.set_row_spacing(2);
    g.set_column_spacing(2);
    g.add_css_class("review-histogram");

    let counts = [rating.star5, rating.star4, rating.star3, rating.star2, rating.star1];
    // Barres normalisées sur la note la plus fréquente, comme GsReviewBar.
    let max = counts.iter().copied().max().unwrap_or(0).max(1);

    for (row, count) in counts.iter().enumerate() {
        let level = 5 - row as i32;

        let bar = gtk::ProgressBar::new();
        bar.set_fraction(*count as f64 / max as f64);
        bar.set_halign(gtk::Align::Fill);
        bar.set_hexpand(true);
        bar.set_valign(gtk::Align::Center);
        bar.set_height_request(6);
        bar.set_margin_end(5);
        bar.add_css_class("review-histogram-bar");
        g.attach(&bar, 0, row as i32, 1, 1);

        for col in 1..=level {
            let star = gtk::Image::from_icon_name("starred-symbolic");
            star.set_pixel_size(10);
            star.set_sensitive(false);
            star.add_css_class("star-off");
            g.attach(&star, col, row as i32, 1, 1);
        }
    }
    g
}
