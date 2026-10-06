//! Helpers shared by views and components (icons, textures, row setup).

use crate::i18n::t;
use gettextrs::ngettext;
use onyx_core::Package;

/// Shows a simple toast on an `AdwToastOverlay`.
pub fn toast(overlay: &adw::ToastOverlay, text: &str) {
    overlay.add_toast(adw::Toast::new(text));
}

/// Puts `text` on the clipboard. The core never touches it (it knows nothing of
/// GTK): the front end decides what to do when an action cannot complete alone.
pub fn copy_to_clipboard(text: &str) {
    if let Some(display) = gtk::gdk::Display::default() {
        display.clipboard().set_text(text);
    }
}

/// Turns a config apply result into a displayable message, and performs the side
/// effect owned by the front end (copying the command when Onyx could not run it
/// itself). Shared by the details page and the updates view.
pub fn describe_apply(res: Result<onyx_core::ApplyOutcome, String>) -> String {
    use onyx_core::ApplyOutcome::*;
    match res {
        Ok(Done(line)) if line.is_empty() => t("Configuration applied."),
        Ok(Done(line)) => t("Configuration applied: {}").replace("{}", &line),
        Ok(Launched) => t("Applying in a terminal. Follow the build there."),
        Ok(NoTerminal(argv)) => {
            copy_to_clipboard(&argv.join(" "));
            t("No terminal found. The apply command was copied.")
        }
        Ok(UnknownLayout) => t("Onyx cannot find your Nix configuration. Nothing was applied."),
        // gettext("") returns the catalog header.
        Err(e) if e.is_empty() => t("Failed: {}").replace("{}", ""),
        Err(e) => t("Failed: {}").replace("{}", &t(&e)),
    }
}

/// Decodes image bytes (PNG/JPEG…) into a GTK texture. Call on the main thread
/// (texture creation). `None` when decoding fails.
pub fn texture_from_bytes(data: &[u8]) -> Option<gtk::gdk::Texture> {
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(data.to_vec())).ok()?;
    Some(gtk::gdk::Texture::for_pixbuf(&pixbuf))
}

/// Same, resized to `px`×`px`. Required for tile icons: a `gtk::Picture`
/// requests the image intrinsic size (128/256px), which would widen the tiles
/// (and, in a homogeneous FlowBox, every tile), so the texture is bounded to
/// the wanted size.
pub fn texture_from_bytes_sized(data: &[u8], px: i32) -> Option<gtk::gdk::Texture> {
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(data.to_vec())).ok()?;
    let scaled = pixbuf
        .scale_simple(px, px, gtk::gdk_pixbuf::InterpType::Bilinear)
        .unwrap_or(pixbuf);
    Some(gtk::gdk::Texture::for_pixbuf(&scaled))
}

use adw::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

// ── Smooth wheel scrolling ────────────────────────────────────────────────────

/// GTK4 scrolls the mouse wheel in **discrete steps** (no animation), which
/// looks jarring. Wheel events are intercepted and the vertical adjustment is
/// animated with a libadwaita `SpringAnimation` for smooth inertial scrolling.
/// Touchpads and touch screens (already smooth natively) keep the default
/// behavior.
pub fn smooth_scroll(scroller: &gtk::ScrolledWindow) {
    let vadj = scroller.vadjustment();

    // Animation target: push the value into the adjustment every frame.
    let cb = adw::CallbackAnimationTarget::new({
        let vadj = vadj.clone();
        move |v| vadj.set_value(v)
    });
    // damping_ratio=1.0 (critically damped, no bounce), mass=1, stiffness=400.
    let params = adw::SpringParams::new(1.0, 1.0, 400.0);
    let anim = adw::SpringAnimation::new(scroller, 0.0, 0.0, params, cb);
    anim.set_clamp(true); // never overshoot the bounds
    anim.set_epsilon(0.1);

    let target = Rc::new(Cell::new(0f64));
    let ctrl = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    ctrl.connect_scroll(move |c, _dx, dy| {
        // Touchpads and touch screens keep native scrolling.
        if let Some(ev) = c.current_event() {
            if let Some(dev) = ev.device() {
                if dev.source() != gtk::gdk::InputSource::Mouse {
                    return gtk::glib::Propagation::Proceed;
                }
            }
        }

        let page = vadj.page_size();
        let max = (vadj.upper() - page).max(vadj.lower());
        // While animating, chain from its target (steps add up);
        // otherwise start from the real position.
        let playing = anim.state() == adw::AnimationState::Playing;
        let base = if playing { target.get() } else { vadj.value() };
        let step = (page * 0.16).clamp(48.0, 180.0); // a few lines per step
        let to = (base + dy * step).clamp(vadj.lower(), max);

        target.set(to);
        anim.set_value_from(vadj.value());
        anim.set_value_to(to);
        anim.play();
        gtk::glib::Propagation::Stop
    });
    scroller.add_controller(ctrl);
}

