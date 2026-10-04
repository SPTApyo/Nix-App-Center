# Onyx

App store déclaratif pour Nix — *try-first* : lancer un paquet instantanément
(`nix run`), et ne basculer en installation permanente que délibérément
(édition d'un `store-packages.nix` généré, puis `home-manager switch`).

## Architecture

- `crates/core` — logique pure (search / run / keep), zéro dépendance UI.
- `crates/gui`  — façade GTK4 + libadwaita via relm4.
- (à venir) `crates/cli` — façade texte, banc d'essai de la boucle.

## Développement

```bash
nix develop            # rustc, cargo, gtk4, libadwaita, rust-analyzer
cargo run -p onyx-gui  # lance la fenêtre (affichée par WSLg)
```

## Installation

```bash
nix build .        # → ./result/bin/onyx (+ onyx-cli, .desktop, icône)
nix run .          # lance directement la fenêtre
nix run .#onyx-cli -- search firefox   # banc d'essai texte
```

## Licence

Copyright © 2026 SPTApyo.

Onyx est un logiciel libre sous **GNU GPL-3.0-or-later** — voir [LICENSE](LICENSE).
Vous pouvez l'utiliser, l'étudier, le modifier et le redistribuer ; tout dérivé
distribué doit rester libre sous la même licence, et conserver cet avis de copyright.
