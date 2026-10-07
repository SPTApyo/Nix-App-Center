# Contributing

Issues and pull requests are welcome. For anything bigger than a small fix,
open an issue first so we agree on the approach.

## Setup

You only need Nix with flakes. Everything else comes from the dev shell.

```bash
git clone https://github.com/SPTApyo/nix-software.git
cd nix-software
nix develop
just setup   # enables the commit message hook
just         # lists every command
```

| Command | What it does |
| --- | --- |
| just run | run the app |
| just check | everything CI checks: format, clippy, tests, translations, dependencies |
| just test | unit tests |
| just cov | core coverage, fails under the threshold |
| just cov-html | coverage report in the browser |
| just e2e | end-to-end tests, screenshots in target/e2e |
| just fmt | format the code |
| just pot | update po/ after changing strings |

## Workflow

1. main is protected. Every change goes through a pull request.
2. Branch from main and name the branch after its intent:
   feat/..., fix/..., refactor/..., docs/..., test/..., ci/..., chore/...
3. One pull request, one topic. Keep it small enough to review in one sitting.
4. CI must be green before merging.
5. Pull requests are squash merged: the pull request title becomes the commit
   on main, so it follows the commit rules below.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org):

```
type(scope): summary
```

- type: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert.
- scope is optional: core, gui, cli, e2e, i18n, nix, deps...
- summary: imperative, lower case, no final period, 72 characters max for
  the whole line.
- A breaking change adds ! after the type (feat!: ...) and explains the
  break in the body.
- One commit, one intent. No wip, fix typo again or mixed changes.

The hook enabled by just setup checks this locally. CI checks every commit
of a pull request.

## Tests

- Logic lives in crates/core and gets unit tests next to the code. The GUI
  stays thin.
- A bug fix comes with a test that fails without the fix.
- Code that writes files, runs commands as root or deletes store paths needs
  tests for its error cases, not only the happy path.
- Tests never touch the real system: use the sandboxes the existing tests
  use (temporary directories, NIX_SOFTWARE_CONFIG_DIR, fake binaries).
- Core line coverage must stay at or above the threshold in the justfile
  (just cov). Raise the threshold when coverage goes up, never lower it to
  make CI pass. Use just cov-html to see which lines no test reaches.
- UI changes: run just e2e and look at the screenshots in target/e2e.

## Code

- Keep it simple. No new dependency for what a few lines can do.
- New dependencies must pass just deny (licenses compatible with
  GPL-3.0-or-later, no known advisory).
- No lint suppressions: fix what clippy reports.
- Write user facing strings in English and wrap them in t() (or n_() for
  static tables translated later). Use plural() for counts.
- Never commit secrets, tokens or generated files (target/, result).

## Translations

Translations live in po/. The template is po/nix-software.pot.

Update the template and the catalogs after changing strings:

```bash
just pot
```

Add a language:

1. Copy po/nix-software.pot to po/xx.po and fill the header.
2. Add xx to po/LINGUAS.
3. Translate, then check with msgfmt --check po/xx.po.

To try a translation locally (cargo compiles po/ for you):

```bash
LANGUAGE=xx just run
```

## Matching Flathub apps to nixpkgs

Nix Software guesses which nixpkgs attribute matches a Flathub app. When the
guess is wrong or missing, add an entry to crates/core/src/data/app_mapping.json
(nixpkgs attribute to AppStream id) and check the id against the Flathub
AppStream data.

## Security

Report vulnerabilities privately, see [SECURITY.md](SECURITY.md).
