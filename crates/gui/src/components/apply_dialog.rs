//! Progress of an apply or update: phase, a smooth bar with percentage and
//! time left, and the build log on demand. Closing the dialog lets the work
//! go on; a notification tells when it ends.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use nix_app_center_core::{ApplyOutcome, Phase, StreamEvent};

use crate::i18n::t;

thread_local! {
    /// True while an apply, update or rollback runs.
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

/// Greys `widget` out while an apply, update or rollback runs, then gives
/// it back the state it had.
pub fn guard(widget: &impl IsA<gtk::Widget>) {
    let weak = widget.downgrade();
    let before: Cell<Option<(bool, Option<gtk::glib::GString>)>> = Cell::new(None);
    gtk::glib::timeout_add_local(Duration::from_millis(500), move || {
        let Some(w) = weak.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };
        let busy = RUNNING.with(Cell::get);
        match before.take() {
            None if busy => {
                before.set(Some((w.is_sensitive(), w.tooltip_text())));
                w.set_sensitive(false);
                w.set_tooltip_text(Some(&t("Another operation is running")));
            }
            Some((sensitive, tooltip)) if !busy => {
                w.set_sensitive(sensitive);
                w.set_tooltip_text(tooltip.as_deref());
            }
            kept => before.set(kept),
        }
        gtk::glib::ControlFlow::Continue
    });
}

enum Event {
    Stream(StreamEvent),
    Done(Result<ApplyOutcome, String>),
}

/// The phase as a short sentence.
fn phase_text(phase: Phase) -> String {
    match phase {
        Phase::Starting => t("Preparing…"),
        Phase::Downloading { done, expected } => crate::util::plural(
            "Downloading {done} of {} package",
            "Downloading {done} of {} packages",
            expected as i64,
        )
        .replace("{done}", &done.to_string()),
        Phase::Building { done, expected } => t("Building {done} of {total}")
            .replace("{done}", &done.to_string())
            .replace("{total}", &expected.to_string()),
        Phase::Activating => t("Activating the new configuration…"),
    }
}

/// "45 % · about 2 min left", from the share done and the time spent.
fn progress_text(fraction: f64, elapsed: Duration) -> String {
    let percent = format!("{:.0} %", fraction * 100.0);
    if fraction < 0.05 || elapsed < Duration::from_secs(5) {
        return percent;
    }
    let left = elapsed.as_secs_f64() * (1.0 - fraction) / fraction;
    let estimate = if left < 60.0 {
        t("less than a minute left")
    } else {
        crate::util::plural(
            "about {} min left",
            "about {} min left",
            (left / 60.0).round() as i64,
        )
    };
    format!("{percent} · {estimate}")
}

pub fn present(
    parent: &impl IsA<gtk::Widget>,
    title: &str,
    steps: Vec<Vec<String>>,
    done: impl Fn(Result<ApplyOutcome, String>) + 'static,
) {
    present_then(parent, title, steps, |_| None, done);
}

