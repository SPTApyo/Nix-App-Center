//! Petits helpers de normalisation de chaînes, partagés par `search`,
//! `flathub` et `categories` (tous doivent, d'une façon ou d'une autre,
//! rapprocher un nom nixpkgs d'un nom Flathub/AppStream).

/// Normalise un nom pour comparaison approximative : minuscule + espaces retirés.
/// (Distinct de `appstream_db::alnum`, qui retire aussi ponctuation/tirets — les
/// deux normalisations ne sont PAS interchangeables, voir le commentaire dans
/// `appstream_db.rs`.)
pub(crate) fn sanitize(name: &str) -> String {
    name.to_lowercase().replace(' ', "")
}

/// Les trois clés candidates pour rapprocher une app Flathub d'un attr nixpkgs :
/// nom sans espaces, 1ᵉʳ mot du nom, dernier segment de l'app-id. Politique
/// unique partagée par `categories` (collections ET apps du même développeur) —
/// deux implémentations divergeraient silencieusement.
pub(crate) fn matching_keys(name: &str, app_id: &str) -> [String; 3] {
    [
        sanitize(name),
        app_id.rsplit('.').next().unwrap_or("").to_lowercase(),
        name.split_whitespace().next().unwrap_or("").to_lowercase(),
    ]
}

/// Échappe les métacaractères regex pour un usage sûr dans un pattern `nix search`.
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
