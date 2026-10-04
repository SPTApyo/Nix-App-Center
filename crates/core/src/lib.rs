//! Onyx core — logique pure (search / run / keep), sans dépendance UI.
//!
//! Les façades (CLI, GUI relm4) consomment ce crate ; lui ne connaît ni GTK ni
//! terminal. L'API est volontairement modelée en *données* (taxonomie + actions)
//! pour que chaque façade rende la même logique à sa sauce, et qu'on étende sans
//! casser : AppStream (icônes/catégories) et home-manager switch viendront se
//! brancher sur ces coutures.
//!
//! # Organisation des modules
//!
//! Ce fichier n'est que la racine du crate : déclarations de modules et
//! ré-exports publics. Chaque module a une seule responsabilité :
//!
//! - [`package`] — modèle de paquet + taxonomie + actions (pur, zéro I/O)
//! - [`search`] — recherche nixpkgs classée + enrichissement AppStream
//! - [`appstream_db`] — base AppStream locale (cache Flatpak, instantané)
//! - [`flathub`] — métadonnées Flathub par app (icône/description/releases)
//! - [`detail`] — métadonnées via `nix eval <attr>.meta`
//! - [`app_meta`] — fusion AppStream ↔ Nix pour la page détail
//! - [`ratings`] / [`reviews`] — notes et avis ODRS
//! - [`resolve`] — « DNS des apps » : attr nixpkgs → app-id (overlay curé + communautaire)
//! - [`oars`] — classification d'âge (attributs OARS → « 12 » / « 12+ »)
//! - [`permissions`] — tuile Sécurité (permissions du bundle Flatpak)
//! - [`categories`] — collections Flathub (populaires/récentes/catégories) → nixpkgs
//! - [`session_cache`] — mémoïsation en session de `app_meta`/icônes
//! - [`cache`] — cache disque à TTL des résultats de recherche
//! - [`manifest`] — manifeste « garder » (`keep`/`unkeep`)
//! - [`tried`] — historique « essayés » (try-first) + ramasse-miettes
//! - [`launch`] — `nix run` (l'action vedette) et `nix shell` (le try-first des CLI)
//! - [`config_layout`] — détection de la disposition Nix (où écrire, comment appliquer)
//! - [`config_attach`] — greffe consentie sur la config de l'utilisateur (plan → diff → commit)
//! - [`nix_check`] — validation du Nix de l'utilisateur (parse instantané, dry-build)
//! - [`config_gen`] — génération des fichiers Onyx + application de la config
//! - [`updates`] — détection des mises à jour
//! - [`locate`] — recherche par binaire (`nix-locate`)
//! - [`http`] / [`util`] / [`paths`] — petits helpers partagés

mod app_meta;
mod appstream_db;
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
mod package;
mod paths;
mod permissions;
mod ratings;
mod resolve;
mod reviews;
mod search;
mod session_cache;
mod tried;
mod updates;
mod util;

pub use app_meta::{app_meta, AppMeta};
pub use appstream_db::init_background as init_appstream_db;
pub use categories::{
    category_sections_union, popular_packages, recently_updated_packages, resolve_dev_apps,
};
pub use config_attach::{commit as attach_commit, detach, is_attached, plan as attach_plan, AttachPlan};
pub use config_gen::{apply_config, ApplyOutcome};
pub use config_layout::{detect as detect_layout, Layout, LayoutKind, ScopeTarget};
pub use detail::{closure_size, detail, PackageDetail};
pub use flathub::{Addon, DevApp, Release};
pub use launch::{run, run_command, shell, shell_command, ShellOutcome};
pub use locate::locate_binary;
pub use config_gen::default_home_block;
pub use manifest::{keep, load_manifest, set_blocks, unkeep, KeptEntry, Manifest};
pub use nix_check::{dry_build, parse_block, DryRun};
pub use package::{actions_for, Action, Actions, Kind, Package};
pub use permissions::{safety, Safety};
pub use ratings::{rating_for, Rating};
pub use resolve::{report_mapping, resolve_app_id};
pub use reviews::{featured_review, reviews_for, Review};
pub use search::{search, search_apps, SearchResult};
pub use session_cache::{
    cached_app_meta, cached_bytes_if_present, cached_fetch_bytes, known_icon_url,
    remember_icon_url,
};
pub use tried::{garbage_collect, load_tried, record_tried, TriedEntry, TriedLog};
pub use updates::{available_updates, Update};

// ── Utilitaire de test : verrou global pour les tests qui mutent l'environnement
// (`set_var`/`remove_var` sont process-globaux ; les tests Rust tournent en
// parallèle → on sérialise ceux qui touchent ONYX_STORE_PACKAGES_PATH/XDG_*).
// Cross-module (`manifest`, `config_gen`), donc à la racine du crate.
#[cfg(test)]
pub(crate) fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
