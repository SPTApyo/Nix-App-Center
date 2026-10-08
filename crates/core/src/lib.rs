//! Nix Software core: pure logic (search / run / keep), no UI dependency.
//!
//! Front ends (CLI, relm4 GUI) consume this crate; it knows neither GTK nor
//! the terminal. The API is deliberately shaped as *data* (taxonomy + actions)
//! so every front end renders the same logic its own way, and so it can grow
//! without breaking.
//!
//! # Module layout
//!
//! This file is only the crate root: module declarations and public
//! re-exports. Each module has a single responsibility:
//!
//! - [`package`]: package model + taxonomy + actions (pure, zero I/O)
//! - [`search`]: ranked nixpkgs search + AppStream enrichment
//! - [`appstream_db`]: local AppStream database (Flatpak cache or Flathub download)
//! - [`flathub`]: per-app Flathub metadata (icon/description/releases)
//! - [`detail`]: metadata through `nix eval <attr>.meta`
//! - [`app_meta`]: AppStream ↔ Nix merge for the details page
//! - [`ratings`] / [`reviews`]: ODRS ratings and reviews
//! - [`resolve`]: "app DNS", nixpkgs attr to app id (curated + community overlay)
//! - [`oars`]: age rating (OARS attributes to "12" / "12+")
//! - [`categories`]: Flathub collections (popular/recent/categories) to nixpkgs
//! - [`session_cache`]: in-session memoization of `app_meta`/icons
//! - [`cache`]: TTL disk cache of search results
//! - [`manifest`]: "keep" manifest (`keep`/`unkeep`)
//! - [`tried`]: "tried" history (try-first) + garbage collection
//! - [`launch`]: `nix run` (the headline action) and `nix shell` (try-first for CLIs)
//! - [`config_layout`]: Nix layout detection (where to write, how to apply)
//! - [`config_attach`]: consented attach to the user's config (plan, diff, commit)
//! - [`nix_check`]: validation of the user's Nix (instant parse, dry-build)
//! - [`config_gen`]: generation of the Nix Software files + applying the config
//! - [`updates`]: nixpkgs source of the config, update check and update
//! - [`profiles`]: apps already installed in the Nix profiles
//! - [`user_profile`]: `nix profile` installs when no config is managed
//! - [`storage`]: where the store space goes (categories, generations, apps)
//! - [`settings`]: user settings (detection overrides, update checks)
//! - [`options`]: NixOS option search (channel options.json)
//! - [`locate`]: search by binary (`nix-locate`)
//! - [`http`] / [`util`] / [`paths`]: small shared helpers

mod app_meta;
mod appstream_db;
mod build_flags;
mod cache;
mod categories;
mod config_attach;
mod config_gen;
mod config_layout;
mod detail;
mod flathub;
mod http;
mod launch;
mod locate;
mod manifest;
mod nix_check;
mod oars;
mod options;
mod package;
mod paths;
mod profiles;
mod ratings;
mod resolve;
mod reviews;
mod search;
mod session_cache;
mod settings;
mod storage;
mod tried;
mod updates;
mod user_profile;
mod util;

pub use app_meta::{app_meta, AppMeta};
pub use appstream_db::{
    apps_for_file_type, brand_colors, init_background as init_appstream_db,
    is_settled as appstream_settled, local_app, package_for_app, search_local, wait_appstream,
    LocalHit,
};
pub use build_flags::{overridden_flags, BuildFlag};
pub use categories::{
    category_sections_union, popular_packages, recently_updated_packages, resolve_dev_apps,
};
pub use config_attach::{
    commit as attach_commit, detach, is_attached, plan as attach_plan, AttachPlan,
};
pub use config_gen::{apply_argv, apply_config, run_streaming, ApplyOutcome};
pub use config_gen::{default_home_block, override_home_block};
pub use config_layout::{
    detect as detect_layout, verify as verify_layout, Layout, LayoutKind, ScopeTarget, Verified,
};
pub use detail::{closure_size, detail, PackageDetail};
pub use flathub::{flathub_installs, Addon, DevApp, Release};
pub use launch::{run, run_command, run_failure, shell, shell_command, ShellOutcome};
pub use locate::{download_locate_db, locate_binary, locate_ready};
pub use manifest::{keep, load_manifest, set_blocks, unkeep, KeptEntry, Manifest};
pub use nix_check::{dry_build, parse_block, DryRun};
pub use options::{module_for, search_options, NixOption};
pub use package::{actions_for, Action, Actions, IconKind, Kind, Package};
pub use profiles::{installed_app, profile_apps, store_sizes, ProfileApp, Scope};
pub use ratings::{rating_for, Rating};
pub use resolve::{report_mapping, resolve_app_id};
pub use reviews::{
    act_on_review, featured_review, reviews_for, submit_review, Review, ReviewAction,
};
pub use search::{search, search_apps, SearchResult};
pub use session_cache::{
    cached_app_meta, cached_bytes_if_present, cached_fetch_bytes, forget_app_meta, known_icon_url,
    remember_icon_url,
};
pub use settings::{load_settings, save_settings, Settings};
pub use storage::{
    clear_cache, home_usage, storage_report, AppOrigin, AppSize, Category, GenProfile, Generation,
    HomeDir, HomeKind, StorageReport,
};
pub use tried::{garbage_collect, load_tried, record_tried, remove_tried, TriedEntry, TriedLog};
pub use updates::{
    app_updates, current_source, latest, newer_release, nixpkgs_ref, updatable_apps,
    update_command, AppUpdate, Latest, Source, UpdateMethod,
};
pub use user_profile::{profile_add, profile_elements, profile_remove, ProfileElement};
pub use util::now_unix;

// ── Test utility: global lock for tests that mutate the environment
// (`set_var`/`remove_var` are process global and Rust tests run in
// parallel, so tests touching NIX_SOFTWARE_*/XDG_* variables are serialized).
// Cross-module (`manifest`, `config_gen`), hence at the crate root.
#[cfg(test)]
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
