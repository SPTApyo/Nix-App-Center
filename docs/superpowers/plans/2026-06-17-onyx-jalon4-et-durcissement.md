# Onyx — Jalon 4 (cœur fonctionnel) + Durcissement UI — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Brancher la chaîne « Garder » de bout en bout (`keep` → `store-packages.nix` → `home-manager switch`), donner un onglet « Mises à jour » réel, ajouter la recherche par binaire (`nix-locate`), un cache de recherche, et vérifier/durcir l'UI existante.

**Architecture :** Tout le nouveau fonctionnel arrive en **nouveaux modules** de `crates/core` (`config_gen.rs`, `updates.rs`, `locate.rs`, `cache.rs`) — fichiers neufs = pas de collision entre agents. `lib.rs` ne reçoit que des lignes `mod`/`pub use`. L'UI (`crates/gui`) consomme ces coutures. Le core est testé en TDD (signal vert du ralph loop) ; l'UI est vérifiée à des checkpoints supervisés (rendu GTK + réseau Flathub non auto-vérifiables).

**Tech Stack :** Rust, `serde`/`serde_json`, `std::process::Command` (nix/home-manager), GTK4 + libadwaita + relm4 (gui). Build/test via `nix develop -c cargo …`.

**Contraintes machine (2026-06-17) :** `nix-locate` et `home-manager` **absents du PATH**. Donc `locate` et `apply_config` doivent **dégrader gracieusement** (détecter l'absence du binaire → message clair) ; leurs tests parsent des fixtures, ils ne lancent pas le vrai binaire. La génération de texte (`store-packages.nix`, parsing) est 100 % testable hors-ligne.

**Toujours** : exécuter cargo dans le devShell → `nix develop -c cargo test -p onyx-core`. Commits fréquents.

---

## File Structure

| Fichier | Responsabilité | Track |
|---|---|---|
| `crates/core/src/config_gen.rs` | **(neuf)** génère `store-packages.nix`, l'écrit, lance `home-manager switch` | A |
| `crates/core/src/updates.rs` | **(neuf)** compare versions kept vs nixpkgs courant → `Vec<Update>` | A |
| `crates/core/src/locate.rs` | **(neuf)** `nix-locate bin/<cmd>` → `Vec<Package>` (parse fixture) | A |
| `crates/core/src/cache.rs` | **(neuf)** cache disque TTL des résultats `nix search` | A |
| `crates/core/src/lib.rs` | ajoute `mod`/`pub use`, appelle `config_gen` depuis `keep`/`unkeep` | A |
| `crates/cli/src/main.rs` | sous-commandes `updates`, `locate`, `apply` (banc d'essai) | A |
| `crates/gui/src/views/updates.rs` | **(neuf)** onglet MAJ réel (remplace le placeholder) | B |
| `crates/gui/src/app.rs` | branche l'onglet MAJ, toasts feedback keep/erreurs | B |
| `crates/gui/src/views/detail.rs` | feedback keep (toast), audit rendu | B |
| `crates/gui/src/util.rs` | helper toast/erreur partagé | B |

---

# TRACK A — Cœur fonctionnel (core, TDD, ralph loop autonome)

## Task A1 : Câblage des modules (serial — fait EN PREMIER, avant tout parallélisme)

**Files:**
- Modify: `crates/core/src/lib.rs` (zone des `use`, en tête)

- [ ] **Step 1 : Déclarer les modules dans lib.rs**

Ajouter juste après les `use` (vers la ligne 14) :

```rust
mod cache;
mod config_gen;
mod locate;
mod updates;

pub use cache::{cached_search, clear_search_cache};
pub use config_gen::{apply_config, generate_store_packages, store_packages_path, sync_config};
pub use locate::locate_binary;
pub use updates::{available_updates, latest_version, version_newer, Update};
```

- [ ] **Step 2 : Créer 4 fichiers modules vides compilables**

Créer `crates/core/src/cache.rs`, `config_gen.rs`, `locate.rs`, `updates.rs`, chacun avec juste `// module` au départ — mais comme `lib.rs` les `pub use`, il faut des stubs. Pour ne pas casser le build, créer chaque fichier avec les signatures stub `unimplemented!()` (remplacées dans les tâches suivantes). Exemple `updates.rs` :

```rust
use crate::Package;

#[derive(Debug, Clone)]
pub struct Update {
    pub package: Package,
    pub installed_version: String,
    pub available_version: String,
}

pub fn version_newer(_candidate: &str, _current: &str) -> bool { unimplemented!() }
pub fn latest_version(_attr: &str) -> Option<String> { unimplemented!() }
pub fn available_updates() -> Vec<Update> { unimplemented!() }
```

(Idem stubs pour les signatures `pub use` des 3 autres fichiers.)

- [ ] **Step 3 : Vérifier que ça compile**

Run: `nix develop -c cargo build -p onyx-core`
Expected: compile (warnings `unused`/`unimplemented` OK).

- [ ] **Step 4 : Commit**

```bash
git add crates/core/src/{lib.rs,cache.rs,config_gen.rs,locate.rs,updates.rs}
git commit -m "feat(core): scaffold config_gen/updates/locate/cache modules"
```

> **Après A1, les tâches A2/A3/A4/A5 portent chacune sur UN fichier distinct → dispatch parallèle possible.**

---

## Task A2 : Génération `store-packages.nix` (config_gen.rs)

**Files:**
- Modify: `crates/core/src/config_gen.rs`
- Test: `crates/core/src/config_gen.rs` (module `#[cfg(test)]`)

- [ ] **Step 1 : Test de génération de texte**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Package;

    fn pkg(attr: &str) -> Package { Package::from_attr(attr) }

    #[test]
    fn generates_valid_nix_from_attrs() {
        let kept = vec![pkg("firefox"), pkg("haskellPackages.aeson")];
        let out = generate_store_packages(&kept);
        assert!(out.starts_with("{ pkgs, ... }:"));
        assert!(out.contains("home.packages = with pkgs; ["));
        assert!(out.contains("firefox"));
        assert!(out.contains("haskellPackages.aeson"));
        assert!(out.trim_end().ends_with("}"));
    }

    #[test]
    fn empty_manifest_is_valid() {
        let out = generate_store_packages(&[]);
        assert!(out.contains("home.packages = with pkgs; [ ];"));
    }

    #[test]
    fn is_deterministic_and_sorted() {
        let a = generate_store_packages(&[pkg("btop"), pkg("aria2")]);
        let b = generate_store_packages(&[pkg("aria2"), pkg("btop")]);
        assert_eq!(a, b, "ordre d'entrée ne doit pas changer la sortie");
    }
}
```

- [ ] **Step 2 : Lancer → échoue**

Run: `nix develop -c cargo test -p onyx-core config_gen`
Expected: FAIL (`unimplemented!`).

- [ ] **Step 3 : Implémenter generate_store_packages + chemin**

```rust
use crate::Package;
use std::path::PathBuf;

/// Emplacement du fichier généré. Surchargeable pour les tests via
/// `ONYX_STORE_PACKAGES_PATH`. Par défaut : ~/.config/home-manager/onyx-packages.nix
pub fn store_packages_path() -> PathBuf {
    if let Some(p) = std::env::var_os("ONYX_STORE_PACKAGES_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/home-manager/onyx-packages.nix")
}

/// Rend le fichier home-manager `{ pkgs, ... }: { home.packages = with pkgs; [ … ]; }`.
/// Trié pour un diff stable. NE touche jamais la config curée de l'utilisateur.
pub fn generate_store_packages(kept: &[Package]) -> String {
    let mut attrs: Vec<&str> = kept.iter().map(|p| p.attr.as_str()).collect();
    attrs.sort_unstable();
    attrs.dedup();
    if attrs.is_empty() {
        return "{ pkgs, ... }:\n{\n  home.packages = with pkgs; [ ];\n}\n".to_string();
    }
    let body = attrs
        .iter()
        .map(|a| format!("    {a}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{{ pkgs, ... }}:\n{{\n  home.packages = with pkgs; [\n{body}\n  ];\n}}\n")
}
```

- [ ] **Step 4 : Lancer → passe**

Run: `nix develop -c cargo test -p onyx-core config_gen`
Expected: PASS (3 tests).

- [ ] **Step 5 : Commit**

```bash
git add crates/core/src/config_gen.rs
git commit -m "feat(core): generate store-packages.nix from kept manifest"
```

---

## Task A3 : Écriture + `home-manager switch` (config_gen.rs, suite)

**Files:**
- Modify: `crates/core/src/config_gen.rs`
- Test: même module `#[cfg(test)]`

- [ ] **Step 1 : Test d'écriture (sync_config écrit le fichier au bon endroit)**

```rust
    #[test]
    fn sync_config_writes_file() {
        let dir = std::env::temp_dir().join(format!("onyx-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("onyx-packages.nix");
        std::env::set_var("ONYX_STORE_PACKAGES_PATH", &path);

        sync_config(&[Package::from_attr("firefox")]).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("firefox"));
        std::env::remove_var("ONYX_STORE_PACKAGES_PATH");
    }
```

- [ ] **Step 2 : Lancer → échoue (sync_config absent)**

Run: `nix develop -c cargo test -p onyx-core config_gen::tests::sync_config_writes_file`
Expected: FAIL.

- [ ] **Step 3 : Implémenter sync_config + apply_config (dégradation gracieuse)**

```rust
use std::process::Command;

/// Régénère et écrit `store-packages.nix` à partir du manifeste donné.
/// N'exécute PAS home-manager (séparé exprès : l'écriture est sûre et testable,
/// le switch est une opération système best-effort).
pub fn sync_config(kept: &[Package]) -> std::io::Result<()> {
    let path = store_packages_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, generate_store_packages(kept))
}

/// Lance `home-manager switch`. Dégrade gracieusement si le binaire est absent
/// (cas fréquent : NixOS sans home-manager standalone) → message clair, pas de panic.
pub fn apply_config() -> std::io::Result<String> {
    let out = match Command::new("home-manager").arg("switch").output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "home-manager introuvable. Installe home-manager (standalone) puis importe \
                 ~/.config/home-manager/onyx-packages.nix dans ton home.nix.",
            ));
        }
        Err(e) => return Err(e),
    };
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or("").trim().to_string())
    } else {
        Err(std::io::Error::other(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ))
    }
}
```

- [ ] **Step 4 : Lancer → passe**

Run: `nix develop -c cargo test -p onyx-core config_gen`
Expected: PASS (4 tests).

- [ ] **Step 5 : Commit**

```bash
git add crates/core/src/config_gen.rs
git commit -m "feat(core): write store-packages.nix and run home-manager switch (graceful)"
```

---

## Task A4 : Brancher keep/unkeep sur la génération (lib.rs)

**Files:**
- Modify: `crates/core/src/lib.rs:1136-1154` (`keep`/`unkeep`)

- [ ] **Step 1 : Test — keep régénère le fichier**

Ajouter dans `lib.rs` un `#[cfg(test)]` (s'il n'existe pas) :

```rust
#[cfg(test)]
mod keep_tests {
    use super::*;
    #[test]
    fn keep_then_unkeep_updates_store_file() {
        let base = std::env::temp_dir().join(format!("onyx-kt-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::env::set_var("XDG_DATA_HOME", &base);
        std::env::set_var("ONYX_STORE_PACKAGES_PATH", base.join("onyx-packages.nix"));

        keep(&Package::from_attr("ripgrep")).unwrap();
        let after_keep = std::fs::read_to_string(base.join("onyx-packages.nix")).unwrap();
        assert!(after_keep.contains("ripgrep"));

        unkeep("ripgrep").unwrap();
        let after_unkeep = std::fs::read_to_string(base.join("onyx-packages.nix")).unwrap();
        assert!(!after_unkeep.contains("ripgrep"));

        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("ONYX_STORE_PACKAGES_PATH");
    }
}
```

- [ ] **Step 2 : Lancer → échoue (keep ne génère pas encore)**

Run: `nix develop -c cargo test -p onyx-core keep_tests`
Expected: FAIL (fichier absent).

- [ ] **Step 3 : Appeler sync_config dans keep et unkeep**

Dans `keep()` après `save_manifest(&m)?;` et dans `unkeep()` après son `save_manifest(&m)?;`, ajouter la régénération depuis le manifeste à jour :

```rust
    // (dans keep, après save_manifest)
    config_gen::sync_config(&m.kept)?;
```
```rust
    // (dans unkeep, après save_manifest)
    config_gen::sync_config(&m.kept)?;
```

Note : ne PAS appeler `apply_config()` automatiquement ici (le switch reste un acte délibéré déclenché par l'UI/CLI). `sync_config` seul = idempotent et sûr.

- [ ] **Step 4 : Lancer → passe**

Run: `nix develop -c cargo test -p onyx-core keep_tests`
Expected: PASS.

- [ ] **Step 5 : Commit**

```bash
git add crates/core/src/lib.rs
git commit -m "feat(core): keep/unkeep regenerate store-packages.nix"
```

---

## Task A5 : Comparaison de versions + MAJ disponibles (updates.rs)

**Files:**
- Modify: `crates/core/src/updates.rs`
- Test: même module

- [ ] **Step 1 : Tests de comparaison de versions**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compares_numeric_segments() {
        assert!(version_newer("1.2.0", "1.1.9"));
        assert!(version_newer("2.0", "1.99"));
        assert!(!version_newer("1.0", "1.0"));
        assert!(!version_newer("1.0", "1.1"));
    }
    #[test]
    fn handles_unequal_length() {
        assert!(version_newer("1.2.1", "1.2"));
        assert!(!version_newer("1.2", "1.2.1"));
    }
}
```

- [ ] **Step 2 : Lancer → échoue**

Run: `nix develop -c cargo test -p onyx-core updates`
Expected: FAIL.

- [ ] **Step 3 : Implémenter version_newer**

```rust
/// Compare deux versions « façon Nix » par segments numériques (point-séparés).
/// Heuristique simple (pas de pré-release) suffisante pour signaler une MAJ.
pub fn version_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split(|c: char| c == '.' || c == '-')
            .map(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
            .map(|s| s.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(candidate), parse(current));
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y { return x > y; }
    }
    false
}
```

- [ ] **Step 4 : Lancer → passe** — `nix develop -c cargo test -p onyx-core updates` → PASS.

- [ ] **Step 5 : Implémenter latest_version + available_updates**

```rust
use crate::{load_manifest, Package};
use std::process::Command;

/// Version courante de l'attr dans nixpkgs (1 `nix search` ancré). `None` si absent.
pub fn latest_version(attr: &str) -> Option<String> {
    let pat = format!("^{}$", regex_escape(attr));
    let out = Command::new("nix")
        .args(["search", "nixpkgs", &pat, "--json"])
        .output().ok()?;
    if !out.status.success() { return None; }
    let map: std::collections::HashMap<String, serde_json::Value> =
        serde_json::from_slice(&out.stdout).ok()?;
    map.values().next()?.get("version")?.as_str().map(String::from)
}

fn regex_escape(s: &str) -> String {
    s.chars().flat_map(|c| {
        if ".+*?()|[]{}^$\\".contains(c) { vec!['\\', c] } else { vec![c] }
    }).collect()
}

/// Pour chaque paquet « gardé », compare sa version stockée à celle de nixpkgs.
/// Best-effort : ignore les paquets introuvables. (Peut être lent : N appels nix —
/// l'appelant GUI doit le lancer en thread.)
pub fn available_updates() -> Vec<Update> {
    load_manifest().kept.into_iter().filter_map(|p: Package| {
        let available = latest_version(&p.attr)?;
        if version_newer(&available, &p.version) {
            Some(Update { available_version: available, installed_version: p.version.clone(), package: p })
        } else { None }
    }).collect()
}
```

- [ ] **Step 6 : Vérifier compile + tests** — `nix develop -c cargo test -p onyx-core updates` → PASS.

- [ ] **Step 7 : Commit**

```bash
git add crates/core/src/updates.rs
git commit -m "feat(core): detect available updates by comparing kept vs nixpkgs versions"
```

---

## Task A6 : Recherche par binaire `nix-locate` (locate.rs)

**Files:**
- Modify: `crates/core/src/locate.rs`
- Test: même module (parse de fixture — N'EXÉCUTE pas nix-locate)

- [ ] **Step 1 : Test de parsing de la sortie nix-locate**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &str = "\
ripgrep.out                                       0 s /nix/store/xxx-ripgrep-14.1.0/bin/rg
fd.out                                            0 s /nix/store/yyy-fd-9.0.0/bin/fd
ripgrep.out                                       0 s /nix/store/xxx-ripgrep-14.1.0/share/man/man1/rg.1.gz";

    #[test]
    fn parses_attrs_dedup_bins_only() {
        let pkgs = parse_locate(SAMPLE, "rg");
        let attrs: Vec<_> = pkgs.iter().map(|p| p.attr.as_str()).collect();
        assert!(attrs.contains(&"ripgrep"));
        assert_eq!(attrs.iter().filter(|a| **a == "ripgrep").count(), 1, "dédup");
    }
}
```

- [ ] **Step 2 : Lancer → échoue** — `nix develop -c cargo test -p onyx-core locate` → FAIL.

- [ ] **Step 3 : Implémenter parse_locate + locate_binary**

```rust
use crate::Package;
use std::process::Command;

/// Parse la sortie de `nix-locate bin/<cmd>` : 1ʳᵉ colonne = `attr.output`, on garde
/// les lignes qui pointent vers un `/bin/`, on déduplique par attr.
fn parse_locate(stdout: &str, _cmd: &str) -> Vec<Package> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in stdout.lines() {
        if !line.contains("/bin/") { continue; }
        let Some(first) = line.split_whitespace().next() else { continue };
        let attr = first.rsplit_once('.').map(|(a, _)| a).unwrap_or(first);
        if seen.insert(attr.to_string()) {
            out.push(Package::from_attr(attr));
        }
    }
    out
}

/// Recherche les paquets fournissant le binaire `cmd` via `nix-locate bin/<cmd>`.
/// Dégrade gracieusement si nix-locate absent (→ vec vide ; l'UI affiche un état).
pub fn locate_binary(cmd: &str) -> Vec<Package> {
    let arg = format!("bin/{cmd}");
    match Command::new("nix-locate").args(["--top-level", "--whole-name", &arg]).output() {
        Ok(o) if o.status.success() => parse_locate(&String::from_utf8_lossy(&o.stdout), cmd),
        _ => Vec::new(),
    }
}
```

- [ ] **Step 4 : Lancer → passe** — `nix develop -c cargo test -p onyx-core locate` → PASS.

- [ ] **Step 5 : Commit**

```bash
git add crates/core/src/locate.rs
git commit -m "feat(core): nix-locate binary search (graceful when absent)"
```

---

## Task A7 : Cache disque des recherches (cache.rs)

**Files:**
- Modify: `crates/core/src/cache.rs`
- Test: même module

- [ ] **Step 1 : Tests cache (hit/expiry/clear)**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Package;
    fn iso() { std::env::set_var("XDG_CACHE_HOME",
        std::env::temp_dir().join(format!("onyx-cache-{}", std::process::id()))); }

    #[test]
    fn caches_and_serves() {
        iso();
        clear_search_cache();
        let mut calls = 0;
        let f = || { calls += 1; vec![Package::from_attr("firefox")] };
        let a = cached_search("ff", 60, f);
        // 2ᵉ appel : servi du cache, closure non rappelée
        let g = || vec![Package::from_attr("SHOULD_NOT_RUN")];
        let b = cached_search("ff", 60, g);
        assert_eq!(a.len(), b.len());
        assert_eq!(b[0].attr, "firefox");
    }

    #[test]
    fn expires() {
        iso();
        clear_search_cache();
        let _ = cached_search("xx", 0, || vec![Package::from_attr("a")]);
        let b = cached_search("xx", 0, || vec![Package::from_attr("b")]);
        assert_eq!(b[0].attr, "b", "ttl=0 → toujours recalculé");
    }
}
```

- [ ] **Step 2 : Lancer → échoue** — `nix develop -c cargo test -p onyx-core cache` → FAIL.

- [ ] **Step 3 : Implémenter le cache**

```rust
use crate::Package;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".cache")
        }).join("onyx")
}

