# Agent instructions

How to work in this repository as a coding agent. Project documentation lives
in [docs/](docs/README.md) and [CONTRIBUTING.md](CONTRIBUTING.md); this file
only covers verification, safety and which doc to read. It duplicates nothing
from them.

## Verification

Run everything inside the dev shell: nix develop -c just check. It covers
format, clippy, unit tests, translations and dependencies.

- One test: nix develop -c cargo test -p nix-software-core NAME.
- Coverage of the core: nix develop -c just cov. It fails under the threshold
  set in the justfile. Never lower the threshold to make it pass.
- just check does not run the end-to-end suite. Run it (just e2e) when you
  change the UI or the details actions, and say which suites you ran.

clippy runs with -D warnings. Fix what it reports; never add #[allow(...)].
Formatting belongs to rustfmt: run just fmt, do not hand-format.

## Safety

The app edits Nix configurations and runs commands as root. While working:

- Never write to /etc/nixos, the user's own configuration or the real
  ~/.local/share/nix-software. Point NIX_SOFTWARE_CONFIG_DIR at a temporary
  copy when you need a configuration.
- Never run nixos-rebuild, home-manager switch, pkexec, sudo or
  nix-collect-garbage yourself. The e2e suite replaces them with fakes.
- Do not kill a running nix-software process you did not start: it may be the
  user's. The e2e suite uses its own D-Bus session and display.
- Network access is needed for nixpkgs, Flathub and ODRS. Unit tests must not
  depend on it.

## Committing

Do not commit, push or open a pull request unless asked. When asked, follow
the commit rules in CONTRIBUTING.md; the hook enabled by just setup checks
them. Branch first if you are on main.

## Driving the app

tests/e2e/run.sh --serve starts an isolated app on a GTK Broadway display.
tests/e2e/session.py then clicks, types and takes screenshots against it:

```bash
python3 tests/e2e/session.py target/e2e/look.png click:443,48 wait:800
```

Use it to check UI changes instead of guessing, and look at the screenshot.

## Which doc to read

- Finding your way around: docs/repo-layout.md
- Why the app works the way it does: docs/design.md
- Writing or fixing tests: docs/testing.md
- What is done and what is missing: docs/roadmap.md
- Commits, pull requests, translations: CONTRIBUTING.md
- Packaging, releases, nixpkgs: docs/packaging.md
- Security boundaries: SECURITY.md
