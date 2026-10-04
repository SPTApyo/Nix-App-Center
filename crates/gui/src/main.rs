//! Onyx GUI: relm4 + libadwaita front end.
//!
//! Modules:
//!   - `app`        : the shell (window, header, navigation, orchestration)
//!   - `views`      : the pages (home, category, details)
//!   - `components` : reusable building blocks (search row, tile)
//!   - `util`       : shared helpers (icons, textures)

mod app;
mod components;
mod i18n;
mod util;
mod views;

use app::AppModel;
use relm4::RelmApp;

fn main() {
    // Before any widget: bind gettext to the system locale.
    i18n::init();
    let app = RelmApp::new("dev.onyx.Onyx");
    app.run::<AppModel>(());
}