fn key_path(key: &str) -> PathBuf {
    let safe: String = key.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    cache_dir().join(format!("search-{safe}.json"))
}

fn now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

#[derive(serde::Serialize, serde::Deserialize)]
struct Entry { at: u64, results: Vec<Package> }

/// Sert `key` depuis le cache si frais (< ttl_secs), sinon appelle `compute`,
/// stocke et renvoie. ttl_secs = 0 → toujours recalculer.
pub fn cached_search<F: FnOnce() -> Vec<Package>>(key: &str, ttl_secs: u64, compute: F) -> Vec<Package> {
    let path = key_path(key);
    if ttl_secs > 0 {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(e) = serde_json::from_slice::<Entry>(&bytes) {
                if now().saturating_sub(e.at) < ttl_secs { return e.results; }
            }
        }
    }
    let results = compute();
    let _ = std::fs::create_dir_all(cache_dir());
    if let Ok(json) = serde_json::to_vec(&Entry { at: now(), results: results.clone() }) {
        let _ = std::fs::write(&path, json);
    }
    results
}

/// Vide le cache de recherche (best-effort).
pub fn clear_search_cache() { let _ = std::fs::remove_dir_all(cache_dir()); }
```

- [ ] **Step 4 : Lancer → passe** — `nix develop -c cargo test -p onyx-core cache` → PASS.

- [ ] **Step 5 : Commit**

```bash
git add crates/core/src/cache.rs
git commit -m "feat(core): disk-backed search cache with TTL"
```

---

## Task A8 : Façade CLI (banc d'essai des nouvelles coutures)

**Files:**
- Modify: `crates/cli/src/main.rs`

- [ ] **Step 1 : Ajouter sous-commandes `updates`, `locate`, `apply`**

Brancher sur le `match` d'args existant (suivre le style des sous-commandes search/run/keep en place) :

```rust
        "updates" => {
            for u in onyx_core::available_updates() {
                println!("{}: {} → {}", u.package.attr, u.installed_version, u.available_version);
            }
        }
        "locate" => {
            let cmd = args.next().expect("usage: onyx-cli locate <bin>");
            for p in onyx_core::locate_binary(&cmd) { println!("{}", p.attr); }
        }
        "apply" => match onyx_core::apply_config() {
            Ok(msg) => println!("home-manager switch ok: {msg}"),
            Err(e) => eprintln!("échec: {e}"),
        },
