# Nix Software design notes

This document explains why Nix Software works the way it does. The code says what it
does; these notes keep the reasons.

## Product principles

- **Try first.** Running a package with `nix run` is the primary action. Keeping
  it permanently is a deliberate, secondary step.
- **Nix Software never overwrites anything.** It owns its own generated files. Its only
  change to a file the user wrote is one import line, added once, after showing
  the exact diff, with explicit consent, and removable later.
- **No silent magic.** When Nix Software cannot tell where the configuration lives, it
  does not guess. It shows the snippet to paste and stops there.
- **The network is optional.** Flathub and ODRS only provide cosmetics (icons,
  screenshots, ratings). Every network call has a short timeout and a silent
  fallback; install and run always go through Nix.

## Architecture

```text
crates/core   pure logic, no UI dependency (search, run, keep, config engine)
crates/gui    GTK4 + libadwaita front end built with relm4
crates/cli    text front end, used as a test bench for the core
```

The core models packages and actions as data (`Package`, `Action`,
`actions_for`) so each front end renders the same logic its own way.

## Package identity: nixpkgs vs AppStream

nixpkgs and Flathub have no official mapping. Nix Software matches them in this order:

1. the user overlay (`$XDG_DATA_HOME/nix-software/app-mapping.json`), for local fixes;
2. the community overlay (`crates/core/src/data/app_mapping.json`), curated in
   this repository;
3. name heuristics: name without spaces, last app id segment, first word.

A heuristic key is dropped when an overlay pins that attr to another app (for
example nixpkgs `moonlight` is a Discord client, not Moonlight game
streaming). In search results, several attrs often share one AppStream entry
(`gimp`, `gimp3`, `gimp-with-plugins`): only the best ranked one takes the app
identity, the others stay plain packages named by attr.

To fix a wrong match, add an entry to the community overlay and open a pull
request.

## AppStream data

GNOME Software reads Flatpak's local `appstream.xml.gz`, which is why it can
browse without network requests. Nix Software does the same when Flatpak is
installed. Most NixOS machines have no Flatpak, so Nix Software downloads the same
file from `dl.flathub.org` into `$XDG_CACHE_HOME/nix-software/`. A copy older than a
day is used at once and refreshed in the background for the next start.

The database loads in a background thread at startup. A details page opened
during the load waits for it (bounded), because the local data holds things
the REST API lacks (add-ons, apps by the same developer). Metadata is
memoized for the session; raw `nix search` results are cached on disk for an
hour.

## Config engine

Nix Software detects the layout of the user's configuration by scanning text, never
by parsing Nix:

| Layout          | Apply command                                   |
| --------------- | ----------------------------------------------- |
| NixOS flake     | `sudo nixos-rebuild switch --flake <dir>#<host>` |
| NixOS (legacy)  | `sudo nixos-rebuild switch`                     |
| HM flake        | `home-manager switch --flake <dir>#<user>`      |
| HM standalone   | `home-manager switch`                           |
| Unknown         | none, Nix Software shows a snippet                      |

`$NIX_SOFTWARE_CONFIG_DIR` overrides the NixOS root. Tests use it so they never touch
`/etc/nixos` or `~`.

Nix Software writes `nix-software-home.nix` and `nix-software-system.nix`, generated from its
manifest (`kept.json`). On NixOS both files are imported from the top-level
`imports` list of `configuration.nix`, the safest place to splice a line. A
flake without `configuration.nix` (one that extends another flake, say) gets
them in the `modules` list of `flake.nix` instead. When home-manager is a
NixOS module, `nix-software-home.nix` wraps its content in
`home-manager.users.<user>`; without home-manager it fills
`users.users.<user>.packages`.

`/etc/nixos` belongs to root. Files the user cannot write are written in one
pkexec call, so the password is asked once per change: the content travels
inside the script, each file goes through `mktemp` in its own folder, and root
runs with the system PATH only. The Nix Software files are written before the
manifest, so a refused password leaves everything as it was.

A plain kept app is one entry of an aggregated `home.packages` list. When the
user edits its Nix in the advanced editor, the app is promoted to its own
module, delimited by `# nix-software:begin <attr>` / `# nix-software:end <attr>` markers.
Each promoted app is a separate module so two blocks defining the same option
are merged by the module system instead of clashing.

The advanced editor checks syntax on every keystroke (`nix-instantiate
--parse`, about 50 ms) and offers an optional, user triggered `dry-build` for
semantic errors.

## Actions

