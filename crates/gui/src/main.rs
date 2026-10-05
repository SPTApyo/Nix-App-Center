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
mod search_provider;
mod util;
mod views;

use app::AppModel;
use gtk::prelude::*;
use relm4::RelmApp;

fn main() {
    // Before any widget: bind gettext to the system locale.
    i18n::init();
    let app = RelmApp::new("dev.onyx.Onyx");
    relm4::main_application().connect_startup(|app| search_provider::register(app.upcast_ref()));
    app.run::<AppModel>(());
}