```

- [ ] **Step 2 : Compiler** — `nix develop -c cargo build -p onyx-cli` → OK.

- [ ] **Step 3 : Smoke test (best-effort selon outils présents)**

Run: `nix develop -c cargo run -p onyx-cli -- apply`
Expected: message clair « home-manager introuvable… » (dégradation gracieuse OK).

- [ ] **Step 4 : Commit**

```bash
git add crates/cli/src/main.rs
git commit -m "feat(cli): updates/locate/apply subcommands"
```

---

# TRACK B — UI (supervisée, checkpoints rendu)

> **Track B ne s'auto-vérifie pas.** Chaque tâche finit par un checkpoint « lancer la GUI et regarder ». Le ralph loop peut écrire le code, mais l'orchestrateur (humain ou session principale) doit confirmer le rendu avant de cocher l'étape « vérifier ».

## Task B0 : Baseline visuelle (checkpoint humain)

- [ ] **Step 1 : Lancer la GUI**

Run: `nix develop -c cargo run -p onyx-gui`
Expected: la fenêtre s'affiche. Naviguer : Accueil, recherche, catégorie, **page détail** (le plus à risque), onglet Installées, MAJ (placeholder), Essayées.

- [ ] **Step 2 : Noter les écarts visuels** dans `docs/superpowers/plans/ui-baseline-2026-06-17.md` (captures ou description : détail/carousel, espacements, sections vides mal masquées, plantages réseau).

- [ ] **Step 3 : Commit des notes** — `git add docs && git commit -m "docs: UI visual baseline"`

---

## Task B1 : Onglet « Mises à jour » réel (views/updates.rs)

**Files:**
- Create: `crates/gui/src/views/updates.rs`
- Modify: `crates/gui/src/views/mod.rs` (ajouter `pub mod updates;`)
- Modify: `crates/gui/src/app.rs:440-459` (remplacer le placeholder « Tout est à jour » par le composant, en gardant l'illustration up-to-date quand la liste est vide)

- [ ] **Step 1 : Créer le composant `UpdatesModel`** (relm4 `#[relm4::component(pub)]`), en suivant le patron de `views/installed.rs` (le plus proche : liste chargée d'un appel core). Charge `onyx_core::available_updates()` **dans un thread** (ne pas bloquer l'UI), affiche un skeleton pendant (helpers `util::skeleton_list`), puis : liste de `PackageRow` si MAJ, sinon l'illustration `up-to-date.svg` + « Tout est à jour ». Bouton « Tout mettre à jour » → `onyx_core::apply_config()` en thread → toast.

