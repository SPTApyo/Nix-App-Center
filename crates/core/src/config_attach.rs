//! Attachement : greffer les fichiers Onyx sur la config de l'utilisateur.
//!
//! C'est le seul endroit où Onyx touche à un fichier qu'il n'a pas écrit. Règle
//! absolue, et unique raison d'être de ce module :
//!
//! > **Aucune écriture sans plan affiché et validation explicite.**
//!
//! [`plan`] ne fait que *calculer* — il n'écrit rien. Il rend un [`AttachPlan`] qui
//! contient le diff exact, le chemin de la sauvegarde et le contenu résultant. C'est la
//! façade qui montre tout ça et demande l'accord. Seul [`commit`] écrit.
//!
//! La greffe est **une ligne** dans la liste `imports` de plus haut niveau. On ne parse
//! pas de Nix imbriqué et on ne reconstruit jamais un fichier depuis un AST : le
//! formatage, les commentaires et la logique de l'utilisateur survivent intacts.
//!
//! Si le fichier n'a pas de liste `imports` au plus haut niveau, Onyx **ne devine pas** :
//! il rend un plan « manuel » (le snippet à coller) et n'écrit rien.

use crate::config_layout::Layout;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachPlan {
    /// Déjà greffé : rien à faire.
    AlreadyAttached,
    /// Onyx sait patcher. Tout est calculé, rien n'est écrit.
    Patch {
        /// Le fichier de l'utilisateur à modifier.
        site: PathBuf,
        /// La sauvegarde qui sera écrite avant toute modification.
        backup: PathBuf,
        /// Les lignes d'import ajoutées (`./onyx-home.nix`…).
        added: Vec<String>,
        /// Diff unifié, à montrer tel quel à l'utilisateur.
        diff: String,
        /// Le contenu résultant, déjà calculé.
        patched: String,
    },
    /// Onyx ne trouve pas de liste `imports` au plus haut niveau : il refuse de deviner.
    /// À l'utilisateur de coller ces lignes où il veut.
    Manual { site: PathBuf, snippet: String },
    /// Disposition inconnue : il n'y a rien sur quoi se greffer.
    NoLayout,
}

/// Chemin d'import relatif au fichier greffé (`./onyx-home.nix`).
fn import_line(onyx_file: &Path) -> String {
    let name = onyx_file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("onyx.nix");
    format!("./{name}")
}

/// Les lignes d'import qu'exige cette disposition, sans doublon (les deux portées
/// partagent le même `imports` sur NixOS).
fn required_imports(layout: &Layout) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    for scope in [&layout.home, &layout.system].into_iter().flatten() {
        let entry = (scope.import_site.clone(), import_line(&scope.onyx_file));
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    out
}

/// Vrai si toutes les lignes d'import requises sont déjà là.
pub fn is_attached(layout: &Layout) -> bool {
    let needed = required_imports(layout);
    if needed.is_empty() {
        return false;
    }
    needed.iter().all(|(site, line)| {
        std::fs::read_to_string(site)
            .map(|src| src.contains(line.trim_start_matches("./")))
            .unwrap_or(false)
    })
}

