//! Carousel de screenshots — calque de `gs-screenshot-carousel.ui`.
//!
//! ```text
//! GtkStack
//! ├─ "carousel" → GtkBox vertical .frame .view
//! │                ├─ GtkOverlay
//! │                │   ├─ AdwCarousel (vexpand)
//! │                │   ├─ overlay: GtkRevealer halign start ─ bouton ‹ (.osd .circular)
//! │                │   └─ overlay: GtkRevealer halign end   ─ bouton › (.osd .circular)
//! │                └─ AdwCarouselIndicatorDots
//! └─ "fallback"  → GtkBox vertical .screenshot-image .frame .view (h 423)
//!                   ├─ GtkImage image-missing-symbolic 96
//!                   └─ GtkLabel « Aucune capture d'écran » .title-2 .dim-label
//! ```
//!
//! Les révélateurs suivent la position, exactement comme
//! `gs_screenshot_carousel_update_buttons` : ‹ si `position >= 0.5`,
//! › si `position < n_pages - 1.5`.

use adw::prelude::*;


/// Hauteur de la page de repli. Choisie par GNOME pour se juxtaposer en 16:9.
const FALLBACK_HEIGHT: i32 = 423;
/// Côté des boutons de navigation (`.ui` : width/height-request 64, marges 9).
const NAV_BUTTON_SIZE: i32 = 64;
const NAV_BUTTON_MARGIN: i32 = 9;

pub struct ScreenshotCarousel {
    root: gtk::Stack,
    carousel: adw::Carousel,
}

impl ScreenshotCarousel {
    pub fn new() -> Self {
        let carousel = adw::Carousel::new();
        carousel.set_vexpand(true);
        // GNOME le désactive : la page détail est elle-même scrollable.
        carousel.set_allow_scroll_wheel(false);

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&carousel));

        let prev_rev = nav_revealer(gtk::Align::Start, "go-previous-symbolic");
        let next_rev = nav_revealer(gtk::Align::End, "go-next-symbolic");
        overlay.add_overlay(&prev_rev);
        overlay.add_overlay(&next_rev);

        connect_nav(&carousel, &prev_rev, true);
        connect_nav(&carousel, &next_rev, false);

        let dots = adw::CarouselIndicatorDots::new();
        dots.set_carousel(Some(&carousel));

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.add_css_class("frame");
        page.add_css_class("view");
        page.add_css_class("screenshot-carousel");
        page.append(&overlay);
        page.append(&dots);

        let root = gtk::Stack::new();
        root.add_named(&page, Some("carousel"));
        root.add_named(&fallback_page(), Some("fallback"));
        root.set_visible_child_name("fallback");

        // Révélateurs et dots se recalculent à chaque changement de position.
        let update = {
            let carousel = carousel.clone();
            let prev_rev = prev_rev.clone();
            let next_rev = next_rev.clone();
            move || {
                let pos = carousel.position();
                let n = carousel.n_pages() as f64;
                prev_rev.set_reveal_child(pos >= 0.5);
                next_rev.set_reveal_child(pos < n - 1.5);
            }
        };
        let u1 = update.clone();
        carousel.connect_position_notify(move |_| u1());
        let u2 = update.clone();
        carousel.connect_n_pages_notify(move |_| u2());
        update();

        Self { root, carousel }
    }

    pub fn widget(&self) -> &gtk::Stack {
        &self.root
    }

    /// Ajoute une page. La première bascule le stack sur le carousel.
    pub fn append(&self, child: &impl IsA<gtk::Widget>) {
        self.carousel.append(child);
        self.root.set_visible_child_name("carousel");
    }

    /// Aucune capture disponible → page de repli (« Aucune capture d'écran »).
    pub fn show_fallback(&self) {
        self.root.set_visible_child_name("fallback");
    }
}

fn nav_revealer(align: gtk::Align, icon: &str) -> gtk::Revealer {
    let btn = gtk::Button::from_icon_name(icon);
    btn.set_width_request(NAV_BUTTON_SIZE);
    btn.set_height_request(NAV_BUTTON_SIZE);
    btn.set_margin_top(NAV_BUTTON_MARGIN);
    btn.set_margin_bottom(NAV_BUTTON_MARGIN);
    btn.set_margin_start(NAV_BUTTON_MARGIN);
    btn.set_margin_end(NAV_BUTTON_MARGIN);
    btn.add_css_class("circular");
    btn.add_css_class("image-button");
    btn.add_css_class("osd");
    btn.set_tooltip_text(Some(if align == gtk::Align::Start {
        "Capture précédente"
    } else {
        "Capture suivante"
    }));

    let rev = gtk::Revealer::new();
    rev.set_halign(align);
    rev.set_valign(gtk::Align::Center);
    rev.set_transition_type(gtk::RevealerTransitionType::Crossfade);
    rev.set_child(Some(&btn));
    rev
}

/// Fait défiler d'une page au clic. `backwards` = bouton ‹.
fn connect_nav(carousel: &adw::Carousel, revealer: &gtk::Revealer, backwards: bool) {
    let Some(btn) = revealer.child().and_downcast::<gtk::Button>() else {
        return;
    };
    let carousel = carousel.clone();
    btn.connect_clicked(move |_| {
        let target = if backwards {
            carousel.position() - 1.0
        } else {
            carousel.position() + 1.0
        };
        let n = carousel.n_pages();
        let idx = target.round().clamp(0.0, n.saturating_sub(1) as f64) as u32;
        carousel.scroll_to(&carousel.nth_page(idx), true);
    });
}

fn fallback_page() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 12);
    b.set_height_request(FALLBACK_HEIGHT);
    b.set_hexpand(true);
    b.set_halign(gtk::Align::Fill);
    b.add_css_class("screenshot-image");
    b.add_css_class("frame");
    b.add_css_class("view");

    let img = gtk::Image::from_icon_name("image-missing-symbolic");
    img.set_pixel_size(96);
    img.set_valign(gtk::Align::End);
    img.set_vexpand(true);
    img.add_css_class("dim-label");
    b.append(&img);

    let l = gtk::Label::new(Some("Aucune capture d'écran"));
    l.set_halign(gtk::Align::Center);
    l.set_valign(gtk::Align::Start);
    l.set_vexpand(true);
    l.add_css_class("title-2");
    l.add_css_class("dim-label");
    b.append(&l);
    b
}
