//! Package model + taxonomy + action model. Pure: zero I/O.

use serde::{Deserialize, Serialize};

// ── Package model ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub attr: String,
    pub name: String,
    pub version: String,
    pub description: String,
}

impl Package {
    /// Builds a minimal package from its nixpkgs attribute alone.
    /// Useful for front ends that only have an attr (e.g. `onyx-cli run <attr>`).
    pub fn from_attr(attr: impl Into<String>) -> Self {
        let attr = attr.into();
        Self {
            name: attr.clone(),
            version: String::new(),
            description: String::new(),
            attr,
        }
    }

    /// Package set (attr prefix): `haskellPackages.aeson` gives `haskellPackages`.
    /// `None` for a top-level package (`firefox`). Free (no `nix eval`).
    pub fn package_set(&self) -> Option<&str> {
        self.attr.split_once('.').map(|(prefix, _)| prefix)
    }
}

// ── Taxonomy ──────────────────────────────────────────────────────────────────

/// Kind of package, which decides its primary action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Runnable application (has a binary / `meta.mainProgram`): *try-first*.
    App,
    /// Extension, theme, library: *keep-only* (no `nix run`).
    KeepOnly,
}

impl Package {
    /// Taxonomy inference seam.
    ///
    /// `nix search` JSON has no `meta.mainProgram`; until AppStream / `nix eval`
    /// are cross-checked, every package is treated as `App`. This is where it
    /// will be refined, without touching the front ends.
    pub fn kind(&self) -> Kind {
        Kind::App
    }

    pub fn is_runnable(&self) -> bool {
        self.kind() == Kind::App
    }
}

// ── Action model (the "pill split button" as data) ────────────────────────────

/// An action offered for a package. Front ends decide how to render it (main
/// button, menu entry, shortcut…).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Run without installing: `nix run nixpkgs#<attr>`. The headline action.
    Run,
    /// Open a terminal with the package in `PATH`: `nix shell nixpkgs#<attr>`.
    /// The *try-first* path for CLI tools, which `Run` (launching the main
    /// program and returning) does not serve.
    Shell,
    /// Add to the Onyx manifest (and so to the generated Nix files).
    Keep,
    /// Copy the `nix run` command (the front end owns the clipboard).
    CopyRunCommand,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::Run => "Run",
            Action::Shell => "Try in a Shell",
            Action::Keep => "Add to Configuration",
            Action::CopyRunCommand => "Copy Command",
        }
    }
}

/// A package's actions: 1 primary + N secondary.
pub struct Actions {
    pub primary: Action,
    pub secondary: Vec<Action>,
}

/// Models, per package, the primary + secondary actions from the taxonomy.
pub fn actions_for(pkg: &Package) -> Actions {
    if pkg.is_runnable() {
        Actions {
            primary: Action::Run,
            secondary: vec![Action::Keep, Action::Shell, Action::CopyRunCommand],
        }
    } else {
        Actions {
            primary: Action::Keep,
            secondary: vec![Action::Shell, Action::CopyRunCommand],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_set_returns_prefix_for_dotted_attr() {
        assert_eq!(
            Package::from_attr("haskellPackages.aeson").package_set(),
            Some("haskellPackages")
        );
    }

    #[test]
    fn package_set_none_for_top_level_attr() {
        assert_eq!(Package::from_attr("firefox").package_set(), None);
    }

    #[test]
    fn default_kind_is_app_and_runnable() {
        let pkg = Package::from_attr("ripgrep");
        assert_eq!(pkg.kind(), Kind::App);
        assert!(pkg.is_runnable());
    }

    #[test]
    fn actions_for_runnable_has_run_primary() {
        let actions = actions_for(&Package::from_attr("ripgrep"));
        assert_eq!(actions.primary, Action::Run);
        assert_eq!(
            actions.secondary,
            vec![Action::Keep, Action::Shell, Action::CopyRunCommand]
        );
    }
}
