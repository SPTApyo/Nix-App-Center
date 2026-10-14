# Repository layout

Where everything lives, and which file to open first for a given change.

## Crates

- crates/core: all the logic, no UI dependency. Every module starts with a
  comment saying what it does.
- crates/gui: GTK 4 and libadwaita front end written with relm4. Views are
  pages, components are reusable widgets.
- crates/cli: text front end over the core, handy to try a change by hand.

## Where to start

| Change | Start in |
| --- | --- |
| Search results, ranking | crates/core/src/search.rs, crates/gui/src/components/app_list.rs |
| Matching a package to a Flathub app | crates/core/src/resolve.rs, crates/core/src/data/app_mapping.json |
| nixsoftware.org data (curated apps, similar apps) | crates/core/src/nixsoftware.rs |
| AppStream parsing, icons, screenshots | crates/core/src/appstream_db.rs, crates/core/src/session_cache.rs |
| Try (nix run, nix shell) | crates/core/src/launch.rs, crates/core/src/tried.rs |
| Keep, the generated Nix files | crates/core/src/manifest.rs, crates/core/src/config_gen.rs |
| Detecting the user's setup | crates/core/src/config_layout.rs |
| The import line in the user's config | crates/core/src/config_attach.rs |
| Writes as root (pkexec) | crates/core/src/paths.rs |
| What is installed, and how | crates/core/src/installed.rs, crates/core/src/profiles.rs, crates/core/src/foreign.rs |
| Modules and packages in the user's files | crates/core/src/enabled.rs |
| nix profile and nix-env | crates/core/src/user_profile.rs |
| Install modes, held versions | crates/core/src/manifest.rs, crates/core/src/versions.rs, crates/gui/src/components/install_options.rs |
| Applying, progress | crates/core/src/config_gen.rs, crates/core/src/progress.rs, crates/gui/src/components/apply_dialog.rs |
| Updates tab | crates/core/src/updates.rs, crates/gui/src/views/updates.rs |
| Storage tab | crates/core/src/storage.rs, crates/gui/src/views/storage.rs |
| Details page | crates/gui/src/views/detail.rs, crates/gui/src/components/details/ |
| Reviews and ratings | crates/core/src/reviews.rs, crates/core/src/ratings.rs |
| NixOS and Home Manager options, flake modules | crates/core/src/options.rs, crates/gui/src/components/option_dialog.rs |
| App shell, tabs, About | crates/gui/src/app.rs |
| Styles and animations | crates/gui/data/style.css |
| Nix package, NixOS module | nix/package.nix, nix/module.nix, nix/test.nix, default.nix |
| GNOME Shell search | crates/gui/src/search_provider.rs |

## Other directories

- crates/gui/data: desktop entry, AppStream metainfo, icons, stylesheet,
  search provider. nix/package.nix installs them.
- data/screenshots: the screenshots of the README and of the AppStream
  metainfo, taken in the end-to-end environment.
- po: translations. po/update-pot.sh rebuilds the template.
- tests/e2e: end-to-end suite, see [testing.md](testing.md).
- scripts: commit message check, shared by the git hook and CI.
- .github: CI, Dependabot, issue and pull request templates.