The details page is the only place to act on a package. Its split button has
"Run" as primary action, and a menu with keep/remove, try in a shell, copy
command, advanced options and apply. Search rows are purely navigational.

A trash button sits between the main button and the menu arrow once there
is something to take off.
It removes what Nix Software put there: a kept app leaves the configuration,
an app installed with nix profile is uninstalled, a tried app is deleted
from the store. Apps your own configuration installs get no trash button.

"Try in a Shell" exists because `nix run` is useless for CLI tools: it opens
a terminal with the package in `PATH` (`nix shell`). Terminal detection tries
`xdg-terminal-exec` first, then common emulators. Without any terminal, the
command is copied to the clipboard instead.

## Installed and Updates

See [roadmap.md](roadmap.md) for how each Nix setup stores and pins its
packages.

Installed lists, in this order: packages Nix Software keeps, apps tried with nix run,
then the apps the user and system profiles already hold. The last two come
from the desktop files of `/etc/profiles/per-user/<user>`, `~/.nix-profile`
and `/run/current-system/sw`, so they show the local names and icons without
any evaluation. They are read only: Nix Software does not edit what it did not write.

Removing a tried app deletes its dead closure from the store, minus what the
other tried apps need and minus anything a dead path outside that set still
refers to (nix store delete would refuse it). Derivers and sibling outputs go
along because keep-outputs and keep-derivations tie them together. The
system is never at risk: live paths are never deleted.

Updates reads the nixpkgs revision the system runs (`nixos-version --json`),
finds its branch in the config flake.lock, asks `nix flake metadata` for the
branch head and evaluates the versions the head ships for the installed apps.
Updating moves the pin and applies the config in one terminal command. When
nixpkgs comes through another flake input, Nix Software says which one and does not
touch it.

## Holding a version

Every package follows the nixpkgs revision the configuration pins, so holding
one app means taking it from another revision. A hold records that revision,
the hash of its tarball and the day it started (kept.json, field pin); the
revision is the truth, the version is only shown. The generated file imports
each held revision once, in a let binding shared by its apps:

    nixpkgs-cf5e76507c6e = import (builtins.fetchTarball {
      url = "https://github.com/NixOS/nixpkgs/archive/<rev>.tar.gz";
      sha256 = "<hash>";
    }) {
      config = lib.filterAttrs (_: v: v != null) pkgs.config;
      system = pkgs.stdenv.hostPlatform.system;
    };

fetchTarball works with flakes and channels without editing flake.nix. Pure
evaluation, the flake default, refuses it without a hash, so the hash is
computed once with nix-prefetch-url --unpack when the user holds an app. The
held nixpkgs reuses the main one's settings (allowUnfree and the rest);
passing pkgs.config whole fails, since its unset options are null and do not
type check again. Overlays of the main nixpkgs do not apply to the held one.

Held apps leave the update list and show in their own section with a Release
button; their page warns that they get no security updates, more strongly
after 30 days. Build options and the install scope are structured fields too
(flags, system_wide), so a hold combines with them. Manifests written before
read their old generated blocks back into these fields.

"Choose a Version" lists the versions nixpkgs shipped over time, from the
Devbox package index (search.devbox.sh, the data behind nixhub.io): each one
comes with the nixpkgs revision and store path that shipped it, and a request
to cache.nixos.org says whether it downloads or builds locally. Picking one
is a hold at that revision. Only the package name is sent to the index.

## Progress of an apply

Applying or updating runs nixos-rebuild with --log-format internal-json:
Nix then reports structured events instead of text (crates/core/src/progress.rs).
Copy and build activities carry done and expected counts, summed across
activities, which give the percentage; the time left extrapolates from the
time spent once five percent is done. Evaluation and activation report no
counts, so the bar pulses there instead of showing a made-up number. Messages
and build log lines go to the details log, and the last error line of a
failure becomes the message the user sees. Commands without JSON logs (a
custom apply command, Home Manager) are read as plain text: "these 12 paths
will be fetched", then one "copying path" or "building" line each. The garbage
collector keeps a pulsing bar.

An apply is a list of steps run in order, stopping at the first failure. An
update is two steps: refresh the lock or channel, then apply.

## Building as the user

A NixOS flake switch is split in two. nixos-rebuild build runs as the user in
a fresh private temporary folder, then root only activates the result:
nix-env sets the system profile and switch-to-configuration switches to it.
Root cannot read a git repository it does not own, and has neither the user's
SSH keys nor their git credentials, so building as root fails on a
configuration kept in a private repository. The result path is checked to be
a plain store path before root runs anything from it.

