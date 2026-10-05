//! Running a package without installing it (*try-first*, Onyx's headline
//! action): `nix run` for an app, `nix shell` (in a terminal) for a CLI tool.

use crate::package::Package;
use crate::tried::record_tried;
use std::io::BufRead;
use std::process::{Child, Command, Stdio};

/// Experimental features to enable on every modern `nix` call.
const NIX_FEATURES: [&str; 2] = ["--extra-experimental-features", "nix-command flakes"];

/// Full argv to try `attr` with `nix <sub>` (`run` or `shell`).
///
/// The user asked to try this package from a page that already says whether it
/// is proprietary or broken, so those nixpkgs gates are lifted (`--impure` lets
/// nix read the variables). Insecure packages stay refused: that one needs an
/// explicit opt-in. `env` keeps the variables when a terminal server spawns the
/// command (ptyxis, gnome-terminal), which would drop the caller's environment.
fn try_argv(sub: &str, attr: &str) -> Vec<String> {
    [
        "env",
        "NIXPKGS_ALLOW_UNFREE=1",
        "NIXPKGS_ALLOW_BROKEN=1",
        "nix",
        sub,
        "--impure",
    ]
    .into_iter()
    .map(str::to_string)
    .chain([format!("nixpkgs#{attr}")])
    .chain(NIX_FEATURES.iter().map(|s| s.to_string()))
    .collect()
}

/// The matching `nix run` command (for display / copy).
pub fn run_command(pkg: &Package) -> String {
    format!("nix run nixpkgs#{}", pkg.attr)
}

/// Spawns `nix run nixpkgs#<attr>` without waiting for the application. The
/// first run may download (slow). Records the package in the "tried" history
/// (try-first) along the way. Pass the child to [`run_failure`] from a
/// background thread to learn whether it failed.
pub fn run(pkg: &Package) -> std::io::Result<Child> {
    let argv = try_argv("run", &pkg.attr);
    let child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    // The log must never make the launch fail.
    let _ = record_tried(pkg);
    let attr = pkg.attr.clone();
    std::thread::spawn(move || crate::tried::record_out_paths(&attr));
    Ok(child)
}

/// Waits for a [`run`] child and returns the last line nix printed on stderr
/// when it exits with an error (unknown attr, evaluation refused, build
/// failure…). Blocks until the application exits: call it from a thread.
pub fn run_failure(mut child: Child) -> Option<String> {
    // Drain stderr while waiting, keeping only the last meaningful line.
    let last = child.stderr.take().and_then(|err| {
        std::io::BufReader::new(err)
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.trim().is_empty())
            .last()
    });
    match child.wait() {
        Ok(status) if status.success() => None,
        _ => Some(last.map(|l| l.trim().to_string()).unwrap_or_default()),
    }
}

// ── nix shell ─────────────────────────────────────────────────────────────────

/// The matching `nix shell` command (for display / copy).
pub fn shell_command(pkg: &Package) -> String {
    format!("nix shell nixpkgs#{}", pkg.attr)
}

/// What happened to a shell request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellOutcome {
    /// A terminal was spawned with the package in `PATH`.
    Spawned,
    /// No emulator found: nothing ran. The front end copies
    /// [`shell_command`] to the clipboard and tells the user.
    NoTerminal,
}

/// A terminal emulator and its syntax for "run this command".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Terminal {
    pub bin: &'static str,
    /// Separator before the command (`--`, `-e`…). `None` = command passed directly.
    pub exec_arg: Option<&'static str>,
}

/// By preference. `xdg-terminal-exec` is the specified (freedesktop) way; the
/// rest is a fallback on common emulators.
const TERMINALS: [Terminal; 7] = [
    Terminal {
        bin: "xdg-terminal-exec",
        exec_arg: None,
    },
    Terminal {
        bin: "ptyxis",
        exec_arg: Some("--"),
    },
    Terminal {
        bin: "kgx",
        exec_arg: Some("--"),
    },
    Terminal {
        bin: "gnome-terminal",
        exec_arg: Some("--"),
    },
    Terminal {
        bin: "foot",
        exec_arg: None,
    },
    Terminal {
        bin: "alacritty",
        exec_arg: Some("-e"),
    },
    Terminal {
        bin: "xterm",
        exec_arg: Some("-e"),
    },
];

/// First terminal available according to `exists`. Pure: `exists` is injected
/// so the test does not depend on binaries installed on the machine.
pub(crate) fn pick_terminal(exists: impl Fn(&str) -> bool) -> Option<Terminal> {
    TERMINALS.into_iter().find(|t| exists(t.bin))
}

