//! What an apply changes, read from `nix store diff-closures` between the
//! running system (or home) and the one just built, before activating it.

use std::path::Path;
use std::process::Command;

/// How a package changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Added,
    Removed,
    Updated,
    /// Same version, rebuilt.
    Rebuilt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub name: String,
    /// Versions before and after; empty when absent.
    pub before: Vec<String>,
    pub after: Vec<String>,
    /// Size difference in bytes.
    pub size: i64,
    pub kind: Kind,
}

/// The changes an apply makes, shown before it activates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    pub changes: Vec<Change>,
    /// The kernel or initrd changes: the new one runs after a restart.
    pub reboot: bool,
    /// Changes the system, so every user of the computer.
    pub system: bool,
}

impl ChangeSet {
    /// Total size difference in bytes.
    pub fn size(&self) -> i64 {
        self.changes.iter().map(|c| c.size).sum()
    }

    pub fn count(&self, kind: &Kind) -> usize {
        self.changes.iter().filter(|c| &c.kind == kind).count()
    }
}

/// The review of switching from `old` to `new`; `None` when Nix cannot
/// compare them. Blocking.
pub fn review(old: &Path, new: &Path, system: bool) -> Option<ChangeSet> {
    let out = Command::new("nix")
        .args(["--extra-experimental-features", "nix-command", "store"])
        .arg("diff-closures")
        .arg(old)
        .arg(new)
        .env("NO_COLOR", "1")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let reboot = system
        && ["kernel", "initrd"].iter().any(|f| {
            std::fs::read_link(new.join(f)).ok()
                != std::fs::read_link(Path::new("/run/booted-system").join(f)).ok()
        });
    Some(ChangeSet {
        changes: parse(&String::from_utf8_lossy(&out.stdout)),
        reboot,
        system,
    })
}

/// The changes of a `nix store diff-closures` output.
pub fn parse(text: &str) -> Vec<Change> {
    strip_ansi(text).lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Change> {
    let (name, rest) = line.split_once(": ")?;
    let mut parts: Vec<&str> = rest.split(", ").collect();
    let size = match parts.last().and_then(|p| parse_size(p)) {
        Some(s) => {
            parts.pop();
            s
        }
        None => 0,
    };
    let versions = parts.join(", ");
    let (before, after) = match versions.split_once(" → ") {
        Some((b, a)) => (version_list(b), version_list(a)),
        None => (Vec::new(), Vec::new()),
    };
    let kind = match (versions.is_empty(), before.is_empty(), after.is_empty()) {
        (true, _, _) => Kind::Rebuilt,
        (false, true, false) => Kind::Added,
        (false, false, true) => Kind::Removed,
        _ => Kind::Updated,
    };
    Some(Change {
        name: name.to_string(),
        before,
        after,
        size,
        kind,
    })
}

/// "1.2, 1.3" as versions; "∅" is none, "ε" an empty version.
fn version_list(s: &str) -> Vec<String> {
    s.split(", ")
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "∅")
        .map(|v| {
            if v == "ε" {
                String::new()
            } else {
                v.to_string()
            }
        })
        .collect()
}

/// "8.0 KiB" or "-2.8 MiB" in bytes.
fn parse_size(s: &str) -> Option<i64> {
    let (num, unit) = s.trim().split_once(' ')?;
    let n: f64 = num.parse().ok()?;
    let scale = match unit {
        "B" | "bytes" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((n * scale).round() as i64)
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Skip to the end of the escape sequence.
            for e in chars.by_ref() {
                if e.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_kind_of_line() {
        let text = "7zz: 26.02 → 26.04, \u{1b}[31;1m8.0 KiB\u{1b}[0m\n\
                    a52dec: \u{1b}[32;1m-147.7 KiB\u{1b}[0m\n\
                    abseil-cpp: ∅ → 20260817.0, 3.7 MiB\n\
                    alsa-utils: 1.2.14 → ∅, -2.8 MiB\n\
                    android-tools: 35.0.2_fish → 35.0.2-fish\n\
                    antigravity-cli: 1.2.3, 1.2.3-fish → 1.4.1, 1.4.1-fish, 8.2 MiB\n\
                    appimage: ∅ → ε\n";
        let c = parse(text);
        assert_eq!(c.len(), 7);
        assert_eq!(c[0].kind, Kind::Updated);
        assert_eq!(c[0].after, ["26.04"]);
        assert_eq!(c[0].size, 8192);
        assert_eq!(c[1].kind, Kind::Rebuilt);
        assert_eq!(c[1].size, -151245);
        assert_eq!(c[2].kind, Kind::Added);
        assert_eq!(c[3].kind, Kind::Removed);
        assert_eq!(c[4].size, 0);
        assert_eq!(c[5].before, ["1.2.3", "1.2.3-fish"]);
        assert_eq!(c[5].after, ["1.4.1", "1.4.1-fish"]);
        assert_eq!(c[6].kind, Kind::Added);
        assert_eq!(c[6].after, [""]);
    }

    #[test]
    fn totals_the_review() {
        let r = ChangeSet {
            changes: parse("a: 1 → 2, 1.0 KiB\nb: ∅ → 1, 2.0 KiB\nc: -1.0 KiB\n"),
            ..Default::default()
        };
        assert_eq!(r.size(), 2048);
        assert_eq!(r.count(&Kind::Updated), 1);
        assert_eq!(r.count(&Kind::Added), 1);
    }
}