Flake inputs that point to a local folder (git+file or path) are passed again
with --override-input, so the configuration is built from the files on disk,
including changes that are not committed or pushed yet. This covers the
common setup of a small /etc/nixos flake that imports the real configuration
from the home folder.

For setups the detection cannot guess, Preferences has an advanced apply
command run by the shell. A leading sudo runs it as administrator through
pkexec, so tools that call sudo on their own do not work there. Detection
still decides where apps are written.

The dialog can be closed while the work goes on; a desktop notification
reports the end when the app is not in front. There is no notification per
step: GNOME asks apps not to flood the user.

## Installed pill

Tiles and search rows of installed apps carry a pill: a green check in a
darker round, then where the app is (a screen for the system profile, a
house for the user profiles, layers for Flatpak, an arrow box for an
AppImage), then how a Nix app came (a box for a package, a puzzle piece for
a module). On tiles it floats in the top right corner.

One index answers for tiles, rows and app pages
(crates/core/src/installed.rs). It is read at most every five seconds:

- the store references of each profile (`nix-store --query --references`),
  walking through the buildEnv layers (user-environment,
  home-manager-path, system-path). Command line tools, fonts and themes
  have no desktop file but are there; fonts.packages are read from the
  fontconfig cache file, since they stay out of the system profile;
- the desktop files of the profiles, for the names apps go by (steam ships
  steam.desktop but resolves to steam-unwrapped);
- `flatpak list`, and desktop files in ~/.local/share/applications whose
  Exec runs a .AppImage (crates/core/src/foreign.rs).

Tiles and rows register their pill; a successful apply reads the machine
again and redraws every pill on screen, so the home page follows a change
without a restart.

An app matches by pname, desktop file name or Flathub app id. A nested
attribute such as haskellPackages.Obsidian must match the version too, or
every package named like an installed app would show the pill.

Modules are first guessed from the files: any `X.enable` not set to false in
the .nix files of the configuration or of its local inputs, the two Nix
Software writes included, whatever its form. The guess is instant and works on a configuration that does not
evaluate. In the background, Nix then evaluates which of those names are
really enabled (NixOS and Home Manager options), and the answer is cached
until a file changes.

## Who installed it

An app installed but not kept by Nix Software is looked up in the user's
files: the line listing it in a `packages = …;` or `systemPackages = …;`
definition, lists combined with `++` and `with pkgs;` included. The app page
names the file and line, and offers Take Over: the app is kept, and the user
deletes their own line. Nix Software never edits that file.

The trash button removes what Nix Software can remove: a kept app, a nix
profile or nix-env install, a Flatpak (`flatpak uninstall`), an AppImage
(the image and its desktop file, after a confirmation naming both), a tried
app.

When the installed version is neither the nixpkgs one nor an older one
waiting for an apply, the configuration changes the app, usually with an
overlay. The page says so, and Updates leaves it alone.

## Add-ons

Add-ons come from AppStream and have no nixpkgs attribute. A click on one
searches nixpkgs for its name and opens the best match, preferring the
parent's package set (gimpPlugins for gimp); without a match, a toast says
it is not in nixpkgs.

## Install modes

A kept app is installed one of four ways: in the user's home, in the
system packages, through its NixOS module (`X.enable = true` in its system
block) or through its Home Manager module (the same line in its home
block). Home Manager's options come from its docs-json package.
Installation Options… in its menu switches between them with two choices:
where (home or everyone) and how (package or module); the module offered
follows the place, the NixOS one for everyone, Home Manager's for the home. It is offered only while the entry
is one of these; custom blocks say more than a mode and are left alone.

## Other flakes

Search also runs `nix search` on the GitHub inputs of the configuration's
flake.lock and of its local inputs, nixpkgs and plumbing (flake-utils,
home-manager…) aside, in parallel. Their packages carry the input name,
shown as their origin. They are written as
`inputs.NAME.packages.${pkgs.stdenv.hostPlatform.system}.ATTR`; the
generated file then takes `inputs ? { }`, which the configuration must pass
through specialArgs (or extraSpecialArgs for Home Manager).

## Pending changes

Every change to the configuration leaves a marker in the data folder; a
successful apply of the configuration removes it. While it is there, an
Apply button shows in the header bar, so nobody has to open an app to find
it.

## Nix Software in its own catalog

Once in nixpkgs, Nix Software lists itself. Its page works like any other
(add, remove, hold), except Try and Open: the app is single instance, so a
second launch only raises the window already open. The primary button says
so instead.

## Storage

