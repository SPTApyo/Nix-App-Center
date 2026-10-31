# User guide

How to use Nix App Center. Short on purpose.

## The idea

1. **Try**: the app starts without being installed. Close it and nothing
   changed.
2. **Add to Configuration**: the app is written into your configuration.
   Nothing is installed yet.
3. **Apply**: your system is rebuilt and the app is installed. The Apply
   button shows at the top of the window whenever something changed. Once
   built, Nix App Center lists what changes (updated, added, removed apps)
   and switches only when you confirm.

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

## Proprietary and insecure apps

Nix installs a proprietary app, such as Steam or Discord, only with your
permission. When you add one, Nix App Center asks: allow this app, or allow
every proprietary app you add from now on (Preferences can change that
later). Only the apps you add are allowed, nothing else on your system.
If your configuration already allows the app, nothing is asked.

An app nixpkgs marks insecure asks every time, with its known
vulnerabilities.

## Updates

The Updates tab shows what can be updated. Update refreshes your
configuration's nixpkgs, then applies.

To stay on a version, open the app, then Choose a Version… under its
version. Release it from the same place or from Updates.

An app from another flake may have no ready build. If its flake has its own
binary cache, Nix App Center offers it when you add the flake in
Preferences, and from the Download Size tile of the app's page. Use it only
for a flake you trust: its server signs what you install. Preferences lists
the caches in use and removes them.

## Going back

Something broke after an apply? Undo is right in the apply dialog. Later,
the History list at the bottom of Updates shows every earlier version: Go
Back switches to it, and the apps you added with Nix App Center go back too.

## Storage

The Storage tab shows what fills the Nix store. Free Up Space removes what
nothing uses anymore. Why? on an app says what keeps it in the store: old
generations, the running system or a running program. Clean Up Automatically,
in Preferences, deletes unused paths and generations older than 30 days every
week.

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
