//! Nix Software CLI, a text front end over the core.
//!
//! Usage:
//!   nix-software-cli search <query…>   list matching packages
//!   nix-software-cli info <attr>       rich metadata (nix eval meta)
//!   nix-software-cli run <attr>        `nix run nixpkgs#<attr>` (try-first)
//!   nix-software-cli shell <attr>      `nix shell` in a terminal (try-first for CLIs)
//!   nix-software-cli keep <attr>       add to the Nix Software manifest
//!   nix-software-cli unkeep <attr>     remove from the manifest
//!   nix-software-cli updates           nixpkgs pin vs its branch head, app changes
//!   nix-software-cli versions <attr>   earlier versions and their nixpkgs revision
//!   nix-software-cli locate <bin>      packages providing a binary (nix-locate)
//!   nix-software-cli layout [--verify] what Nix Software detects of your config (read only)
//!   nix-software-cli attach [--commit] consented attach (diff; --commit to write)
//!   nix-software-cli detach            remove the Nix Software import line
//!   nix-software-cli edit <attr>       read a Nix block (home scope) on stdin, then set_blocks
//!   nix-software-cli apply             apply the config (command derived from the layout)
//!   nix-software-cli gc                store garbage collection (nix-collect-garbage)
//!   nix-software-cli list              print the manifest

