//! Writes the Nix App Center files from the manifest and applies the
//! configuration. Plain apps go in one package list; apps with their own
//! Nix block go in marked regions, one per app.

use crate::config_layout::{current_user, detect, Layout, LayoutKind};
use crate::manifest::KeptEntry;
use std::process::Command;

/// How the user file is written, from the layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HomeShape {
    /// A home-manager module (standalone home-manager).
    HomeManager,
    /// A NixOS module wrapping home-manager options in `home-manager.users.<user>`.
    NixosHm(String),
    /// A NixOS module without Home Manager: `users.users.<user>.packages`.
    NixosUser(String),
}

impl HomeShape {
    pub(crate) fn of(layout: &Layout) -> Self {
        let nixos = matches!(
            layout.kind,
            LayoutKind::NixosFlake { .. } | LayoutKind::NixosLegacy { .. }
        );
        match (nixos, &layout.hm_user) {
            (true, Some(user)) => Self::NixosHm(user.clone()),
            (true, None) => Self::NixosUser(current_user()),
            (false, _) => Self::HomeManager,
        }
    }

    fn packages_option(&self) -> String {
        match self {
            Self::NixosUser(user) => format!("users.users.\"{user}\".packages"),
            _ => "home.packages".to_string(),
        }
    }
}

/// The block equivalent to a plain entry, to start editing from.
pub fn default_home_block(layout: &Layout, attr: &str) -> String {
    default_block(&HomeShape::of(layout), attr)
}

/// Tarball of nixpkgs at `rev`.
pub(crate) fn nixpkgs_tarball(rev: &str) -> String {
    format!("https://github.com/NixOS/nixpkgs/archive/{rev}.tar.gz")
}

/// Name of the binding holding nixpkgs at `rev`.
fn pin_binding(rev: &str) -> String {
    format!("nixpkgs-{}", rev.get(..12).unwrap_or(rev))
}

/// The package expression of a plain entry: its attr, from the held
/// revision when pinned, with its build options.
/// The Nix expression of an entry; `None` when its names are not plain
/// identifiers, since they come from other flakes and end up run as root.
fn package_expr(e: &KeptEntry) -> Option<String> {
    let attr = &e.package.attr;
    let ident = |s: &str, dots: bool| {
        !s.is_empty()
            && !s.starts_with(['.', '-'])
            && s.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '\'') || (dots && c == '.')
            })
    };
    if !ident(attr, true) {
        return None;
    }
    if let Some(input) = &e.package.source {
        if attr.contains('.') {
            return None;
        }
        // A flake added in Preferences, no input: its locked reference.
        if input.contains(':') {
            let locked = ["github:", "gitlab:", "sourcehut:"]
                .iter()
                .any(|p| input.starts_with(p))
                && input
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.:/~".contains(c));
            return locked.then(|| {
                format!(
                    "(builtins.getFlake \"{input}\").packages.${{pkgs.stdenv.hostPlatform.system}}.{attr}"
                )
            });
        }
        if !ident(input, false) {
            return None;
        }
        return Some(format!(
            "inputs.{input}.packages.${{pkgs.stdenv.hostPlatform.system}}.{attr}"
        ));
    }
    let pin = valid_pin(e);
    if pin.is_none() && e.flags.is_empty() {
        return Some(attr.clone());
    }
    let base = match pin {
        Some(p) => format!("{}.{attr}", pin_binding(&p.rev)),
        None => format!("pkgs.{attr}"),
    };
    Some(crate::build_flags::with_flags(&base, &e.flags))
}

