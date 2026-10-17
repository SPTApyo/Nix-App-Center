//! Nix App Center GUI: the relm4 and libadwaita front end.

mod app;
mod components;
mod i18n;
mod search_provider;
mod util;
mod views;

use app::{AppModel, AppMsg};
use gtk::prelude::*;
use relm4::RelmApp;

fn main() {
    nix_app_center_core::migrate_legacy_dirs();
    i18n::init();
    let app = RelmApp::new("io.github.sptapyo.NixAppCenter");
    let main_app = relm4::main_application();
    main_app.set_flags(gtk::gio::ApplicationFlags::HANDLES_OPEN);
    main_app.connect_startup(|app| search_provider::register(app.upcast_ref()));
    main_app.connect_open(open_links);
    app.run::<AppModel>(());
}

/// Opens the app page of each `nixpkg://ATTR` link, as nixsoftware.org sends.
fn open_links(app: &gtk::Application, files: &[gtk::gio::File], _hint: &str) {
    app.activate();
    let Some(tx) = search_provider::APP_SENDER.get() else {
        return;
    };
    for pkg in files
        .iter()
        .filter_map(|f| nix_app_center_core::Package::from_uri(&f.uri()))
    {
        let _ = tx.send(AppMsg::OpenDetail(pkg));
    }
}
