//! Modèle de paquet + taxonomie + modèle d'actions. Pur : zéro I/O.

use serde::{Deserialize, Serialize};

// ── Modèle de paquet ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub attr: String,
    pub name: String,
    pub version: String,
    pub description: String,
}

impl Package {
    /// Construit un paquet minimal à partir de son seul attribut nixpkgs.
    /// Utile pour les façades qui n'ont qu'un attr (ex. `onyx-cli run <attr>`).
    pub fn from_attr(attr: impl Into<String>) -> Self {
        let attr = attr.into();
        Self {
            name: attr.clone(),
            version: String::new(),
            description: String::new(),
            attr,
        }
    }

    /// Jeu de paquets (préfixe de l'attr) : `haskellPackages.aeson` → `haskellPackages`.
    /// `None` pour un paquet top-level (`firefox`). Gratuit (pas de `nix eval`).
    pub fn package_set(&self) -> Option<&str> {
        self.attr.split_once('.').map(|(prefix, _)| prefix)
    }
}

// ── Taxonomie ─────────────────────────────────────────────────────────────────

/// Nature d'un paquet, qui détermine son action primaire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Application lançable (a un binaire / `meta.mainProgram`) → *try-first*.
    App,
    /// Extension, thème, bibliothèque → *keep-only* (pas de `nix run`).
    KeepOnly,
}

impl Package {
    /// Couture d'inférence de la taxonomie.
    ///
    /// Le JSON de `nix search` ne porte pas `meta.mainProgram` ; tant qu'on n'a
    /// pas croisé AppStream / `nix eval`, on considère tout paquet comme `App`.
    /// C'est ICI qu'on raffinera (Jalon 2+), sans toucher aux façades.
    pub fn kind(&self) -> Kind {
        Kind::App
    }

    pub fn is_runnable(&self) -> bool {
        self.kind() == Kind::App
    }
}

// ── Modèle d'actions (le « split-button pilule » en données) ──────────────────

/// Une action proposée pour un paquet. Les façades décident du rendu (bouton
/// principal, entrée de menu, raccourci…).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Lancer sans installer : `nix run nixpkgs#<attr>`. L'action vedette.
    Run,
    /// Ouvrir un terminal avec le paquet dans le `PATH` : `nix shell nixpkgs#<attr>`.
    /// Le *try-first* des outils CLI, que `Run` (qui lance le main program et rend la
    /// main) ne sert pas.
    Shell,
    /// Ajouter au manifeste Onyx (→ `store-packages.nix` + home-manager plus tard).
    Keep,
    /// Copier la commande `nix run` (le presse-papier est géré par la façade).
    CopyRunCommand,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::Run => "Lancer",
            Action::Shell => "Essayer dans un shell",
            Action::Keep => "Ajouter à ma config",
            Action::CopyRunCommand => "Copier la commande",
        }
    }
}

/// L'ensemble des actions d'un paquet : 1 primaire + N secondaires.
pub struct Actions {
    pub primary: Action,
    pub secondary: Vec<Action>,
}

/// Modélise, par paquet, l'action primaire + les secondaires selon la taxonomie.
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
        assert_eq!(Package::from_attr("haskellPackages.aeson").package_set(), Some("haskellPackages"));
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
