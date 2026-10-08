//! The app pages.

pub mod category;
pub mod detail;
pub mod home;
pub mod installed;
pub mod preferences;
pub mod storage;
pub mod updates;

#[cfg(feature = "dev-gallery")]
pub mod gallery;

pub use category::CategoryModel;
pub use detail::DetailModel;
pub use home::HomeModel;
pub use installed::InstalledModel;
pub use storage::StorageModel;
pub use updates::UpdatesModel;
