# User guide

How to use Nix Software. Short on purpose.

## The idea

1. **Try**: the app starts without being installed. Close it and nothing
   changed.
2. **Add to Configuration**: the app is written into your configuration.
   Nothing is installed yet.
3. **Apply**: your system is rebuilt and the app is installed.

The first time you add an app, Nix Software asks to add one import line to
your configuration. It shows the change before writing anything.

## What is installed

Installed apps glow in the lists:

- blue with a screen icon: installed on the system, for every user;
- green with a house icon: installed in your home, for you only.

The Installed tab lists them all, with what you tried and can clean up.

## Updates

The Updates tab shows what can be updated. Update refreshes your
configuration's nixpkgs, then applies.

To stay on a version, open the app, then Version… in its menu. Release it
from the same place or from Updates.

## Storage

The Storage tab shows what fills the Nix store. Free Up Space removes what
nothing uses anymore.

## When it goes wrong

- **Apps go to the wrong place**: open Preferences and look at Detected. Leave
  the fields empty to let Nix Software find your configuration.
- **Apply fails**: open Show Details in the progress window. The last error
  line is usually the one that matters, and it is often in your own
  configuration, not in the app you added.
- **Unusual setup**: Preferences, Advanced, Apply command. Put the command you
  run by hand. Start it with sudo if it needs root.
