#!/usr/bin/env bash
# Regenerates po/nix-software.pot from the sources and merges it into every po/<lang>.po.
# Run from the repository root inside `nix develop`.
set -euo pipefail
shopt -s nullglob
cd "$(dirname "$0")/.."
find crates/gui/src crates/core/src -name '*.rs' | sort > po/POTFILES
xgettext --language=Rust --from-code=UTF-8 \
  --keyword=t --keyword=n_ --keyword=pgettext:1c,2 --keyword=pgettext:1c,2 --keyword=ngettext:1,2 --keyword=plural:1,2 \
  --add-comments=TRANSLATORS --sort-by-file \
  --package-name=nix-software --package-version=0.1.0 \
  --msgid-bugs-address=https://github.com/SPTApyo/nix-software/issues \
  --files-from=po/POTFILES --output=po/nix-software.pot
rm po/POTFILES
if git diff --quiet --ignore-matching-lines='POT-Creation-Date' -- po/nix-software.pot; then
  git checkout -- po/nix-software.pot
fi
for po in po/*.po; do
  msgmerge --quiet --update --backup=none "$po" po/nix-software.pot
  msgattrib --no-obsolete --output-file="$po" "$po"
done
