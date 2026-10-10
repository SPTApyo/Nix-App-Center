//! gettext translations, following the system locale like GNOME Software.
//! English strings are the msgids; catalogs live in `po/`.

use gettextrs::{bind_textdomain_codeset, bindtextdomain, setlocale, textdomain, LocaleCategory};

/// Translates a displayed string, like `_()` in C.
pub use gettextrs::gettext as t;
/// Translates a string that means different things in different places.
pub use gettextrs::pgettext;

const DOMAIN: &str = "nix-software";

/// The `.mo` folder: the package's, then the one `build.rs` compiles, then
/// the system's.
fn localedir() -> String {
    if let Ok(dir) = std::env::var("NIX_SOFTWARE_LOCALEDIR") {
        return dir;
    }
    if let Some(dir) = option_env!("NIX_SOFTWARE_DEV_LOCALEDIR") {
        return dir.to_string();
    }
    "/usr/share/locale".to_string()
}

/// Binds gettext to the locale and the domain. Call first in `main`.
pub fn init() {
    // SAFETY: first in main, no other thread.
    unsafe {
        setlocale(LocaleCategory::LcAll, "");
    }
    let _ = bindtextdomain(DOMAIN, localedir());
    let _ = bind_textdomain_codeset(DOMAIN, "UTF-8");
    let _ = textdomain(DOMAIN);
}

/// Marks a string for extraction without translating it, like `N_()`.
pub const fn n_(s: &'static str) -> &'static str {
    s
}
