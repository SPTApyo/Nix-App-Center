//! Build log dialog: runs an apply or update command and shows its output
//! live, so a rebuild stays inside the app. The dialog cannot be closed while
//! the command runs; `done` receives the outcome once it ends.

use adw::prelude::*;
use onyx_core::ApplyOutcome;

use crate::i18n::t;

enum Event {
    Line(String),
    Done(Result<ApplyOutcome, String>),
}

pub fn present(
    parent: &impl IsA<gtk::Widget>,
    title: &str,
    argv: Vec<String>,
    done: impl Fn(Result<ApplyOutcome, String>) + 'static,
) {
    let spinner = adw::Spinner::new();
    let status = gtk::Label::new(Some(&t("Running…")));
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_hexpand(true);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    head.append(&spinner);
    head.append(&status);

    let command = gtk::Label::new(Some(&argv.join(" ")));
    command.add_css_class("dim-label");
    command.add_css_class("monospace");
    command.set_xalign(0.0);
    command.set_wrap(true);
    command.set_selectable(true);

    let buffer = gtk::TextBuffer::new(None);
    let view = gtk::TextView::with_buffer(&buffer);
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_monospace(true);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.add_css_class("card");
    view.set_top_margin(8);
    view.set_bottom_margin(8);
    view.set_left_margin(8);
    view.set_right_margin(8);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&view)
        .vexpand(true)
        .min_content_height(320)
        .build();

    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_top(12);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);
    body.append(&head);
    body.append(&command);
    body.append(&scroller);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&body));
    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(720)
        .content_height(520)
        .child(&toolbar)
        .can_close(false)
        .build();
    dialog.present(Some(parent));

    let (tx, rx) = async_channel::unbounded::<Event>();
    std::thread::spawn(move || {
        let lines = tx.clone();
        let res = onyx_core::run_streaming(&argv, move |l| {
            let _ = lines.send_blocking(Event::Line(l));
        })
        .map_err(|e| e.to_string());
        let _ = tx.send_blocking(Event::Done(res));
    });

    gtk::glib::spawn_future_local(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                Event::Line(line) => {
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &format!("{line}\n"));
                    let mark = buffer.create_mark(None, &buffer.end_iter(), false);
                    view.scroll_mark_onscreen(&mark);
                    buffer.delete_mark(&mark);
                }
                Event::Done(res) => {
                    spinner.set_visible(false);
                    status.set_label(&crate::util::describe_apply(res.clone()));
                    dialog.set_can_close(true);
                    done(res);
                    break;
                }
            }
        }
    });
}
