//! Onyx CLI: text front end of `core`, a test bench for the
//! search → try → keep loop. No logic here: everything comes from `onyx_core`.
//!
//! Usage:
//!   onyx-cli search <query…>   list matching packages
//!   onyx-cli info <attr>       rich metadata (nix eval meta)
//!   onyx-cli run <attr>        `nix run nixpkgs#<attr>` (try-first)
//!   onyx-cli shell <attr>      `nix shell` in a terminal (try-first for CLIs)
//!   onyx-cli keep <attr>       add to the Onyx manifest
//!   onyx-cli unkeep <attr>     remove from the manifest
//!   onyx-cli updates           nixpkgs pin vs its branch head, app changes
//!   onyx-cli locate <bin>      packages providing a binary (nix-locate)
//!   onyx-cli layout [--verify] what Onyx detects of your config (read only)
//!   onyx-cli attach [--commit] consented attach (diff; --commit to write)
//!   onyx-cli detach            remove the Onyx import line
//!   onyx-cli edit <attr>       read a Nix block (home scope) on stdin, then set_blocks
//!   onyx-cli apply             apply the config (command derived from the layout)
//!   onyx-cli gc                store garbage collection (nix-collect-garbage)
//!   onyx-cli list              print the manifest

use onyx_core::Package;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("search") => {
            let query = args[2..].join(" ");
            let pkgs = onyx_core::search(&query);
            if pkgs.is_empty() {
                eprintln!("No results for “{query}”");
                return ExitCode::FAILURE;
            }
            for p in pkgs {
                let action = onyx_core::actions_for(&p).primary.label();
                println!(
                    "{:<36} {:<12} [{action}]  {}",
                    p.attr, p.version, p.description
                );
            }
            ExitCode::SUCCESS
        }

        Some("info") => match args.get(2) {
            Some(attr) => match onyx_core::detail(attr) {
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
                eprintln!("→ {}", onyx_core::run_command(&pkg, false));
                match onyx_core::run(&pkg).map(onyx_core::run_failure) {
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
                eprintln!("→ {}", onyx_core::shell_command(&pkg, false));
                match onyx_core::shell(&pkg) {
                    Ok(onyx_core::ShellOutcome::Spawned) => ExitCode::SUCCESS,
                    Ok(onyx_core::ShellOutcome::NoTerminal) => {
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
                match onyx_core::keep(&pkg) {
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
            Some(attr) => match onyx_core::unkeep(attr) {
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
            let source = onyx_core::current_source();
            println!(
                "nixpkgs: {} at {} ({:?})",
                source.branch.as_deref().unwrap_or("unknown branch"),
                source.rev.as_deref().unwrap_or("unknown revision"),
                source.method
            );
            let Some(latest) = source.branch.as_deref().and_then(onyx_core::latest) else {
                eprintln!("cannot reach the branch head");
                return ExitCode::FAILURE;
            };
            if source.rev.as_deref() == Some(latest.rev.as_str()) {
                println!("(up to date)");
                return ExitCode::SUCCESS;
            }
            println!("latest: {}", latest.rev);
            for u in onyx_core::app_updates(&latest.rev, &onyx_core::profile_apps()) {
                println!("{}: {} → {}", u.app.pname, u.app.version, u.new_version);
            }
            match onyx_core::update_command(&source, &onyx_core::detect_layout()) {
                Some(argv) => println!("update with: {}", argv.join(" ")),
                None => println!("update the flake that pins nixpkgs, then apply"),
            }
            ExitCode::SUCCESS
        }

        Some("locate") => match args.get(2) {
            Some(cmd) => {
                let pkgs = onyx_core::locate_binary(cmd);
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

        // Diagnostic: what Onyx believes about your config. Read only.
        Some("layout") => {
            let l = onyx_core::detect_layout();
            println!("layout: {:?}", l.kind);
            if args.get(2).map(String::as_str) == Some("--verify") {
                match onyx_core::verify_layout(&l) {
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
                        s.onyx_file.display(),
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

        // Consented attach: shows the diff, writes ONLY with `--commit`.
        Some("attach") => {
            let layout = onyx_core::detect_layout();
            let plan = onyx_core::attach_plan(&layout);
            let commit = args.iter().any(|a| a == "--commit");
            match &plan {
                onyx_core::AttachPlan::Patch {
                    site, backup, diff, ..
                } => {
                    println!(
                        "attach to {} (backup: {})",
                        site.display(),
                        backup.display()
                    );
                    print!("{diff}");
                }
                onyx_core::AttachPlan::Manual { site, snippet } => {
                    println!(
                        "no top-level `imports` list in {}; add it yourself:\n{snippet}",
                        site.display()
                    );
                    return ExitCode::SUCCESS;
                }
                onyx_core::AttachPlan::AlreadyAttached => {
                    println!("already attached, nothing to do.");
                    return ExitCode::SUCCESS;
                }
                onyx_core::AttachPlan::NoLayout => {
                    eprintln!("Nix configuration not found, nothing to attach to.");
                    return ExitCode::FAILURE;
                }
            }
            if commit {
                match onyx_core::attach_commit(&plan) {
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
            let layout = onyx_core::detect_layout();
            match onyx_core::detach(&layout) {
                Ok(()) => {
                    println!("detached: the Onyx import line was removed.");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }

        // Reads a home scope Nix block on stdin, parses it, saves it.
        // Empty stdin turns the app back into a plain list entry.
        Some("edit") => match args.get(2) {
            Some(attr) => {
                use std::io::Read;
                let mut block = String::new();
                let _ = std::io::stdin().read_to_string(&mut block);
                let block = block.trim().to_string();
                let home = (!block.is_empty()).then(|| block.clone());
                if let Some(b) = &home {
                    if let Err(e) = onyx_core::parse_block(b) {
                        eprintln!("invalid Nix syntax:\n{e}");
                        return ExitCode::FAILURE;
                    }
                }
                let pkg = Package::from_attr(attr);
                match onyx_core::set_blocks(&pkg, home, None) {
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

        // Store garbage collection (frees unreferenced packages).
        Some("gc") => match onyx_core::garbage_collect() {
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
            use onyx_core::ApplyOutcome::*;
            match onyx_core::apply_config() {
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
            let manifest = onyx_core::load_manifest();
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
        "usage: onyx-cli <search <query…> | info <attr> | run <attr> | shell <attr> | keep <attr> | unkeep <attr> | updates | locate <bin> | layout | attach [--commit] | detach | edit <attr> | apply | gc | list>"
    );
    ExitCode::FAILURE
}
