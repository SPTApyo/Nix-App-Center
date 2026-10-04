//! Composants réutilisables. Trois niveaux :
//!   - items : `AppTile` (grille), `PackageRow` (liste) — visuel d'une app
//!   - layouts : `AppList` — conteneur résultats de recherche
//!   - `details` : les widgets composites de la page détail, calqués un à un
//!     sur ceux de GNOME Software
//!
//! Principe : un seul style de grille et un seul style de liste partout.

pub mod app_list;
pub mod app_tile;
pub mod details;
pub mod featured_carousel;
pub mod package_row;

pub use app_list::{AppList, AppListInput};
pub use app_tile::AppTile;
pub use featured_carousel::{FeaturedCarousel, FeaturedInput};