- [ ] **Step 2 : Brancher dans app.rs** — remplacer le `updates_box` statique par `model.updates.widget()` (cf. convention mémoire : `set_content`/`append` après `view_output!`).

- [ ] **Step 3 : Compiler** — `nix develop -c cargo build -p onyx-gui` → OK.

- [ ] **Step 4 : CHECKPOINT rendu (humain)** — `nix develop -c cargo run -p onyx-gui`, ouvrir l'onglet MAJ : skeleton → soit liste, soit « Tout est à jour ». Garder un paquet avec une version ancienne pour tester la branche « liste ».

- [ ] **Step 5 : Commit** — `git add crates/gui/src/views/{updates.rs,mod.rs} crates/gui/src/app.rs && git commit -m "feat(gui): real updates tab backed by core::available_updates"`

---

## Task B2 : Feedback « Garder » + déclencheur switch (detail.rs / app.rs / util.rs)

**Files:**
- Modify: `crates/gui/src/util.rs` (helper toast)
- Modify: `crates/gui/src/views/detail.rs` (action Garder → toast)
- Modify: `crates/gui/src/app.rs` (AdwToastOverlay autour du contenu si absent)

- [ ] **Step 1 : Helper toast** dans `util.rs` :

```rust
pub fn toast(overlay: &adw::ToastOverlay, text: &str) {
    overlay.add_toast(adw::Toast::new(text));
}
```

