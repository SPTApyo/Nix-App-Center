# Nix Software

A declarative app store for Nix, built around "try first".

Launch any package from nixpkgs right away (nix run, or a nix shell for
command line tools). Nothing is installed. When you want to keep an app,
Nix Software adds it to a small Nix module that it generates and that your own
configuration imports, then you apply it the usual way.

Nix Software detects your setup (NixOS with or without flakes, Home Manager as a
module, flake or standalone) and only touches your files after you accept a
diff. A backup is written before any change.

## Features

- Search nixpkgs, with Flathub metadata (names, screenshots, ratings, age
  ratings, permissions) when the app exists there too.
- Try an app without installing it, unfree packages included.
- Keep an app: it lands in nix-software-home.nix or nix-software-system.nix, imported by
  your configuration.
- Installed shows what Nix Software keeps, what you tried (removable one by one
  from the store) and the apps your system and user profiles already hold.
- Updates shows the nixpkgs revision your config is pinned to, what the
  branch head changes for your apps, and updates in one click.
- Search packages, apps and NixOS options. Optional search by command.
- Apply and update inside the app, with a live build log.
- Write reviews, find Nix Software apps from the GNOME Shell overview.
- Without a managed configuration, install for your user with nix profile.
- English and French interface.

## Build and run

You need Nix with flakes enabled.

```bash
nix run .                          # open the window
nix run .#nix-software-cli -- search gimp  # text interface
nix build .                        # result/bin/nix-software and result/bin/nix-software-cli
```

## Development

```bash
nix develop                         # Rust toolchain, GTK 4, libadwaita, gettext
cargo run -p nix-software-gui               # run the app
cargo test --workspace              # unit tests
cargo clippy --workspace --all-targets -- -D warnings
```

Crates:

- crates/core: all the logic (search, run, keep, config detection and
  generation). No UI dependency.
- crates/gui: GTK 4 and libadwaita front end, written with relm4.
- crates/cli: text front end, handy to test the core by hand.

Design notes live in [docs/design.md](docs/design.md).

Useful environment variables:

- NIX_SOFTWARE_CONFIG_DIR: NixOS configuration directory (defaults to /etc/nixos).
  Point it to a copy to experiment safely.
- NIX_SOFTWARE_LOCALEDIR: where the compiled translations are (set by the Nix
  package; cargo run compiles po/ on its own).

## End-to-end tests

The GUI runs under the GTK Broadway backend and Playwright drives it from a
headless Chromium. Each run uses a throwaway home and config directory, so
your real configuration is never touched.

```bash
tests/e2e/run.sh           # full scenario, screenshots in target/e2e
tests/e2e/run.sh --serve   # keep an isolated app running
```

The scenario searches a package, opens its page, runs it, adds it to the
configuration, accepts the attach diff, then removes it. It needs network
access for nixpkgs and Flathub.

## Translations

Strings are in English in the sources and translated with gettext. See
[CONTRIBUTING.md](CONTRIBUTING.md).

## License

Copyright © 2026 SPTApyo.

Nix Software is free software under the GNU GPL-3.0-or-later. See [LICENSE](LICENSE).