/// The entry's hold, when its revision and hash are safe to write as Nix.
fn valid_pin(e: &KeptEntry) -> Option<&crate::manifest::Pin> {
    e.pin.as_ref().filter(|p| {
        crate::versions::is_rev(&p.rev)
            && !p.sha256.is_empty()
            && p.sha256.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

/// `let` bindings importing each held revision once, shared by its apps.
fn pin_bindings<'a>(entries: impl Iterator<Item = &'a KeptEntry>) -> String {
    let mut pins: Vec<&crate::manifest::Pin> = entries.filter_map(valid_pin).collect();
    pins.sort_by(|a, b| a.rev.cmp(&b.rev));
    pins.dedup_by(|a, b| a.rev == b.rev);
    if pins.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "let
",
    );
    for p in pins {
        out.push_str(&format!(
            "  {} = import (builtins.fetchTarball {{\n    url = \"{}\";\n    sha256 = \"{}\";\n  }}) {{\n    config = lib.filterAttrs (_: v: v != null) pkgs.config;\n    system = pkgs.stdenv.hostPlatform.system;\n  }};\n",
            pin_binding(&p.rev),
            nixpkgs_tarball(&p.rev),
            p.sha256
        ));
    }
    out.push_str(
        "in
",
    );
    out
}

/// True for a plain entry of the user scope.
fn in_home_list(e: &KeptEntry) -> bool {
    e.home_nix.is_none() && !e.system_wide
}

/// `<option> = with pkgs; [ … ];` over `entries`, sorted.
fn package_list<'a>(option: &str, entries: impl Iterator<Item = &'a KeptEntry>) -> String {
    let mut items: Vec<String> = entries.filter_map(package_expr).collect();
    items.sort_unstable();
    items.dedup();
    if items.is_empty() {
        return format!("{option} = with pkgs; [ ];\n");
    }
    let mut out = format!("{option} = with pkgs; [\n");
    for item in items {
        out.push_str(&format!("  {item}\n"));
    }
    out.push_str("];\n");
    out
}

fn default_block(shape: &HomeShape, attr: &str) -> String {
    format!("{} = [ pkgs.{attr} ];", shape.packages_option())
}

fn header(warn: &str) -> String {
    format!("# {warn}\n# Generated by Nix App Center, do not edit by hand.\n")
}

/// Start marker of an app region.
fn begin(attr: &str) -> String {
    format!("# nix-app-center:begin {attr}")
}

fn end(attr: &str) -> String {
    format!("# nix-app-center:end {attr}")
}

/// Indents every non-empty line by `n` spaces.
fn indent(body: &str, n: usize) -> String {
    let pad = " ".repeat(n);
    body.lines()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                format!("{pad}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// An app block as a standalone module, so blocks defining the same
/// option merge instead of clashing.
fn module_region(attr: &str, block: &str) -> String {
    format!(
        "{}\n({{ pkgs, lib, config, ... }}: {{\n{}\n}})\n{}",
        begin(attr),
        indent(block.trim_end(), 2),
        end(attr)
    )
}

/// The `imports` list of app regions; empty without any.
fn imports_list(regions: &[String]) -> String {
    if regions.is_empty() {
        return String::new();
    }
    let mut out = String::from("imports = [\n");
    for r in regions {
        out.push_str(&indent(r, 2));
        out.push('\n');
    }
    out.push_str("];\n");
    out
}

/// The app regions of a scope, sorted.
fn regions(kept: &[KeptEntry], scope: fn(&KeptEntry) -> Option<&String>) -> Vec<String> {
    // An empty block means nothing in this scope. A line break in a
    // name would end its marker comment and run as Nix.
    let mut promues: Vec<&KeptEntry> = kept
        .iter()
        .filter(|e| scope(e).is_some_and(|b| !b.trim().is_empty()))
        .filter(|e| !e.package.attr.chars().any(char::is_control))
        .collect();
    promues.sort_by(|a, b| a.package.attr.cmp(&b.package.attr));
    promues
        .iter()
        .map(|e| module_region(&e.package.attr, scope(e).map(String::as_str).unwrap_or("")))
        .collect()
}

/// User scope body: app regions, then the plain package list.
fn home_body(kept: &[KeptEntry], packages: &str) -> String {
    let mut out = imports_list(&regions(kept, |e| e.home_nix.as_ref()));
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&package_list(
        packages,
        kept.iter().filter(|e| in_home_list(e)),
    ));
    out
}

/// System scope body: app regions, then the apps for every user.
fn system_body(kept: &[KeptEntry]) -> String {
    let mut out = imports_list(&regions(kept, |e| e.system_nix.as_ref()));
    if kept.iter().any(|e| e.system_wide) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&package_list(
            "environment.systemPackages",
            kept.iter().filter(|e| e.system_wide),
        ));
    }
    out
}

