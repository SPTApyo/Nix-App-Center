//! Internationalization, same mechanism as GNOME Software: gettext driven by the
//! system locale, **no language picker**. English source strings are the `msgid`s;
//! translations live in `po/<lang>.po` and are compiled to `.mo` files.
//!
//! Every visible string goes through [`gettext`] (imported as `t` for brevity).
//! At runtime gettext picks the catalog from `LC_MESSAGES`/`LANGUAGE`.

use gettextrs::{bind_textdomain_codeset, bindtextdomain, setlocale, textdomain, LocaleCategory};

/// Short alias of `gettextrs::gettext`, like `_()` in C. Use it on every displayed
/// string: `t("Search…")`.
pub use gettextrs::gettext as t;

const DOMAIN: &str = "nix-software";

/// Where to find `.mo` catalogs, by priority:
/// 1. `NIX_SOFTWARE_LOCALEDIR`, set by the Nix package wrapper (`share/locale`);
/// 2. `NIX_SOFTWARE_DEV_LOCALEDIR`, compiled by `build.rs` (dev: `cargo run`/`cargo test`);
/// 3. the standard system path.
fn localedir() -> String {
    if let Ok(dir) = std::env::var("NIX_SOFTWARE_LOCALEDIR") {
        return dir;
    }
    if let Some(dir) = option_env!("NIX_SOFTWARE_DEV_LOCALEDIR") {
        return dir.to_string();
    }
    "/usr/share/locale".to_string()
}

/// Binds gettext to the system locale and the "nix-software" domain. Call once, at the
/// very start of `main`, before building any widget.
pub fn init() {
    // SAFETY: runs first in main, no other thread yet.
    unsafe {
        setlocale(LocaleCategory::LcAll, "");
    }
    let _ = bindtextdomain(DOMAIN, localedir());
    let _ = bind_textdomain_codeset(DOMAIN, "UTF-8");
    let _ = textdomain(DOMAIN);
}

/// Marks a string for extraction without translating it (`N_()` in C). For
/// `const` tables; translate with [`t`] where the string is displayed.
pub const fn n_(s: &'static str) -> &'static str {
    s
}
