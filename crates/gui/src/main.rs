//! Onyx GUI — façade relm4 + libadwaita.
//!
//! Architecture par modules :
//!   - `app`        : la coquille (fenêtre, header, navigation, orchestration)
//!   - `views`      : les pages (accueil, catégorie, détail)
//!   - `components` : les briques remplissables (ligne de recherche, tuile)
//!   - `util`       : helpers partagés (icônes, textures)

mod app;
mod components;
mod i18n;
mod util;
mod views;

use app::AppModel;
use relm4::RelmApp;

fn main() {
    // Avant tout widget : branche gettext sur la locale système (comme GNOME Software).
    i18n::init();
    let app = RelmApp::new("dev.onyx.Onyx");
    app.run::<AppModel>(());
}