/// The module's arguments; `inputs` only when an app comes from another
/// flake, with a default so configurations without it still evaluate.
fn module_args(kept: &[KeptEntry]) -> String {
    if kept.iter().any(|e| {
        e.package
            .source
            .as_deref()
            .is_some_and(|s| !s.contains(':'))
    }) {
        "{ pkgs, lib, config, inputs ? { }, ... }:\n".into()
    } else {
        "{ pkgs, lib, config, ... }:\n".into()
    }
}

/// `nix-app-center-home.nix`, shaped by the layout: a Home Manager module,
/// or a NixOS module using `home-manager.users` or `users.users`.
pub(crate) fn render_home(kept: &[KeptEntry], shape: &HomeShape) -> String {
    let body = home_body(kept, &shape.packages_option());
    let mut out = header("User scope.");
    out.push_str(&module_args(kept));
    out.push_str(&pin_bindings(kept.iter().filter(|e| in_home_list(e))));
    out.push_str("{\n");
    match shape {
        HomeShape::NixosHm(user) => {
            out.push_str(&format!("  home-manager.users.{user} = {{\n"));
            out.push_str(&indent(&body, 4));
            out.push_str("\n  };\n");
        }
        HomeShape::HomeManager | HomeShape::NixosUser(_) => {
            out.push_str(&indent(&body, 2));
            out.push('\n');
        }
    }
    out.push_str("}\n");
    out
}

