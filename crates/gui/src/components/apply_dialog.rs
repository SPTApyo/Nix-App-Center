//! Progress of an apply, update or other long task: phase, a smooth bar
//! with percentage and time left, the build log on demand and a Stop
//! button. Closing the dialog lets the work go on: the header's
//! [`indicator`] shows it and opens the dialog again; a notification tells
//! when it ends.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use nix_app_center_core::{ApplyOutcome, ChangeKind, ChangeSet, Phase, StreamEvent};

use crate::i18n::t;

thread_local! {
    /// True while an apply, update or rollback runs.
    static RUNNING: Cell<bool> = const { Cell::new(false) };
    /// The dialog of the task running now, for the header indicator.
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
    /// Tasks started while another runs, in order.
    static QUEUE: RefCell<std::collections::VecDeque<Box<dyn FnOnce()>>> =
        RefCell::new(std::collections::VecDeque::new());
    /// The last task's dialog, for its notification.
    static LAST: RefCell<Option<(adw::Dialog, gtk::Widget)>> = const { RefCell::new(None) };
}

/// Opens the dialog of the running or last task again.
pub fn show_last() {
    LAST.with_borrow(|last| {
        if let Some((dialog, parent)) = last {
            if dialog.parent().is_none() {
                dialog.present(Some(&anchor(parent)));
            }
        }
    });
}

/// `widget` while it is in a window, else the app's window: the page that
/// started a task may be closed by the time its dialog shows.
fn anchor(widget: &gtk::Widget) -> gtk::Widget {
    if widget.root().is_some() {
        return widget.clone();
    }
    relm4::main_application()
        .active_window()
        .map_or_else(|| widget.clone(), |w| w.upcast())
}

/// What the header indicator mirrors of the running task.
struct Active {
    dialog: adw::Dialog,
    parent: gtk::Widget,
    phase: gtk::Label,
    bar: gtk::ProgressBar,
    target: Rc<Cell<Option<f64>>>,
}

/// True while an apply, update or other task runs.
pub fn is_running() -> bool {
    RUNNING.with(Cell::get)
}

/// A progress bar for the header, shown while a task runs with its dialog
/// closed; clicking it opens the dialog again.
pub fn indicator() -> gtk::Button {
    let label = gtk::Label::new(None);
    label.add_css_class("caption");
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(22);
    let bar = gtk::ProgressBar::new();
    bar.set_pulse_step(0.1);
    let column = gtk::Box::new(gtk::Orientation::Vertical, 3);
    column.set_valign(gtk::Align::Center);
    column.set_size_request(170, -1);
    column.append(&label);
    column.append(&bar);
    let button = gtk::Button::builder()
        .child(&column)
        .tooltip_text(t("Show Progress"))
        .visible(false)
        .build();
    button.add_css_class("flat");
    button.add_css_class("task-indicator");
    button.connect_clicked(|_| {
        ACTIVE.with_borrow(|a| {
            if let Some(a) = a {
                a.dialog.present(Some(&anchor(&a.parent)));
            }
        });
    });
    gtk::glib::timeout_add_local(Duration::from_millis(200), {
        let button = button.downgrade();
        move || {
            let Some(button) = button.upgrade() else {
                return gtk::glib::ControlFlow::Break;
            };
            ACTIVE.with_borrow(|a| match a {
                Some(a) if a.dialog.parent().is_none() => {
                    button.set_visible(true);
                    let waiting = QUEUE.with_borrow(|q| q.len());
                    label.set_label(&if waiting == 0 {
                        a.phase.label().to_string()
                    } else {
                        format!(
                            "{} · {}",
                            a.phase.label(),
                            crate::util::plural("{} waiting", "{} waiting", waiting as i64)
                        )
                    });
                    match a.target.get() {
                        Some(_) => bar.set_fraction(a.bar.fraction()),
                        None => bar.pulse(),
                    }
                }
                _ => button.set_visible(false),
            });
            gtk::glib::ControlFlow::Continue
        }
    });
    button
}

/// The work a dialog runs: it reports progress and may ask for a review.
type Job = Box<
    dyn FnOnce(
            Box<dyn Fn(StreamEvent) + Send + Sync>,
            Box<dyn Fn(ChangeSet) -> bool + Send + Sync>,
        ) -> Result<ApplyOutcome, String>
        + Send,
