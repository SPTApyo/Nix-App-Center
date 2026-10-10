//! Apps a configuration turns on through a module, read from its files:
//! `steam.enable` anywhere in its .nix files or those of its local inputs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config_layout::LayoutKind;

/// Lowercase names `X` of every `X.enable` not set to false. Blocking.
pub fn enabled_modules() -> HashSet<String> {
    let layout = crate::detect_layout();
    let dir = match &layout.kind {
        LayoutKind::NixosFlake { dir, .. }
        | LayoutKind::NixosLegacy { dir }
        | LayoutKind::HmFlake { dir, .. }
        | LayoutKind::HmStandalone { dir } => dir.clone(),
        LayoutKind::Unknown => return HashSet::new(),
    };
    let inputs: Vec<PathBuf> = crate::config_layout::local_overrides(&dir)
        .iter()
        .filter_map(|s| s.strip_prefix("git+file://").or(s.strip_prefix("path:")))
        .map(PathBuf::from)
        .collect();
    let mut out = HashSet::new();
    for d in std::iter::once(dir).chain(inputs) {
        scan(&d, &mut out, 0);
    }
    out
}

fn scan(dir: &Path, out: &mut HashSet<String>, depth: u8) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|e| e.path()) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "result" {
            continue;
        }
        if path.is_dir() && depth < 6 {
            scan(&path, out, depth + 1);
        } else if name.ends_with(".nix") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                names_in(&text, out);
            }
        }
    }
}

fn names_in(text: &str, out: &mut HashSet<String>) {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    for line in text.lines() {
        let code = line.split('#').next().unwrap_or("");
        let mut rest = code;
        while let Some(i) = rest.find(".enable") {
            let after = &rest[i + ".enable".len()..];
            let name: String = rest[..i]
                .chars()
                .rev()
                .take_while(|c| ident(*c))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let value = after.split(';').next().unwrap_or("");
            if !name.is_empty() && !after.starts_with(ident) && !value.contains("false") {
                out.insert(name.to_lowercase());
            }
            rest = after;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_enabled_modules_in_any_form() {
        let mut out = HashSet::new();
        names_in(
            "programs.steam.enable = true;\n\
             programs = {\n  gamemode.enable = config.my.gaming.enable;\n};\n\
             services.foo.enable = false;\n\
             # services.bar.enable = true;\n\
             x.enableFoo = true;\n",
            &mut out,
        );
        assert!(out.contains("steam"));
        assert!(out.contains("gamemode"));
        assert!(!out.contains("foo"));
        assert!(!out.contains("bar"));
        assert!(!out.contains("x"));
    }
}
