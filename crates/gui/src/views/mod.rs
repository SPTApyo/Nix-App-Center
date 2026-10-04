//! Views are the app pages: home, in-app search, category, installed
//! (including tried apps), updates, details.

pub mod category;
pub mod detail;
pub mod home;
pub mod installed;
pub mod updates;

#[cfg(feature = "dev-gallery")]
pub mod gallery;

pub use category::CategoryModel;
pub use detail::DetailModel;
pub use home::HomeModel;
pub use installed::InstalledModel;
pub use updates::UpdatesModel;
