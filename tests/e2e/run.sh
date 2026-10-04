#!/usr/bin/env bash
# Runs the end-to-end scenario against a debug build.
#   tests/e2e/run.sh            run the scenario, screenshots in target/e2e
#   tests/e2e/run.sh --serve    keep an isolated app running for session.py
# Needs Nix with flakes; network access is used for nixpkgs and Flathub.
set -euo pipefail
cd "$(dirname "$0")/../.."
nix develop -c cargo build -p onyx-gui
PLAYWRIGHT_BROWSERS_PATH=$(nix build --no-link --print-out-paths nixpkgs#playwright-driver.browsers)
gtk=$(nix build --no-link --print-out-paths nixpkgs#gtk4)
export PLAYWRIGHT_BROWSERS_PATH PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=true
export PATH="$gtk/bin:$PATH"
exec nix develop -c nix shell --impure --expr \
  'with import (builtins.getFlake "nixpkgs") {}; python3.withPackages (p: [ p.playwright ])' \
  -c python3 "${E2E_SCRIPT:-tests/e2e/onyx_e2e.py}" "$@"
