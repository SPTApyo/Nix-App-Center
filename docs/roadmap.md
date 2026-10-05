# Roadmap

Where Onyx stands against GNOME Software and nix-software-center, what is
broken today, and how Onyx should read and change a Nix setup.

## How a Nix machine keeps its software

Onyx has to deal with every common setup. What matters for an app store is
where packages are declared, where they end up once applied, and where the
nixpkgs revision is pinned.

| Setup | Declared in | Ends up in | nixpkgs pinned by | Apply | Update nixpkgs |
|---|---|---|---|---|---|
| NixOS, flake | environment.systemPackages | /run/current-system/sw | flake.lock of the system flake | sudo nixos-rebuild switch --flake dir#host | nix flake update nixpkgs --flake dir, then apply |
| NixOS, channels | environment.systemPackages | /run/current-system/sw | root channel nixos | sudo nixos-rebuild switch | sudo nixos-rebuild switch --upgrade |
| NixOS user packages | users.users.NAME.packages | /etc/profiles/per-user/NAME | same as the system | same as the system | same as the system |
| Home Manager as a NixOS module | home-manager.users.NAME home.packages | /etc/profiles/per-user/NAME (useUserPackages) or ~/.nix-profile | the system nixpkgs (useGlobalPkgs) or its own pkgs | same as the system | same as the system |
| Home Manager standalone, flake | home.packages | ~/.nix-profile (home-path) | ~/.config/home-manager/flake.lock | home-manager switch --flake dir | nix flake update nixpkgs --flake dir, then apply |
| Home Manager standalone, channels | home.packages | ~/.nix-profile (home-path) | user channels | home-manager switch | nix-channel --update, then apply |
| nix profile / nix-env | imperative | ~/.nix-profile | per package | none | nix profile upgrade |
| nix run / nix shell (Onyx "try") | nothing | store only, no GC root | the registry nixpkgs | none | none |

Facts Onyx can rely on without evaluating anything:

- The running system lists its apps in /run/current-system/sw/share/applications,
  user apps in /etc/profiles/per-user/NAME/share/applications and
  ~/.nix-profile/share/applications. Each desktop file is a symlink into the
  store path of its package, whose name gives pname and version.
- nixos-version --json gives the nixpkgs revision the system was built from,
  whatever the config shape.
- A flake.lock holds every transitive input. The node whose locked rev matches
  that revision tells the branch it follows (original.ref) and whether it is a
  direct input of the root flake (updatable there) or comes through another
  flake (must be updated in that flake).
- A store path that no GC root reaches is dead. nix store delete removes dead
  paths only, so deleting a tried app can never break the system. With
  keep-outputs and keep-derivations set, the deriver .drv must be deleted
  together with its outputs.

## Missing compared to GNOME Software

- Installed list with one row per app, size and an uninstall button.
- Real updates: what changes, one button to update, notifications, automatic
  checks in the background.
- Update preferences (automatic updates, metered connections).
- Software repositories dialog (here: nixpkgs branch, flake inputs).
- System upgrade banner when a new NixOS release is out.
- Storage usage per app (installed size, cache, data).
- Launch an installed app from its page ("Open" button).
- GNOME Shell search provider.
- Writing reviews (ODRS) and reporting a review.
- Firmware updates (fwupd), web apps. Out of scope for Onyx.

## Missing compared to nix-software-center

- Settings page: config path, flake path, flake attribute, override of the
  detection.
- In-app progress of a rebuild (it shows the build output in a dialog).
- Updating the system channel or flake inputs from the app.
- Installing to nix profile / nix-env for users without a managed config.
- Searching options (it ships nixos-conf-editor next to it).

## Current problems in Onyx

Fixed in this round:

- Installed only showed what Onyx itself recorded, not what is on the machine.
- A tried app could not be removed alone. The only option was a full
  nix-collect-garbage of the whole store.
- Tried apps could show the generic icon. The AppStream file was downloaded
  again before loading once it was a day old, so the first listing fell back on
  the network and stayed poor.
- Updates compared the version saved when keeping a package with the registry
  nixpkgs (usually unstable), not with the nixpkgs the config is pinned to. It
  had no action. It was useless.
- "Free Up Space" stayed visible with an empty history.

Still open:

- Run uses the registry nixpkgs while Keep uses the config nixpkgs, so the
  version you try is not always the version you get.
- Copy Command gives nix run nixpkgs#x without NIXPKGS_ALLOW_UNFREE, so it
  fails for unfree packages.
- A flake inside a git repository only sees tracked files. Onyx does not run
  git add --intent-to-add on the files it creates.
- Apply runs in a terminal (sudo needs a tty). There is no progress inside the
  app and no polkit flow.
- Layout detection scans text. Layouts built from another flake (extendModules,
  flake-parts, multiple hosts with shared modules) are guessed, never parsed.
- nix-locate (nix-index) is optional and missing on most machines, so the
  "which package provides this command" search is dead there.
- The safety tile needs Flatpak permission data: apps that are not on Flathub
  show "unknown".
- Search runs nix search on the registry nixpkgs: slow the first time, and
  again after each nixpkgs update.
- Reviews are read only.
