//! Small string normalization helpers shared by `search`, `flathub` and
//! `categories` (all of them, one way or another, match a nixpkgs name with a
//! Flathub/AppStream name).

/// Marks a user-visible string for translation extraction (`N_()` in C). The
/// core has no gettext: front ends translate it where it is displayed.
pub(crate) const fn n_(s: &'static str) -> &'static str {
    s
}

/// Normalizes a name for loose comparison: lowercase, spaces removed.
/// (Distinct from `appstream_db::alnum`, which also strips punctuation and
/// dashes: the two normalizations are NOT interchangeable, see the comment in
/// `appstream_db.rs`.)
pub(crate) fn sanitize(name: &str) -> String {
    name.to_lowercase().replace(' ', "")
}

/// The three candidate keys to match a Flathub app with a nixpkgs attr, most
/// reliable first: name without spaces, last app-id segment, first word of the
/// name. Single policy shared by `categories` (collections AND same developer
/// apps): two implementations would silently diverge.
pub(crate) fn matching_keys(name: &str, app_id: &str) -> [String; 3] {
    [
        sanitize(name),
        app_id.rsplit('.').next().unwrap_or("").to_lowercase(),
        name.split_whitespace().next().unwrap_or("").to_lowercase(),
    ]
}

/// Escapes regex metacharacters for safe use in a `nix search` pattern.
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
