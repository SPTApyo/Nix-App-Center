//! Search by binary through `nix-locate bin/<cmd>`.

use crate::Package;
use std::process::Command;

/// Parses `nix-locate bin/<cmd>` output: first column = `attr.output`; keeps
/// lines pointing to a `/bin/`, deduplicated by attr.
fn parse_locate(stdout: &str) -> Vec<Package> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in stdout.lines() {
        if !line.contains("/bin/") {
            continue;
        }
        let Some(first) = line.split_whitespace().next() else {
            continue;
        };
        let attr = first.rsplit_once('.').map(|(a, _)| a).unwrap_or(first);
        if seen.insert(attr.to_string()) {
            out.push(Package::from_attr(attr));
        }
    }
    out
}

/// Searches packages providing the `cmd` binary through `nix-locate bin/<cmd>`.
/// Degrades gracefully when nix-locate is missing (empty vec).
pub fn locate_binary(cmd: &str) -> Vec<Package> {
    let arg = format!("bin/{cmd}");
    match Command::new("nix-locate")
        .args(["--top-level", "--whole-name", &arg])
        .output()
    {
        Ok(o) if o.status.success() => parse_locate(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
ripgrep.out                                       0 s /nix/store/xxx-ripgrep-14.1.0/bin/rg
fd.out                                            0 s /nix/store/yyy-fd-9.0.0/bin/fd
ripgrep.out                                       0 s /nix/store/xxx-ripgrep-14.1.0/share/man/man1/rg.1.gz";

    #[test]
    fn parses_attrs_dedup_bins_only() {
        let pkgs = parse_locate(SAMPLE);
        let attrs: Vec<_> = pkgs.iter().map(|p| p.attr.as_str()).collect();
        assert!(attrs.contains(&"ripgrep"));
        assert_eq!(attrs.iter().filter(|a| **a == "ripgrep").count(), 1);
    }
}
