//! Small helpers shared across the core.

/// Marks a string for translation; the front end translates it.
pub(crate) const fn n_(s: &'static str) -> &'static str {
    s
}

/// A `nix` command with the flakes commands enabled, whatever nix.conf says.
pub(crate) fn nix() -> std::process::Command {
    let mut c = std::process::Command::new("nix");
    c.args(["--extra-experimental-features", "nix-command flakes"]);
    c
}

/// The user's home folder.
pub(crate) fn home() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
}

/// Now, in Unix seconds.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The last `error:` line of a command output, else its last line.
pub(crate) fn last_error_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .rev()
        .find_map(|l| {
            l.trim()
                .strip_prefix("error:")
                .map(str::trim)
                .filter(|l| !l.is_empty())
        })
        .or_else(|| text.lines().rev().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or("")
        .to_string()
}

/// Lowercase name without spaces, for loose comparison.
pub(crate) fn sanitize(name: &str) -> String {
    name.to_lowercase().replace(' ', "")
}

/// Keys matching a Flathub app to a nixpkgs attr, most reliable first.
pub(crate) fn matching_keys(name: &str, app_id: &str) -> [String; 3] {
    [
        sanitize(name),
        app_id.rsplit('.').next().unwrap_or("").to_lowercase(),
        name.split_whitespace().next().unwrap_or("").to_lowercase(),
    ]
}

/// Escapes regex metacharacters for a `nix search` pattern.
pub(crate) fn regex_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\.+*?()[]{}^$|".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_lowercases_and_strips_spaces() {
        assert_eq!(sanitize("Extreme Tux Racer"), "extremetuxracer");
        assert_eq!(sanitize("VLC"), "vlc");
    }

    #[test]
    fn regex_escape_escapes_special_chars() {
        assert_eq!(regex_escape("c++"), "c\\+\\+");
        assert_eq!(regex_escape("a.b"), "a\\.b");
        assert_eq!(regex_escape("(foo)"), "\\(foo\\)");
        assert_eq!(regex_escape("plain"), "plain");
    }
}