/// Calcule le plan d'attachement. **N'écrit rien.**
pub fn plan(layout: &Layout) -> AttachPlan {
    if !layout.is_usable() {
        return AttachPlan::NoLayout;
    }
    if is_attached(layout) {
        return AttachPlan::AlreadyAttached;
    }

    let needed = required_imports(layout);
    let Some((site, _)) = needed.first().cloned() else {
        return AttachPlan::NoLayout;
    };

    // Sur NixOS les deux portées se greffent au même endroit : un seul fichier à patcher.
    let lines: Vec<String> = needed
        .iter()
        .filter(|(s, _)| *s == site)
        .map(|(_, l)| l.clone())
        .collect();

    let src = std::fs::read_to_string(&site).unwrap_or_default();
    let missing: Vec<String> = lines
        .iter()
        .filter(|l| !src.contains(l.trim_start_matches("./")))
        .cloned()
        .collect();

    if missing.is_empty() {
        return AttachPlan::AlreadyAttached;
    }

    match splice_imports(&src, &missing) {
        Some((patched, at)) => AttachPlan::Patch {
            backup: backup_path(&site),
            diff: unified_insert_diff(&site, &src, &missing, at),
            added: missing,
            site,
            patched,
        },
        None => AttachPlan::Manual {
            site,
            snippet: format!("imports = [\n{}\n];", missing.iter().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n")),
        },
    }
}

/// Écrit le plan : sauvegarde d'abord, patch atomique ensuite. À n'appeler qu'après
/// accord explicite de l'utilisateur.
pub fn commit(p: &AttachPlan) -> std::io::Result<()> {
    match p {
        AttachPlan::Patch { site, backup, patched, .. } => {
            // La sauvegarde précède toute modification : si l'écriture échoue à
            // mi-chemin, l'utilisateur a toujours son fichier d'origine.
            let original = std::fs::read(site)?;
            crate::paths::write_atomic(backup, &original)?;
            crate::paths::write_atomic(site, patched.as_bytes())
        }
        _ => Ok(()),
    }
}

/// Retire les lignes d'import d'Onyx. Les fichiers Onyx restent sur le disque, inertes.
pub fn detach(layout: &Layout) -> std::io::Result<()> {
    for (site, line) in required_imports(layout) {
        let Ok(src) = std::fs::read_to_string(&site) else { continue };
        let kept: Vec<&str> = src
            .lines()
            .filter(|l| l.trim() != line && l.trim() != format!("{line};"))
            .collect();
        let mut out = kept.join("\n");
        if src.ends_with('\n') {
            out.push('\n');
        }
        if out != src {
            crate::paths::write_atomic(&site, out.as_bytes())?;
        }
    }
    Ok(())
}

fn backup_path(site: &Path) -> PathBuf {
    let mut name = site.file_name().unwrap_or_default().to_os_string();
    name.push(".onyx-bak");
    site.with_file_name(name)
}

// ── Greffe textuelle ──────────────────────────────────────────────────────────

/// Insère `lines` juste avant le `]` de la liste `imports` de plus haut niveau.
/// Retourne le contenu patché et l'index de la ligne d'insertion.
///
/// `None` si aucune liste `imports` exploitable : Onyx préfère ne rien faire plutôt que
/// deviner un endroit où écrire dans le fichier de quelqu'un d'autre.
fn splice_imports(src: &str, lines: &[String]) -> Option<(String, usize)> {
    let open = find_imports_bracket(src)?;
    let close = matching_bracket(src, open)?;

    // Indentation : celle de la dernière entrée de la liste, sinon deux espaces.
    let inner = &src[open + 1..close];
    let pad = inner
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(4);
    let pad = " ".repeat(pad.max(2));

    let insert: String = lines.iter().map(|l| format!("{pad}{l}\n")).collect();

    // On insère au début de la ligne qui porte le `]`, pour ne pas casser sa mise en page.
    let line_start = src[..close].rfind('\n').map(|i| i + 1).unwrap_or(close);
    let mut patched = String::with_capacity(src.len() + insert.len());
    patched.push_str(&src[..line_start]);
    patched.push_str(&insert);
    patched.push_str(&src[line_start..]);

    let at = src[..line_start].lines().count();
    Some((patched, at))
}

/// Index du `[` de `imports = [`, hors commentaires.
fn find_imports_bracket(src: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(rel) = src[from..].find("imports") {
        let idx = from + rel;
        from = idx + "imports".len();

        if is_in_comment(src, idx) {
            continue;
        }
        // Un identifiant plus long qui finit par « imports » n'est pas notre liste.
        let prev_ok = src[..idx]
            .chars()
            .next_back()
            .map(|c| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_')
            .unwrap_or(true);
        if !prev_ok {
            continue;
        }

        let rest = &src[from..];
        let after_eq = rest.trim_start();
        if let Some(list) = after_eq.strip_prefix('=') {
            if let Some(bracket_rel) = list.find(|c: char| !c.is_whitespace()) {
                if list.as_bytes()[bracket_rel] == b'[' {
                    let offset = rest.len() - after_eq.len() + 1 + bracket_rel;
                    return Some(from + offset);
                }
            }
        }
    }
    None
}

fn is_in_comment(src: &str, idx: usize) -> bool {
    let line_start = src[..idx].rfind('\n').map(|i| i + 1).unwrap_or(0);
    src[line_start..idx].contains('#')
}

/// `]` correspondant au `[` d'`open`, en tenant compte de l'imbrication, des chaînes et
/// des commentaires.
fn matching_bracket(src: &str, open: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    let mut in_string = false;
    let mut in_comment = false;

    while i < bytes.len() {
        let c = bytes[i];
        if in_comment {
            if c == b'\n' {
                in_comment = false;
            }
        } else if in_string {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_string = false;
            }
        } else {
            match c {
                b'#' => in_comment = true,
                b'"' => in_string = true,
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Diff unifié d'une insertion pure — pas besoin d'un algorithme de diff : on sait
/// exactement où et quoi on insère.
fn unified_insert_diff(site: &Path, src: &str, added: &[String], at: usize) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let ctx = 3usize;
    let start = at.saturating_sub(ctx);
    let end = (at + ctx).min(lines.len());

    let mut out = format!("--- {}\n+++ {}\n", site.display(), site.display());
    out.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        start + 1,
        end - start,
        start + 1,
        end - start + added.len()
    ));
    for line in &lines[start..at] {
        out.push_str(&format!(" {line}\n"));
    }
    for a in added {
        out.push_str(&format!("+    {a}\n"));
    }
    for line in &lines[at..end] {
        out.push_str(&format!(" {line}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_layout::{LayoutKind, ScopeTarget};

    const CONFIG: &str = "{ config, pkgs, ... }:\n\
                          {\n  \
                          imports = [\n    \
                          ./hardware-configuration.nix\n  \
                          ];\n\n  \
                          services.foo.enable = true;\n\
                          }\n";

    fn fixture(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("onyx-attach-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("configuration.nix"), CONFIG).unwrap();
        dir
    }

    fn layout_for(dir: &Path) -> Layout {
        let site = dir.join("configuration.nix");
        Layout {
            kind: LayoutKind::NixosFlake { dir: dir.into(), host: "Test".into() },
            home: Some(ScopeTarget {
                onyx_file: dir.join("onyx-home.nix"),
                import_site: site.clone(),
            }),
            system: Some(ScopeTarget {
                onyx_file: dir.join("onyx-system.nix"),
                import_site: site,
            }),
            apply: vec!["sudo".into(), "nixos-rebuild".into(), "switch".into()],
            hm_user: Some("alice".into()),
        }
    }

    #[test]
    fn plan_computes_but_writes_nothing() {
        let dir = fixture("plan");
        let layout = layout_for(&dir);

        let p = plan(&layout);
        let AttachPlan::Patch { added, diff, patched, .. } = &p else {
            panic!("attendu un patch, obtenu {p:?}");
        };
        assert_eq!(added, &["./onyx-home.nix", "./onyx-system.nix"]);
        assert!(diff.contains("+    ./onyx-home.nix"));
        assert!(patched.contains("./hardware-configuration.nix"));

        // Le fichier sur disque est INTACT tant que commit() n'a pas été appelé.
        let on_disk = std::fs::read_to_string(dir.join("configuration.nix")).unwrap();
        assert_eq!(on_disk, CONFIG, "plan() ne doit rien écrire");
    }

    #[test]
    fn commit_patches_and_backs_up() {
        let dir = fixture("commit");
        let layout = layout_for(&dir);

        commit(&plan(&layout)).unwrap();

        let patched = std::fs::read_to_string(dir.join("configuration.nix")).unwrap();
        assert!(patched.contains("./onyx-home.nix"));
        assert!(patched.contains("./onyx-system.nix"));
        // Ce qui existait est toujours là, à l'identique.
        assert!(patched.contains("./hardware-configuration.nix"));
        assert!(patched.contains("services.foo.enable = true;"));

        let backup = std::fs::read_to_string(dir.join("configuration.nix.onyx-bak")).unwrap();
        assert_eq!(backup, CONFIG, "la sauvegarde doit précéder la modification");
    }

    #[test]
    fn attaching_twice_does_not_duplicate() {
        let dir = fixture("idempotent");
        let layout = layout_for(&dir);

        commit(&plan(&layout)).unwrap();
        assert!(is_attached(&layout));
        assert_eq!(plan(&layout), AttachPlan::AlreadyAttached);

        commit(&plan(&layout)).unwrap();
        let src = std::fs::read_to_string(dir.join("configuration.nix")).unwrap();
        assert_eq!(src.matches("./onyx-home.nix").count(), 1);
    }

    #[test]
    fn detach_restores_the_original_byte_for_byte() {
        let dir = fixture("detach");
        let layout = layout_for(&dir);

        commit(&plan(&layout)).unwrap();
        detach(&layout).unwrap();

        let src = std::fs::read_to_string(dir.join("configuration.nix")).unwrap();
        assert_eq!(src, CONFIG, "détacher doit rendre le fichier d'origine");
    }

    #[test]
    fn refuses_to_guess_when_no_imports_list() {
        let dir = fixture("no-imports");
        std::fs::write(
            dir.join("configuration.nix"),
            "{ pkgs, ... }:\n{\n  services.foo.enable = true;\n}\n",
        )
        .unwrap();
        let layout = layout_for(&dir);

        let p = plan(&layout);
        let AttachPlan::Manual { snippet, .. } = &p else {
            panic!("Onyx ne doit pas deviner où écrire ; obtenu {p:?}");
        };
        assert!(snippet.contains("./onyx-home.nix"));
    }

    #[test]
    fn ignores_imports_inside_a_comment() {
        let src = "{\n  # imports = [ ./vieux.nix ];\n  imports = [\n    ./vrai.nix\n  ];\n}\n";
        let (patched, _) = splice_imports(src, &["./onyx-home.nix".to_string()]).unwrap();
        assert!(patched.contains("./vrai.nix"));
        assert!(patched.contains("./onyx-home.nix"));
        // La ligne commentée reste commentée, et intacte.
        assert!(patched.contains("# imports = [ ./vieux.nix ];"));
    }

    #[test]
    fn matches_bracket_across_nesting() {
        let src = "{ imports = [ ./a.nix (lib.mkIf x [ ./b.nix ]) ]; }";
        let open = find_imports_bracket(src).unwrap();
        let close = matching_bracket(src, open).unwrap();
        assert_eq!(&src[close..close + 1], "]");
        // Le `]` retenu est celui de la liste, pas celui du sous-tableau.
        assert!(src[close..].starts_with("]; }"));
    }

    #[test]
    fn no_layout_yields_no_plan() {
        let layout = Layout {
            kind: LayoutKind::Unknown,
            home: None,
            system: None,
            apply: vec![],
            hm_user: None,
        };
        assert_eq!(plan(&layout), AttachPlan::NoLayout);
    }
}