// ── Skeletons (animated placeholders while loading) ───────────────────────────

fn skeleton_box(w: i32, h: i32) -> gtk::Box {
    let b = gtk::Box::builder()
        .width_request(w)
        .height_request(h)
        .build();
    b.add_css_class("skeleton");
    b
}

/// Skeleton row content, mirroring `PackageRow` EXACTLY
/// (64 px icon, same margins and spacing) so the layout never jumps.
fn skeleton_row() -> gtk::Box {
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .spacing(6)
        .hexpand(true)
        .build();
    text.append(&skeleton_box(180, 15));
    text.append(&skeleton_box(260, 12));

    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();
    row.append(&skeleton_box(64, 64));
    row.append(&text);
    row
}

/// A skeleton tile, mirroring `AppTile` EXACTLY
/// (card, width 190, padding 17, gap 14, 64 px icon).
pub fn skeleton_tile() -> gtk::Box {
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .spacing(6)
        .build();
    text.append(&skeleton_box(90, 16));
    text.append(&skeleton_box(120, 13));
    text.append(&skeleton_box(80, 13));

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(14)
        .margin_top(17)
        .margin_bottom(17)
        .margin_start(17)
        .margin_end(17)
        .build();
    inner.append(&skeleton_box(64, 64));
    inner.append(&text);

    let tile = gtk::Box::builder()
        .width_request(190)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Start)
        .build();
    tile.add_css_class("card");
    tile.append(&inner);
    tile
}

/// Skeleton list in a `boxed-list` (same container as the real lists).
pub fn skeleton_list(n: usize) -> gtk::ListBox {
    let lb = boxed_list();
    for _ in 0..n {
        lb.append(&skeleton_row());
    }
    lb
}

/// Skeleton grid, same FlowBox config as the real grids (3 columns).
pub fn skeleton_grid(n: usize) -> gtk::FlowBox {
    let f = gtk::FlowBox::builder()
        .homogeneous(true)
        .column_spacing(14)
        .row_spacing(14)
        .min_children_per_line(1)
        .max_children_per_line(3)
        .selection_mode(gtk::SelectionMode::None)
        .hexpand(true)
        .valign(gtk::Align::Start)
        .build();
    for _ in 0..n {
        f.append(&skeleton_tile());
    }
    f
}

/// Icon display state (shimmer placeholder, then real or generic icon).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IconState {
    /// Download in progress: show the animated shimmer.
    Loading,
    /// No AppStream icon: generic theme icon.
    Generic,
    /// Real icon loaded.
    Real,
}

/// Primary screen metrics: `(physical_width_px, scale_factor)`.
/// Physical width = logical width × scale. Falls back to `(1920, 1)` without a screen.
pub(crate) fn screen_metrics() -> (i32, i32) {
    if let Some(display) = gtk::gdk::Display::default() {
        if let Some(obj) = display.monitors().item(0) {
            if let Ok(m) = obj.downcast::<gtk::gdk::Monitor>() {
                let scale = m.scale_factor().max(1);
                let w = m.geometry().width().max(1);
                return (w * scale, scale);
            }
        }
    }
    (1920, 1)
}

/// Escapes text for widgets that parse Pango markup (`adw::ActionRow`
/// title/subtitle), otherwise a `&` breaks parsing.
pub fn esc(s: &str) -> String {
    gtk::glib::markup_escape_text(s).to_string()
}

/// Row for an app of a Nix profile, named and drawn from its desktop file.
/// `None` for entries hidden from menus.
pub fn desktop_row(desktop_file: &std::path::Path, subtitle: &str) -> Option<adw::ActionRow> {
    use gtk::gio::prelude::AppInfoExt;
    let info = gtk::gio::DesktopAppInfo::from_filename(desktop_file)?;
    if !info.should_show() {
        return None;
    }
    let row = adw::ActionRow::builder()
        .title(esc(&info.name()))
        .subtitle(esc(subtitle))
        .build();
    adw::prelude::ActionRowExt::add_prefix(&row, &row_icon(info.icon().as_ref()));
    Some(row)
}