- [ ] **Step 2 : ToastOverlay** — vérifier/ajouter un `adw::ToastOverlay` enveloppant le contenu principal dans `app.rs` ; exposer un canal pour que `detail` puisse y poster.

- [ ] **Step 3 : Brancher Garder** — quand l'utilisateur garde un paquet (action existante de `detail.rs`), après `core::keep`, afficher un toast : « Ajouté à ta config. Lance « Tout mettre à jour » ou `home-manager switch`. » et un bouton d'action de toast « Appliquer maintenant » → `apply_config()` en thread → toast résultat (succès ou message « home-manager introuvable »).

- [ ] **Step 4 : Compiler** — `nix develop -c cargo build -p onyx-gui` → OK.

- [ ] **Step 5 : CHECKPOINT rendu (humain)** — garder un paquet depuis la page détail → toast visible ; vérifier que `~/.config/home-manager/onyx-packages.nix` est bien (ré)écrit.

- [ ] **Step 6 : Commit** — `git add crates/gui/src/{util.rs,app.rs,views/detail.rs} && git commit -m "feat(gui): toast feedback on keep + apply trigger"`

---

## Task B3 : Gestion d'erreurs (réseau Flathub / nix absent / run échoué)

**Files:**
- Modify: `crates/gui/src/views/detail.rs`, `crates/gui/src/app.rs`, `crates/gui/src/components/app_list.rs`

