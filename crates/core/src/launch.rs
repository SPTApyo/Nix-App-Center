//! Trying packages without installing: `nix run` for apps, `nix shell` in a
//! terminal for command line tools.

use crate::package::Package;
use crate::tried::record_tried;
use std::io::BufRead;
use std::process::{Child, Command, Stdio};

const NIX_FEATURES: [&str; 2] = ["--extra-experimental-features", "nix-command flakes"];

/// argv of `nix <sub>` (`run` or `shell`) for `attr`. Unfree and broken
/// packages are allowed, insecure ones are not; `env` keeps the variables
/// when a terminal server spawns the command.
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
    .chain([target(attr)])
    .chain(NIX_FEATURES.iter().map(|s| s.to_string()))
    .collect()
}

/// The `nix run` command to copy; `unfree` adds the needed opt-in.
pub fn run_command(pkg: &Package, unfree: bool) -> String {
    copy_command("run", &target(&pkg.attr), unfree)
}

fn target(attr: &str) -> String {
    format!("{}#{attr}", crate::nixpkgs_ref())
}

fn copy_command(sub: &str, target: &str, unfree: bool) -> String {
    if unfree {
        format!("NIXPKGS_ALLOW_UNFREE=1 nix {sub} --impure {target}")
    } else {
        format!("nix {sub} {target}")
    }
}

/// Starts `nix run` for `pkg` without waiting and records the try. Give
/// the child to [`run_failure`] to learn whether it failed.
pub fn run(pkg: &Package) -> std::io::Result<Child> {
    let argv = try_argv("run", &pkg.attr);
    let child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let _ = record_tried(pkg);
    let attr = pkg.attr.clone();
    std::thread::spawn(move || crate::tried::record_out_paths(&attr));
    Ok(child)
}

/// The last nix error line when the `run` child fails. Blocks until exit.
pub fn run_failure(mut child: Child) -> Option<String> {
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

/// The `nix shell` command to copy.
pub fn shell_command(pkg: &Package, unfree: bool) -> String {
    copy_command("shell", &target(&pkg.attr), unfree)
}

/// Result of a shell request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellOutcome {
    /// A terminal was spawned with the package in `PATH`.
    Spawned,
    /// No terminal emulator: nothing ran.
    NoTerminal,
}

/// A terminal emulator and how it takes a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Terminal {
    pub bin: &'static str,
    /// Argument before the command (`--`, `-e`), if any.
    pub exec_arg: Option<&'static str>,
}

/// Terminals by preference, `xdg-terminal-exec` first.
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

/// The first terminal `exists` accepts.
pub(crate) fn pick_terminal(exists: impl Fn(&str) -> bool) -> Option<Terminal> {
    TERMINALS.into_iter().find(|t| exists(t.bin))
}

/// True when `bin` is in `PATH`.
pub(crate) fn in_path(bin: &str) -> bool {
    find_in_path(bin).is_some()
}

/// Full path of `bin` in `PATH`.
pub(crate) fn find_in_path(bin: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(bin))
        .find(|p| p.is_file())
}

/// argv running `command` in `term`.
fn terminal_argv(term: Terminal, command: &[String]) -> Vec<String> {
    let mut argv = vec![term.bin.to_string()];
    argv.extend(term.exec_arg.map(str::to_string));
    argv.extend_from_slice(command);
    argv
}

/// Runs `command` in a terminal emulator, if there is one.
pub(crate) fn spawn_in_terminal(command: &[String]) -> std::io::Result<ShellOutcome> {
    let Some(term) = pick_terminal(in_path) else {
        return Ok(ShellOutcome::NoTerminal);
    };
    let argv = terminal_argv(term, command);
    Command::new(&argv[0])
        .args(&argv[1..])
        // Our pipes would keep the caller waiting.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(ShellOutcome::Spawned)
}

/// Opens a terminal where `pkg` is in `PATH`.
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
    fn copy_command_adds_the_unfree_opt_in_only_when_needed() {
        assert_eq!(
            copy_command("run", "nixpkgs#ripgrep", false),
            "nix run nixpkgs#ripgrep"
        );
        assert_eq!(
            copy_command("shell", "nixpkgs#steam", true),
            "NIXPKGS_ALLOW_UNFREE=1 nix shell --impure nixpkgs#steam"
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
        assert!(argv.iter().any(|a| a.ends_with("#spotify")));
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
        // The higher terminal in the list wins.
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