The Storage tab reads every store path once (`nix path-info --all --json`,
sizes and references) and the GC roots (`nix-store --gc --print-roots`), then
walks the reference graph in memory. Each path counts in the first category
that reaches it, so the pie chart adds up to the store: the current system
(not entering the user environments), the user environments, programs
running now (`/proc` roots), old generations, other roots (dev shells, build
results), apps tried with Nix Software, and garbage. A generation's "frees" figure is
what only it reaches among all roots. The two clean-ups run through the build
log dialog: `nix-store --gc`, and `nix-collect-garbage --delete-old` as root.

## Applying, settings and integration

Every apply or update runs through one dialog that streams the command
output. A command that starts with sudo runs with pkexec instead, so the
password prompt is graphical and the build stays in the app; without pkexec
it falls back on a terminal.

Settings live in `$XDG_CONFIG_HOME/nix-software/settings.json`: configuration folder
and host overrides, the daily update check, and the random id used only when
posting a review (reading reviews is anonymous). NIX_SOFTWARE_CONFIG_DIR, used by the
tests, bypasses the saved overrides.

"Install for Everyone" sends newly kept apps to environment.systemPackages,
as one region per app in the system file, instead of the user's packages.
Apps kept before keep their scope.

Without a managed configuration, Keep installs with nix profile instead. That
is also the case on other Linux distributions with Nix: there is no
nixos-version, so Updates reads the branch the flake registry points
nixpkgs to, compares only the apps of ~/.nix-profile, and updates with
nix profile upgrade --all. A profile has no single revision, so it is up to
date when no app changes.
Packages without an AppStream icon get a glyph and a color from a rough
guess of their family (package set, then description words).

The GNOME Shell search provider answers from the local AppStream database
only, so it never waits on the network; activating a result resolves it to
nixpkgs and opens its page.

## Details page

The details page copies the layout of GNOME Software's `gs-details-page.ui`
widget by widget (one module per widget in `crates/gui/src/components/details`),
while keeping Nix Software semantics: context tiles show Nix signals (closure
download size, `broken`/`insecure`), the version badge marks the release Nix gives with the configuration's
nixpkgs ("Installed" when it is the one on the machine, "Available"
otherwise), and sections without Nix data stay hidden.

The version section follows nixpkgs, not Flathub. Flathub's release list
starts at the release equal to the nixpkgs version (ignoring a leading v and
trailing .0), so newer Flathub releases Nix cannot install are left out.
When Flathub has no such release, the history comes from nixpkgs itself:
the versions and dates the Devbox index lists for the attribute, loaded in
the background when the page opens, from the nixpkgs version down, without
notes (crates/core/src/app_meta.rs, nix_releases). The same request feeds
Choose a Version…, at the end of the section, so the list is ready when
clicked; the binary cache checks run side by side.

Some apps only work well through their NixOS module: steam, docker or
tailscale need services, groups or firewall rules the package alone does not
set up. The page looks the package up in the NixOS options of the config's
branch: a module counts when its package option defaults to the package
(programs.steam.package = pkgs.steam) or when it is named after it
(programs.gamemode). When such a module exists, Add to Configuration first
asks where and how to install the app, so the choice is made once, at the
start; Installation Options… changes it later.

Build options are the boolean arguments of the package recipe
(withQt5 ? true in vlc), read from the recipe file meta.position points to:
Nix gives the argument names but not their defaults. Saving keeps the app as
(pkgs.vlc.override { withQt5 = false; }) in the user block. The page warns
that a changed option builds the app locally, since the binary cache only
holds the default build. Build options belong to the package, so they only
show for an app kept as a plain package, not through a module. Apps the
user's own configuration installs show no options either: Nix Software does
not edit what it did not write.

## First launch

A welcome tour opens on first launch (settings.json, field onboarded): try,
keep, the configuration found with a check by Nix, applying with the two
main settings, and where to go next. Each page plays a short entrance
animation when it comes into view. The tour stays in the main menu. Actions that are not obvious
carry a tooltip saying what they change and when.

## Internationalization

Same mechanism as GNOME Software: gettext driven by the system locale, no
language picker. English source strings are the msgids. Translations live in
`po/<lang>.po`; `po/update-pot.sh` regenerates `po/nix-software.pot` and merges it into
every catalog. Core strings shown in the UI are marked with `n_()` and
translated by the GUI where they are displayed.

## Development gallery

`cargo run -p nix-software-gui --features dev-gallery` adds a "Gallery" tab listing
real, labeled scenarios (app with rating, broken package, no metadata…) that
open the real views. The feature is off by default and absent from release
builds.
