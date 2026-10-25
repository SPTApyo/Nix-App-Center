//! Progress of an apply or update: phase, a smooth bar with percentage and
//! time left, and the build log on demand. Closing the dialog lets the work
//! go on; a notification tells when it ends.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use nix_app_center_core::{ApplyOutcome, ChangeKind, ChangeSet, Phase, StreamEvent};

use crate::i18n::t;

enum Event {
    Stream(StreamEvent),
    /// Built: what activating it changes, waiting for the user.
    Review(ChangeSet),
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

    let review = gtk::Box::new(gtk::Orientation::Vertical, 10);
    review.set_visible(false);
    let progress = gtk::Box::new(gtk::Orientation::Vertical, 10);
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
    ] {
        progress.append(w);
    }
    body.append(&progress);
    body.append(&review);
    let storage = gtk::Button::builder()
        .label(t("Open Storage"))
        .halign(gtk::Align::Start)
        .action_name("app.show-tab")
        .action_target(&"storage".to_variant())
        .visible(false)
        .build();
    progress.append(&storage);
    body.append(&toggle);
    body.append(&revealer);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&body));
    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(640)
        .child(&toolbar)
        .build();
    dialog.present(Some(parent));
    storage.connect_clicked({
        let dialog = dialog.clone();
        move |_| {
            dialog.close();
        }
    });

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
    let (tx, rx) = async_channel::unbounded::<Event>();
    let (answer_tx, answer_rx) = async_channel::bounded::<bool>(1);
    let settings = nix_app_center_core::load_settings();
    let ask = settings.review_changes;
    if applies && ask && settings.apply_command.is_some() {
        progress.append(&crate::util::dim_label(&t(
            "Your own apply command runs as it is: the changes cannot be shown first.",
        )));
    }
    std::thread::spawn(move || {
        let events = tx.clone();
        let reviews = tx.clone();
        let res = nix_app_center_core::run_reviewed(
            &steps,
            move |e| {
                let _ = events.send_blocking(Event::Stream(e));
            },
            move |set| {
                if !ask || reviews.send_blocking(Event::Review(set)).is_err() {
                    return true;
                }
                answer_rx.recv_blocking().unwrap_or(false)
            },
        )
        .map_err(|e| e.to_string());
        let _ = tx.send_blocking(Event::Done(res));
    });
    // Closing the dialog while it waits keeps the running system.
    dialog.connect_closed({
        let answer_tx = answer_tx.clone();
        move |_| {
            let _ = answer_tx.try_send(false);
        }
    });

    let started = Instant::now();
    let last_phase: Rc<RefCell<Option<Phase>>> = Rc::new(RefCell::new(None));
    let title = title.to_string();
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
                Event::Review(set) => {
                    fill_review(&review, &set, &answer_tx, &progress);
                    progress.set_visible(false);
                    review.set_visible(true);
                }
                Event::Done(res) => {
                    finished.set(true);
                    progress.set_visible(true);
                    review.set_visible(false);
                    let full = matches!(&res, Err(e) if e.contains("No space left on device"));
                    let message = if full {
                        t("The disk is full. Free up space in Storage, then apply again.")
                    } else {
                        crate::util::describe_apply(res.clone())
                    };
                    match &res {
                        Ok(outcome) => {
                            if applies && matches!(outcome, ApplyOutcome::Done(_)) {
                                nix_app_center_core::applied();
                            }
                            crate::util::refresh_installed();
                            target.set(Some(1.0));
                            phase.set_label(&t("Done"));
                            detail.set_label(&format!("100 % · {}", t("finished")));
                        }
                        Err(_) => {
                            storage.set_visible(full);
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

/// The review step alone, for the development gallery: nothing runs.
#[cfg(feature = "dev-gallery")]
pub fn present_review(parent: &impl IsA<gtk::Widget>, set: &ChangeSet) {
    let review = gtk::Box::new(gtk::Orientation::Vertical, 10);
    review.set_margin_top(12);
    review.set_margin_bottom(18);
    review.set_margin_start(18);
    review.set_margin_end(18);
    let (answer, _) = async_channel::bounded::<bool>(1);
    fill_review(
        &review,
        set,
        &answer,
        &gtk::Box::new(gtk::Orientation::Vertical, 0),
    );
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&review));
    adw::Dialog::builder()
        .title(t("Applying Configuration"))
        .content_width(640)
        .child(&toolbar)
        .build()
        .present(Some(parent));
}

/// Fills `review` with what `set` changes and the buttons answering.
fn fill_review(
    review: &gtk::Box,
    set: &ChangeSet,
    answer: &async_channel::Sender<bool>,
    progress: &gtk::Box,
) {
    while let Some(child) = review.first_child() {
        review.remove(&child);
    }
    let heading = gtk::Label::new(Some(&t("Review the Changes")));
    heading.add_css_class("heading");
    heading.set_xalign(0.0);
    review.append(&heading);
    review.append(&crate::util::dim_label(&summary(set)));
    if set.reboot {
        review.append(&banner(&t(
            "The kernel changes: restart the computer to use the new one.",
        )));
    }
    if set.system {
        review.append(&banner(&t(
            "These changes apply to every user of this computer.",
        )));
    }

    let mut shown: Vec<&nix_app_center_core::Change> = set
        .changes
        .iter()
        .filter(|c| c.kind != ChangeKind::Rebuilt)
        .collect();
    let apps = known_apps();
    shown.sort_by_key(|c| {
        (
            !apps.contains(&c.name.to_lowercase()),
            c.name.to_lowercase(),
        )
    });
    if !shown.is_empty() {
        let list = crate::util::boxed_list();
        for c in shown {
            let row = adw::ActionRow::builder()
                .title(crate::util::esc(&c.name))
                .subtitle(crate::util::esc(&change_text(c)))
                .build();
            if c.size != 0 {
                let size = gtk::Label::new(Some(&signed_size(c.size)));
                size.add_css_class("dim-label");
                row.add_suffix(&size);
            }
            list.append(&row);
        }
        review.append(
            &gtk::ScrolledWindow::builder()
                .child(&list)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .propagate_natural_height(true)
                .max_content_height(320)
                .build(),
        );
    }

    let cancel = gtk::Button::with_mnemonic(&t("_Cancel"));
    let apply = gtk::Button::with_mnemonic(&t("_Apply Changes"));
    apply.add_css_class("suggested-action");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&cancel);
    buttons.append(&apply);
    review.append(&buttons);
    for (button, yes) in [(cancel, false), (apply.clone(), true)] {
        let (answer, review, progress) = (answer.clone(), review.clone(), progress.clone());
        button.connect_clicked(move |_| {
            let _ = answer.try_send(yes);
            review.set_visible(false);
            progress.set_visible(true);
        });
    }
    apply.grab_focus();
}

