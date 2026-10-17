#!/usr/bin/env bash
# Regenerates po/nix-app-center.pot from the sources and merges it into every po/<lang>.po.
# Run from the repository root inside `nix develop`.
set -euo pipefail
shopt -s nullglob
cd "$(dirname "$0")/.."
find crates/gui/src crates/core/src -name '*.rs' | sort > po/POTFILES
xgettext --language=Rust --from-code=UTF-8 \
  --keyword=t --keyword=n_ --keyword=pgettext:1c,2 --keyword=pgettext:1c,2 --keyword=ngettext:1,2 --keyword=plural:1,2 \
  --add-comments=TRANSLATORS --sort-by-file \
  --package-name=nix-app-center --package-version=0.2.1 \
  --msgid-bugs-address=https://github.com/SPTApyo/nix-app-center/issues \
  --files-from=po/POTFILES --output=po/nix-app-center.pot
rm po/POTFILES
if git diff --quiet --ignore-matching-lines='POT-Creation-Date' -- po/nix-app-center.pot; then
  git checkout -- po/nix-app-center.pot
fi
for po in po/*.po; do
  msgmerge --quiet --update --backup=none "$po" po/nix-app-center.pot
  msgattrib --no-obsolete --output-file="$po" "$po"
done
