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
3. One pull request, one topic. If its description needs the word "and", it
   is probably two pull requests. As a guide, stay under 400 changed lines
   and 10 files, lockfiles and generated files aside. Bigger than that: open
   an issue first and agree on the approach.
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
  use (temporary directories, NIX_SOFTWARE_CONFIG_DIR, fake binaries). See
  [docs/testing.md](docs/testing.md).
- Core line coverage must stay at or above the threshold in the justfile
  (just cov). Raise the threshold when coverage goes up, never lower it to
  make CI pass. Use just cov-html to see which lines no test reaches.
- UI changes: run just e2e and look at the screenshots in target/e2e.

## Comments

We follow the rustdoc conventions, kept short:

- //! at the top of a module: what the module is for, one to three lines.
- /// on a function or a type: the first line says what it does and what it
  returns. Add a sentence for a parameter only when its name and type do not
  say enough, and an # Errors or # Panics section when the caller must know.
  At most as many lines as parameters, plus three.
- // inside a function: only the non-obvious why, eight words at most.
- No history: no "this used to", no bug stories, no "copy of" notes. The
  code says what is; docs/design.md says why.

## AI-assisted contributions

Use whatever tools you like. The rule is the same either way: you are the
author, and you understand every line you submit. You have run it, read the
diff, and can explain why it is written that way. "The model wrote it" is not
an answer to a review question.

The repository is set up for coding agents: [AGENTS.md](AGENTS.md) gives them
the verification commands and the safety rules (never touch the real
configuration, never run pkexec or nixos-rebuild), and points them to the
right doc in [docs/](docs/README.md). CLAUDE.md loads the same file, and
.claude/skills holds reusable workflows such as adding an end-to-end step.

Large generated pull requests nobody has read get closed without a detailed
review. The size guide above applies however the code was written.

## Code

- Keep it simple. No new dependency for what a few lines can do.
- New dependencies must pass just deny (licenses compatible with
  GPL-3.0-or-later, no known advisory).
- No lint suppressions: fix what clippy reports.
- Write user facing strings in English and wrap them in t() (or n_() for
  static tables translated later). Use plural() for counts.
- Never commit secrets, tokens or generated files (target/, result).

## Documentation

docs/ explains the project to people working on it, README.md and
docs/user-guide.md to people using it. AGENTS.md only says how an agent
operates here; how a subsystem works belongs in docs/.

- A doc describes the project as it is. Plans, to-do lists, progress notes
  and changelogs do not go in the repository: plans live in issues, changes
  in the release notes.
- Explain why. The code already says what.
- First line is the title, then one or two sentences on what the doc
  answers. Add every new doc to docs/README.md.
- Kebab-case file names named after the topic: storage.md, not
  storage-guide.md.
- Cite source paths from the repository root: crates/core/src/config_attach.rs.
- English, short sentences, plain words, no em dashes, no backticks around
  names in prose.
- Update the doc in the same pull request as the behavior it describes.

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
