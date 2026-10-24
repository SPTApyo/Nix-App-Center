//! Reusable widgets: app tiles and rows, the search results list, and the
//! parts of the details page.

pub mod app_list;
pub mod app_tile;
pub mod apply_dialog;
pub mod details;
pub mod featured_carousel;
pub mod go_back;
pub mod install_options;
pub mod option_dialog;
pub mod package_row;
pub mod version_dialog;
pub mod welcome;

pub use app_list::{AppList, AppListInput};
pub use app_tile::AppTile;
pub use featured_carousel::{FeaturedCarousel, FeaturedInput};
