# Projet B — Moteur de config

*2026-07-12*

## Problème

Onyx écrit `~/.config/home-manager/onyx-packages.nix` et applique via
`home-manager switch`. Les deux hypothèses sont fausses dès qu'on quitte le cas
home-manager *standalone*.

Constat sur la machine de dev, qui est une installation NixOS parfaitement ordinaire :

- La config vit dans un **flake NixOS** (`/etc/nixos/flake.nix`,
  `nixosConfigurations.Zenbook`), avec home-manager branché comme **module NixOS**
  (`home-manager.nixosModules.home-manager` + `./home.nix`).
- `grep -rn onyx /etc/nixos` ne retourne **rien** : le fichier généré par Onyx est un
  **orphelin**. Rien ne l'importe. « Ajouter à ma config » n'a aucun effet réel.
- Le binaire `home-manager` **n'existe pas** sur la machine. `apply_config()` échoue.
  La vraie commande est `sudo nixos-rebuild switch --flake /etc/nixos#Zenbook`.

Autrement dit : le geste central du produit — « garder une app » — est **silencieusement
sans effet** sur la disposition la plus courante de NixOS.

Second manque, exprimé par l'utilisateur : une app gardée n'est qu'un nom dans une liste
`home.packages`. Il n'existe aucun chemin pour aller vers la vraie puissance de Nix —
configurer un module (`services.foo.port`), ouvrir un port, surcharger un paquet.
**« L'édition, c'est la clé. »**

## Principe directeur

> **Onyx n'écrase jamais rien.** Il possède ses propres fichiers. Sa seule intrusion dans
> la config de l'utilisateur est **une ligne d'import**, ajoutée **une fois**, **avec son
> accord explicite**, après affichage du diff, et **retirable**.

Corollaire, non négociable : **aucune écriture dans un fichier que l'utilisateur a écrit,
sans diff affiché et validation explicite.** Pas de magie silencieuse.

## Lot B1 — Détection de la disposition

Nouveau module `crates/core/src/config_layout.rs`.

```rust
pub enum LayoutKind {
    NixosFlake   { dir: PathBuf, host: String },  // /etc/nixos/flake.nix
    NixosLegacy  { dir: PathBuf },                // configuration.nix, sans flake
    HmFlake      { dir: PathBuf, user: String },
    HmStandalone { dir: PathBuf },                // ~/.config/home-manager/home.nix
    Unknown,                                      // → mode « snippet manuel »
}

pub struct Layout {
    pub kind: LayoutKind,
    pub home_target: Option<ScopeTarget>,    // où poser onyx-home.nix, et quel `imports` patcher
    pub system_target: Option<ScopeTarget>,  // idem, portée système
    pub apply: Vec<String>,                  // la commande d'apply, argv complet
    pub hm_user: Option<String>,             // l'utilisateur home-manager, si HM est un module NixOS
}
```

**Heuristiques**, dans l'ordre :

1. `$ONYX_CONFIG_DIR` s'il est posé (échappatoire, et point d'injection pour les tests).
2. `/etc/nixos/flake.nix` contenant `nixosConfigurations.<host>` → `NixosFlake`.
   `host` = le nom d'hôte de la machine s'il figure parmi les `nixosConfigurations`,
   sinon l'unique entrée s'il n'y en a qu'une, sinon → demander à l'utilisateur.
3. home-manager est considéré branché comme module NixOS si le flake mentionne
   `home-manager.nixosModules` ; `hm_user` est extrait de `home-manager.users.<user>`.
4. `/etc/nixos/configuration.nix` sans flake → `NixosLegacy`.
5. `~/.config/home-manager/{flake.nix,home.nix}` → `HmFlake` / `HmStandalone`.
6. Rien → `Unknown`.

**La commande d'apply découle de la disposition :**

