# Testing

How the tests are organized, how to run them, and how they stay away from the
real system.

## Unit tests

Tests sit next to the code, in a tests module at the bottom of each file.
Run them with just test, or one test with
cargo test -p nix-app-center-core NAME.

Most logic is split into a pure function and a thin wrapper that runs
commands. Test the pure function: plan_removal in tried.rs, parse_roots and
analyse in storage.rs, source_from_lock in updates.rs and
root_write_script in paths.rs are the models.

Tests that read environment variables or the data directory take
crate::env_lock() first, because cargo runs tests in parallel and the
environment is shared. Each of them works in its own temporary directory
(see sandbox in manifest.rs and fixture in config_layout.rs).

## Coverage

just cov measures line coverage of crates/core and fails under the threshold
in the justfile. just cov-html opens a report showing the lines no test
reaches. The GUI is not measured: the end-to-end suite covers it.

Look at coverage before deciding where to add tests. Code that writes files,
runs commands as root or deletes store paths comes first.

## Nix checks

nix build .#checks.x86_64-linux.module starts a NixOS virtual machine with
programs.nix-app-center enabled and checks the app, its desktop entry and its
search provider are there (nix/test.nix, the same test nixpkgs runs).
nix build .#checks.x86_64-linux.apply runs a real nixos-rebuild in a
virtual machine, offline (nix/test-apply.nix): the CLI attaches, keeps hello,
applies, removes it, goes back to the generation that had it, and checks the
configuration came back with it. It takes a few minutes and several GB, so
run it when you change applying or going back.
nix build .#nix-app-center.tests.version checks the packaged CLI answers.
CI runs the module test and the version check, and builds on aarch64-linux
too.

## End-to-end tests

tests/e2e/nix_app_center_e2e.py runs the real app on a GTK Broadway display
and drives it with Playwright from a headless Chromium. just e2e runs the
full scenario and writes screenshots to target/e2e.

Each run isolates everything:

- throwaway XDG data, config and cache directories;
- a fake NixOS configuration, through NIX_APP_CENTER_CONFIG_DIR;
- its own D-Bus session, so a running Nix App Center does not swallow it;
- fake pkexec, nixos-rebuild and xdg-terminal-exec in PATH. They log how the
  app called them, and the scenario asserts on those logs.

Broadway draws on a canvas, so the scenario clicks by coordinates. The
constants at the top of the file (TAB_INSTALLED, PRIMARY...) name them.

To add a step, start an isolated app and look before you click:

```bash
tests/e2e/run.sh --serve
python3 tests/e2e/session.py target/e2e/look.png click:820,145 wait:800
```

session.py accepts click:X,Y, type:TEXT, key:NAME, wait:MS and
scroll:DY@X,Y. Check each screenshot, then copy the working commands into
the scenario with an assertion on the files or fake logs they should change.