>;

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
    let command = steps
        .iter()
        .map(|s| s.join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    let applies = nix_app_center_core::apply_argv().is_some_and(|a| steps.contains(&a));
    let job: Job = Box::new(move |events, review| {
        nix_app_center_core::run_reviewed(&steps, events, review).map_err(|e| e.to_string())
    });
    let spec = Spec {
        title: title.to_string(),
        command,
        applies,
        stoppable: true,
    };
    run(parent, spec, job, after, done);
}

/// Runs `task` in the progress dialog, without steps to show or stop;
/// `describe` words its result.
pub fn present_task<T: Send + 'static>(
    parent: &impl IsA<gtk::Widget>,
    title: &str,
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
    describe: impl Fn(&T) -> String + 'static,
    done: impl Fn(Result<T, String>) + 'static,
) {
    let slot: std::sync::Arc<std::sync::Mutex<Option<T>>> = Default::default();
    let job: Job = Box::new({
        let slot = slot.clone();
        move |_, _| {
            let value = task()?;
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(value);
            Ok(ApplyOutcome::Done(String::new()))
        }
    });
    let taken: Rc<RefCell<Option<T>>> = Rc::default();
    let after = {
        let (slot, taken) = (slot.clone(), taken.clone());
        move |res: &Result<ApplyOutcome, String>| {
            res.as_ref().ok()?;
            let value = slot.lock().unwrap_or_else(|e| e.into_inner()).take()?;
            let text = describe(&value);
            *taken.borrow_mut() = Some(value);
            Some(text)
        }
    };
    let finish = move |res: Result<ApplyOutcome, String>| {
        done(res.and_then(|_| {
            taken
                .borrow_mut()
                .take()
                .ok_or_else(|| t("The task gave no result."))
        }));
    };
    let spec = Spec {
        title: title.to_string(),
        command: String::new(),
        applies: false,
        stoppable: false,
    };
    run(parent, spec, job, after, finish);
}

/// What the dialog shows around its job.
struct Spec {
    title: String,
    /// The commands run, shown above the log; empty hides the log.
    command: String,
    /// The job applies the configuration: history and Undo follow.
    applies: bool,
    /// The job can be stopped like Ctrl+C.
    stoppable: bool,
}