| Disposition    | Commande                                                    |
| -------------- | ----------------------------------------------------------- |
| `NixosFlake`   | `sudo nixos-rebuild switch --flake <dir>#<host>`             |
| `NixosLegacy`  | `sudo nixos-rebuild switch`                                  |
| `HmFlake`      | `home-manager switch --flake <dir>#<user>`                   |
| `HmStandalone` | `home-manager switch`                                        |
| `Unknown`      | *(aucune — Onyx affiche le snippet et ne propose pas d'apply)* |

Le grep du flake est de la **détection**, pas du parsing de Nix : on cherche des marqueurs
textuels, jamais on ne réécrit à partir d'un AST. La détection peut se tromper — c'est
pourquoi elle passe par un écran de confirmation (B2) et n'est jamais appliquée en
silence.

`apply_config()` cesse de coder en dur `home-manager switch` et exécute `layout.apply`.
**C'est ce lot qui répare la dette du Projet A.**

## Lot B2 — Attachement consenti

### Le point de greffe

`/etc/nixos/configuration.nix` porte, au **niveau supérieur** :

```nix
  imports = [
    ./hardware-configuration.nix
  ];
```

C'est la splice la plus sûre possible : une ligne insérée avant le `]` d'une liste
top-level. **On ne parse pas de Nix imbriqué**, on n'entre pas dans
`home-manager.users.<user> = { imports = [ … ] }` — trop fragile.

Cela impose la forme des fichiers générés : `onyx-home.nix` doit être un module **NixOS**
(pas home-manager) quand HM est un module NixOS, et enrober son contenu :

```nix
# /etc/nixos/onyx-home.nix — généré par Onyx, ne pas éditer à la main
{ pkgs, ... }:
{
  home-manager.users.alice.home.packages = with pkgs; [ quickshare ];
}
```

En disposition `HmStandalone`, le même fichier est un module home-manager ordinaire
(`home.packages = …`) importé depuis `~/.config/home-manager/home.nix`. **La forme du
fichier est fonction de la disposition** ; c'est `config_gen` qui en décide.

### Le geste

`attach(layout) -> AttachPlan` produit un **plan**, il n'écrit rien. Le plan contient :
les fichiers à créer, le fichier à patcher, le diff unifié exact, le chemin du backup.

La façade affiche le plan et attend une validation explicite. `apply_plan(plan)` écrit
alors : backup (`<fichier>.onyx-bak`), puis patch atomique, puis fichiers Onyx.

`detach()` fait l'inverse : retire la ligne d'import, laisse les fichiers Onyx en place
(ils deviennent inertes).

Si la disposition est `Unknown`, aucun attachement n'est proposé : Onyx affiche le bloc
Nix et la ligne d'import à coller, et s'arrête là. Un app store qui ne sait pas où il est
ne doit pas deviner.

## Lot B3 — Manifeste enrichi, deux portées

`kept.json` porte aujourd'hui un `Vec<Package>`. Il devient :

```rust
pub struct KeptEntry {
    pub package: Package,
    pub home_nix: Option<String>,    // bloc Nix brut, portée utilisateur
    pub system_nix: Option<String>,  // bloc Nix brut, portée système
}
```

`home_nix: None` = app **simple** : elle est agrégée dans la liste `home.packages`, comme
aujourd'hui. Dès qu'un bloc existe, l'app est **promue** en région délimitée :

```nix
  # onyx:begin quickshare
  services.quickshare = { enable = true; port = 8080; };
  # onyx:end quickshare
```

Les marqueurs rendent chaque app individuellement identifiable, remplaçable et
supprimable dans le fichier généré — c'est ce qui rend l'édition réversible et le diff
lisible.

**Migration** : `kept.json` en ancien format (un `Vec<Package>` nu) est lu et converti en
`KeptEntry` sans bloc. Aucune perte, aucune action de l'utilisateur.

`config_gen` génère les **deux** fichiers à partir du manifeste : les blocs `system_nix`
vont dans `onyx-system.nix`, tout le reste dans `onyx-home.nix`. Un fichier sans contenu
est généré vide plutôt que supprimé, pour que la ligne d'import ne pointe jamais dans le
vide.

## Lot B4 — Éditeur Nix avancé

Un bouton « Options avancées » sur la page détail ouvre un dialogue d'édition du bloc de
l'app, avec un sélecteur de portée (utilisateur / système).

- **Parse instantané** à la frappe (debounce ~300 ms) : le bloc est enrobé en module
  (`{ pkgs, lib, config, ... }: { <bloc> }`) et passé à `nix-instantiate --parse`.
  ~50 ms, ne valide que la **syntaxe**. Erreur → soulignée, bouton « Valider » désactivé.
- **Bouton « Vérifier »**, optionnel et déclenché par l'utilisateur : lance un `dry-build`
  (`nixos-rebuild dry-build --flake <dir>#<host>`) dans un thread, avec barre de
  progression et sortie d'erreur. C'est ce qui attrape les erreurs *sémantiques* — option
  inconnue, type faux — que le parse ne voit pas. Il est **optionnel** par choix
  délibéré : il coûte de 30 s à 2 min, et l'imposer à chaque édition rendrait l'éditeur
  pénible.
- **Écriture** : diff affiché, backup, écriture atomique.
- **Application** : la commande de `layout.apply` est lancée dans un **terminal détecté**
  (la chaîne `pick_terminal` du Projet A) plutôt que via un `pkexec` opaque — l'utilisateur
  voit le build, peut l'interrompre, et saisit son mot de passe dans un contexte qu'il
  comprend.

Un `nixos-rebuild switch` qui échoue ne casse pas le système : NixOS conserve la
génération précédente. Le risque réel n'est pas le boot, c'est **l'attente** — d'où le
dry-build.

## Ordre d'implémentation

B1 → B2 → B3 → B4. B1 seul répare déjà la dette du Projet A (`apply_config` cassé). B3 est
prérequis de B4 (il n'y a rien à éditer tant que le manifeste ne sait pas porter un bloc).

## Tests

Tout ce qui décide se teste **sans toucher au vrai système** :

- `config_layout` : batterie de dispositions en fixtures sur disque temporaire
  (flake NixOS + HM module, flake NixOS nu, HM standalone, rien) → `LayoutKind` et
  `apply` attendus.
- `attach()` : produit le bon plan et le bon diff sur une fixture ; **n'écrit rien**.
- `apply_plan()` : sur fixture, patche la bonne ligne, crée le backup, est idempotent
  (deux attachements ne dupliquent pas l'import).
- `detach()` : restitue le fichier d'origine, octet pour octet.
- `config_gen` : un manifeste mixte (apps simples + blocs) rend les deux fichiers attendus,
  marqueurs compris ; le rendu est stable (trié) pour un diff propre.
- Migration : un `kept.json` ancien format est lu sans perte.

**Aucun test n'écrit dans `/etc/nixos` ni dans `~/.config`** : tout passe par
`$ONYX_CONFIG_DIR` et `$XDG_DATA_HOME` pointés sur un répertoire temporaire, comme le font
déjà les tests de `manifest.rs`.