- [ ] **Step 1 : Recherche sans résultat / nix qui échoue** → état vide explicite (pas un skeleton infini). Vérifier `app_list.rs` gère le cas `Vec` vide après chargement.

- [ ] **Step 2 : Réseau Flathub indisponible** → icônes génériques sans bloquer (déjà partiellement géré : `IconState`), s'assurer qu'un échec `fetch_bytes` retombe sur l'icône générique et ne laisse pas de shimmer infini.

- [ ] **Step 3 : `run` échoué** (nix absent / attr invalide) → toast d'erreur au lieu d'un échec silencieux. Le `run()` core renvoie `io::Result` ; remonter l'erreur jusqu'à un toast.

- [ ] **Step 4 : Compiler** — `nix develop -c cargo build -p onyx-gui` → OK.

- [ ] **Step 5 : CHECKPOINT rendu (humain)** — simuler hors-ligne (couper le réseau) → l'app reste utilisable, icônes génériques, pas de shimmer figé ; lancer un attr bidon → toast d'erreur.

- [ ] **Step 6 : Commit** — `git add -A crates/gui && git commit -m "feat(gui): graceful error states (network/nix/run failures)"`

---

## Task B4 : Recherche par binaire dans l'UI (nix-locate)

**Files:**
- Modify: `crates/gui/src/components/app_list.rs` ou `crates/gui/src/app.rs`