/// `nix-app-center-system.nix`, written even when empty since it is imported.
pub(crate) fn render_system(kept: &[KeptEntry]) -> String {
    let body = system_body(kept);
    let mut out = header("System scope (NixOS).");
    out.push_str(&module_args(kept));
    out.push_str(&pin_bindings(kept.iter().filter(|e| e.system_wide)));
    out.push_str("{\n");
    if !body.trim().is_empty() {
        out.push_str(&indent(body.trim_end(), 2));
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

/// Writes the Nix App Center files for the detected layout; nothing when
/// the layout is unknown.
pub(crate) fn sync_config(kept: &[KeptEntry]) -> std::io::Result<()> {
    sync_to(&detect(), kept)
}

pub(crate) fn sync_to(layout: &Layout, kept: &[KeptEntry]) -> std::io::Result<()> {
    let mut files = Vec::new();
    if let Some(home) = &layout.home {
        files.push((
            home.nix_app_center_file.clone(),
            render_home(kept, &HomeShape::of(layout)).into_bytes(),
        ));
    }
    if let Some(system) = &layout.system {
        files.push((
            system.nix_app_center_file.clone(),
            render_system(kept).into_bytes(),
        ));
    }
    // Skip unchanged files: no needless password prompt.
    files.retain(|(path, body)| std::fs::read(path).ok().as_ref() != Some(body));
    crate::paths::write_files(&files)?;
    for (path, _) in &files {
        crate::paths::track_in_git(path);
    }
    Ok(())
}

/// Result of applying the configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// Finished, with its last output line.
    Done(String),
    /// Started in a terminal.
    Launched,
    /// Needs a terminal and none exists; carries the command to copy.
    NoTerminal(Vec<String>),
    /// Unknown layout: nothing applied.
    UnknownLayout,
}

/// Applies the configuration with the layout's command.
pub fn apply_config() -> std::io::Result<ApplyOutcome> {
    let layout = detect();
    if !layout.is_usable() {
        return Ok(ApplyOutcome::UnknownLayout);
    }
    run_apply(&layout.apply)
}

/// The apply command of the detected layout, `None` when it is unknown.
pub fn apply_argv() -> Option<Vec<String>> {
    let layout = detect();
    layout.is_usable().then_some(layout.apply)
}

/// `argv` asking nixos-rebuild for progress events, also inside a script.
fn with_progress_log(argv: Vec<String>) -> Vec<String> {
    let flags = ["--log-format", "internal-json"];
    let mut out = Vec::with_capacity(argv.len() + 2);
    for arg in argv {
        if arg == "nixos-rebuild" || arg.ends_with("/nixos-rebuild") {
            out.push(arg);
            out.extend(flags.map(String::from));
        } else {
            out.push(arg.replace(
                "nixos-rebuild ",
                "nixos-rebuild --log-format internal-json ",
            ));
        }
    }
    out
}

/// Runs `steps` in order and passes progress and log lines to `on_event`;
/// stops at the first failure. A `sudo` step asks for rights through pkexec.
/// Switching a flake builds as the user (their git access, files they own)
/// and only activates as root. Blocking.
pub fn run_streaming(
    steps: &[Vec<String>],
    on_event: impl Fn(crate::progress::StreamEvent) + Send + Sync + 'static,
) -> std::io::Result<ApplyOutcome> {
    if steps.iter().all(Vec::is_empty) {
        return Ok(ApplyOutcome::UnknownLayout);
    }
    let shared = Shared {
        last: std::sync::Arc::new(std::sync::Mutex::new(String::new())),
        tracker: std::sync::Arc::new(std::sync::Mutex::new(crate::progress::Tracker::default())),
        on_event: std::sync::Arc::new(on_event),
    };
    let mut outcome = Ok(ApplyOutcome::UnknownLayout);
    for step in steps.iter().filter(|s| !s.is_empty()) {
        outcome = match split_flake_switch(step) {
            Some((build, action)) => build_then_activate(&build, &action, &shared),
            None => run_step(step, &shared),
        };
        if !matches!(outcome, Ok(ApplyOutcome::Done(_))) {
            break;
        }
    }
    outcome
}

/// What every step of a run shares.
struct Shared<F> {
    last: std::sync::Arc<std::sync::Mutex<String>>,
    tracker: std::sync::Arc<std::sync::Mutex<crate::progress::Tracker>>,
    on_event: std::sync::Arc<F>,
}

/// For `sudo nixos-rebuild <switch|boot|test> --flake …`: the same command
/// building as the user, and the action to activate.
fn split_flake_switch(step: &[String]) -> Option<(Vec<String>, String)> {
    let (sudo, rest) = step.split_first()?;
    let is_rebuild = rest
        .first()
        .is_some_and(|b| b == "nixos-rebuild" || b.ends_with("/nixos-rebuild"));
    if sudo != "sudo" || !is_rebuild || !rest.iter().any(|a| a == "--flake") {
        return None;
    }
    let i = rest
        .iter()
        .position(|a| matches!(a.as_str(), "switch" | "boot" | "test"))?;
    let mut build = rest.to_vec();
    let action = std::mem::replace(&mut build[i], "build".into());
    Some((build, action))
}

/// Builds as the user, then activates the result as root.
fn build_then_activate<F: Fn(crate::progress::StreamEvent) + Send + Sync + 'static>(
    build: &[String],
    action: &str,
    shared: &Shared<F>,
) -> std::io::Result<ApplyOutcome> {
    use std::os::unix::fs::DirBuilderExt;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let dir = std::env::temp_dir().join(format!(
        "nix-app-center-build-{}-{nanos}",
        std::process::id()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    let built = run_in(build, &dir, shared);
    let out = std::fs::canonicalize(dir.join("result"));
    let _ = std::fs::remove_dir_all(&dir);
    if !matches!(built, Ok(ApplyOutcome::Done(_))) {
        return built;
    }
    let out = out?;
    let out = out.to_string_lossy().to_string();
    let safe = out.starts_with("/nix/store/")
        && out[11..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+-._".contains(&b));
    if !safe {
        return Err(std::io::Error::other(crate::util::n_(
            "The build gave an unexpected result.",
        )));
    }
    let profile = if action == "test" {
        String::new()
    } else {
        format!("nix-env -p /nix/var/nix/profiles/system --set {out} && ")
    };
    let activate = vec![
        "sudo".to_string(),
        "sh".into(),
        "-c".into(),
        format!("{profile}{out}/bin/switch-to-configuration {action}"),
    ];
    run_step(&activate, shared)
}

fn run_step<F: Fn(crate::progress::StreamEvent) + Send + Sync + 'static>(
    argv: &[String],
    shared: &Shared<F>,
) -> std::io::Result<ApplyOutcome> {
    let home = crate::util::home();
    let dir = if home.is_dir() { home } else { "/".into() };
    run_in(argv, &dir, shared)
}

/// Runs one command in `dir`, streaming its output.
fn run_in<F: Fn(crate::progress::StreamEvent) + Send + Sync + 'static>(
    argv: &[String],
    dir: &std::path::Path,
    shared: &Shared<F>,
) -> std::io::Result<ApplyOutcome> {
    use std::io::BufRead;
    let elevated = argv.first().is_some_and(|b| b == "sudo");
    let argv: Vec<String> = match argv.split_first() {
        Some((_, rest)) if elevated => {
            let (Some(pkexec), Some(env)) = (
                crate::launch::find_in_path("pkexec"),
                crate::paths::root_tool("env"),
            ) else {
                return run_apply(argv);
            };
            [
                pkexec.display().to_string(),
                env.display().to_string(),
                format!("PATH={}", crate::paths::ROOT_PATH),
            ]
            .into_iter()
            .chain(rest.iter().cloned())
            .collect()
        }
        Some(_) => argv.to_vec(),
        None => return Ok(ApplyOutcome::UnknownLayout),
    };
    let argv = with_progress_log(argv);
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(dir)
        .env("SHELL", "/bin/sh")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let pump = |reader: Box<dyn std::io::Read + Send>| {
        let (on_event, last, tracker) = (
            shared.on_event.clone(),
            shared.last.clone(),
            shared.tracker.clone(),
        );
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(reader)
                .lines()
                .map_while(Result::ok)
            {
                let mut t = tracker.lock().unwrap_or_else(|e| e.into_inner());
                let text = t.feed(&line);
                let progress = crate::progress::StreamEvent::Progress(t.fraction(), t.phase());
                drop(t);
                if let Some(text) = text {
                    if !text.trim().is_empty() {
                        *last.lock().unwrap_or_else(|e| e.into_inner()) = text.trim().to_string();
                    }
                    on_event(crate::progress::StreamEvent::Line(text));
                }
                on_event(progress);
            }
        })
    };
    let out = pump(Box::new(child.stdout.take().expect("piped stdout")));
    let err = pump(Box::new(child.stderr.take().expect("piped stderr")));
    let status = child.wait()?;
    let _ = (out.join(), err.join());
    let last = shared
        .last
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let error = shared
        .tracker
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .error()
        .map(str::to_string);
    match status.code() {
        Some(0) => Ok(ApplyOutcome::Done(last)),
        // pkexec: 126 dismissed, 127 refused or failed.
        Some(126) if elevated => Err(std::io::Error::other(crate::util::n_(
            "Authentication was cancelled.",
        ))),
        Some(127) if elevated && last.is_empty() => Err(std::io::Error::other(crate::util::n_(
            "Administrator rights were refused.",
        ))),
        _ => Err(std::io::Error::other(error.unwrap_or(last))),
    }
}