/// 32 px app icon for list rows, generic when `icon` is missing.
pub fn row_icon(icon: Option<&gtk::gio::Icon>) -> gtk::Image {
    let img = match icon {
        Some(i) => gtk::Image::from_gicon(i),
        None => gtk::Image::from_icon_name("application-x-executable"),
    };
    img.set_pixel_size(32);
    img
}

/// A row of 5 stars (copy of `GsStarWidget`): filled up to the rounded
/// average, empty after.
pub fn stars_widget(stars: f32, size: i32) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    b.add_css_class("app-stars");
    let filled = stars.round().clamp(0.0, 5.0) as i32;
    for i in 0..5 {
        let img = gtk::Image::from_icon_name("starred-symbolic");
        img.set_pixel_size(size);
        img.add_css_class(if i < filled { "star-on" } else { "star-off" });
        b.append(&img);
    }
    b
}

/// Relative date ("Today", "2 weeks ago"…), like GNOME which shows age
/// rather than a bare date. `ts` is epoch seconds; 0 or negative gives an
/// empty string.
pub fn format_relative(ts: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_relative_from(ts, now)
}

/// Testable core of [`format_relative`]: "now" is a parameter.
fn format_relative_from(ts: i64, now: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    let days = (now - ts).max(0) / 86_400;
    match days {
        0 => t("Today"),
        1 => t("Yesterday"),
        2..=6 => plural("{} day ago", "{} days ago", days),
        7..=29 => plural("{} week ago", "{} weeks ago", days / 7),
        30..=364 => plural("{} month ago", "{} months ago", days / 30),
        _ => plural("{} year ago", "{} years ago", days / 365),
    }
}

/// `ngettext` with the count substituted for `{}`.
pub fn plural(one: &str, many: &str, n: i64) -> String {
    ngettext(one, many, n.clamp(0, u32::MAX as i64) as u32).replace("{}", &n.to_string())
}

#[cfg(test)]
mod tests {
    use super::format_relative_from;

    /// Arbitrary `now`; `ts` is moved N days back.
    fn ago(days: i64) -> String {
        const NOW: i64 = 1_800_000_000;
        format_relative_from(NOW - days * 86_400, NOW)
    }

    #[test]
    fn relative_dates_at_boundaries() {
        assert_eq!(ago(0), "Today");
        assert_eq!(ago(1), "Yesterday");
        assert_eq!(ago(6), "6 days ago");
        assert_eq!(ago(7), "1 week ago");
        assert_eq!(ago(13), "1 week ago");
        assert_eq!(ago(14), "2 weeks ago");
        assert_eq!(ago(29), "4 weeks ago");
        assert_eq!(ago(30), "1 month ago");
        assert_eq!(ago(60), "2 months ago");
        assert_eq!(ago(365), "1 year ago");
        assert_eq!(ago(730), "2 years ago");
    }

    #[test]
    fn unknown_timestamp_is_empty() {
        assert_eq!(format_relative_from(0, 1_800_000_000), "");
        assert_eq!(format_relative_from(-5, 1_800_000_000), "");
    }

    #[test]
    fn future_timestamp_is_today() {
        // A release dated in the future must not give a negative count.
        assert_eq!(format_relative_from(1_900_000_000, 1_800_000_000), "Today");
    }

    #[test]
    fn app_item_from_search_preserves_recognized_without_icon() {
        // An app recognized by AppStream but without icon must stay an
        // "Application" (the search split reads `recognized`).
        let r = onyx_core::SearchResult {
            package: onyx_core::Package::from_attr("gedit"),
            icon_url: None,
            rating: None,
            recognized: true,
        };
        let item = super::AppItem::from_search(r);
        assert!(item.recognized);
        assert!(item.icon_url.is_none());
    }
}

/// Fallback icon (no AppStream icon): a glyph and a color class per family
/// of package, so lists of unknown packages stay tellable apart at a glance.
/// Colors live in `style.css` (`.kind-*`).
pub fn fallback_icon(pkg: &Package) -> (&'static str, &'static str) {
    use onyx_core::IconKind::*;
    match pkg.icon_kind() {
        Game => ("applications-games-symbolic", "kind-game"),
        Audio => ("audio-x-generic-symbolic", "kind-audio"),
        Video => ("video-x-generic-symbolic", "kind-video"),
        Graphics => ("applications-graphics-symbolic", "kind-graphics"),
        Office => ("x-office-document-symbolic", "kind-office"),
        Development => ("applications-engineering-symbolic", "kind-development"),
        Terminal => ("utilities-terminal-symbolic", "kind-terminal"),
        Internet => ("web-browser-symbolic", "kind-internet"),
        Security => ("security-high-symbolic", "kind-security"),
        Science => ("applications-science-symbolic", "kind-science"),
        Education => ("accessories-dictionary-symbolic", "kind-education"),
        System => ("emblem-system-symbolic", "kind-system"),
        Font => ("font-x-generic-symbolic", "kind-font"),
        Theme => ("preferences-desktop-appearance-symbolic", "kind-theme"),
        Library => ("application-x-addon-symbolic", "kind-library"),
        Generic => ("package-x-generic-symbolic", "kind-generic"),
    }
}

