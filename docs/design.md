# Onyx design notes

This document explains why Onyx works the way it does. The code says what it
does; these notes keep the reasons.

## Product principles

- **Try first.** Running a package with `nix run` is the primary action. Keeping
  it permanently is a deliberate, secondary step.
- **Onyx never overwrites anything.** It owns its own generated files. Its only
  change to a file the user wrote is one import line, added once, after showing
  the exact diff, with explicit consent, and removable later.
- **No silent magic.** When Onyx cannot tell where the configuration lives, it
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

nixpkgs and Flathub have no official mapping. Onyx matches them in this order:

1. the user overlay (`$XDG_DATA_HOME/onyx/app-mapping.json`), for local fixes;
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
browse without network requests. Onyx does the same when Flatpak is
installed. Most NixOS machines have no Flatpak, so Onyx downloads the same
file from `dl.flathub.org` into `$XDG_CACHE_HOME/onyx/`. A copy older than a
day is used at once and refreshed in the background for the next start.

The database loads in a background thread at startup. A details page opened
during the load waits for it (bounded), because the local data holds things
the REST API lacks (add-ons, apps by the same developer). Metadata is
memoized for the session; raw `nix search` results are cached on disk for an
hour.

## Config engine

Onyx detects the layout of the user's configuration by scanning text, never
by parsing Nix:

| Layout          | Apply command                                   |
| --------------- | ----------------------------------------------- |
| NixOS flake     | `sudo nixos-rebuild switch --flake <dir>#<host>` |
| NixOS (legacy)  | `sudo nixos-rebuild switch`                     |
| HM flake        | `home-manager switch --flake <dir>#<user>`      |
| HM standalone   | `home-manager switch`                           |
| Unknown         | none, Onyx shows a snippet                      |

`$ONYX_CONFIG_DIR` overrides the NixOS root. Tests use it so they never touch
`/etc/nixos` or `~`.

Onyx writes `onyx-home.nix` and `onyx-system.nix`, generated from its
manifest (`kept.json`). On NixOS both files are imported from the top-level
`imports` list of `configuration.nix`, the safest place to splice a line.
When home-manager is a NixOS module, `onyx-home.nix` is a NixOS module
wrapping its content in `home-manager.users.<user>`.

A plain kept app is one entry of an aggregated `home.packages` list. When the
user edits its Nix in the advanced editor, the app is promoted to its own
module, delimited by `# onyx:begin <attr>` / `# onyx:end <attr>` markers.
Each promoted app is a separate module so two blocks defining the same option
are merged by the module system instead of clashing.

The advanced editor checks syntax on every keystroke (`nix-instantiate
--parse`, about 50 ms) and offers an optional, user triggered `dry-build` for
semantic errors. Applying runs in a real terminal: the build is long, asks
for a password, and must stay visible and interruptible.

## Actions

The details page is the only place to act on a package. Its split button has
"Run" as primary action, and a menu with keep/remove, try in a shell, copy
command, advanced options and apply. Search rows are purely navigational.

"Try in a Shell" exists because `nix run` is useless for CLI tools: it opens
a terminal with the package in `PATH` (`nix shell`). Terminal detection tries
`xdg-terminal-exec` first, then common emulators. Without any terminal, the
command is copied to the clipboard instead.

## Installed and Updates

See [roadmap.md](roadmap.md) for how each Nix setup stores and pins its
packages.

Installed lists, in this order: packages Onyx keeps, apps tried with nix run,
then the apps the user and system profiles already hold. The last two come
from the desktop files of `/etc/profiles/per-user/<user>`, `~/.nix-profile`
and `/run/current-system/sw`, so they show the local names and icons without
any evaluation. They are read only: Onyx does not edit what it did not write.

Removing a tried app deletes its dead closure from the store, minus what the
other tried apps need and minus anything a dead path outside that set still
refers to (nix store delete would refuse it). Derivers and sibling outputs go
along because keep-outputs and keep-derivations tie them together. The
system is never at risk: live paths are never deleted.

Updates reads the nixpkgs revision the system runs (`nixos-version --json`),
finds its branch in the config flake.lock, asks `nix flake metadata` for the
branch head and evaluates the versions the head ships for the installed apps.
Updating moves the pin and applies the config in one terminal command. When
nixpkgs comes through another flake input, Onyx says which one and does not
touch it.

## Storage

The Storage tab reads every store path once (`nix path-info --all --json`,
sizes and references) and the GC roots (`nix-store --gc --print-roots`), then
walks the reference graph in memory. Each path counts in the first category
that reaches it, so the pie chart adds up to the store: the current system
(not entering the user environments), the user environments, programs
running now (`/proc` roots), old generations, other roots (dev shells, build
results), apps tried with Onyx, and garbage. A generation's "frees" figure is
what only it reaches among all roots. The two clean-ups run through the build
log dialog: `nix-store --gc`, and `nix-collect-garbage --delete-old` as root.

## Applying, settings and integration

Every apply or update runs through one dialog that streams the command
output. A command that starts with sudo runs with pkexec instead, so the
password prompt is graphical and the build stays in the app; without pkexec
it falls back on a terminal.

Settings live in `$XDG_CONFIG_HOME/onyx/settings.json`: configuration folder
and host overrides, the daily update check, and the random id used only when
posting a review (reading reviews is anonymous). ONYX_CONFIG_DIR, used by the
tests, bypasses the saved overrides.

Without a managed configuration, Keep installs with nix profile instead.
Packages without an AppStream icon get a glyph and a color from a rough
guess of their family (package set, then description words).

The GNOME Shell search provider answers from the local AppStream database
only, so it never waits on the network; activating a result resolves it to
nixpkgs and opens its page.

## Details page

The details page copies the layout of GNOME Software's `gs-details-page.ui`
widget by widget (one module per widget in `crates/gui/src/components/details`),
while keeping Onyx semantics: context tiles show Nix signals (closure
download size, `broken`/`insecure`), the version badge means "this release is
the one in nixpkgs", and sections without Nix data stay hidden.

## Internationalization

Same mechanism as GNOME Software: gettext driven by the system locale, no
language picker. English source strings are the msgids. Translations live in
`po/<lang>.po`; `po/update-pot.sh` regenerates `po/onyx.pot` and merges it into
every catalog. Core strings shown in the UI are marked with `n_()` and
translated by the GUI where they are displayed.

## Development gallery

`cargo run -p onyx-gui --features dev-gallery` adds a "Gallery" tab listing
real, labeled scenarios (app with rating, broken package, no metadata…) that
open the real views. The feature is off by default and absent from release
builds.
