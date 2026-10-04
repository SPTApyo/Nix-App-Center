//! Internationalisation — même mécanisme que GNOME Software : gettext piloté par la
//! locale du système, **aucun sélecteur de langue**. Les chaînes du code (en français)
//! sont les `msgid` ; les traductions vivent dans `po/<lang>.po`, compilées en `.mo`.
//!
//! Chaque chaîne visible est enveloppée dans [`gettext`] (importé `t` par convention
//! courte). À l'exécution, gettext choisit le catalogue d'après `LC_MESSAGES`/`LANGUAGE`.

use gettextrs::{bind_textdomain_codeset, bindtextdomain, setlocale, textdomain, LocaleCategory};

/// Alias court de `gettextrs::gettext`, façon `_()` en C. À utiliser sur chaque chaîne
/// affichée : `t("Rechercher…")`.
pub use gettextrs::gettext as t;

const DOMAIN: &str = "onyx";

/// Où trouver les catalogues `.mo`, par priorité :
/// 1. `ONYX_LOCALEDIR` — posé par le wrapper du paquet Nix (`share/locale`) ;
/// 2. `ONYX_DEV_LOCALEDIR` — compilé par `build.rs` (dev : `cargo run`/`cargo test`) ;
/// 3. le chemin système standard.
fn localedir() -> String {
    if let Ok(dir) = std::env::var("ONYX_LOCALEDIR") {
        return dir;
    }
    if let Some(dir) = option_env!("ONYX_DEV_LOCALEDIR") {
        return dir.to_string();
    }
    "/usr/share/locale".to_string()
}

/// Branche gettext sur la locale système et le domaine « onyx ». À appeler une fois,
/// tout au début de `main`, avant toute construction de widget.
pub fn init() {
    setlocale(LocaleCategory::LcAll, "");
    let _ = bindtextdomain(DOMAIN, localedir());
    let _ = bind_textdomain_codeset(DOMAIN, "UTF-8");
    let _ = textdomain(DOMAIN);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Preuve du pipeline (lancer avec `LANGUAGE=en … -- --nocapture`) : sous une locale
    /// anglaise, « Explorer » doit être traduit en « Explore » depuis `po/en.po`.
    #[test]
    fn prints_resolved_translation() {
        init();
        eprintln!("[i18n] localedir={}", localedir());
        eprintln!("[i18n] Explorer -> {}", t("Explorer"));
        eprintln!("[i18n] Mises à jour -> {}", t("Mises à jour"));
    }
}