pub(crate) fn run_apply(argv: &[String]) -> std::io::Result<ApplyOutcome> {
    let Some((bin, args)) = argv.split_first() else {
        return Ok(ApplyOutcome::UnknownLayout);
    };

    // sudo needs a tty: use a terminal.
    if bin == "sudo" {
        return match crate::launch::spawn_in_terminal(argv)? {
            crate::launch::ShellOutcome::Spawned => Ok(ApplyOutcome::Launched),
            crate::launch::ShellOutcome::NoTerminal => Ok(ApplyOutcome::NoTerminal(argv.to_vec())),
        };
    }

    let out = match Command::new(bin).args(args).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("“{bin}” not found: Nix App Center cannot apply your configuration."),
            ));
        }
        Err(e) => return Err(e),
    };

    if out.status.success() {
        Ok(ApplyOutcome::Done(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .last()
                .unwrap_or("")
                .trim()
                .to_string(),
        ))
    } else {
        Err(std::io::Error::other(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(script: &str) -> Vec<String> {
        vec!["sh".into(), "-c".into(), script.into()]
    }

    #[test]
    fn run_streaming_forwards_lines_and_reports_the_last() {
        let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = lines.clone();
        let res = run_streaming(
            &[argv("echo building >&2; sleep 0.1; echo done")],
            move |e| {
                if let crate::progress::StreamEvent::Line(l) = e {
                    sink.lock().unwrap().push(l)
                }
            },
        );
        assert_eq!(res.unwrap(), ApplyOutcome::Done("done".into()));
        let mut got = lines.lock().unwrap().clone();
        got.sort();
        assert_eq!(got, ["building", "done"]);
    }

    #[test]
    fn nixos_rebuild_is_asked_for_progress_events() {
        let direct = with_progress_log(vec!["nixos-rebuild".into(), "switch".into()]);
        assert_eq!(
            direct,
            ["nixos-rebuild", "--log-format", "internal-json", "switch"]
        );
        let custom = with_progress_log(vec![
            "sh".into(),
            "-c".into(),
            "cd /x && nixos-rebuild switch --flake .#h".into(),
        ]);
        assert_eq!(
            custom[2],
            "cd /x && nixos-rebuild --log-format internal-json switch --flake .#h"
        );
        let other = with_progress_log(vec!["home-manager".into(), "switch".into()]);
        assert_eq!(other, ["home-manager", "switch"]);
    }

    #[test]
    fn a_flake_switch_builds_as_the_user() {
        let step: Vec<String> = ["sudo", "nixos-rebuild", "switch", "--flake", "/etc/nixos#h"]
            .map(String::from)
            .to_vec();
        let (build, action) = split_flake_switch(&step).unwrap();
        assert_eq!(build, ["nixos-rebuild", "build", "--flake", "/etc/nixos#h"]);
        assert_eq!(action, "switch");
        let channels: Vec<String> = ["sudo", "nixos-rebuild", "switch", "--upgrade"]
            .map(String::from)
            .to_vec();
        assert!(
            split_flake_switch(&channels).is_none(),
            "channels need root to update"
        );
    }

    #[test]
    fn steps_stop_at_the_first_failure() {
        let res = run_streaming(&[argv("exit 3"), argv("touch /nonexistent/never")], |_| {});
        assert!(res.is_err());
        let res = run_streaming(&[argv("true"), argv("echo second")], |_| {});
        assert_eq!(res.unwrap(), ApplyOutcome::Done("second".into()));
    }

    #[test]
    fn run_streaming_fails_with_the_last_error_line() {
        let res = run_streaming(&[argv("echo 'error: boom' >&2; exit 1")], |_| {});
        assert_eq!(res.unwrap_err().to_string(), "error: boom");
    }
    use crate::Package;

    fn simple(attr: &str) -> KeptEntry {
        KeptEntry::simple(Package::from_attr(attr))
    }

    fn promue(attr: &str, home: Option<&str>, system: Option<&str>) -> KeptEntry {
        KeptEntry {
            package: Package::from_attr(attr),
            home_nix: home.map(str::to_string),
            system_nix: system.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn home_aggregates_simple_entries_sorted() {
        let kept = vec![simple("ripgrep"), simple("firefox")];
        let out = render_home(&kept, &HomeShape::HomeManager);
        assert!(out.contains("home.packages = with pkgs; ["));
        let fx = out.find("firefox").unwrap();
        let rg = out.find("ripgrep").unwrap();
        assert!(fx < rg, "rendering must be sorted for a stable diff");
        assert!(
            !out.contains("nix-app-center:begin"),
            "no promoted app here"
        );
    }

    #[test]
    fn home_wraps_in_hm_user_when_module() {
        let out = render_home(&[simple("firefox")], &HomeShape::NixosHm("alice".into()));
        assert!(out.contains("home-manager.users.alice = {"));
        assert!(out.contains("home.packages"));
    }

    #[test]
    fn home_is_plain_hm_module_when_standalone() {
        let out = render_home(&[simple("firefox")], &HomeShape::HomeManager);
        assert!(!out.contains("home-manager.users"));
        assert!(out.contains("home.packages"));
    }

    #[test]
    fn nixos_without_home_manager_uses_user_packages() {
        // NixOS has no `home.packages`.
        let out = render_home(&[simple("hello")], &HomeShape::NixosUser("alice".into()));
        assert!(out.contains("users.users.\"alice\".packages = with pkgs; ["));
        assert!(!out.contains("home.packages"));
        assert_eq!(
            default_block(&HomeShape::NixosUser("alice".into()), "hello"),
            "users.users.\"alice\".packages = [ pkgs.hello ];"
        );
    }

    #[test]
    fn shape_follows_the_layout() {
        let layout = |kind, hm_user: Option<&str>| Layout {
            kind,
            home: None,
            system: None,
            apply: vec![],
            hm_user: hm_user.map(str::to_string),
        };
        let dir = std::path::PathBuf::from("/x");
        assert_eq!(
            HomeShape::of(&layout(
                LayoutKind::NixosLegacy { dir: dir.clone() },
                Some("bob")
            )),
            HomeShape::NixosHm("bob".into())
        );
        assert!(matches!(
            HomeShape::of(&layout(LayoutKind::NixosLegacy { dir: dir.clone() }, None)),
            HomeShape::NixosUser(_)
        ));
        assert_eq!(
            HomeShape::of(&layout(LayoutKind::HmStandalone { dir }, None)),
            HomeShape::HomeManager
        );
    }

    fn pinned(attr: &str, rev: &str) -> KeptEntry {
        KeptEntry {
            pin: Some(crate::manifest::Pin {
                rev: rev.into(),
                sha256: "0abc".into(),
                since: 0,
                version: None,
            }),
            ..simple(attr)
        }
    }

    #[test]
    fn apps_held_at_one_revision_share_its_import() {
        let rev = "0123456789abcdef0123456789abcdef01234567";
        let mut gimp = pinned("gimp", rev);
        gimp.flags = vec![("withPython".into(), false)];
        let kept = [
            simple("firefox"),
            pinned("vlc", rev),
            gimp,
            pinned("mpv", &"fedcba9876543210".repeat(3)[..40]),
        ];
        let out = render_home(&kept, &HomeShape::HomeManager);
        assert_eq!(out.matches("builtins.fetchTarball").count(), 2, "{out}");
        assert!(out.contains("nixpkgs-0123456789ab.vlc"));
        assert!(out.contains("(nixpkgs-0123456789ab.gimp.override { withPython = false; })"));
        assert!(out.contains("config = lib.filterAttrs (_: v: v != null) pkgs.config;"));
        assert!(out.contains("  firefox\n"));
        let path =
            std::env::temp_dir().join(format!("nix-app-center-pins-{}.nix", std::process::id()));
        std::fs::write(&path, &out).unwrap();
        let parsed = Command::new("nix-instantiate")
            .args(["--store", "dummy://", "--parse"])
            .arg(&path)
            .output();
        let _ = std::fs::remove_file(&path);
        if let Ok(o) = parsed {
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        }
    }

    #[test]
    fn a_malformed_hold_is_never_written() {
        let bad = pinned("vlc", "\"; builtins.exec [ ]; \"");
        let out = render_home(&[bad], &HomeShape::HomeManager);
        assert!(
            !out.contains("builtins.exec") && !out.contains("fetchTarball"),
            "{out}"
        );
        assert!(out.contains("  vlc\n"));
    }

    #[test]
    fn apps_for_everyone_go_to_the_system_packages() {
        let mut vlc = pinned("vlc", &"0123456789abcdef".repeat(3)[..40]);
        vlc.system_wide = true;
        let kept = [simple("firefox"), vlc];
        let system = render_system(&kept);
        assert!(
            system.contains("environment.systemPackages = with pkgs; ["),
            "{system}"
        );
        assert!(system.contains("nixpkgs-0123456789ab.vlc"));
        let home = render_home(&kept, &HomeShape::HomeManager);
        assert!(
            !home.contains("vlc") && !home.contains("fetchTarball"),
            "{home}"
        );
    }

    #[test]
    fn module_app_lives_only_in_the_system_scope() {
        let kept = [
            simple("firefox"),
            promue("steam", Some(""), Some("programs.steam.enable = true;")),
        ];
        let home = render_home(&kept, &HomeShape::HomeManager);
        assert!(!home.contains("steam"), "the module installs it: {home}");
        assert!(home.contains("firefox"));
        let system = render_system(&kept);
        assert!(system.contains("# nix-app-center:begin steam"));
        assert!(system.contains("programs.steam.enable = true;"));
    }

    #[test]
    fn a_name_cannot_leave_its_marker() {
        let evil = promue("x\nsystem.activationScripts.a = \"id\";", Some("{ }"), None);
        let out = render_home(&[evil], &HomeShape::HomeManager);
        assert!(!out.contains("activationScripts"), "{out}");
    }

    #[test]
    fn promoted_entry_is_a_delimited_region_and_leaves_the_list() {
        let kept = vec![
            simple("firefox"),
            promue(
                "quickshare",
                Some("services.quickshare.enable = true;"),
                None,
            ),
        ];
        let out = render_home(&kept, &HomeShape::HomeManager);
        assert!(out.contains("# nix-app-center:begin quickshare"));
        assert!(out.contains("services.quickshare.enable = true;"));
        assert!(out.contains("# nix-app-center:end quickshare"));
        // Promoted apps leave the list.
        let list = out.split("home.packages").nth(1).unwrap();
        assert!(!list.contains("quickshare"));
        assert!(list.contains("firefox"));
    }

    /// A promoted block and the list both define `home.packages`: they must
    /// sit in separate modules to merge.
    #[test]
    fn promoted_block_does_not_redefine_the_aggregate_list() {
        let kept = vec![
            simple("firefox"),
            promue(
                "quickshare",
                Some(&default_block(&HomeShape::HomeManager, "quickshare")),
                None,
            ),
        ];
        let out = render_home(&kept, &HomeShape::NixosHm("alice".into()));

        assert!(
            out.contains("imports = ["),
            "promoted apps go through imports"
        );
        assert!(
            out.contains("({ pkgs, lib, config, ... }: {"),
            "each region is a standalone module"
        );

        // Each `home.packages` lives in its own attrset.
        let region = out
            .split("# nix-app-center:begin quickshare")
            .nth(1)
            .unwrap()
            .split("# nix-app-center:end")
            .next()
            .unwrap();
        assert!(region.contains("home.packages = [ pkgs.quickshare ];"));
        assert!(region.contains("({ pkgs, lib, config, ... }: {"));
    }

    #[test]
    fn default_block_reproduces_what_the_simple_entry_did() {
        // Promoting starts from the plain entry.
        assert_eq!(
            default_block(&HomeShape::HomeManager, "quickshare"),
            "home.packages = [ pkgs.quickshare ];"
        );
    }

    #[test]
    fn system_holds_only_regions() {
        let kept = vec![
            simple("firefox"),
            promue(
                "quickshare",
                None,
                Some("networking.firewall.allowedTCPPorts = [ 8080 ];"),
            ),
        ];
        let out = render_system(&kept);
        assert!(out.contains("# nix-app-center:begin quickshare"));
        assert!(out.contains("networking.firewall.allowedTCPPorts = [ 8080 ];"));
        assert!(
            !out.contains("firefox"),
            "a plain app writes nothing on the system side"
        );
    }

    #[test]
    fn empty_files_are_still_valid_nix() {
        assert!(render_system(&[]).contains("{ pkgs, lib, config, ... }:"));
        let gaming = KeptEntry {
            system_wide: true,
            ..KeptEntry::simple(Package {
                source: Some("nix-gaming".into()),
                ..Package::from_attr("star-citizen")
            })
        };
        let out = render_system(&[gaming]);
        assert!(out.contains("inputs ? { }"));
        assert!(out.contains(
            "inputs.nix-gaming.packages.${pkgs.stdenv.hostPlatform.system}.star-citizen"
        ));
        let evil = KeptEntry {
            system_wide: true,
            ..KeptEntry::simple(Package {
                source: Some("x".into()),
                ..Package::from_attr("a; }; system.activationScripts.x = \"rm\"; {")
            })
        };
        assert!(!render_system(&[evil]).contains("activationScripts"));
        let extra = KeptEntry {
            system_wide: true,
            ..KeptEntry::simple(Package {
                source: Some(format!("github:fufexan/nix-gaming/{}", "a".repeat(40))),
                ..Package::from_attr("osu-lazer-bin")
            })
        };
        let out = render_system(&[extra]);
        assert!(out.contains("(builtins.getFlake \"github:fufexan/nix-gaming/aaaa"));
        assert!(!out.contains("inputs ? { }"));
        assert!(
            render_home(&[], &HomeShape::HomeManager).contains("home.packages = with pkgs; [ ];")
        );
    }
}
