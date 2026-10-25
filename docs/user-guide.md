# User guide

How to use Nix App Center. Short on purpose.

## The idea

1. **Try**: the app starts without being installed. Close it and nothing
   changed.
2. **Add to Configuration**: the app is written into your configuration.
   Nothing is installed yet.
3. **Apply**: your system is rebuilt and the app is installed. The Apply
   button shows at the top of the window whenever something changed.

The first time you add an app, Nix App Center asks to add one import line to
your configuration. It shows the change before writing anything.

## What is installed

Installed apps show a pill: a green check, then where the app is (house:
your home, screen: the system, layers: Flatpak, box with an arrow: AppImage)
and how (box: a package, puzzle piece: a module).

If your own configuration installs an app, its page shows the file and
line, and Take Over moves it to Nix App Center.

To install an app you added another way, open it, then Installation Options…
in its menu. The trash button next to the main button removes the app, whatever
installed it.

The Installed tab lists them all, with what you tried and can clean up.

## Updates

The Updates tab shows what can be updated. Update refreshes your
configuration's nixpkgs, then applies.

To stay on a version, open the app, then Choose a Version… under its
version. Release it from the same place or from Updates.

An app from another flake may have no ready build. If its flake has its own
binary cache, the Download Size tile on its page offers it. Use it only for
a flake you trust: its server signs what you install.

## Storage

The Storage tab shows what fills the Nix store. Free Up Space removes what
nothing uses anymore.

## Keyboard

Ctrl+1 to Ctrl+4 open Explore, Installed, Updates and Storage, Ctrl+F
searches and Alt+Left goes back. Hold Alt to see the underlined letter of
each button: Alt and that letter press it. Ctrl+? lists every shortcut.

## When it goes wrong

- **Apps go to the wrong place**: open Preferences and look at Detected. Leave
  the fields empty to let Nix App Center find your configuration.
- **Apply fails**: open Show Details in the progress window. The last error
  line is usually the one that matters, and it is often in your own
  configuration, not in the app you added.
- **Unusual setup**: Preferences, Advanced, Apply command. Put the command you
  run by hand. Start it with sudo if it needs root.