fn run(
    parent: &impl IsA<gtk::Widget>,
    spec: Spec,
    job: Job,
    after: impl Fn(&Result<ApplyOutcome, String>) -> Option<String> + 'static,
    done: impl Fn(Result<ApplyOutcome, String>) + 'static,
) {
    // One Nix operation at a time: two switches would race. The next waits.
    if RUNNING.with(Cell::get) {
        let parent = parent.clone().upcast::<gtk::Widget>();
        QUEUE.with_borrow_mut(|q| {
            q.push_back(Box::new(move || {
                run(&anchor(&parent), spec, job, after, done)
            }));
        });
        return;
    }
    RUNNING.with(|r| r.set(true));
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
    view.set_wrap_mode(gtk::WrapMode::None);
    view.add_css_class("card");
    view.add_css_class("build-log");
    let tags = buffer.tag_table();
    for (name, color) in [("error", "#e01b24"), ("warning", "#e5a50a")] {
        let tag = gtk::TextTag::builder()
            .name(name)
            .foreground(color)
            .weight(700)
            .build();
        tags.add(&tag);
    }
    view.set_top_margin(8);
    view.set_bottom_margin(8);
    view.set_left_margin(8);
    view.set_right_margin(8);
    let Spec {
        title,
        command,
        applies,
        stoppable,
    } = spec;
    let command_text = command;
    let command = gtk::Label::new(Some(&command_text));
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
            .min_content_height(380)
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
    toggle.set_visible(!command.label().is_empty());
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
    let stop = gtk::Button::with_mnemonic(&t("_Stop"));
    stop.add_css_class("destructive-action");
    stop.set_tooltip_text(Some(&t(
        "Stop like Ctrl+C. Activating the new system is never cut short: it ends first.",
    )));
    stop.set_visible(stoppable);
    stop.connect_clicked({
        let detail = detail.clone();
        move |b| {
            b.set_sensitive(false);
            if !nix_app_center_core::stop_running() {
                detail.set_label(&t("Stopping after the current step…"));
            }
        }
    });
    let header = adw::HeaderBar::new();
    header.pack_start(&stop);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&body));
    let dialog = adw::Dialog::builder()
        .title(title.as_str())
        .content_width(900)
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

    // The generation running before, to undo the apply.
    let before = applies
        .then(|| {
            nix_app_center_core::history()
                .into_iter()
                .find(|p| p.current)
        })
        .flatten();
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
        let res = job(
            Box::new(move |e| {
                let _ = events.send_blocking(Event::Stream(e));
            }),
            Box::new(move |set| {
                if !ask || reviews.send_blocking(Event::Review(set)).is_err() {
                    return true;
                }
                answer_rx.recv_blocking().unwrap_or(false)
            }),
        );
        let _ = tx.send_blocking(Event::Done(res));
    });
    LAST.with_borrow_mut(|last| {
        *last = Some((dialog.clone(), parent.clone().upcast()));
    });
    ACTIVE.with_borrow_mut(|a| {
        *a = Some(Active {
            dialog: dialog.clone(),
            parent: parent.clone().upcast(),
            phase: phase.clone(),
            bar: bar.clone(),
            target: target.clone(),
        });
    });
    // Closing the dialog while it waits keeps the running system. Closed
    // during the build, it comes back for the review.
    dialog.connect_closed({
        let (answer_tx, review) = (answer_tx.clone(), review.clone());
        move |_| {
            if review.is_visible() {
                let _ = answer_tx.try_send(false);
            }
        }
    });
    let parent = parent.clone().upcast::<gtk::Widget>();

    let started = Instant::now();
    let last_phase: Rc<RefCell<Option<Phase>>> = Rc::new(RefCell::new(None));
    let outcome_label = outcome.clone();
    gtk::glib::spawn_future_local(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                Event::Stream(StreamEvent::Line(line)) => {
                    let mut end = buffer.end_iter();
                    let lower = line.trim_start().to_lowercase();
                    let tag = if lower.starts_with("error") {
                        Some("error")
                    } else if lower.starts_with("warning") || lower.starts_with("trace:") {
                        Some("warning")
                    } else {
                        None
                    };
                    match tag {
                        Some(tag) => {
                            buffer.insert_with_tags_by_name(&mut end, &format!("{line}\n"), &[tag])
                        }
                        None => buffer.insert(&mut end, &format!("{line}\n")),
                    }
                    let mark = buffer.create_mark(None, &buffer.end_iter(), false);
                    view.scroll_mark_onscreen(&mark);
                    buffer.delete_mark(&mark);
                }
                Event::Stream(StreamEvent::Progress(fraction, p)) => {
                    if *last_phase.borrow() != Some(p) {
                        phase.set_label(&phase_text(p));
                        last_phase.replace(Some(p));
                        if p == Phase::Activating {
                            stop.set_sensitive(false);
                        }
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
                    if dialog.parent().is_none() {
                        dialog.present(Some(&parent));
                    }
                }
                Event::Done(res) => {
                    RUNNING.with(|r| r.set(false));
                    ACTIVE.with_borrow_mut(|a| *a = None);
                    stop.set_visible(false);
                    finished.set(true);
                    progress.set_visible(true);
                    review.set_visible(false);
                    let full = matches!(&res, Err(e) if e.contains("No space left on device"));
                    let message = if full {
                        t("The disk is full. Free up space in Storage, then apply again.")
                    } else {
                        after(&res).unwrap_or_else(|| crate::util::describe_apply(res.clone()))
                    };
                    match &res {
                        Ok(ApplyOutcome::Cancelled) => {
                            target.set(Some(bar.fraction()));
                            phase.set_label(&t("Stopped"));
                            detail.set_visible(false);
                        }
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
                        Err(e) => {
                            let log = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                            if let (Some(wanted), Some(body)) = (
                                nix_app_center_core::required_file(&format!("{log}\n{e}")),
                                outcome_label.parent().and_downcast::<gtk::Box>(),
                            ) {
                                body.insert_child_after(
                                    &required_file_box(&dialog, wanted),
                                    Some(&outcome_label),
                                );
                            }
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
                    let log = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                    let _ = nix_app_center_core::save_task_log(
                        &format!("{title} · {}", phase.label()),
                        &format!("{command_text}\n\n{message}\n\n{log}"),
                    );
                    done(res);
                    if let Some(next) = QUEUE.with_borrow_mut(|q| q.pop_front()) {
                        gtk::glib::idle_add_local_once(next);
                    }
                    break;
                }
            }
        }
    });
}

