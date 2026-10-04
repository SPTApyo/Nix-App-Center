//! Onyx CLI — façade texte du `core`, banc d'essai de la boucle
//! chercher → essayer → garder. Aucune logique ici : tout vient de `onyx_core`.
//!
//! Usage :
//!   onyx-cli search <query…>   liste les paquets correspondants
//!   onyx-cli info <attr>       métadonnées enrichies (nix eval meta)
//!   onyx-cli run <attr>        `nix run nixpkgs#<attr>` (try-first)
//!   onyx-cli shell <attr>      `nix shell` dans un terminal (try-first des CLI)
//!   onyx-cli keep <attr>       ajoute au manifeste Onyx
//!   onyx-cli unkeep <attr>     retire du manifeste
//!   onyx-cli updates           versions gardées vs nixpkgs courant
//!   onyx-cli locate <bin>      paquets fournissant un binaire (nix-locate)
//!   onyx-cli attach [--commit] greffe consentie (diff ; --commit pour écrire)
//!   onyx-cli detach            retire la ligne d'import Onyx
//!   onyx-cli edit <attr>       lit un bloc Nix (portée home) sur stdin → set_blocks
//!   onyx-cli apply             applique la config (commande déduite de la disposition)
//!   onyx-cli gc                ramasse-miettes du store (nix-collect-garbage)
//!   onyx-cli list              affiche le manifeste

