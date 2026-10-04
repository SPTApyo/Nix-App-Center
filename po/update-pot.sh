#!/usr/bin/env bash
# Regenerates po/onyx.pot from the sources and merges it into every po/<lang>.po.
# Run from the repository root inside `nix develop`.
set -euo pipefail
shopt -s nullglob
cd "$(dirname "$0")/.."
find crates/gui/src crates/core/src -name '*.rs' | sort > po/POTFILES
xgettext --language=Rust --from-code=UTF-8 \
  --keyword=t --keyword=n_ --keyword=ngettext:1,2 --keyword=plural:1,2 \
  --add-comments=TRANSLATORS --sort-by-file \
  --package-name=onyx --package-version=0.1.0 \
  --msgid-bugs-address=https://github.com/SPTApyo/Projet-Onyx/issues \
  --files-from=po/POTFILES --output=po/onyx.pot
rm po/POTFILES
for po in po/*.po; do
  msgmerge --quiet --update --backup=none "$po" po/onyx.pot
  msgattrib --no-obsolete --output-file="$po" "$po"
done
