//! Connects the Nix App Center files to the user's configuration with one line
//! in its `imports` (or flake `modules`) list. [`plan`] computes the diff,
//! [`commit`] writes it after the user approves. Never guesses.

use crate::config_layout::Layout;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachPlan {
    AlreadyAttached,
    /// The patch, computed but not written.
    Patch {
        /// The user's file to modify.
        site: PathBuf,
        /// The backup written before any modification.
        backup: PathBuf,
        /// The import lines added.
        added: Vec<String>,
        /// Unified diff shown to the user.
        diff: String,
        /// The patched content.
        patched: String,
    },
    /// No top-level `imports` list: lines for the user to paste.
    Manual {
        site: PathBuf,
        snippet: String,
    },
    /// Unknown layout: there is nothing to attach to.
    NoLayout,
}

/// The import path relative to the importing file.
fn import_line(nix_app_center_file: &Path) -> String {
    let name = nix_app_center_file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("nix-app-center.nix");
    format!("./{name}")
}

/// The import lines the layout needs, without duplicates.
fn required_imports(layout: &Layout) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    for scope in [&layout.home, &layout.system].into_iter().flatten() {
        let entry = (
            scope.import_site.clone(),
            import_line(&scope.nix_app_center_file),
        );
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    out
}

/// True when every required import line is already there.
pub fn is_attached(layout: &Layout) -> bool {
    let needed = required_imports(layout);
    if needed.is_empty() {
        return false;
    }
    needed.iter().all(|(site, line)| {
        std::fs::read_to_string(site)
            .map(|src| imports(&src, line))
            .unwrap_or(false)
    })
}

/// True when `src` names `line`'s file outside a comment.
fn imports(src: &str, line: &str) -> bool {
    let name = line.trim_start_matches("./");
    src.lines()
        .any(|l| l.find(name).is_some_and(|i| !l[..i].contains('#')))
}

