//! Reusable components, at three levels:
//!   - items: `AppTile` (grid), `PackageRow` (list), the look of one app
//!   - layouts: `AppList`, the search results container
//!   - `details`: the composite widgets of the details page, copied one by one
//!     from GNOME Software
//!
//! Principle: one grid style and one list style everywhere.

pub mod app_list;
pub mod app_tile;
pub mod apply_dialog;
pub mod details;
pub mod featured_carousel;
pub mod package_row;

pub use app_list::{AppList, AppListInput};
pub use app_tile::AppTile;
pub use featured_carousel::{FeaturedCarousel, FeaturedInput};
