# User guide

Nix Software in three steps.

1. **Try**: the app starts without being installed. Close it and nothing
   changed.
2. **Add to Configuration**: the app is written into your configuration.
   Nothing is installed yet.
3. **Apply**: your system is rebuilt and the app is installed.

The first time, Nix Software asks to add one import line to your
configuration, and shows it before writing anything.

If apps go to the wrong place, open Preferences and check where Nix Software
found your configuration. Leave the fields empty to let it find it by itself.