/// The attach plan. Writes nothing.
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

    let lines: Vec<String> = needed
        .iter()
        .filter(|(s, _)| *s == site)
        .map(|(_, l)| l.clone())
        .collect();

    let src = std::fs::read_to_string(&site).unwrap_or_default();
    let missing: Vec<String> = lines
        .iter()
        .filter(|l| !imports(&src, l))
        .cloned()
        .collect();

    if missing.is_empty() {
        return AttachPlan::AlreadyAttached;
    }

    match splice_imports(&src, &missing) {
        Some(patched) => AttachPlan::Patch {
            backup: backup_path(&site),
            diff: unified_diff(&site, &src, &patched),
            added: missing,
            site,
            patched,
        },
        None => AttachPlan::Manual {
            site,
            snippet: format!(
                "imports = [\n{}\n];",
                missing
                    .iter()
                    .map(|l| format!("  {l}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        },
    }
}

/// Writes the plan, backup first. Only after the user approves.
pub fn commit(p: &AttachPlan) -> std::io::Result<()> {
    match p {
        AttachPlan::Patch {
            site,
            backup,
            patched,
            ..
        } => {
            // Backup first: a failed write keeps the original.
            let original = std::fs::read(site)?;
            crate::paths::write_files(&[
                (backup.clone(), original),
                (site.clone(), patched.clone().into_bytes()),
            ])
        }
        _ => Ok(()),
    }
}

/// Removes the import lines; the generated files stay, inert.
pub fn detach(layout: &Layout) -> std::io::Result<()> {
    for (site, line) in required_imports(layout) {
        let Ok(src) = std::fs::read_to_string(&site) else {
            continue;
        };
        let kept: Vec<&str> = src
            .lines()
            .filter(|l| l.trim() != line && l.trim() != format!("{line};"))
            .collect();
        let mut out = kept.join("\n");
        if src.ends_with('\n') {
            out.push('\n');
        }
        if out != src {
            crate::paths::write_files(&[(site.clone(), out.into_bytes())])?;
        }
    }
    Ok(())
}

fn backup_path(site: &Path) -> PathBuf {
    let mut name = site.file_name().unwrap_or_default().to_os_string();
    name.push(".nix-app-center-bak");
    site.with_file_name(name)
}

/// `src` with `lines` inserted before the `]` of its `imports` (or flake
/// `modules`) list. `None` without one.
fn splice_imports(src: &str, lines: &[String]) -> Option<String> {
    let open = find_list_bracket(src, "imports").or_else(|| find_list_bracket(src, "modules"))?;
    let close = matching_bracket(src, open)?;

    // Indent like the last entry.
    let inner = &src[open + 1..close];
    let pad = inner
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .unwrap_or(4);
    let pad = " ".repeat(pad.max(2));

    let insert: String = lines.iter().map(|l| format!("{pad}{l}\n")).collect();

    let line_start = src[..close].rfind('\n').map_or(0, |i| i + 1);
    let mut patched = String::with_capacity(src.len() + insert.len() + 2);
    if line_start > open {
        patched.push_str(&src[..line_start]);
        patched.push_str(&insert);
        patched.push_str(&src[line_start..]);
    } else {
        // `[` and `]` on one line: break the list before `]`.
        let indent: String = src[line_start..]
            .chars()
            .take_while(|c| *c == ' ')
            .collect();
        patched.push_str(src[..close].trim_end());
        patched.push('\n');
        for l in lines {
            patched.push_str(&format!("{indent}  {l}\n"));
        }
        patched.push_str(&indent);
        patched.push_str(&src[close..]);
    }
    Some(patched)
}

/// Index of the `[` of `<key> = [`, outside comments.
fn find_list_bracket(src: &str, key: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(rel) = src[from..].find(key) {
        let idx = from + rel;
        from = idx + key.len();

        if is_in_comment(src, idx) {
            continue;
        }
        // Skip longer identifiers ending in the key.
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

/// The `]` matching the `[` at `open`, skipping strings and comments.
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

/// Unified diff of `src` to `patched`: one hunk around what changed.
fn unified_diff(site: &Path, src: &str, patched: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (src.lines().collect(), patched.lines().collect());
    let same = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[same..]
        .iter()
        .rev()
        .zip(b[same..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let ctx = 3usize;
    let start = same.saturating_sub(ctx);
    let (a_end, b_end) = (a.len() - tail, b.len() - tail);
    let after = tail.min(ctx);
    let mut out = format!("--- {}\n+++ {}\n", site.display(), site.display());
    out.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        start + 1,
        a_end + after - start,
        start + 1,
        b_end + after - start
    ));
    for line in &a[start..same] {
        out.push_str(&format!(" {line}\n"));
    }
    for line in &a[same..a_end] {
        out.push_str(&format!("-{line}\n"));
    }
    for line in &b[same..b_end] {
        out.push_str(&format!("+{line}\n"));
    }
    for line in &a[a_end..a_end + after] {
        out.push_str(&format!(" {line}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flake_without_configuration_nix_gets_a_modules_line() {
        let flake = "{\n  outputs = { c, ... }: {\n    nixosConfigurations.pc = c.extendModules {\n      modules = [\n        ./hardware-configuration.nix\n        ./custom.nix\n      ];\n    };\n  };\n}\n";
        let patched = splice_imports(flake, &["./nix-app-center-system.nix".to_string()]).unwrap();
        assert!(
            patched.contains("        ./custom.nix\n        ./nix-app-center-system.nix\n      ];"),
            "{patched}"
        );
    }
    use crate::config_layout::{LayoutKind, ScopeTarget};

    const CONFIG: &str = "{ config, pkgs, ... }:\n\
                          {\n  \
                          imports = [\n    \
                          ./hardware-configuration.nix\n  \
                          ];\n\n  \
                          services.foo.enable = true;\n\
                          }\n";

    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nix-app-center-attach-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("configuration.nix"), CONFIG).unwrap();
        dir
    }

    fn layout_for(dir: &Path) -> Layout {
        let site = dir.join("configuration.nix");
        Layout {
            kind: LayoutKind::NixosFlake {
                dir: dir.into(),
                host: "Test".into(),
            },
            home: Some(ScopeTarget {
                nix_app_center_file: dir.join("nix-app-center-home.nix"),
                import_site: site.clone(),
            }),
            system: Some(ScopeTarget {
                nix_app_center_file: dir.join("nix-app-center-system.nix"),
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
        let AttachPlan::Patch {
            added,
            diff,
            patched,
            ..
        } = &p
        else {
            panic!("attendu un patch, obtenu {p:?}");
        };
        assert_eq!(
            added,
            &["./nix-app-center-home.nix", "./nix-app-center-system.nix"]
        );
        assert!(diff.contains("+    ./nix-app-center-home.nix"));
        assert!(patched.contains("./hardware-configuration.nix"));

        // Nothing written before commit.
        let on_disk = std::fs::read_to_string(dir.join("configuration.nix")).unwrap();
        assert_eq!(on_disk, CONFIG, "plan() must write nothing");
    }

    #[test]
    fn commit_patches_and_backs_up() {
        let dir = fixture("commit");
        let layout = layout_for(&dir);

        commit(&plan(&layout)).unwrap();

        let patched = std::fs::read_to_string(dir.join("configuration.nix")).unwrap();
        assert!(patched.contains("./nix-app-center-home.nix"));
        assert!(patched.contains("./nix-app-center-system.nix"));
        // What existed is still there, unchanged.
        assert!(patched.contains("./hardware-configuration.nix"));
        assert!(patched.contains("services.foo.enable = true;"));

        let backup =
            std::fs::read_to_string(dir.join("configuration.nix.nix-app-center-bak")).unwrap();
        assert_eq!(backup, CONFIG, "the backup must come before the change");
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
        assert_eq!(src.matches("./nix-app-center-home.nix").count(), 1);
    }

    #[test]
    fn detach_restores_the_original_byte_for_byte() {
        let dir = fixture("detach");
        let layout = layout_for(&dir);

        commit(&plan(&layout)).unwrap();
        detach(&layout).unwrap();

        let src = std::fs::read_to_string(dir.join("configuration.nix")).unwrap();
        assert_eq!(src, CONFIG, "detaching must restore the original file");
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
            panic!("Nix App Center must not guess where to write; got {p:?}");
        };
        assert!(snippet.contains("./nix-app-center-home.nix"));
    }

    #[test]
    fn a_commented_import_does_not_count() {
        let line = "./nix-app-center-home.nix";
        assert!(imports("  imports = [ ./nix-app-center-home.nix ];", line));
        assert!(!imports("  # ./nix-app-center-home.nix\n", line));
        assert!(!imports("  ./a.nix # ./nix-app-center-home.nix\n", line));
    }

    #[test]
    fn a_one_line_list_is_broken_before_its_bracket() {
        let src = "{ ... }:\n{\n  imports = [ ./hardware-configuration.nix ];\n}\n";
        let patched = splice_imports(src, &["./nix-app-center-system.nix".to_string()]).unwrap();
        assert_eq!(
            patched,
            "{ ... }:\n{\n  imports = [ ./hardware-configuration.nix\n    ./nix-app-center-system.nix\n  ];\n}\n"
        );
        let diff = unified_diff(Path::new("c.nix"), src, &patched);
        assert!(diff.contains("-  imports = [ ./hardware-configuration.nix ];\n+  imports = [ ./hardware-configuration.nix\n+    ./nix-app-center-system.nix\n+  ];\n }"), "{diff}");
    }

    #[test]
    fn ignores_imports_inside_a_comment() {
        let src = "{\n  # imports = [ ./vieux.nix ];\n  imports = [\n    ./vrai.nix\n  ];\n}\n";
        let patched = splice_imports(src, &["./nix-app-center-home.nix".to_string()]).unwrap();
        assert!(patched.contains("./vrai.nix"));
        assert!(patched.contains("./nix-app-center-home.nix"));
        // The commented line stays commented, and untouched.
        assert!(patched.contains("# imports = [ ./vieux.nix ];"));
    }

    #[test]
    fn matches_bracket_across_nesting() {
        let src = "{ imports = [ ./a.nix (lib.mkIf x [ ./b.nix ]) ]; }";
        let open = find_list_bracket(src, "imports").unwrap();
        let close = matching_bracket(src, open).unwrap();
        assert_eq!(&src[close..close + 1], "]");
        // The list's `]`, not the inner one.
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
