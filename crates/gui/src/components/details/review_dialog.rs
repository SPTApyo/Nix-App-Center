//! "Write a Review" dialog, after `gs-review-dialog.ui`: a star rating, a
//! summary, the review text and the name shown with it. Posted to ODRS.

use adw::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

use crate::i18n::t;

pub fn present(
    parent: &impl IsA<gtk::Widget>,
    app_id: String,
    version: String,
    done: impl Fn(Result<(), String>) + 'static,
) {
    let stars = Rc::new(Cell::new(0u8));
    let star_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    star_box.set_halign(gtk::Align::Center);
    let buttons: Rc<Vec<gtk::Button>> = Rc::new(
        (1..=5u8)
            .map(|_| {
                let b = gtk::Button::new();
                let img = gtk::Image::from_icon_name("non-starred-symbolic");
                img.set_pixel_size(28);
                b.set_child(Some(&img));
                b.add_css_class("flat");
                b.add_css_class("star");
                star_box.append(&b);
                b
            })
            .collect(),
    );

    let summary = adw::EntryRow::builder().title(t("Summary")).build();
    let name = adw::EntryRow::builder().title(t("Your Name")).build();
    let real = gtk::glib::real_name().to_string_lossy().to_string();
    name.set_text(&if real.is_empty() || real == "Unknown" {
        gtk::glib::user_name().to_string_lossy().to_string()
    } else {
        real
    });
    let fields = crate::util::boxed_list();
    fields.append(&summary);
    fields.append(&name);

    let text = gtk::TextView::new();
    text.set_wrap_mode(gtk::WrapMode::WordChar);
    text.add_css_class("card");
    text.set_top_margin(8);
    text.set_bottom_margin(8);
    text.set_left_margin(8);
    text.set_right_margin(8);
    let text_scroll = gtk::ScrolledWindow::builder()
        .child(&text)
        .min_content_height(160)
        .vexpand(true)
        .build();
    let hint = gtk::Label::new(Some(&t(
        "Reviews are public and go to the Open Desktop Ratings Service, shared with GNOME Software.",
    )));
    hint.add_css_class("dim-label");
    hint.set_wrap(true);
    hint.set_xalign(0.0);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_top(12);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);
    body.append(&star_box);
    body.append(&fields);
    body.append(&text_scroll);
    body.append(&hint);

    let post = gtk::Button::with_mnemonic(&t("_Post"));
    post.add_css_class("suggested-action");
    post.set_sensitive(false);
    let header = adw::HeaderBar::new();
    header.pack_end(&post);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&body));
    let dialog = adw::Dialog::builder()
        .title(t("Write a Review"))
        .content_width(520)
        .content_height(560)
        .child(&toolbar)
        .build();

    // Post needs a rating, a summary and some text.
    let refresh: Rc<dyn Fn()> = Rc::new({
        let (stars, summary, text, post) =
            (stars.clone(), summary.clone(), text.clone(), post.clone());
        move || {
            let buf = text.buffer();
            let has_text = buf.char_count() > 0;
            post.set_sensitive(stars.get() > 0 && !summary.text().trim().is_empty() && has_text);
        }
    });
    for (i, b) in buttons.iter().enumerate() {
        let (stars, buttons, refresh) = (stars.clone(), buttons.clone(), refresh.clone());
        b.connect_clicked(move |_| {
            stars.set(i as u8 + 1);
            for (j, other) in buttons.iter().enumerate() {
                if let Some(img) = other.child().and_downcast::<gtk::Image>() {
                    img.set_icon_name(Some(if j <= i {
                        "starred-symbolic"
                    } else {
                        "non-starred-symbolic"
                    }));
                }
            }
            refresh();
        });
    }
    summary.connect_changed({
        let refresh = refresh.clone();
        move |_| refresh()
    });
    text.buffer().connect_changed({
        let refresh = refresh.clone();
        move |_| refresh()
    });

    let done = Rc::new(done);
    post.connect_clicked({
        let dialog = dialog.clone();
        move |btn| {
            btn.set_sensitive(false);
            let buf = text.buffer();
            let description = buf
                .text(&buf.start_iter(), &buf.end_iter(), false)
                .to_string();
            let (app_id, version, n) = (app_id.clone(), version.clone(), stars.get());
            let (summary, author) = (summary.text().to_string(), name.text().to_string());
            let (dialog, done, btn) = (dialog.clone(), done.clone(), btn.clone());
            gtk::glib::spawn_future_local(async move {
                let res = gtk::gio::spawn_blocking(move || {
                    nix_software_core::submit_review(
                        &app_id,
                        &version,
                        n,
                        &summary,
                        &description,
                        &author,
                    )
                })
                .await
                .unwrap_or_else(|_| Err(String::new()));
                if res.is_ok() {
                    dialog.close();
                } else {
                    btn.set_sensitive(true);
                }
                done(res);
            });
        }
    });
    dialog.present(Some(parent));
}
