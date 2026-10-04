//! Vues = les pages de l'app : accueil, recherche (dans l'app), catégorie,
//! installées (inclut les essayées), mises à jour, détail.

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