/// The saved logs of past tasks, newest first; each opens on demand.
pub fn present_logs(parent: &impl IsA<gtk::Widget>) {
    let list = crate::util::boxed_list();
    for log in nix_app_center_core::task_logs() {
        let row = adw::ExpanderRow::builder()
            .title(crate::util::esc(&log.title))
            .subtitle(crate::util::esc(&crate::util::format_relative(
                log.at as i64,
            )))
            .build();
        let loaded = Cell::new(false);
        row.connect_expanded_notify(move |row| {
            if !row.is_expanded() || loaded.replace(true) {
                return;
            }
            let text = std::fs::read_to_string(&log.path).unwrap_or_default();
            let body = text.split_once('\n').map_or("", |(_, b)| b);
            let view = gtk::TextView::new();
            view.buffer().set_text(body);
            view.set_editable(false);
            view.set_monospace(true);
            view.add_css_class("build-log");
            view.set_top_margin(8);
            view.set_bottom_margin(8);
            view.set_left_margin(8);
            view.set_right_margin(8);
            row.add_row(
                &gtk::ScrolledWindow::builder()
                    .child(&view)
                    .min_content_height(360)
                    .build(),
            );
        });
        list.append(&row);
    }
    let body = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .vexpand(true)
        .build();
    list.set_margin_top(12);
    list.set_margin_bottom(18);
    list.set_margin_start(18);
    list.set_margin_end(18);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&body));
    adw::Dialog::builder()
        .title(t("Logs"))
        .content_width(900)
        .content_height(640)
        .child(&toolbar)
        .build()
        .present(Some(parent));
}

/// A file the build waits for: where to get it, and a button adding it to
/// the store once downloaded.
fn required_file_box(dialog: &adw::Dialog, wanted: nix_app_center_core::RequiredFile) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
    column.add_css_class("card");
    column.add_css_class("required-file");
    let text = gtk::Label::new(Some(
        &t("The package needs {} and nixpkgs may not download it, often because of a license or a login. Download it yourself, then choose it here: the next apply goes on with it.")
            .replace("{}", &wanted.name),
    ));
    text.set_wrap(true);
    text.set_xalign(0.0);
    let state = crate::util::dim_label("");
    state.set_visible(false);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    if let Some(url) = wanted.url.clone() {
        let open = gtk::Button::with_mnemonic(&t("_Open the Download Page"));
        open.connect_clicked(move |_| crate::util::open_uri(&url));
        buttons.append(&open);
    }
    let choose = gtk::Button::with_mnemonic(&t("_Choose the File…"));
    choose.add_css_class("suggested-action");
    let window = dialog.root().and_downcast::<gtk::Window>();
    column.append(&text);
    column.append(&buttons);
    column.append(&state);
    choose.connect_clicked(move |button| {
        let picker = gtk::FileDialog::builder()
            .title(t("Choose {}").replace("{}", &wanted.name))
            .modal(true)
            .build();
        let (button, state, wanted) = (button.clone(), state.clone(), wanted.clone());
        picker.open(window.as_ref(), gtk::gio::Cancellable::NONE, move |file| {
            let Some(path) = file.ok().and_then(|f| f.path()) else {
                return;
            };
            button.set_sensitive(false);
            state.set_visible(true);
            state.set_label(&t("Adding it to the Nix store…"));
            gtk::glib::spawn_future_local(async move {
                let added = gtk::gio::spawn_blocking(move || {
                    nix_app_center_core::add_required_file(&path, &wanted)
                        .map_err(|e| e.to_string())
                })
                .await
                .unwrap_or_else(|_| Err(String::new()));
                match added {
                    Ok(()) => state.set_label(&t("Added. Apply again to install it.")),
                    Err(e) => {
                        state.set_label(&crate::util::failed(&e));
                        button.set_sensitive(true);
                    }
                }
            });
        });
    });
    buttons.append(&choose);
    column
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
    n.set_default_action("app.show-task");
    n.set_body(Some(message));
    relm4::main_application().send_notification(Some("apply"), &n);
}
