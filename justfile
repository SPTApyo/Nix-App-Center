# Every command runs inside the dev shell: nix develop -c just <recipe>.

# List the recipes
default:
    @just --list

# Enable the commit message hook for this clone
setup:
    git config core.hooksPath .githooks

# Everything CI checks, in the same order
check: fmt-check lint test translations deny

# Run the app
run:
    cargo run -p nix-software-gui

test:
    cargo test --workspace

lint:
    cargo clippy --workspace --all-targets -- -D warnings

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

# Line coverage of the core, fails under the CI threshold
cov:
    cargo llvm-cov clean --workspace
    cargo llvm-cov -p nix-software-core --fail-under-lines 70

# HTML coverage report, to see which lines no test reaches
cov-html:
    cargo llvm-cov clean --workspace
    cargo llvm-cov -p nix-software-core --html --open

# Dependency licenses and security advisories
deny:
    cargo deny check

# Regenerate the translation template and catalogs
pot:
    ./po/update-pot.sh

# Fails when the translation files are out of date
translations: pot
    git diff --ignore-matching-lines='POT-Creation-Date' --exit-code po/
    for po in po/*.po; do msgfmt --check -o /dev/null "$po"; done

# End-to-end tests (needs network)
e2e *args:
    tests/e2e/run.sh {{args}}

build:
    nix build .