- [ ] **Step 1 : Quand la recherche texte ne donne rien**, tenter `onyx_core::locate_binary(query)` en thread et proposer une 3ᵉ section « Fournit la commande `<query>` » avec les paquets trouvés (si nix-locate absent → ne rien afficher, déjà géré côté core par vec vide).

- [ ] **Step 2 : Compiler** — `nix develop -c cargo build -p onyx-gui` → OK.

- [ ] **Step 3 : CHECKPOINT rendu (humain)** — (si nix-locate installé) rechercher `rg` → section binaire montre `ripgrep` ; sinon vérifier qu'aucune section vide n'apparaît.

- [ ] **Step 4 : Commit** — `git add -A crates/gui && git commit -m "feat(gui): binary search fallback via nix-locate"`

---

## Task B5 : Audit & corrections rendu page détail / carousel

**Files:**
- Modify: `crates/gui/src/views/detail.rs` (selon les écarts notés en B0)

- [ ] **Step 1 : Reprendre les écarts** listés dans `ui-baseline-2026-06-17.md` (context bar, carousel adaptatif, sections masquées, tuile licence).

- [ ] **Step 2 : Corriger** un écart à la fois, recompiler, regarder.

- [ ] **Step 3 : CHECKPOINT rendu (humain)** — comparer côte à côte avec `~/Videos/gnome copy/src/gs-details-page.ui` (référence). Confirmer fidélité raisonnable.

