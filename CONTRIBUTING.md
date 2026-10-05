# Contributing

Thanks for helping. Issues and pull requests are welcome.

## Before opening a pull request

Run these inside nix develop:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

If you changed the UI, also run tests/e2e/run.sh and look at the
screenshots in target/e2e.

Keep commits small and use Conventional Commits (feat:, fix:, docs:, ...).

## Code

- Logic goes in crates/core and gets a unit test. The GUI stays thin.
- Keep it simple. No new dependency for what a few lines can do.
- Write user facing strings in English and wrap them in t() (or n_() for
  static tables translated later). Use plural() for counts.

## Translations

Translations live in po/. The template is po/onyx.pot.

Update the template and the catalogs after changing strings:

```bash
./po/update-pot.sh
```

Add a language:

1. Copy po/onyx.pot to po/xx.po and fill the header.
2. Add xx to po/LINGUAS.
3. Translate, then check with msgfmt --check po/xx.po.

To try a translation locally (cargo compiles po/ for you):

```bash
LANGUAGE=xx cargo run -p onyx-gui
```

## Matching Flathub apps to nixpkgs

Onyx guesses which nixpkgs attribute matches a Flathub app. When the guess is
wrong or missing, add an entry to crates/core/src/data/app_mapping.json
(nixpkgs attribute to AppStream id) and check the id against the Flathub
AppStream data.
