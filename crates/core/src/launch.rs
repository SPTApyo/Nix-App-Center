//! Lancer un paquet sans l'installer (*try-first*, l'action vedette d'Onyx) :
//! `nix run` pour une app, `nix shell` (dans un terminal) pour un outil CLI.

use crate::package::Package;
use crate::tried::record_tried;
use std::process::Command;

/// Les features expérimentales à activer sur chaque appel `nix` moderne.
const NIX_FEATURES: [&str; 2] = ["--extra-experimental-features", "nix-command flakes"];

/// La commande `nix run` correspondante (pour affichage / copie).
pub fn run_command(pkg: &Package) -> String {
    format!("nix run nixpkgs#{}", pkg.attr)
}

/// Lance `nix run nixpkgs#<attr>` en sous-processus détaché : ne bloque pas sur
/// la sortie de l'application. Le premier lancement peut télécharger (lent).
/// Journalise le paquet dans l'historique « essayés » (try-first) au passage.
pub fn run(pkg: &Package) -> std::io::Result<()> {
    let child = Command::new("nix")
        .arg("run")
        .arg(format!("nixpkgs#{}", pkg.attr))
        .args(NIX_FEATURES)
        .spawn();

    if child.is_ok() {
        // Le journal ne doit jamais faire échouer le lancement.
        let _ = record_tried(pkg);
    }
    child.map(|_| ())
}

// ── nix shell ─────────────────────────────────────────────────────────────────

/// La commande `nix shell` correspondante (pour affichage / copie).
pub fn shell_command(pkg: &Package) -> String {
    format!("nix shell nixpkgs#{}", pkg.attr)
}

/// Ce qu'il est advenu d'une demande de shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellOutcome {
    /// Un terminal a été lancé avec le paquet dans le `PATH`.
    Spawned,
    /// Aucun émulateur trouvé : rien n'a été exécuté. À la façade de copier
    /// [`shell_command`] dans le presse-papier et de le dire à l'utilisateur.
    NoTerminal,
}

/// Un émulateur de terminal et sa syntaxe pour « exécute cette commande ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Terminal {
    pub bin: &'static str,
    /// Séparateur avant la commande (`--`, `-e`…). `None` = commande passée directement.
    pub exec_arg: Option<&'static str>,
}

/// Par ordre de préférence. `xdg-terminal-exec` est la voie spécifiée (freedesktop) ;
/// le reste est un repli sur les émulateurs courants.
const TERMINALS: [Terminal; 7] = [
    Terminal { bin: "xdg-terminal-exec", exec_arg: None },
    Terminal { bin: "ptyxis", exec_arg: Some("--") },
    Terminal { bin: "kgx", exec_arg: Some("--") },
    Terminal { bin: "gnome-terminal", exec_arg: Some("--") },
    Terminal { bin: "foot", exec_arg: None },
    Terminal { bin: "alacritty", exec_arg: Some("-e") },
    Terminal { bin: "xterm", exec_arg: Some("-e") },
];

/// Premier terminal disponible selon `exists`. Pur : `exists` est injecté pour que le
/// test ne dépende pas des binaires réellement installés sur la machine.
pub(crate) fn pick_terminal(exists: impl Fn(&str) -> bool) -> Option<Terminal> {
    TERMINALS.into_iter().find(|t| exists(t.bin))
}

/// Vrai si `bin` est un fichier présent dans le `PATH`.
pub(crate) fn in_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

/// Construit l'argv complet : terminal + séparateur + commande.
fn terminal_argv(term: Terminal, command: &[String]) -> Vec<String> {
    let mut argv = vec![term.bin.to_string()];
    argv.extend(term.exec_arg.map(str::to_string));
    argv.extend_from_slice(command);
    argv
}

/// Lance `command` dans un émulateur de terminal, s'il en existe un.
///
/// Sert à deux choses : `nix shell` (ici) et, plus tard, le `nixos-rebuild switch` du
/// moteur de config — un build long qui doit rester visible et interruptible plutôt que
/// caché derrière un `pkexec` opaque.
pub(crate) fn spawn_in_terminal(command: &[String]) -> std::io::Result<ShellOutcome> {
    let Some(term) = pick_terminal(in_path) else {
        return Ok(ShellOutcome::NoTerminal);
    };
    let argv = terminal_argv(term, command);
    Command::new(&argv[0])
        .args(&argv[1..])
        // Le terminal a son propre tty : lui laisser hériter de nos pipes les maintient
        // ouverts tant qu'il vit, et fait croire à l'appelant qu'il n'a pas fini.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(ShellOutcome::Spawned)
}

/// Ouvre un terminal avec le paquet dans le `PATH` (`nix shell nixpkgs#<attr>`).
///
/// `nix run` lance le *main program* et rend la main : sans argument, c'est inutile pour
/// un outil CLI, qui veut un shell où le binaire est disponible. C'est ce trou que
/// comble `shell` — le geste *try-first* pour tout ce qui n'est pas une app graphique.
pub fn shell(pkg: &Package) -> std::io::Result<ShellOutcome> {
    let command: Vec<String> = ["nix", "shell", &format!("nixpkgs#{}", pkg.attr)]
        .iter()
        .map(|s| s.to_string())
        .chain(NIX_FEATURES.iter().map(|s| s.to_string()))
        .collect();

    let outcome = spawn_in_terminal(&command)?;
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
        assert_eq!(run_command(&Package::from_attr("ripgrep")), "nix run nixpkgs#ripgrep");
    }

    #[test]
    fn shell_command_targets_nixpkgs_attr() {
        assert_eq!(shell_command(&Package::from_attr("ripgrep")), "nix shell nixpkgs#ripgrep");
    }

    #[test]
    fn pick_terminal_honours_preference_order() {
        // xterm ET ptyxis présents → ptyxis gagne (plus haut dans la liste).
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
        let ptyxis = Terminal { bin: "ptyxis", exec_arg: Some("--") };
        assert_eq!(terminal_argv(ptyxis, &cmd), ["ptyxis", "--", "nix", "shell"]);

        let foot = Terminal { bin: "foot", exec_arg: None };
        assert_eq!(terminal_argv(foot, &cmd), ["foot", "nix", "shell"]);
    }
}
