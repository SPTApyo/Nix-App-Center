<p align="center">
  <img src="crates/gui/data/icons/hicolor/scalable/apps/io.github.sptapyo.NixSoftware.svg" alt="Nix Software" width="160" />
</p>

<h1 align="center">Nix Software</h1>

<p align="center">
<em>a declarative app store for Nix, built around "try first"</em>
</p>

<hr/>
<br/>

<p align="center">
  <a href="https://github.com/SPTApyo/nix-software/actions/workflows/ci.yml"><img alt="Tests" src="https://img.shields.io/github/actions/workflow/status/SPTApyo/nix-software/ci.yml?style=flat-square&label=Tests&color=007cf0&labelColor=333333" /></a>
  <a href="https://github.com/SPTApyo/nix-software/releases"><img alt="Version" src="https://img.shields.io/github/v/tag/SPTApyo/nix-software?style=flat-square&label=Version&color=7b2ff7&labelColor=333333" /></a>
  <a href="https://github.com/SPTApyo/nix-software/blob/main/LICENSE"><img alt="License" src="https://img.shields.io/github/license/SPTApyo/nix-software.svg?style=flat-square&label=License&color=ff0080&labelColor=333333" /></a>
  <img alt="GTK 4" src="https://img.shields.io/badge/GTK_4-libadwaita-007cf0?style=flat-square&labelColor=333333" />
  <a href="https://github.com/SPTApyo/nix-software/commits/main"><img alt="Updated" src="https://img.shields.io/github/last-commit/SPTApyo/nix-software.svg?style=flat-square&label=Updated&color=7b2ff7&labelColor=333333" /></a>
</p>

**Nix Software** is a GNOME Software style app store for NixOS and Home Manager. Launch any package from nixpkgs right away, nothing installed. When you want to keep it, Nix Software adds it to a small Nix module it generates, your configuration imports it, and you apply the usual way. Your own files are only touched after you accept a diff, and a backup is written first.

## Features

- **Try first**: run any app with nix run, or open a nix shell for command line tools. Unfree packages included.
- **Keep**: the app lands in nix-software-home.nix or nix-software-system.nix, imported by your configuration.
- **Knows your setup**: NixOS with or without flakes, Home Manager as a module, flake or standalone. Without a managed configuration, it installs for your user with nix profile.
- **Rich pages**: Flathub metadata (names, screenshots, ratings, age ratings, permissions) when the app exists there too, plus reviews you can write.
- **Installed**: what Nix Software keeps, what you tried (removable one by one from the store) and the apps your system and user profiles already hold.
- **Updates**: the nixpkgs revision your config is pinned to, what the branch head changes for your apps, and a one click update with a live build log.
- **Storage**: generations, apps and garbage by size, with clean-ups.
- **Search**: packages, apps and NixOS options, optional search by command, and results in the GNOME Shell overview.
- **English and French** interface.

# Getting Started

Nix Software requires **Nix with flakes enabled**.

## Installation

Try it without installing:

```bash
nix run github:SPTApyo/nix-software
```

On NixOS, as a flake input:

```nix
# flake.nix
inputs.nix-software.url = "github:SPTApyo/nix-software";

# in a module
environment.systemPackages = [
  inputs.nix-software.packages.${pkgs.system}.default
];
```

GNOME Shell search only works once the app is installed in a profile.

From source:

```bash
git clone https://github.com/SPTApyo/nix-software.git
cd nix-software
nix build .   # result/bin/nix-software and result/bin/nix-software-cli
```

# Usage

Open Nix Software from your app grid, or run nix-software.

A text interface ships next to the app:

```bash
nix-software-cli search gimp       # list matching packages
nix-software-cli run vlc           # try an app
nix-software-cli keep vlc          # add it to your configuration
nix-software-cli layout --verify   # what Nix Software detects (read only)
nix-software-cli attach            # show the import diff, --commit to write it
nix-software-cli apply             # apply the configuration
nix-software-cli updates           # nixpkgs pin against its branch head
```

## Environment

- NIX_SOFTWARE_CONFIG_DIR: NixOS configuration directory (default: /etc/nixos). Point it to a copy to experiment safely.
- NIX_SOFTWARE_LOCALEDIR: where the compiled translations are (set by the Nix package).

## Development

```bash
nix develop   # Rust toolchain, GTK 4, libadwaita, gettext, just
just setup    # commit message hook
just check    # format, clippy, tests, translations, dependencies
just run
just e2e      # end-to-end tests, screenshots in target/e2e
```

- crates/core: all the logic, no UI dependency.
- crates/gui: GTK 4 and libadwaita front end, written with relm4.
- crates/cli: text front end.

End-to-end tests drive the app through the GTK Broadway backend with Playwright. Each run uses a throwaway home and config, so your real configuration is never touched. Design notes live in [docs/design.md](docs/design.md). Workflow, commit rules, tests and translations are in [CONTRIBUTING.md](CONTRIBUTING.md).

# Community

For bugs, feature requests, and contributions, please use the [Issue Tracker](https://github.com/SPTApyo/nix-software/issues).

Made with ❤️ by [SPTApyo](https://github.com/SPTApyo).

# License

This software is distributed under the **GNU GPL-3.0-or-later**. See [LICENSE](LICENSE) for details.

Copyright (c) 2026 SPTApyo

The app icon combines two artworks:

- the bag from [GNOME Software](https://gitlab.gnome.org/GNOME/gnome-software), GPL-2.0-or-later, by the GNOME Software authors;
- the snowflake from [NixOS artwork](https://github.com/NixOS/nixos-artwork), [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/), by the NixOS project.