/// [`present`], with `after` running once the steps end and giving the
/// message to show instead of the usual one.
pub fn present_then(
    parent: &impl IsA<gtk::Widget>,
    title: &str,
    steps: Vec<Vec<String>>,
    after: impl Fn(&Result<ApplyOutcome, String>) -> Option<String> + 'static,
    done: impl Fn(Result<ApplyOutcome, String>) + 'static,
) {
    // One Nix operation at a time: two switches would race.
    if RUNNING.with(|r| r.replace(true)) {
        let busy = adw::AlertDialog::new(
            Some(&t("Another Operation Is Running")),
            Some(&t(
                "Wait for the current apply, update or rollback to finish.",
            )),
        );
        busy.add_response("close", &t("_Close"));
        busy.present(Some(parent));
        return;
    }
    let phase = gtk::Label::new(Some(&t("Preparing…")));
    phase.add_css_class("heading");
    phase.set_xalign(0.0);
    phase.set_wrap(true);
    let bar = gtk::ProgressBar::new();
    bar.set_pulse_step(0.08);
    let detail = crate::util::dim_label(&t("Starting…"));
    let outcome = gtk::Label::new(None);
    outcome.set_xalign(0.0);
    outcome.set_wrap(true);
    outcome.set_selectable(true);
    outcome.set_visible(false);

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
    let command = gtk::Label::new(Some(
        &steps
            .iter()
            .map(|s| s.join(" "))
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    command.add_css_class("dim-label");
    command.add_css_class("monospace");
    command.set_xalign(0.0);
    command.set_wrap(true);
    command.set_selectable(true);
    let log = gtk::Box::new(gtk::Orientation::Vertical, 8);
    log.append(&command);
    log.append(
        &gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .min_content_height(280)
            .build(),
    );
    let revealer = gtk::Revealer::builder()
        .child(&log)
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .vexpand(true)
        .build();
    let toggle = gtk::Button::with_mnemonic(&t("Show _Details"));
    toggle.add_css_class("flat");
    toggle.set_tooltip_text(Some(&t("Show the full build log.")));
    toggle.set_halign(gtk::Align::Start);
    toggle.connect_clicked({
        let revealer = revealer.clone();
        move |b| {
            let open = !revealer.reveals_child();
            revealer.set_reveal_child(open);
            b.set_label(&if open {
                t("Hide _Details")
            } else {
                t("Show _Details")
            });
            b.set_use_underline(true);
        }
    });

    let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
    body.set_margin_top(12);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);
    for w in [
        phase.upcast_ref::<gtk::Widget>(),
        bar.upcast_ref(),
        detail.upcast_ref(),
        outcome.upcast_ref(),
        toggle.upcast_ref(),
        revealer.upcast_ref(),
    ] {
        body.append(w);
    }
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&body));
    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(640)
        .child(&toolbar)
        .build();
    dialog.present(Some(parent));

    let target: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
    let finished = Rc::new(Cell::new(false));
    gtk::glib::timeout_add_local(Duration::from_millis(50), {
        let (bar, target, finished) = (bar.clone(), target.clone(), finished.clone());
        let ticks = Cell::new(0u32);
        move || {
            match target.get() {
                Some(goal) => {
                    let now = bar.fraction();
                    bar.set_fraction(now + (goal - now) * 0.15);
                }
                None => {
                    ticks.set(ticks.get() + 1);
                    if ticks.get().is_multiple_of(4) {
                        bar.pulse();
                    }
                }
            }
            gtk::glib::ControlFlow::from(!finished.get() || (bar.fraction() - 1.0).abs() > 0.001)
        }
    });

    let applies = nix_app_center_core::apply_argv().is_some_and(|a| steps.contains(&a));
    // The generation running before, to undo the apply.
    let before = applies
        .then(|| {
            nix_app_center_core::history()
                .into_iter()
                .find(|p| p.current)
        })
        .flatten();
    let (tx, rx) = async_channel::unbounded::<Event>();
    std::thread::spawn(move || {
        let events = tx.clone();
        let res = nix_app_center_core::run_streaming(&steps, move |e| {
            let _ = events.send_blocking(Event::Stream(e));
        })
        .map_err(|e| e.to_string());
        let _ = tx.send_blocking(Event::Done(res));
    });

    let started = Instant::now();
    let last_phase: Rc<RefCell<Option<Phase>>> = Rc::new(RefCell::new(None));
    let title = title.to_string();
    let outcome_label = outcome.clone();
    gtk::glib::spawn_future_local(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                Event::Stream(StreamEvent::Line(line)) => {
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &format!("{line}\n"));
                    let mark = buffer.create_mark(None, &buffer.end_iter(), false);
                    view.scroll_mark_onscreen(&mark);
                    buffer.delete_mark(&mark);
                }
                Event::Stream(StreamEvent::Progress(fraction, p)) => {
                    if *last_phase.borrow() != Some(p) {
                        phase.set_label(&phase_text(p));
                        last_phase.replace(Some(p));
                    }
                    target.set(fraction);
                    detail.set_label(&match fraction {
                        Some(f) => progress_text(f, started.elapsed()),
                        None => t("Working…"),
                    });
                }
                Event::Done(res) => {
                    RUNNING.with(|r| r.set(false));
                    finished.set(true);
                    let message =
                        after(&res).unwrap_or_else(|| crate::util::describe_apply(res.clone()));
                    match &res {
                        Ok(outcome) => {
                            if applies && matches!(outcome, ApplyOutcome::Done(_)) {
                                nix_app_center_core::applied();
                                nix_app_center_core::record_history();
                                let changed = |b: &nix_app_center_core::HistoryPoint| {
                                    nix_app_center_core::history()
                                        .iter()
                                        .any(|p| p.current && p.number != b.number)
                                };
                                if let (Some(point), Some(body)) = (
                                    before.clone().filter(changed),
                                    outcome_label.parent().and_downcast::<gtk::Box>(),
                                ) {
                                    body.insert_child_after(
                                        &undo_button(&dialog, point),
                                        Some(&outcome_label),
                                    );
                                }
                            }
                            crate::util::refresh_installed();
                            target.set(Some(1.0));
                            phase.set_label(&t("Done"));
                            detail.set_label(&format!("100 % · {}", t("finished")));
                        }
                        Err(_) => {
                            target.set(Some(bar.fraction()));
                            phase.set_label(&t("Failed"));
                            detail.set_visible(false);
                            bar.add_css_class("error");
                        }
                    }
                    outcome.set_label(&message);
                    outcome.set_visible(!matches!(res, Ok(ApplyOutcome::Done(_))));
                    notify_if_away(&dialog, &title, &message);
                    done(res);
                    break;
                }
            }
        }
    });
}

/// Goes back to the generation running before the apply.
fn undo_button(dialog: &adw::Dialog, point: nix_app_center_core::HistoryPoint) -> gtk::Button {
    let undo = gtk::Button::with_mnemonic(&t("_Undo"));
    undo.set_halign(gtk::Align::Start);
    undo.set_tooltip_text(Some(&t("Go back to the version running before.")));
    let dialog = dialog.clone();
    undo.connect_clicked(move |_| {
        let parent = dialog.parent();
        dialog.close();
        if let Some(parent) = parent {
            crate::components::go_back::run(&parent, point.clone(), |_| {});
        }
    });
    undo
}

/// A desktop notification when the user is not looking at the app.
fn notify_if_away(dialog: &adw::Dialog, title: &str, message: &str) {
    let window_active = dialog
        .root()
        .and_downcast::<gtk::Window>()
        .is_some_and(|w| w.is_active());
    if dialog.parent().is_some() && window_active {
        return;
    }
    let n = gtk::gio::Notification::new(title);
    n.set_body(Some(message));
    relm4::main_application().send_notification(Some("apply"), &n);
}