use nix_software_core::Package;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("search") => {
            let query = args[2..].join(" ");
            let pkgs = nix_software_core::search(&query);
            if pkgs.is_empty() {
                eprintln!("No results for “{query}”");
                return ExitCode::FAILURE;
            }
            for p in pkgs {
                let action = nix_software_core::actions_for(&p).primary.label();
                println!(
                    "{:<36} {:<12} [{action}]  {}",
                    p.attr, p.version, p.description
                );
            }
            ExitCode::SUCCESS
        }

        Some("info") => match args.get(2) {
            Some(attr) => match nix_software_core::detail(attr) {
                Some(d) => {
                    let kind = if d.is_runnable() {
                        "App (runnable)"
                    } else {
                        "Keep-only"
                    };
                    println!("attr        : {attr}");
                    println!("type        : {kind}");
                    if let Some(mp) = &d.main_program {
                        println!("mainProgram : {mp}");
                    }
                    if let Some(h) = &d.homepage {
                        println!("homepage    : {h}");
                    }
                    if let Some(l) = &d.license {
                        println!("license     : {l}");
                    }
                    if let Some(c) = &d.changelog {
                        println!("changelog   : {c}");
                    }
                    if let Some(ld) = &d.long_description {
                        println!("\n{}", ld.trim());
                    }
                    ExitCode::SUCCESS
                }
                None => {
                    eprintln!("no metadata for “{attr}”");
                    ExitCode::FAILURE
                }
            },
            None => usage(),
        },

        Some("run") => match args.get(2) {
            Some(attr) => {
                let pkg = Package::from_attr(attr);
                eprintln!("→ {}", nix_software_core::run_command(&pkg, false));
                match nix_software_core::run(&pkg).map(nix_software_core::run_failure) {
                    Ok(None) => ExitCode::SUCCESS,
                    Ok(Some(err)) => {
                        eprintln!("{attr} failed: {err}");
                        ExitCode::FAILURE
                    }
                    Err(e) => {
                        eprintln!("could not start: {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            None => usage(),
        },

        Some("shell") => match args.get(2) {
            Some(attr) => {
                let pkg = Package::from_attr(attr);
                eprintln!("→ {}", nix_software_core::shell_command(&pkg, false));
                match nix_software_core::shell(&pkg) {
                    Ok(nix_software_core::ShellOutcome::Spawned) => ExitCode::SUCCESS,
                    Ok(nix_software_core::ShellOutcome::NoTerminal) => {
                        eprintln!("no terminal found; run the command above yourself");
                        ExitCode::FAILURE
                    }
                    Err(e) => {
                        eprintln!("could not open a shell: {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            None => usage(),
        },

        Some("keep") => match args.get(2) {
            Some(attr) => {
                let pkg = Package::from_attr(attr);
                match nix_software_core::keep(&pkg) {
                    Ok(()) => {
                        println!("kept: {attr}");
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("failed: {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            None => usage(),
        },

        Some("unkeep") => match args.get(2) {
            Some(attr) => match nix_software_core::unkeep(attr) {
                Ok(()) => {
                    println!("removed: {attr}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("failed: {e}");
                    ExitCode::FAILURE
                }
            },
            None => usage(),
        },

        Some("updates") => {
            let source = nix_software_core::current_source();
            println!(
                "nixpkgs: {} at {} ({:?})",
                source.branch.as_deref().unwrap_or("unknown branch"),
                source.rev.as_deref().unwrap_or("unknown revision"),
                source.method
            );
            let Some(latest) = source.branch.as_deref().and_then(nix_software_core::latest) else {
                eprintln!("cannot reach the branch head");
                return ExitCode::FAILURE;
            };
            if source.rev.as_deref() == Some(latest.rev.as_str()) {
                println!("(up to date)");
                return ExitCode::SUCCESS;
            }
            println!("latest: {}", latest.rev);
            for u in nix_software_core::app_updates(
                &latest.rev,
                &nix_software_core::updatable_apps(&source),
            ) {
                println!("{}: {} → {}", u.app.pname, u.app.version, u.new_version);
            }
            match nix_software_core::update_command(&source, &nix_software_core::detect_layout()) {
                Some(argv) => println!("update with: {}", argv.join(" ")),
                None => println!("update the flake that pins nixpkgs, then apply"),
            }
            ExitCode::SUCCESS
        }

        Some("versions") => match args.get(2) {
            Some(attr) => {
                let found = nix_software_core::versions(attr);
                if found.is_empty() {
                    eprintln!("no known versions of {attr}");
                    return ExitCode::FAILURE;
                }
                for v in found {
                    let cache = if v.cached { "cached" } else { "builds locally" };
                    println!("{}\t{}\t{}\t{cache}", v.version, v.date, v.rev);
                }
                ExitCode::SUCCESS
            }
            None => usage(),
        },
        Some("locate") => match args.get(2) {
            Some(cmd) => {
                let pkgs = nix_software_core::locate_binary(cmd);
                if pkgs.is_empty() {
                    eprintln!("no package provides “{cmd}” (or nix-locate is missing)");
                    return ExitCode::FAILURE;
                }
                for p in pkgs {
                    println!("{}", p.attr);
                }
                ExitCode::SUCCESS
            }
            None => usage(),
        },

        Some("layout") => {
            let l = nix_software_core::detect_layout();
            println!("layout: {:?}", l.kind);
            if args.get(2).map(String::as_str) == Some("--verify") {
                match nix_software_core::verify_layout(&l) {
                    Ok(v) => println!(
                        "nix: hosts {:?}, home-manager users {:?}",
                        v.hosts, v.hm_users
                    ),
                    Err(e) => println!("nix: cannot evaluate: {e}"),
                }
            }
            match &l.hm_user {
                Some(u) => println!("home-manager: NixOS module, user “{u}”"),
                None => println!("home-manager: not detected as a NixOS module"),
            }
            for (name, scope) in [("home", &l.home), ("system", &l.system)] {
                match scope {
                    Some(s) => println!(
                        "{name} scope: {} (imported from {})",
                        s.nix_software_file.display(),
                        s.import_site.display()
                    ),
                    None => println!("{name} scope: unavailable"),
                }
            }
            if l.apply.is_empty() {
                println!("apply: none (unknown layout)");
            } else {
                println!("apply: {}", l.apply.join(" "));
            }
            ExitCode::SUCCESS
        }

        // Writes only with `--commit`.
        Some("attach") => {
            let layout = nix_software_core::detect_layout();
            let plan = nix_software_core::attach_plan(&layout);
            let commit = args.iter().any(|a| a == "--commit");
            match &plan {
                nix_software_core::AttachPlan::Patch {
                    site, backup, diff, ..
                } => {
                    println!(
                        "attach to {} (backup: {})",
                        site.display(),
                        backup.display()
                    );
                    print!("{diff}");
                }
                nix_software_core::AttachPlan::Manual { site, snippet } => {
                    println!(
                        "no top-level `imports` list in {}; add it yourself:\n{snippet}",
                        site.display()
                    );
                    return ExitCode::SUCCESS;
                }
                nix_software_core::AttachPlan::AlreadyAttached => {
                    println!("already attached, nothing to do.");
                    return ExitCode::SUCCESS;
                }
                nix_software_core::AttachPlan::NoLayout => {
                    eprintln!("Nix configuration not found, nothing to attach to.");
                    return ExitCode::FAILURE;
                }
            }
            if commit {
                match nix_software_core::attach_commit(&plan) {
                    Ok(()) => {
                        println!("→ attached.");
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("attach failed: {e}");
                        ExitCode::FAILURE
                    }
                }
            } else {
                println!("(run again with --commit to actually write)");
                ExitCode::SUCCESS
            }
        }

        Some("detach") => {
            let layout = nix_software_core::detect_layout();
            match nix_software_core::detach(&layout) {
                Ok(()) => {
                    println!("detached: the Nix Software import line was removed.");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }

        // Empty stdin makes the app plain again.
        Some("edit") => match args.get(2) {
            Some(attr) => {
                use std::io::Read;
                let mut block = String::new();
                let _ = std::io::stdin().read_to_string(&mut block);
                let block = block.trim().to_string();
                let home = (!block.is_empty()).then(|| block.clone());
                if let Some(b) = &home {
                    if let Err(e) = nix_software_core::parse_block(b) {
                        eprintln!("invalid Nix syntax:\n{e}");
                        return ExitCode::FAILURE;
                    }
                }
                let pkg = Package::from_attr(attr);
                match nix_software_core::set_blocks(&pkg, home, None) {
                    Ok(()) => {
                        println!("blocks saved for {attr}");
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("failed: {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            None => usage(),
        },

        Some("gc") => match nix_software_core::garbage_collect() {
            Ok(summary) => {
                println!(
                    "{}",
                    if summary.is_empty() {
                        "Nothing to free."
                    } else {
                        &summary
                    }
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("failed: {e}");
                ExitCode::FAILURE
            }
        },

        Some("apply") => {
            use nix_software_core::ApplyOutcome::*;
            match nix_software_core::apply_config() {
                Ok(Done(line)) => {
                    println!("configuration applied: {line}");
                    ExitCode::SUCCESS
                }
                Ok(Launched) => {
                    println!("applying in a terminal");
                    ExitCode::SUCCESS
                }
                Ok(NoTerminal(argv)) => {
                    eprintln!("no terminal found; run it yourself:\n  {}", argv.join(" "));
                    ExitCode::FAILURE
                }
                Ok(UnknownLayout) => {
                    eprintln!("Nix configuration not found, nothing was applied");
                    ExitCode::FAILURE
                }
                Err(e) => {
                    eprintln!("failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }

        Some("list") => {
            let manifest = nix_software_core::load_manifest();
            if manifest.kept.is_empty() {
                println!("(empty manifest)");
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
        "usage: nix-software-cli <search <query…> | info <attr> | run <attr> | shell <attr> | keep <attr> | unkeep <attr> | updates | versions <attr> | locate <bin> | layout | attach [--commit] | detach | edit <attr> | apply | gc | list>"
    );
    ExitCode::FAILURE
}
