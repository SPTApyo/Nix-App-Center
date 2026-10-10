//! Nix Software core: search, try, keep and the config engine, without UI.
//! Front ends render the same data (packages, actions) their own way.

mod app_meta;
mod appstream_db;
mod build_flags;
mod cache;
mod categories;
mod config_attach;
mod config_gen;
mod config_layout;
mod detail;
mod enabled;
mod flathub;
mod foreign;
mod http;
mod installed;
mod launch;
mod locate;
mod manifest;
mod nix_check;
mod oars;
mod options;
mod package;
mod paths;
mod profiles;
mod progress;
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
mod versions;

pub use app_meta::{app_meta, nix_releases, AppMeta};
pub use appstream_db::{
    apps_for_file_type, brand_colors, init_background as init_appstream_db,
    is_settled as appstream_settled, local_app, package_for_app, search_local, wait_appstream,
    LocalHit,
};
pub use build_flags::BuildFlag;
pub use categories::{
    category_sections_union, popular_packages, recently_updated_packages, resolve_dev_apps,
};
pub use config_attach::{
    commit as attach_commit, detach, is_attached, plan as attach_plan, AttachPlan,
};
pub use config_gen::default_home_block;
pub use config_gen::{apply_argv, apply_config, run_streaming, ApplyOutcome};
pub use config_layout::{
    detect as detect_layout, verify as verify_layout, Layout, LayoutKind, ScopeTarget, Verified,
};
pub use detail::{closure_size, detail, PackageDetail};
pub use enabled::{declared_in, enabled_modules};
pub use flathub::{flathub_installs, Addon, DevApp, Release};
pub use foreign::{appimage_files, remove_foreign};
pub use http::exists as url_exists;
pub use installed::{find as find_installed, installed_index, Installed, Origin};
pub use launch::{run, run_command, run_failure, shell, shell_command, ShellOutcome};
pub use locate::{download_locate_db, locate_binary, locate_ready};
pub use manifest::{
    applied, apply_pending, import_flake_module, keep, load_manifest, pin_at, pin_current,
    set_blocks, set_flags, set_mode, set_option, set_pin, unkeep, unset_option, InstallMode,
    KeptEntry, Manifest, Pin,
};
pub use nix_check::{dry_build, parse_block, DryRun};
pub use options::{
    hm_module_for, module_for, search_flake_modules, search_hm_options, search_options, NixOption,
    FLAKE_MODULE,
};
pub use package::{actions_for, Action, Actions, IconKind, Kind, Package};
pub use profiles::{
    installed_app, profile_apps, profile_packages, store_sizes, ProfileApp, ProfilePackage, Scope,
};
pub use progress::{Phase, StreamEvent};
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
    app_updates, current_source, installable, latest, newer_in_unstable, newer_release,
    nixpkgs_ref, pinned_rev, updatable_apps, update_command, version_newer, AppUpdate, Latest,
    Source, UpdateMethod,
};
pub use user_profile::{
    nix_env_names, nix_env_remove, profile_add, profile_elements, profile_remove, ProfileElement,
};
pub use util::now_unix;
pub use versions::{versions, VersionChoice};

// Serializes tests that change environment variables.
#[cfg(test)]
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
