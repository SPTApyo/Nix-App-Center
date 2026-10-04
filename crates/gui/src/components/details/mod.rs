//! Widgets composites de la page détail. Un module par widget GNOME Software,
//! nommé d'après lui, avec le chemin de son `.ui` source en tête de fichier.
//! Toute divergence est ainsi auditable par diff contre le clone.
//!
//! Aucun de ces modules ne connaît `AppMeta` : `views::detail` traduit les
//! métadonnées en paramètres simples et les passe.

pub mod attach_dialog;
pub mod context_bar;
pub mod infobox;
pub mod license_tile;
pub mod nix_editor;
pub mod lozenge;
pub mod review_histogram;
pub mod review_row;
pub mod screenshot_carousel;
pub mod version_history_row;