use onyx_core::Package;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("search") => {
            let query = args[2..].join(" ");
            let pkgs = onyx_core::search(&query);
            if pkgs.is_empty() {
                eprintln!("Aucun résultat pour « {query} »");
                return ExitCode::FAILURE;
            }
            for p in pkgs {
                let action = onyx_core::actions_for(&p).primary.label();
                println!("{:<36} {:<12} [{action}]  {}", p.attr, p.version, p.description);
            }
            ExitCode::SUCCESS
        }

        Some("info") => match args.get(2) {
            Some(attr) => match onyx_core::detail(attr) {
                Some(d) => {
                    let kind = if d.is_runnable() { "App (runnable)" } else { "Keep-only" };
                    println!("attr        : {attr}");
                    println!("type        : {kind}");
                    if let Some(mp) = &d.main_program { println!("mainProgram : {mp}"); }
                    if let Some(h) = &d.homepage { println!("homepage    : {h}"); }
                    if let Some(l) = &d.license { println!("licence     : {l}"); }
                    if let Some(c) = &d.changelog { println!("changelog   : {c}"); }
                    if let Some(ld) = &d.long_description {
                        println!("\n{}", ld.trim());
                    }
                    ExitCode::SUCCESS
                }
                None => {
                    eprintln!("pas de métadonnées pour « {attr} »");
                    ExitCode::FAILURE
                }
            },
            None => usage(),
        },

        Some("run") => match args.get(2) {
            Some(attr) => {
                let pkg = Package::from_attr(attr);
                eprintln!("→ {}", onyx_core::run_command(&pkg));
                match onyx_core::run(&pkg) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("échec du lancement : {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            None => usage(),
        },

        Some("shell") => match args.get(2) {
            Some(attr) => {
                let pkg = Package::from_attr(attr);
                eprintln!("→ {}", onyx_core::shell_command(&pkg));
                match onyx_core::shell(&pkg) {
                    Ok(onyx_core::ShellOutcome::Spawned) => ExitCode::SUCCESS,
                    Ok(onyx_core::ShellOutcome::NoTerminal) => {
                        eprintln!("aucun terminal trouvé — lance la commande ci-dessus toi-même");
                        ExitCode::FAILURE
                    }
                    Err(e) => {
                        eprintln!("échec du shell : {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            None => usage(),
        },

        Some("keep") => match args.get(2) {
            Some(attr) => {
                let pkg = Package::from_attr(attr);
                match onyx_core::keep(&pkg) {
                    Ok(()) => {
                        println!("gardé : {attr}");
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("échec : {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            None => usage(),
        },

        Some("unkeep") => match args.get(2) {
            Some(attr) => match onyx_core::unkeep(attr) {
                Ok(()) => {
                    println!("retiré : {attr}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("échec : {e}");
                    ExitCode::FAILURE
                }
            },
            None => usage(),
        },

        Some("updates") => {
            let ups = onyx_core::available_updates();
            if ups.is_empty() {
                println!("(tout est à jour)");
            } else {
                for u in ups {
                    println!("{}: {} → {}", u.package.attr, u.installed_version, u.available_version);
                }
            }
            ExitCode::SUCCESS
        }

        Some("locate") => match args.get(2) {
            Some(cmd) => {
                let pkgs = onyx_core::locate_binary(cmd);
                if pkgs.is_empty() {
                    eprintln!("aucun paquet ne fournit « {cmd} » (ou nix-locate absent)");
                    return ExitCode::FAILURE;
                }
                for p in pkgs {
                    println!("{}", p.attr);
                }
                ExitCode::SUCCESS
            }
            None => usage(),
        },

        // Diagnostic : ce qu'Onyx croit de ta config. Lecture seule.
        Some("layout") => {
            let l = onyx_core::detect_layout();
            println!("disposition : {:?}", l.kind);
            match &l.hm_user {
                Some(u) => println!("home-manager : module NixOS, utilisateur « {u} »"),
                None => println!("home-manager : pas détecté comme module NixOS"),
            }
            for (nom, scope) in [("home", &l.home), ("système", &l.system)] {
                match scope {
                    Some(s) => println!(
                        "portée {nom} : {} (import depuis {})",
                        s.onyx_file.display(),
                        s.import_site.display()
                    ),
                    None => println!("portée {nom} : indisponible"),
                }
            }
            if l.apply.is_empty() {
                println!("apply : aucun (disposition inconnue)");
            } else {
                println!("apply : {}", l.apply.join(" "));
            }
            ExitCode::SUCCESS
        }

        // Greffe consentie : montre le diff, n'écrit QUE sur `--commit`. Banc d'essai du
        // chemin réel (attach_plan → attach_commit) sans passer par la GUI.
        Some("attach") => {
            let layout = onyx_core::detect_layout();
            let plan = onyx_core::attach_plan(&layout);
            let commit = args.iter().any(|a| a == "--commit");
            match &plan {
                onyx_core::AttachPlan::Patch { site, backup, diff, .. } => {
                    println!("greffe sur {} (backup : {})", site.display(), backup.display());
                    print!("{diff}");
                }
                onyx_core::AttachPlan::Manual { site, snippet } => {
                    println!(
                        "pas de liste `imports` au plus haut niveau de {} — ajoute toi-même :\n{snippet}",
                        site.display()
                    );
                    return ExitCode::SUCCESS;
                }
                onyx_core::AttachPlan::AlreadyAttached => {
                    println!("déjà greffé — rien à faire.");
                    return ExitCode::SUCCESS;
                }
                onyx_core::AttachPlan::NoLayout => {
                    eprintln!("config Nix introuvable — rien à greffer.");
                    return ExitCode::FAILURE;
                }
            }
            if commit {
                match onyx_core::attach_commit(&plan) {
                    Ok(()) => {
                        println!("→ greffé.");
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("échec de la greffe : {e}");
                        ExitCode::FAILURE
                    }
                }
            } else {
                println!("(relance avec --commit pour écrire réellement)");
                ExitCode::SUCCESS
            }
        }

        Some("detach") => {
            let layout = onyx_core::detect_layout();
            match onyx_core::detach(&layout) {
                Ok(()) => {
                    println!("dégreffé — la ligne d'import Onyx a été retirée.");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("échec : {e}");
                    ExitCode::FAILURE
                }
            }
        }

        // Édition avancée en banc d'essai : lit un bloc Nix (portée home) sur stdin, le
        // valide (parse instantané), puis l'enregistre. Stdin vide → l'app redevient
        // une simple entrée de liste (bloc retiré).
        Some("edit") => match args.get(2) {
            Some(attr) => {
                use std::io::Read;
                let mut block = String::new();
                let _ = std::io::stdin().read_to_string(&mut block);
                let block = block.trim().to_string();
                let home = (!block.is_empty()).then(|| block.clone());
                if let Some(b) = &home {
                    if let Err(e) = onyx_core::parse_block(b) {
                        eprintln!("syntaxe Nix invalide :\n{e}");
                        return ExitCode::FAILURE;
                    }
                }
                let pkg = Package::from_attr(attr);
                match onyx_core::set_blocks(&pkg, home, None) {
                    Ok(()) => {
                        println!("blocs enregistrés pour {attr}");
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("échec : {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            None => usage(),
        },

        // Ramasse-miettes du store (libère l'espace des paquets non-référencés).
        Some("gc") => match onyx_core::garbage_collect() {
            Ok(summary) => {
                println!("{}", if summary.is_empty() { "Rien à libérer." } else { &summary });
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("échec : {e}");
                ExitCode::FAILURE
            }
        },

        Some("apply") => {
            use onyx_core::ApplyOutcome::*;
            match onyx_core::apply_config() {
                Ok(Done(line)) => {
                    println!("config appliquée : {line}");
                    ExitCode::SUCCESS
                }
                Ok(Launched) => {
                    println!("application lancée dans un terminal");
                    ExitCode::SUCCESS
                }
                Ok(NoTerminal(argv)) => {
                    eprintln!("aucun terminal trouvé — lance toi-même :\n  {}", argv.join(" "));
                    ExitCode::FAILURE
                }
                Ok(UnknownLayout) => {
                    eprintln!("config Nix introuvable — rien n'a été appliqué");
                    ExitCode::FAILURE
                }
                Err(e) => {
                    eprintln!("échec : {e}");
                    ExitCode::FAILURE
                }
            }
        }

        Some("list") => {
            let manifest = onyx_core::load_manifest();
            if manifest.kept.is_empty() {
                println!("(manifeste vide)");
            } else {
                for p in manifest.packages() {
                    println!("{:<36} {}", p.attr, p.description);
                }
            }
            ExitCode::SUCCESS
        }

        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage : onyx-cli <search <query…> | info <attr> | run <attr> | shell <attr> | keep <attr> | unkeep <attr> | updates | locate <bin> | attach [--commit] | detach | edit <attr> | apply | gc | list>"
    );
    ExitCode::FAILURE
}
