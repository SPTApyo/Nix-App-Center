//! Génération des fichiers Onyx (`onyx-home.nix`, `onyx-system.nix`) et application
//! de la config.
//!
//! **Onyx n'écrase jamais rien.** Il possède ces deux fichiers, entièrement générés
//! depuis le manifeste, et ne touche à aucun fichier écrit par l'utilisateur — sa seule
//! intrusion est la ligne d'`imports` posée une fois par [`crate::config_attach`], avec
//! consentement explicite.
//!
//! Une app **simple** est une entrée agrégée dans `home.packages`. Une app **promue**
//! (l'utilisateur a édité son bloc Nix) est émise comme région délimitée par des
//! marqueurs, ce qui la rend individuellement repérable, remplaçable et supprimable.

use crate::config_layout::{detect, Layout};
use crate::manifest::KeptEntry;
use std::process::Command;

/// Ce que rend un bloc quand on promeut une app depuis une entrée simple : exactement
/// ce que faisait l'entrée simple. L'utilisateur part de l'existant et l'édite — rien
/// ne disparaît sans qu'il le voie.
pub fn default_home_block(attr: &str) -> String {
    format!("home.packages = [ pkgs.{attr} ];")
}

fn header(warn: &str) -> String {
    format!("# {warn}\n# Généré par Onyx — ne pas éditer à la main.\n")
}

/// Ouvre la région d'une app promue. Les marqueurs rendent le diff lisible et la
/// suppression sûre.
fn begin(attr: &str) -> String {
    format!("# onyx:begin {attr}")
}

fn end(attr: &str) -> String {
    format!("# onyx:end {attr}")
}

