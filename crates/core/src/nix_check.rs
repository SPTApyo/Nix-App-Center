//! Checks the Nix the user writes: [`parse_block`] for syntax on each
//! keystroke, [`dry_build`] for options and types on demand.

use crate::config_layout::{Layout, LayoutKind};
use std::process::Command;

/// `block` wrapped as the module it will be generated into.
fn wrap(block: &str) -> String {
    format!("{{ pkgs, lib, config, ... }}:\n{{\n{block}\n}}\n")
}

/// Checks the syntax of `block`.
///
/// # Errors
/// Nix's message, without the temporary file path.
pub fn parse_block(block: &str) -> Result<(), String> {
    // Unique file: checks may run concurrently.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("nix-software-parse-{}-{n}.nix", std::process::id()));
    std::fs::write(&path, wrap(block)).map_err(|e| e.to_string())?;

    let out = Command::new("nix-instantiate")
        .args(["--store", "dummy://", "--parse"])
        .arg(&path)
        .output();
    let _ = std::fs::remove_file(&path);

    let out = match out {
        Ok(o) => o,
        // Without nix-instantiate, do not block the user.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };

    if out.status.success() {
        return Ok(());
    }
    let msg = String::from_utf8_lossy(&out.stderr);
    Err(clean_error(&msg, &path.display().to_string()))
}

/// `stderr` without the temporary file path.
fn clean_error(stderr: &str, tmp_path: &str) -> String {
    let cleaned = stderr.replace(tmp_path, "<bloc>");
    cleaned
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Result of a `dry-build`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DryRun {
    /// The configuration evaluates.
    Ok,
    /// Nix refused, with its message.
    Failed(String),
    /// Nothing to check with.
    Unavailable(String),
}

/// The dry check command of the layout.
fn dry_argv(layout: &Layout) -> Option<Vec<String>> {
    match &layout.kind {
        LayoutKind::NixosFlake { dir, host } => Some(vec![
            "nixos-rebuild".into(),
            "dry-build".into(),
            "--flake".into(),
            format!("{}#{}", dir.display(), host),
        ]),
        LayoutKind::NixosLegacy { .. } => Some(vec!["nixos-rebuild".into(), "dry-build".into()]),
        LayoutKind::HmFlake { dir, user } => Some(vec![
            "home-manager".into(),
            "build".into(),
            "-n".into(),
            "--flake".into(),
            format!("{}#{}", dir.display(), user),
        ]),
        LayoutKind::HmStandalone { .. } => {
            Some(vec!["home-manager".into(), "build".into(), "-n".into()])
        }
        LayoutKind::Unknown => None,
    }
}

/// Evaluates the whole configuration without applying it. Blocking.
pub fn dry_build() -> DryRun {
    let layout = crate::config_layout::detect();
    let Some(argv) = dry_argv(&layout) else {
        return DryRun::Unavailable(
            crate::util::n_("Nix Software cannot find your Nix configuration.").into(),
        );
    };

    let out = match Command::new(&argv[0]).args(&argv[1..]).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return DryRun::Unavailable(format!("“{}” not found.", argv[0]));
        }
        Err(e) => return DryRun::Failed(e.to_string()),
    };

    if out.status.success() {
        DryRun::Ok
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        DryRun::Failed(
            stderr
                .lines()
                .filter(|l| !l.trim().is_empty())
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn wrap_produces_a_standalone_module() {
        let out = wrap("home.packages = [ ];");
        assert!(out.starts_with("{ pkgs, lib, config, ... }:"));
        assert!(out.contains("home.packages = [ ];"));
    }

    #[test]
    fn accepts_valid_nix() {
        assert_eq!(parse_block("home.packages = [ pkgs.ripgrep ];"), Ok(()));
    }

    #[test]
    fn accepts_a_multi_attribute_block() {
        let block = "services.foo = {\n  enable = true;\n  port = 8080;\n};";
        assert_eq!(parse_block(block), Ok(()));
    }

    #[test]
    fn rejects_a_missing_semicolon() {
        // The most common Nix mistake.
        let err = parse_block("home.packages = [ pkgs.ripgrep ]").unwrap_err();
        assert!(!err.is_empty());
        assert!(!err.contains("/tmp"), "the temp path must not leak: {err}");
    }

    #[test]
    fn rejects_a_duplicate_attribute() {
        // The same attribute twice.
        let err = parse_block("home.packages = [ ];\nhome.packages = [ ];").unwrap_err();
        assert!(err.contains("already defined"), "got: {err}");
    }

    #[test]
    fn dry_argv_matches_the_layout() {
        let flake = Layout {
            kind: LayoutKind::NixosFlake {
                dir: PathBuf::from("/etc/nixos"),
                host: "laptop".into(),
            },
            home: None,
            system: None,
            apply: vec![],
            hm_user: None,
        };
        assert_eq!(
            dry_argv(&flake).unwrap(),
            ["nixos-rebuild", "dry-build", "--flake", "/etc/nixos#laptop"]
        );
        // A dry build needs no rights.
        assert!(!dry_argv(&flake).unwrap().contains(&"sudo".to_string()));
        assert!(!dry_argv(&flake).unwrap().contains(&"switch".to_string()));
    }

    #[test]
    fn no_dry_run_without_a_layout() {
        let unknown = Layout {
            kind: LayoutKind::Unknown,
            home: None,
            system: None,
            apply: vec![],
            hm_user: None,
        };
        assert!(dry_argv(&unknown).is_none());
    }
}
