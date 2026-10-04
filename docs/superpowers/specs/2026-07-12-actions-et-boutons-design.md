# Projet A — Actions & boutons

*2026-07-12*

## Problème

Deux surfaces exposent les actions d'un paquet, de deux façons différentes :

- **La ligne de recherche** (`PackageRow`) porte un `adw::SplitButton` « Lancer » +
  popover { Ajouter à ma config, Copier la commande }.
- **La page détail** porte quatre boutons distincts : « Lancer » (`suggested-action`),
  « Ajouter », une poubelle (visible si gardé), une icône « copier ».

C'est incohérent, et la ligne de liste mélange deux gestes : cliquer la ligne ouvre le
détail, cliquer le bouton agit — sur une ligne de 64 px de haut, la cible d'erreur est
grande.

Par ailleurs `nix run` est la seule façon d'essayer un paquet. Pour un outil CLI
(`ripgrep`, `jq`), `nix run` sans argument lance le binaire et rend la main : ça ne sert
à rien. Le geste *try-first* qu'Onyx revendique manque donc son public le plus naturel.

## Décisions

1. **La ligne de recherche perd tous ses boutons.** Elle devient purement
   navigationnelle : cliquer n'importe où ouvre le détail. Un seul geste, une seule
   cible.
2. **La page détail reçoit le split-button de la liste**, en remplacement de ses quatre
   boutons. Il devient le point unique où l'on agit sur un paquet.
3. **Nouvelle action `nix shell`** — ouvrir un terminal avec le paquet dans le `PATH`.

## Conception

### Liste — `crates/gui/src/components/package_row.rs`

Suppression de l'`adw::SplitButton` et de son popover. Les variantes `RowMsg::Run`,
`RowMsg::Keep` et `RowMsg::CopyCommand` disparaissent avec leurs bras d'`update()` ; il
ne reste que `Open` et `IconLoaded`. Le `GestureClick` déjà présent sur la racine suffit
à ouvrir le détail.

### Détail — `crates/gui/src/views/detail.rs`

Le contenu de `box_install_remove` est remplacé par un unique `adw::SplitButton` :

- **Primaire** : « Lancer », `suggested-action` → `DetailMsg::Run`.
- **Menu** (popover, boutons plats, comme la liste aujourd'hui) :
  - « Ajouter à ma config » si `!model.kept`, « Retirer de ma config » si `model.kept`
    — c'est ce qui remplace le bouton poubelle ; le libellé est en `#[watch]`.
  - « Essayer dans un shell » → `DetailMsg::Shell`.
  - « Copier la commande » → `DetailMsg::CopyCommand`.
  - séparateur.
  - « Appliquer la config » → `DetailMsg::ApplyConfig`.

`box_with_source` et le bouton d'origine (`nixpkgs`) ne changent pas.

**« Appliquer la config » était jusqu'ici atteignable uniquement via le bouton d'un toast
éphémère** affiché après un « Ajouter ». Si l'utilisateur ratait le toast, il n'avait plus
aucun moyen d'appliquer depuis la page. L'exposer dans le menu le rend rattrapable.

### Core — `crates/core/src/launch.rs`

```rust
pub fn shell_command(pkg: &Package) -> String   // "nix shell nixpkgs#<attr>"
pub fn shell(pkg: &Package) -> std::io::Result<ShellOutcome>

pub enum ShellOutcome {
    Spawned,      // un terminal a été lancé
    NoTerminal,   // aucun émulateur trouvé — la façade copie la commande
}
```

`shell()` cherche un émulateur de terminal, puis y lance
`nix shell nixpkgs#<attr> --extra-experimental-features "nix-command flakes"`.

La détection est isolée en **fonction pure** pour être testable sans dépendre des
binaires réellement installés sur la machine de test :

```rust
pub(crate) fn pick_terminal(exists: impl Fn(&str) -> bool) -> Option<Terminal>
```

Chaîne de recherche, dans l'ordre — chaque émulateur a sa propre syntaxe pour « exécute
cette commande » :

| binaire             | argument d'exécution |
| ------------------- | -------------------- |
| `xdg-terminal-exec` | *(commande directe)* |
| `ptyxis`            | `--`                 |
| `kgx`               | `--`                 |
| `gnome-terminal`    | `--`                 |
| `foot`              | *(commande directe)* |
| `alacritty`         | `-e`                 |
| `xterm`             | `-e`                 |

Si aucun n'existe, `shell()` retourne `NoTerminal` **sans rien exécuter**. C'est la façade
qui copie alors `shell_command()` dans le presse-papier et l'annonce dans un toast : le
core reste sans I/O presse-papier, conformément à sa règle actuelle.

### Core — `crates/core/src/package.rs`

`Action::Shell` rejoint l'enum et `actions_for()`, pour que le modèle d'actions reste la
description honnête de ce qu'Onyx sait faire.

### CLI — `crates/cli/src/main.rs`

`onyx-cli shell <attr>` : donne un moyen d'exercer le chemin réel sans passer par la GUI
(le screenshot ne fonctionne pas dans l'environnement de dev).

## Gestion d'erreur

| Situation                   | Comportement                                                    |
| --------------------------- | --------------------------------------------------------------- |
| Aucun terminal trouvé       | Commande copiée + toast « Aucun terminal trouvé — commande copiée » |
| Échec du `spawn`            | Toast d'échec, aucun état modifié                                |
| `home-manager` absent       | Message d'erreur existant (voir la dette ci-dessous)             |

## Dette connue, assumée

« Appliquer la config » appelle `apply_config()`, qui lance `home-manager switch`. Sur une
installation NixOS où home-manager est un *module NixOS* (le cas de la machine de dev), ce
binaire n'existe pas et l'action échoue avec un message clair. **Elle ne corrompt rien**,
mais l'entrée de menu ne devient honnête qu'une fois le Projet B (moteur de config) livré,
qui remplace les entrailles d'`apply_config()` par une commande déduite de la disposition
réelle. Cette dette est explicitement acceptée pour garder les deux projets séparables.

## Tests

Unitaires, purs, dans `core` :

- `shell_command()` rend bien `nix shell nixpkgs#<attr>`.
- `pick_terminal()` avec un prédicat `exists` simulé : respecte l'ordre de préférence,
  retourne `None` quand rien n'existe.
- `actions_for()` inclut `Action::Shell` dans les secondaires d'un paquet lançable.

Vérification de bout en bout : `onyx-cli shell ripgrep`, puis `cargo check -p onyx-gui`.