/// True when `bin` is a file present in `PATH`.
pub(crate) fn in_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

/// Builds the full argv: terminal + separator + command.
fn terminal_argv(term: Terminal, command: &[String]) -> Vec<String> {
    let mut argv = vec![term.bin.to_string()];
    argv.extend(term.exec_arg.map(str::to_string));
    argv.extend_from_slice(command);
    argv
}

/// Runs `command` in a terminal emulator, if one exists.
///
/// Used for two things: `nix shell` (here) and the config engine's
/// `nixos-rebuild switch`, a long build that must stay visible and
/// interruptible rather than hidden behind an opaque `pkexec`.
pub(crate) fn spawn_in_terminal(command: &[String]) -> std::io::Result<ShellOutcome> {
    let Some(term) = pick_terminal(in_path) else {
        return Ok(ShellOutcome::NoTerminal);
    };
    let argv = terminal_argv(term, command);
    Command::new(&argv[0])
        .args(&argv[1..])
        // The terminal has its own tty: inheriting our pipes keeps them open
        // while it lives and makes the caller think it has not finished.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(ShellOutcome::Spawned)
}

/// Opens a terminal with the package in `PATH` (`nix shell nixpkgs#<attr>`).
///
/// `nix run` launches the *main program* and returns: without arguments that
/// is useless for a CLI tool, which wants a shell where the binary is
/// available. `shell` fills that gap, the *try-first* gesture for anything
/// that is not a graphical app.
pub fn shell(pkg: &Package) -> std::io::Result<ShellOutcome> {
    let outcome = spawn_in_terminal(&try_argv("shell", &pkg.attr))?;
    if outcome == ShellOutcome::Spawned {
        let _ = record_tried(pkg);
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_targets_nixpkgs_attr() {
        assert_eq!(
            run_command(&Package::from_attr("ripgrep")),
            "nix run nixpkgs#ripgrep"
        );
    }

    #[test]
    fn shell_command_targets_nixpkgs_attr() {
        assert_eq!(
            shell_command(&Package::from_attr("ripgrep")),
            "nix shell nixpkgs#ripgrep"
        );
    }

    #[test]
    fn try_argv_lifts_unfree_and_broken_but_not_insecure() {
        let argv = try_argv("run", "spotify");
        assert_eq!(
            argv[..3],
            ["env", "NIXPKGS_ALLOW_UNFREE=1", "NIXPKGS_ALLOW_BROKEN=1"]
        );
        assert!(argv.contains(&"--impure".to_string()));
        assert!(argv.contains(&"nixpkgs#spotify".to_string()));
        assert!(!argv.iter().any(|a| a.contains("INSECURE")));
    }

    #[test]
    fn run_failure_reports_the_last_stderr_line() {
        let child = Command::new("sh")
            .args(["-c", "echo first >&2; echo 'error: refused' >&2; exit 1"])
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        assert_eq!(run_failure(child).as_deref(), Some("error: refused"));

        let ok = Command::new("true").stderr(Stdio::piped()).spawn().unwrap();
        assert_eq!(run_failure(ok), None);
    }

    #[test]
    fn pick_terminal_honours_preference_order() {
        // xterm AND ptyxis present: ptyxis wins (higher in the list).
        let picked = pick_terminal(|b| b == "xterm" || b == "ptyxis").unwrap();
        assert_eq!(picked.bin, "ptyxis");
        assert_eq!(picked.exec_arg, Some("--"));
    }

    #[test]
    fn pick_terminal_prefers_xdg_terminal_exec() {
        let picked = pick_terminal(|_| true).unwrap();
        assert_eq!(picked.bin, "xdg-terminal-exec");
        assert_eq!(picked.exec_arg, None);
    }

    #[test]
    fn pick_terminal_none_when_nothing_installed() {
        assert!(pick_terminal(|_| false).is_none());
    }

    #[test]
    fn terminal_argv_inserts_separator_when_needed() {
        let cmd = vec!["nix".to_string(), "shell".to_string()];
        let ptyxis = Terminal {
            bin: "ptyxis",
            exec_arg: Some("--"),
        };
        assert_eq!(
            terminal_argv(ptyxis, &cmd),
            ["ptyxis", "--", "nix", "shell"]
        );

        let foot = Terminal {
            bin: "foot",
            exec_arg: None,
        };
        assert_eq!(terminal_argv(foot, &cmd), ["foot", "nix", "shell"]);
    }
}
