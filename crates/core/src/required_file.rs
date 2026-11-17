//! Files nixpkgs may not download itself (`requireFile`), such as an
//! installer behind a login: found in a failed build's log, then added to
//! the store from the copy the user downloaded.

use std::path::Path;
use std::process::Command;

/// A file a build waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredFile {
    /// The file name the store path must have.
    pub name: String,
    /// sha256, sha1 or sha512.
    pub algo: String,
    /// Where to download it, when the message says.
    pub url: Option<String>,
}

/// The file a build log asks for, from requireFile's message:
/// "nix-store --add-fixed sha256 NAME" and "Please go to URL".
pub fn required_file(log: &str) -> Option<RequiredFile> {
    let rest = &log[log.find("nix-store --add-fixed ")? + "nix-store --add-fixed ".len()..];
    let mut words = rest.split_whitespace();
    let algo = words.next()?.to_string();
    let name = words.next()?.to_string();
    let plain = |c: char| c.is_ascii_alphanumeric() || "+-._?=".contains(c);
    if !["sha256", "sha1", "sha512"].contains(&algo.as_str())
        || !name.chars().all(plain)
        || name.starts_with('.')
    {
        return None;
    }
    let url = log
        .find("Please go to ")
        .and_then(|i| log[i + "Please go to ".len()..].split_whitespace().next())
        .map(|u| u.trim_end_matches(['.', ',', ')']).to_string())
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"));
    Some(RequiredFile { name, algo, url })
}

/// Adds `file` to the store under the name the build expects. Blocking.
///
/// # Errors
/// Copying failed, or nix-store's message.
pub fn add_required_file(file: &Path, wanted: &RequiredFile) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let dir = std::env::temp_dir().join(format!(
        "nix-app-center-file-{}-{nanos}",
        std::process::id()
    ));
    // A new private folder: nothing planted in it can be followed.
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    let copy = dir.join(&wanted.name);
    let added = std::fs::copy(file, &copy).and_then(|_| {
        Command::new("nix-store")
            .args(["--add-fixed", &wanted.algo])
            .arg(&copy)
            .output()
    });
    let _ = std::fs::remove_dir_all(&dir);
    let out = added?;
    if out.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(crate::util::last_error_line(
            &out.stderr,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MESSAGE: &str = "error: builder failed\n***\nUnfortunately, we cannot download file CiscoPacketTracer_900_Ubuntu_64bit.deb automatically.\nPlease go to https://www.netacad.com to download it yourself, and add it to the Nix store\nusing either\n  nix-store --add-fixed sha256 CiscoPacketTracer_900_Ubuntu_64bit.deb\nor\n  nix-prefetch-url --type sha256 file:///path/to/CiscoPacketTracer_900_Ubuntu_64bit.deb\n***\n";

    #[test]
    fn reads_the_file_a_build_waits_for() {
        assert_eq!(
            required_file(MESSAGE),
            Some(RequiredFile {
                name: "CiscoPacketTracer_900_Ubuntu_64bit.deb".into(),
                algo: "sha256".into(),
                url: Some("https://www.netacad.com".into()),
            })
        );
        assert_eq!(required_file("error: attribute 'x' missing"), None);
    }

    #[test]
    fn refuses_names_that_are_no_plain_file_name() {
        let log = |name: &str| format!("nix-store --add-fixed sha256 {name}\n");
        assert!(required_file(&log("../../etc/passwd")).is_none());
        assert!(required_file(&log("a/b.deb")).is_none());
        assert!(required_file(&log(".hidden")).is_none());
        assert!(required_file("nix-store --add-fixed md4 x.deb").is_none());
        let custom = log("game.zip");
        assert_eq!(required_file(&custom).map(|f| f.url), Some(None));
    }
}
