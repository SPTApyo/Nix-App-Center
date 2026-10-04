//! Validation du Nix écrit par l'utilisateur, à deux niveaux — l'un instantané, l'autre
//! coûteux mais complet.
//!
//! - [`parse_block`] : ~50 ms. Ne voit que la **syntaxe**. Assez rapide pour tourner à
//!   chaque frappe.
//! - [`dry_build`] : de 30 s à 2 min. Évalue toute la config et attrape les erreurs
//!   *sémantiques* — option inconnue, type faux — que le parse ne peut pas voir.
//!   **Optionnel et déclenché par l'utilisateur** : l'imposer à chaque édition rendrait
//!   l'éditeur pénible.
//!
//! Un `nixos-rebuild switch` qui échoue ne casse pas le système (NixOS garde la
//! génération précédente) : ce qu'on protège ici, ce n'est pas le boot, c'est le temps
//! de l'utilisateur — découvrir une faute de frappe après le mot de passe et trois
//! minutes de build.

use crate::config_layout::{Layout, LayoutKind};
use std::process::Command;

/// Enrobe un bloc en module autonome, tel qu'il sera généré, pour que l'erreur de parse
/// corresponde à ce qui finira vraiment dans le fichier.
fn wrap(block: &str) -> String {
    format!("{{ pkgs, lib, config, ... }}:\n{{\n{block}\n}}\n")
}

/// Vérifie la **syntaxe** d'un bloc Nix. Instantané.
///
/// `Ok(())` si le bloc est syntaxiquement valide, `Err(message)` sinon — le message est
/// celui de Nix, épuré du chemin du fichier temporaire qui ne dirait rien à personne.
pub fn parse_block(block: &str) -> Result<(), String> {
    // Nom unique par appel : le parse tourne à chaque frappe, et deux vérifications
    // concurrentes qui partagent un fichier temporaire se le suppriment sous les pieds.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("onyx-parse-{}-{n}.nix", std::process::id()));
    std::fs::write(&path, wrap(block)).map_err(|e| e.to_string())?;

    let out = Command::new("nix-instantiate")
        .arg("--parse")
        .arg(&path)
        .output();
    let _ = std::fs::remove_file(&path);

    let out = match out {
        Ok(o) => o,
        // Sans `nix-instantiate`, on ne peut rien affirmer : ne pas bloquer l'utilisateur
        // sur une vérification qu'on est incapable de faire.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };

    if out.status.success() {
        return Ok(());
    }
    let msg = String::from_utf8_lossy(&out.stderr);
    Err(clean_error(&msg, &path.display().to_string()))
}

/// Retire le chemin du fichier temporaire des messages de Nix : il n'apporte rien et
/// brouille l'erreur utile.
fn clean_error(stderr: &str, tmp_path: &str) -> String {
    let cleaned = stderr.replace(tmp_path, "<bloc>");
    cleaned
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Le résultat d'un `dry-build`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DryRun {
    /// La config entière évalue : le bloc est bon, options et types compris.
    Ok,
    /// Nix a refusé. Le message est le sien.
    Failed(String),
    /// Aucun outil pour vérifier (disposition inconnue, ou binaire absent).
    Unavailable(String),
}

/// La commande de vérification à sec correspondant à la disposition.
fn dry_argv(layout: &Layout) -> Option<Vec<String>> {
    match &layout.kind {
        LayoutKind::NixosFlake { dir, host } => Some(vec![
            "nixos-rebuild".into(),
            "dry-build".into(),
            "--flake".into(),
            format!("{}#{}", dir.display(), host),
        ]),
        LayoutKind::NixosLegacy { .. } => {
            Some(vec!["nixos-rebuild".into(), "dry-build".into()])
        }
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

/// Évalue toute la config **sans rien appliquer** ni demander de mot de passe
/// (`dry-build` ne construit ni ne bascule). Long : à lancer dans un thread.
pub fn dry_build() -> DryRun {
    let layout = crate::config_layout::detect();
    let Some(argv) = dry_argv(&layout) else {
        return DryRun::Unavailable("Onyx ne trouve pas ta config Nix.".into());
    };

    let out = match Command::new(&argv[0]).args(&argv[1..]).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return DryRun::Unavailable(format!("« {} » introuvable.", argv[0]));
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
        // La faute la plus courante en Nix.
        let err = parse_block("home.packages = [ pkgs.ripgrep ]").unwrap_err();
        assert!(!err.is_empty());
        assert!(!err.contains("/tmp"), "le chemin temporaire ne doit pas fuiter : {err}");
    }

    #[test]
    fn rejects_a_duplicate_attribute() {
        // Exactement le bug qu'avait la génération : deux fois le même attribut.
        let err = parse_block("home.packages = [ ];\nhome.packages = [ ];").unwrap_err();
        assert!(err.contains("already defined"), "obtenu : {err}");
    }

    #[test]
    fn dry_argv_matches_the_layout() {
        let flake = Layout {
            kind: LayoutKind::NixosFlake {
                dir: PathBuf::from("/etc/nixos"),
                host: "Zenbook".into(),
            },
            home: None,
            system: None,
            apply: vec![],
            hm_user: None,
        };
        assert_eq!(
            dry_argv(&flake).unwrap(),
            ["nixos-rebuild", "dry-build", "--flake", "/etc/nixos#Zenbook"]
        );
        // Un dry-build n'a ni sudo ni switch : il n'écrit rien, ne demande rien.
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
