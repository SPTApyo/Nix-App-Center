//! Why a store path is still there: the roots that keep it alive, grouped,
//! and the chain of packages from the running system down to it.

use std::process::Command;

/// What keeps a path alive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keeper {
    /// System generations, by number.
    System(Vec<u32>),
    /// Home Manager generations, by number.
    HomeManager(Vec<u32>),
    /// The system running now.
    CurrentSystem,
    /// The system the computer started with.
    BootedSystem,
    /// The Home Manager setup in use.
    CurrentHome,
    /// The user's nix profile.
    UserProfile,
    /// A program running now.
    Running,
    /// Another root: a result link, a dev shell.
    Other(String),
}

/// Why `path` is there: its keepers, and the chain from the running system
/// (empty when the system does not use it). Blocking.
pub fn why(path: &str) -> (Vec<Keeper>, Vec<String>) {
    let roots = Command::new("nix-store")
        .args(["--query", "--roots", path])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let keepers = keepers(&roots);
    let chain = Command::new("nix")
        .args([
            "--extra-experimental-features",
            "nix-command",
            "why-depends",
        ])
        .arg("/run/current-system")
        .arg(path)
        .env("NO_COLOR", "1")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| chain(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    (keepers, chain)
}

/// Groups `nix-store --query --roots` lines (`link -> target`).
fn keepers(roots: &str) -> Vec<Keeper> {
    let mut system = Vec::new();
    let mut home = Vec::new();
    let mut out = Vec::new();
    let push = |k: Keeper, out: &mut Vec<Keeper>| {
        if !out.contains(&k) {
            out.push(k);
        }
    };
    for line in roots.lines() {
        let link = line.split(" -> ").next().unwrap_or(line).trim();
        let name = link.rsplit('/').next().unwrap_or(link);
        let number = |base: &str| {
            name.strip_prefix(base)?
                .strip_prefix('-')?
                .strip_suffix("-link")?
                .parse::<u32>()
                .ok()
        };
        if link.starts_with("/proc/") {
            push(Keeper::Running, &mut out);
        } else if link == "/run/current-system" {
            push(Keeper::CurrentSystem, &mut out);
        } else if link == "/run/booted-system" {
            push(Keeper::BootedSystem, &mut out);
        } else if link.ends_with("/home-manager/gcroots/current-home") {
            push(Keeper::CurrentHome, &mut out);
        } else if let Some(n) = number("system") {
            system.push(n);
        } else if let Some(n) = number("home-manager") {
            home.push(n);
        } else if number("profile").is_some() || name == ".nix-profile" {
            push(Keeper::UserProfile, &mut out);
        } else if !link.is_empty() {
            push(Keeper::Other(link.to_string()), &mut out);
        }
    }
    let mut groups = Vec::new();
    if !system.is_empty() {
        system.sort_unstable();
        groups.push(Keeper::System(system));
    }
    if !home.is_empty() {
        home.sort_unstable();
        groups.push(Keeper::HomeManager(home));
    }
    groups.extend(out);
    groups
}

/// The package names of a `nix why-depends` tree, top down.
fn chain(tree: &str) -> Vec<String> {
    tree.lines()
        .filter_map(|l| {
            let path = &l[l.find("/nix/store/")?..];
            let base = path.trim_start_matches("/nix/store/");
            let name = base.split_once('-').map_or(base, |(_, n)| n);
            // Drop anything after the store path, colors included.
            Some(name.split(['\u{1b}', ' ', '/']).next()?.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_the_roots() {
        let roots = "/nix/var/nix/profiles/system-38-link -> /nix/store/a-nixos-system\n\
                     /nix/var/nix/profiles/system-12-link -> /nix/store/b-nixos-system\n\
                     /proc/4500/maps -> /nix/store/c-espeak\n\
                     /proc/4511/maps -> /nix/store/c-espeak\n\
                     /home/me/.local/state/nix/profiles/home-manager-3-link -> /nix/store/d-hm\n\
                     /home/me/project/result -> /nix/store/e-thing\n\
                     /run/current-system -> /nix/store/a-nixos-system\n";
        assert_eq!(
            keepers(roots),
            vec![
                Keeper::System(vec![12, 38]),
                Keeper::HomeManager(vec![3]),
                Keeper::Running,
                Keeper::Other("/home/me/project/result".into()),
                Keeper::CurrentSystem,
            ]
        );
    }

    #[test]
    fn reads_the_dependency_chain() {
        let tree = "/nix/store/b6r5-nixos-system-desktop-26.05\n\
                    └───/nix/store/pwxj-system-path\n    \
                    └───/nix/store/x1-bash-interactive-5.3p9\u{1b}[0m\n";
        assert_eq!(
            chain(tree),
            [
                "nixos-system-desktop-26.05",
                "system-path",
                "bash-interactive-5.3p9"
            ]
        );
    }
}