/// Makes a widget outside lists fade in, `index` steps after the page shows.
/// Rows of boxed lists and grid items get it from the stylesheet already.
pub fn appear(widget: &impl gtk::prelude::IsA<gtk::Widget>, index: usize) {
    use gtk::prelude::WidgetExt;
    const DELAYS: [&str; 12] = [
        "appear-0",
        "appear-1",
        "appear-2",
        "appear-3",
        "appear-4",
        "appear-5",
        "appear-6",
        "appear-7",
        "appear-8",
        "appear-9",
        "appear-10",
        "appear-11",
    ];
    widget.add_css_class("appear");
    widget.add_css_class(DELAYS[index.min(DELAYS.len() - 1)]);
}

/// The app's boxed list: rounded card of rows, no selection.
pub fn boxed_list() -> gtk::ListBox {
    use gtk::prelude::WidgetExt;
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_valign(gtk::Align::Start);
    list
}

/// Wrapped, dimmed text, left aligned.
pub fn dim_label(text: &str) -> gtk::Label {
    use gtk::prelude::WidgetExt;
    let l = gtk::Label::new(Some(text));
    l.add_css_class("dim-label");
    l.set_wrap(true);
    l.set_xalign(0.0);
    l
}

/// A page section: title, optional explanation, then the content.
pub fn section(
    title: &str,
    subtitle: Option<&str>,
    child: &impl gtk::prelude::IsA<gtk::Widget>,
) -> gtk::Box {
    use gtk::prelude::{BoxExt, WidgetExt};
    let sec = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
    titles.set_hexpand(true);
    let h = gtk::Label::new(Some(title));
    h.add_css_class("title-2");
    h.set_xalign(0.0);
    titles.append(&h);
    if let Some(s) = subtitle {
        titles.append(&dim_label(s));
    }
    head.append(&titles);
    sec.append(&head);
    sec.append(child);
    sec
}

/// Small fallback tile for list rows, `px` square.
pub fn fallback_badge(pkg: &Package, px: i32) -> gtk::Widget {
    use gtk::prelude::*;
    let (icon, class) = fallback_icon(pkg);
    let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tile.set_size_request(px, px);
    tile.set_valign(gtk::Align::Center);
    tile.set_halign(gtk::Align::Start);
    tile.set_hexpand(false);
    tile.add_css_class("fallback-tile");
    tile.add_css_class("small");
    tile.add_css_class(class);
    let glyph = gtk::Image::from_icon_name(icon);
    glyph.set_pixel_size(px * 5 / 8);
    glyph.set_size_request(px, px);
    glyph.add_css_class("fallback-glyph");
    tile.append(&glyph);
    tile.upcast()
}

/// A displayable app: the nix package + an optional Flathub icon URL (set for
/// category/popular, `None` for text search) + an optional ODRS rating (set
/// only by search, like GNOME).
/// Shared data unit of lists (`PackageRow`) and grids (`AppTile`).
#[derive(Debug, Clone)]
pub struct AppItem {
    pub package: Package,
    pub icon_url: Option<String>,
    pub rating: Option<onyx_core::Rating>,
    /// Matched to an AppStream entry: the criterion of the search "Applications"
    /// section (a recognized app without icon is still an application).
    pub recognized: bool,
}

impl AppItem {
    pub fn new(package: Package, icon_url: Option<String>) -> Self {
        // Home/category grids: everything comes from Flathub, so an icon
        // means recognition (the search split does not apply here).
        let recognized = icon_url.is_some();
        Self {
            package,
            icon_url,
            rating: None,
            recognized,
        }
    }

    pub fn from_search(r: onyx_core::SearchResult) -> Self {
        Self {
            package: r.package,
            icon_url: r.icon_url,
            rating: r.rating,
            recognized: r.recognized,
        }
    }
}