- [ ] **Step 4 : Commit** — `git add crates/gui/src/views/detail.rs && git commit -m "fix(gui): detail page visual fidelity corrections"`

---

## Self-Review (couverture spec)

- Chaîne « Garder » → store-packages.nix → switch : **A2, A3, A4, A8, B2** ✅
- Onglet MAJ réel : **A5, B1** ✅
- nix-locate : **A6, B4** ✅
- Latence 1er search : **A7** (cache ; câblage GUI optionnel — à brancher dans `core::search` si désiré, hors périmètre minimal) ✅
- Vérification + durcissement UI : **B0, B2, B3, B5** ✅
- Dégradation gracieuse (home-manager/nix-locate absents) : **A3, A6** ✅

## Notes de design (à ne pas reperdre)

- **`sync_config` ≠ `apply_config`** : écrire le `.nix` est sûr/idempotent (auto dans keep/unkeep) ; lancer `home-manager switch` est un acte délibéré (bouton UI / `onyx-cli apply`).
- **« Mises à jour » sous Nix** : il n'y a pas de notion apt-like ; on compare la version *gardée* (stockée dans le manifeste au moment du keep) à la version *courante* de nixpkgs. C'est une heuristique honnête, pas un gestionnaire de paquets impératif.
- **Cache de recherche** : `cached_search` est dispo mais le câblage dans `core::search` (clé = query, ttl ~ 1 j) est laissé optionnel pour ne pas changer le comportement de recherche pendant ce sprint.
