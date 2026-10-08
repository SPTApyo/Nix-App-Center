//! Search by command through nix-locate and the nix-index database.

use crate::Package;
use std::path::PathBuf;
use std::process::Command;

const INDEX_URL: &str =
    "https://github.com/nix-community/nix-index-database/releases/latest/download/index-{arch}-linux";

/// Folder of the downloaded database.
fn db_dir() -> PathBuf {
    crate::cache::cache_dir().join("nix-index")
}

/// True when a search by command can run.
pub fn locate_ready() -> bool {
    crate::launch::in_path("nix-locate") || db_dir().join("files").is_file()
}

/// Downloads the nix-index database (about 100 MB). Blocking.
pub fn download_locate_db() -> std::io::Result<()> {
    let url = INDEX_URL.replace("{arch}", std::env::consts::ARCH);
    let resp = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(900))
        .call()
        .map_err(std::io::Error::other)?;
    let dir = db_dir();
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join("files.part");
    std::io::copy(&mut resp.into_reader(), &mut std::fs::File::create(&tmp)?)?;
    std::fs::rename(tmp, dir.join("files"))
}

/// Packages of `nix-locate --minimal` output, one per attr.
fn parse_locate(stdout: &str) -> Vec<Package> {
    let mut seen = std::collections::HashSet::new();
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| l.rsplit_once('.').map(|(a, _)| a).unwrap_or(l))
        .filter(|attr| seen.insert(attr.to_string()))
        .map(Package::from_attr)
        .collect()
}

/// Packages providing the command `cmd`; empty without nix-locate.
pub fn locate_binary(cmd: &str) -> Vec<Package> {
    let arg = format!("/bin/{cmd}");
    let args = ["--minimal", "--at-root", "--whole-name", &arg];
    let output = if crate::launch::in_path("nix-locate") {
        Command::new("nix-locate").args(args).output()
    } else if db_dir().join("files").is_file() {
        crate::util::nix()
            .args(["shell", &format!("{}#nix-index", crate::nixpkgs_ref())])
            .args(["-c", "nix-locate", "--db"])
            .arg(db_dir())
            .args(args)
            .output()
    } else {
        return Vec::new();
    };
    match output {
        Ok(o) if o.status.success() => parse_locate(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "ripgrep.out\nripgrep-all.out\nripgrep.out\n";

    #[test]
    fn parses_minimal_output_dedup() {
        let pkgs = parse_locate(SAMPLE);
        let attrs: Vec<_> = pkgs.iter().map(|p| p.attr.as_str()).collect();
        assert_eq!(attrs, ["ripgrep", "ripgrep-all"]);
    }
}