/// "2 updated, 1 added · +120 MB", or that nothing changes.
fn summary(set: &ChangeSet) -> String {
    let count = |kind: ChangeKind| set.count(&kind) as i64;
    let mut parts = Vec::new();
    for (n, text) in [
        (
            count(ChangeKind::Updated),
            crate::util::plural("{} updated", "{} updated", count(ChangeKind::Updated)),
        ),
        (
            count(ChangeKind::Added),
            crate::util::plural("{} added", "{} added", count(ChangeKind::Added)),
        ),
        (
            count(ChangeKind::Removed),
            crate::util::plural("{} removed", "{} removed", count(ChangeKind::Removed)),
        ),
        (
            count(ChangeKind::Rebuilt),
            crate::util::plural("{} rebuilt", "{} rebuilt", count(ChangeKind::Rebuilt)),
        ),
    ] {
        if n > 0 {
            parts.push(text);
        }
    }
    if parts.is_empty() {
        return t("No package changes, only settings.");
    }
    let mut text = parts.join(", ");
    if set.size() != 0 {
        text.push_str(" · ");
        text.push_str(&signed_size(set.size()));
    }
    text
}

/// "131.0 → 132.0", "Added: 1.0" or "Removed".
fn change_text(c: &nix_app_center_core::Change) -> String {
    match c.kind {
        ChangeKind::Added => t("Added: {}").replace("{}", &c.after.join(", ")),
        ChangeKind::Removed => t("Removed"),
        ChangeKind::Updated => format!("{} → {}", c.before.join(", "), c.after.join(", ")),
        ChangeKind::Rebuilt => t("Rebuilt"),
    }
}

fn signed_size(bytes: i64) -> String {
    let size = gtk::glib::format_size(bytes.unsigned_abs());
    if bytes < 0 {
        format!("−{size}")
    } else {
        format!("+{size}")
    }
}

fn banner(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_wrap(true);
    l.add_css_class("warning");
    l
}

/// Lowercase names of the apps the user has or keeps, listed first.
fn known_apps() -> std::collections::HashSet<String> {
    let mut names: std::collections::HashSet<String> = nix_app_center_core::load_manifest()
        .kept
        .iter()
        .map(|e| e.package.attr.to_lowercase())
        .collect();
    names.extend(
        nix_app_center_core::profile_apps()
            .into_iter()
            .map(|a| a.pname.to_lowercase()),
    );
    names
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
