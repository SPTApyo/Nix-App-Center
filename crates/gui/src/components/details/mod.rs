//! Composite widgets of the details page. One module per GNOME Software
//! widget, named after it, with the path of its source `.ui` at the top.
//! Any divergence can thus be audited by diffing against the clone.
//!
//! None of these modules knows `AppMeta`: `views::detail` turns metadata into
//! plain parameters and passes them.

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
