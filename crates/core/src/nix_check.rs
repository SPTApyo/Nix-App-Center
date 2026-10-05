//! Validation of the Nix written by the user, at two levels: one instant, the
//! other costly but complete.
//!
//! - [`parse_block`]: ~50 ms. Only sees **syntax**. Fast enough to run on every
//!   keystroke.
//! - [`dry_build`]: 30 s to 2 min. Evaluates the whole configuration and catches
//!   *semantic* errors (unknown option, wrong type) that parsing cannot see.
//!   **Optional and user triggered**: forcing it on every edit would make the
//!   editor painful.
//!
//! A failing `nixos-rebuild switch` does not break the system (NixOS keeps the
//! previous generation): what is protected here is not the boot, it is the
//! user's time, finding a typo after the password prompt and three minutes of
//! build.

use crate::config_layout::{Layout, LayoutKind};
use std::process::Command;

/// Wraps a block as a standalone module, as it will be generated, so the parse
/// error matches what will really end up in the file.
fn wrap(block: &str) -> String {
    format!("{{ pkgs, lib, config, ... }}:\n{{\n{block}\n}}\n")
}

/// Checks the **syntax** of a Nix block. Instant.
///
/// `Ok(())` when the block is syntactically valid, `Err(message)` otherwise; the
/// message is Nix's, stripped of the temp file path that would mean nothing.
pub fn parse_block(block: &str) -> Result<(), String> {
    // Unique name per call: parsing runs on every keystroke, and two
    // concurrent checks sharing a temp file delete it under each other.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("onyx-parse-{}-{n}.nix", std::process::id()));
    std::fs::write(&path, wrap(block)).map_err(|e| e.to_string())?;

    let out = Command::new("nix-instantiate")
        .arg("--parse")
        .arg(&path)
        .output();
    let _ = std::fs::remove_file(&path);

    let out = match out {
        Ok(o) => o,
        // Without `nix-instantiate` nothing can be asserted: do not block the
        // user on a check that cannot be done.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };

    if out.status.success() {
        return Ok(());
    }
    let msg = String::from_utf8_lossy(&out.stderr);
    Err(clean_error(&msg, &path.display().to_string()))
}

/// Strips the temp file path from Nix messages: it adds nothing and blurs the
/// useful error.
fn clean_error(stderr: &str, tmp_path: &str) -> String {
    let cleaned = stderr.replace(tmp_path, "<bloc>");
    cleaned
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The result of a `dry-build`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DryRun {
    /// The whole configuration evaluates: the block is fine, options and types included.
    Ok,
    /// Nix refused. The message is its own.
    Failed(String),
    /// No tool to check with (unknown layout, or missing binary).
    Unavailable(String),
}

/// The dry check command matching the layout.
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

/// Evaluates the whole configuration **without applying anything** or asking
/// for a password (`dry-build` neither builds nor switches). Slow: run in a thread.
pub fn dry_build() -> DryRun {
    let layout = crate::config_layout::detect();
    let Some(argv) = dry_argv(&layout) else {
        return DryRun::Unavailable(
            crate::util::n_("Onyx cannot find your Nix configuration.").into(),
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
        // Exactly the bug generation had: the same attribute twice.
        let err = parse_block("home.packages = [ ];\nhome.packages = [ ];").unwrap_err();
        assert!(err.contains("already defined"), "obtenu : {err}");
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
        // A dry-build has no sudo and no switch: it writes nothing, asks nothing.
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