/// Indente chaque ligne non vide de `n` espaces.
fn indent(body: &str, n: usize) -> String {
    let pad = " ".repeat(n);
    body.lines()
        .map(|l| if l.trim().is_empty() { String::new() } else { format!("{pad}{l}") })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Le bloc d'une app promue, enrobé en **module Nix à part entière**.
///
/// C'est ce qui rend l'édition libre possible : deux blocs (ou un bloc et la liste
/// agrégée) qui définissent tous deux `home.packages` sont *fusionnés* par le système de
/// modules, alors que côte à côte dans un même attrset ils seraient une redéfinition —
/// « attribute 'home.packages' already defined », et le fichier ne parse même pas.
fn module_region(attr: &str, block: &str) -> String {
    format!(
        "{}\n({{ pkgs, lib, config, ... }}: {{\n{}\n}})\n{}",
        begin(attr),
        indent(block.trim_end(), 2),
        end(attr)
    )
}

/// La liste `imports` des apps promues. Vide s'il n'y en a aucune.
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

/// Les régions des apps promues pour une portée donnée, triées (diff stable).
fn regions(kept: &[KeptEntry], scope: fn(&KeptEntry) -> Option<&String>) -> Vec<String> {
    let mut promues: Vec<&KeptEntry> = kept.iter().filter(|e| scope(e).is_some()).collect();
    promues.sort_by(|a, b| a.package.attr.cmp(&b.package.attr));
    promues
        .iter()
        .map(|e| module_region(&e.package.attr, scope(e).map(String::as_str).unwrap_or("")))
        .collect()
}

/// Le corps de la portée utilisateur : les modules des apps promues, puis la liste
/// agrégée des apps simples.
fn home_body(kept: &[KeptEntry]) -> String {
    let mut simples: Vec<&str> = kept
        .iter()
        .filter(|e| e.home_nix.is_none())
        .map(|e| e.package.attr.as_str())
        .collect();
    simples.sort_unstable();
    simples.dedup();

    let mut out = imports_list(&regions(kept, |e| e.home_nix.as_ref()));
    if !out.is_empty() {
        out.push('\n');
    }

    if simples.is_empty() {
        out.push_str("home.packages = with pkgs; [ ];\n");
    } else {
        out.push_str("home.packages = with pkgs; [\n");
        for attr in simples {
            out.push_str(&format!("  {attr}\n"));
        }
        out.push_str("];\n");
    }
    out
}

/// Le corps de la portée système : uniquement des modules (pas d'équivalent « liste
/// simple » — on n'écrit du système que délibérément).
fn system_body(kept: &[KeptEntry]) -> String {
    imports_list(&regions(kept, |e| e.system_nix.as_ref()))
}

/// Rend `onyx-home.nix`.
///
/// Sa **forme dépend de la disposition** : quand home-manager est un module NixOS, le
/// fichier est un module *NixOS* qui enrobe son contenu dans `home-manager.users.<user>`
/// — c'est ce qui permet de le greffer sur le `imports` top-level de `configuration.nix`,
/// la splice la plus sûre. En home-manager standalone, c'est un module HM ordinaire.
pub(crate) fn render_home(kept: &[KeptEntry], hm_user: Option<&str>) -> String {
    let body = home_body(kept);
    let mut out = header("Portée utilisateur (home-manager).");
    out.push_str("{ pkgs, lib, config, ... }:\n{\n");
    match hm_user {
        Some(user) => {
            out.push_str(&format!("  home-manager.users.{user} = {{\n"));
            out.push_str(&indent(&body, 4));
            out.push_str("\n  };\n");
        }
        None => {
            out.push_str(&indent(&body, 2));
            out.push('\n');
        }
    }
    out.push_str("}\n");
    out
}

/// Rend `onyx-system.nix`. Généré même vide : la ligne d'import ne doit jamais pointer
/// dans le vide.
pub(crate) fn render_system(kept: &[KeptEntry]) -> String {
    let body = system_body(kept);
    let mut out = header("Portée système (NixOS).");
    out.push_str("{ pkgs, lib, config, ... }:\n{\n");
    if !body.trim().is_empty() {
        out.push_str(&indent(body.trim_end(), 2));
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

/// Régénère les fichiers Onyx depuis le manifeste, selon la disposition détectée.
///
/// Si la disposition est inconnue, **rien n'est écrit** : Onyx ne sème pas un fichier
/// orphelin que rien n'importe (c'était précisément le bug d'origine). Le manifeste,
/// lui, est bien conservé — c'est la façade qui alerte l'utilisateur.
pub(crate) fn sync_config(kept: &[KeptEntry]) -> std::io::Result<()> {
    sync_to(&detect(), kept)
}

pub(crate) fn sync_to(layout: &Layout, kept: &[KeptEntry]) -> std::io::Result<()> {
    if let Some(home) = &layout.home {
        let body = render_home(kept, layout.hm_user.as_deref());
        crate::paths::write_atomic(&home.onyx_file, body.as_bytes())?;
    }
    if let Some(system) = &layout.system {
        let body = render_system(kept);
        crate::paths::write_atomic(&system.onyx_file, body.as_bytes())?;
    }
    Ok(())
}

// ── Application ───────────────────────────────────────────────────────────────

/// Ce qu'il est advenu d'une application de la config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// Exécuté jusqu'au bout, sortie capturée (cas sans `sudo`).
    Done(String),
    /// Lancé dans un terminal : le build est long et réclame un mot de passe, il doit
    /// rester visible et interruptible. L'utilisateur suit la sortie là-bas.
    Launched,
    /// `sudo` requis mais aucun terminal trouvé. La façade copie l'argv et le dit ;
    /// le core ne touche pas au presse-papier.
    NoTerminal(Vec<String>),
    /// Disposition inconnue : Onyx ignore où est ta config, il ne devine pas.
    UnknownLayout,
}

/// Applique la config avec **la commande qui correspond à la disposition réelle**.
///
/// Codait jusqu'ici `home-manager switch` en dur — faux sur une installation NixOS où
/// home-manager est un module (le binaire standalone n'existe pas), c'est-à-dire le cas
/// le plus courant.
pub fn apply_config() -> std::io::Result<ApplyOutcome> {
    let layout = detect();
    if !layout.is_usable() {
        return Ok(ApplyOutcome::UnknownLayout);
    }
    run_apply(&layout.apply)
}

fn run_apply(argv: &[String]) -> std::io::Result<ApplyOutcome> {
    let Some((bin, args)) = argv.split_first() else {
        return Ok(ApplyOutcome::UnknownLayout);
    };

    // `sudo` réclame un tty pour le mot de passe : le capturer en pipe le ferait échouer
    // (ou pendre) en silence. On passe donc par un vrai terminal, où l'utilisateur voit
    // le build et peut l'interrompre.
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
                format!("« {bin} » introuvable — Onyx ne peut pas appliquer ta config."),
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
    use crate::Package;

    fn simple(attr: &str) -> KeptEntry {
        KeptEntry::simple(Package::from_attr(attr))
    }

    fn promue(attr: &str, home: Option<&str>, system: Option<&str>) -> KeptEntry {
        KeptEntry {
            package: Package::from_attr(attr),
            home_nix: home.map(str::to_string),
            system_nix: system.map(str::to_string),
        }
    }

    #[test]
    fn home_aggregates_simple_entries_sorted() {
        let kept = vec![simple("ripgrep"), simple("firefox")];
        let out = render_home(&kept, None);
        assert!(out.contains("home.packages = with pkgs; ["));
        let fx = out.find("firefox").unwrap();
        let rg = out.find("ripgrep").unwrap();
        assert!(fx < rg, "le rendu doit être trié pour un diff stable");
        assert!(!out.contains("onyx:begin"), "aucune app promue ici");
    }

    #[test]
    fn home_wraps_in_hm_user_when_module() {
        let out = render_home(&[simple("firefox")], Some("alice"));
        assert!(out.contains("home-manager.users.alice = {"));
        assert!(out.contains("home.packages"));
    }

    #[test]
    fn home_is_plain_hm_module_when_standalone() {
        let out = render_home(&[simple("firefox")], None);
        assert!(!out.contains("home-manager.users"));
        assert!(out.contains("home.packages"));
    }

    #[test]
    fn promoted_entry_is_a_delimited_region_and_leaves_the_list() {
        let kept = vec![
            simple("firefox"),
            promue("quickshare", Some("services.quickshare.enable = true;"), None),
        ];
        let out = render_home(&kept, None);
        assert!(out.contains("# onyx:begin quickshare"));
        assert!(out.contains("services.quickshare.enable = true;"));
        assert!(out.contains("# onyx:end quickshare"));
        // Promue = sortie de la liste agrégée : c'est son module qui décide désormais.
        let list = out.split("home.packages").nth(1).unwrap();
        assert!(!list.contains("quickshare"));
        assert!(list.contains("firefox"));
    }

    /// Le bloc promu par défaut redéfinit `home.packages`, que la liste agrégée définit
    /// déjà. Dans un même attrset, Nix refuse — « attribute already defined », le fichier
    /// ne parse même pas. Chaque app promue doit donc être un module distinct, que le
    /// système de modules fusionne.
    #[test]
    fn promoted_block_does_not_redefine_the_aggregate_list() {
        let kept = vec![
            simple("firefox"),
            promue("quickshare", Some(&default_home_block("quickshare")), None),
        ];
        let out = render_home(&kept, Some("alice"));

        assert!(out.contains("imports = ["), "les apps promues passent par imports");
        assert!(
            out.contains("({ pkgs, lib, config, ... }: {"),
            "chaque région est un module à part entière"
        );

        // Une seule définition de `home.packages` par attrset : celle du module promu est
        // dans le sien, celle de la liste agrégée dans l'attrset parent.
        let region = out
            .split("# onyx:begin quickshare")
            .nth(1)
            .unwrap()
            .split("# onyx:end")
            .next()
            .unwrap();
        assert!(region.contains("home.packages = [ pkgs.quickshare ];"));
        assert!(region.contains("({ pkgs, lib, config, ... }: {"));
    }

    #[test]
    fn default_block_reproduces_what_the_simple_entry_did() {
        // Promouvoir ne doit rien faire disparaître en silence : on part de l'existant.
        assert_eq!(default_home_block("quickshare"), "home.packages = [ pkgs.quickshare ];");
    }

    #[test]
    fn system_holds_only_regions() {
        let kept = vec![
            simple("firefox"),
            promue("quickshare", None, Some("networking.firewall.allowedTCPPorts = [ 8080 ];")),
        ];
        let out = render_system(&kept);
        assert!(out.contains("# onyx:begin quickshare"));
        assert!(out.contains("networking.firewall.allowedTCPPorts = [ 8080 ];"));
        assert!(!out.contains("firefox"), "une app simple n'écrit rien côté système");
    }

    #[test]
    fn empty_files_are_still_valid_nix() {
        assert!(render_system(&[]).contains("{ pkgs, lib, config, ... }:"));
        assert!(render_home(&[], None).contains("home.packages = with pkgs; [ ];"));
    }
}
