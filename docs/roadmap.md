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

## Compared to GNOME Software

Done:

- Installed list with one row per app, its size, and a remove button for what
  Onyx owns (kept, tried, nix profile).
- Updates: what changes per app, one button to update, a daily background
  check with a notification and a badge, a preference to turn it off.
- Banner when a newer NixOS release is out.
- "Open" on the page of an app already installed.
- GNOME Shell search provider.
- Writing reviews to ODRS.
- Storage tab: where the store space goes (system, user, running programs,
  old generations, development, tried apps, garbage), every generation with
  what deleting it frees, apps by size, and the two clean-ups.

Missing:

- Cache and data usage per app in the home folder (only the store is measured).
- Metered connection awareness for the background check.
- Reporting or voting on a review.
- Firmware updates (fwupd) and web apps: out of scope for Onyx.

## Compared to nix-software-center

Done:

- Preferences: configuration folder, host, a check of the detection by Nix
  itself.
- Rebuild progress inside the app (pkexec and a live build log).
- Updating the system channel or the nixpkgs flake input from the app.
- Installing with nix profile when no configuration is managed.
- NixOS option search.

## Problems fixed

- Installed only showed what Onyx itself recorded, not what is on the machine.
- A tried app could not be removed alone.
- AppStream icons: the sharpest variant was picked even when Flathub does not
  host it (VLC), now smaller ones are tried in turn. The daily AppStream
  refresh no longer blocks the start.
- Updates compared against the registry nixpkgs and had no action.
- Run, search and details used the registry nixpkgs while Keep used the
  config nixpkgs. They now all use the revision the config is pinned to.
- Copy Command lacked NIXPKGS_ALLOW_UNFREE for unfree packages.
- Generated files were invisible to a flake in a git repository. Onyx now runs
  git add --intent-to-add on them.
- The safety tile showed the Flatpak sandbox permissions, which do not apply
  to a Nix package. It now says the app is not sandboxed.
- Command search passed an option nix-locate does not know and never worked.
- Reviews never loaded: ODRS rejects a request without a version.
- A name match could pick an app the overlay pins to another attribute
  (zoom, the Z-code player, showed Zoom's ratings).
- Packages without an icon all looked the same. They now get a glyph and a
  color per family (game, audio, development, library…).

## Still open

- Layout detection scans text. Preferences let the user correct it and check
  it with Nix, but layouts built from another flake are still guessed.
- When nixpkgs comes through another flake input, Onyx explains it but cannot
  update that flake.
- The first search downloads nixpkgs when the config revision is not in the
  local cache.
- GNOME Shell only finds the search provider when Onyx is installed in a
  profile that links share/gnome-shell/search-providers (the NixOS GNOME
  module does).
