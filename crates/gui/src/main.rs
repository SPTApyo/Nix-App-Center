//! Nix Software GUI: the relm4 and libadwaita front end.

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
    i18n::init();
    let app = RelmApp::new("io.github.sptapyo.NixSoftware");
    relm4::main_application().connect_startup(|app| search_provider::register(app.upcast_ref()));
    app.run::<AppModel>(());
}
