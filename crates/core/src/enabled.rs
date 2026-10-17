//! Apps a configuration turns on through a module, read from its files:
//! `steam.enable` anywhere in its .nix files or those of its local inputs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config_layout::LayoutKind;

/// The .nix files of the configuration folder and of its local inputs,
/// the two Nix App Center writes included. Blocking.
pub fn config_files() -> Vec<(PathBuf, String)> {
    let layout = crate::detect_layout();
    let dir = match &layout.kind {
        LayoutKind::NixosFlake { dir, .. }
        | LayoutKind::NixosLegacy { dir }
        | LayoutKind::HmFlake { dir, .. }
        | LayoutKind::HmStandalone { dir } => dir.clone(),
        LayoutKind::Unknown => return Vec::new(),
    };
    let inputs: Vec<PathBuf> = crate::config_layout::local_overrides(&dir)
        .iter()
        .filter_map(|s| s.strip_prefix("git+file://").or(s.strip_prefix("path:")))
        .map(PathBuf::from)
        .collect();
    let mut out = Vec::new();
    for d in std::iter::once(dir).chain(inputs) {
        scan(&d, &mut out, 0);
    }
    out
}

/// Lowercase names `X` of every `X.enable` not set to false. Blocking.
/// Read from the files, then confirmed by Nix once it evaluated them.
pub fn enabled_modules() -> HashSet<String> {
    let files = config_files();
    let mut guess = HashSet::new();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (path, text) in &files {
        names_in(text, &mut guess);
        std::hash::Hash::hash(&(path, text), &mut hasher);
    }
    let key = std::hash::Hasher::finish(&hasher);
    let cache = crate::cache::cache_dir().join("enabled-modules.json");
    let cached: Option<(u64, Vec<String>)> = std::fs::read(&cache)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    match cached {
        Some((k, confirmed)) if k == key => confirmed.into_iter().collect(),
        _ => {
            confirm_later(key, guess.clone(), cache);
            guess
        }
    }
}

/// Asks Nix which of `names` are enabled and caches the answer, once per
/// run, in the background. Evaluation failures keep the guess.
fn confirm_later(key: u64, names: HashSet<String>, cache: PathBuf) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let layout = crate::detect_layout();
    let LayoutKind::NixosFlake { dir, host } = layout.kind else {
        return;
    };
    std::thread::spawn(move || {
        let list = serde_json::to_string(&names.iter().collect::<Vec<_>>()).unwrap_or_default();
        let home = layout
            .hm_user
            .map(|u| format!("(c.home-manager.users.\"{u}\" or {{}})"))
            .unwrap_or_else(|| "{}".into());
        let apply = format!(
            "c: let h = {home}; on = s: n: (s.programs.${{n}}.enable or s.services.${{n}}.enable or s.virtualisation.${{n}}.enable or s.hardware.${{n}}.enable or false) == true; in builtins.filter (n: on c n || on h n) (builtins.fromJSON ''{list}'')"
        );
        let out = crate::util::nix()
            .args(["eval", "--json"])
            .arg(format!(
                "{}#nixosConfigurations.\"{host}\".config",
                dir.display()
            ))
            .args(["--apply", &apply])
            .args(crate::config_layout::local_overrides(&dir))
            .output();
        let Ok(out) = out else { return };
        if !out.status.success() {
            return;
        }
        if let Ok(confirmed) = serde_json::from_slice::<Vec<String>>(&out.stdout) {
            let _ = std::fs::write(
                cache,
                serde_json::to_vec(&(key, confirmed)).unwrap_or_default(),
            );
        }
    });
}

/// Where the user's own configuration lists `attr` as a package: file and
/// line, 1-based. Blocking.
pub fn declared_in(attr: &str) -> Option<(PathBuf, usize)> {
    config_files()
        .into_iter()
        .filter(|(path, _)| {
            !path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("nix-app-center-"))
        })
        .find_map(|(path, text)| package_line(&text, attr).map(|n| (path, n)))
}

/// The line listing `attr` inside the lists of a `packages = …;` or
/// `systemPackages = …;` definition, however they are combined.
fn package_line(text: &str, attr: &str) -> Option<usize> {
    let ident = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '\'');
    let matches = |w: &str| {
        let w = w.strip_prefix("pkgs.").unwrap_or(w);
        w == attr
            || w.split_once(".override")
                .is_some_and(|(name, _)| name == attr)
    };
    let (mut in_def, mut depth, mut after_with) = (false, 0usize, false);
    let mut prev = String::new();
    for (i, line) in text.lines().enumerate() {
        let code = line.split('#').next().unwrap_or("");
        let mut rest = code;
        if !in_def {
            let Some(at) = code.find("ackages") else {
                continue;
            };
            let Some(eq) = code[at..].find('=') else {
                continue;
            };
            in_def = true;
            rest = &code[at + eq + 1..];
        }
        let mut word = String::new();
        for c in rest.chars().chain(std::iter::once(' ')) {
            if ident(c) {
                word.push(c);
                continue;
            }
            if depth > 0 && matches(&word) {
                return Some(i + 1);
            }
            // `with pkgs;` does not end the definition.
            if !word.is_empty() {
                after_with = prev == "with";
                prev = word.clone();
            }
            word.clear();
            match c {
                '[' => depth += 1,
                ']' => depth = depth.saturating_sub(1),
                ';' if depth == 0 && !after_with => in_def = false,
                _ => {}
            }
        }
    }
    None
}

fn scan(dir: &Path, out: &mut Vec<(PathBuf, String)>, depth: u8) {
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
                out.push((path, text));
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

    #[test]
    fn finds_the_line_declaring_a_package() {
        let text = "{ pkgs, ... }:\n{\n  home.packages = with pkgs; [\n    vlc\n    gimp # editor\n  ];\n  programs.gearlever = 1;\n  environment.systemPackages = [ pkgs.steam ];\n}\n";
        assert_eq!(package_line(text, "gimp"), Some(5));
        assert_eq!(package_line(text, "steam"), Some(8));
        assert_eq!(package_line(text, "gearlever"), None);
        assert_eq!(package_line(text, "editor"), None);
        let combined = "  home.packages =\n    lib.optionals x (\n      with pkgs;\n      [\n        (bottles.override { a = true; })\n        cemu\n      ]\n    )\n    ++ [ pkgs.lmstudio ];\n  other = [ foo ];\n";
        assert_eq!(package_line(combined, "bottles"), Some(5));
        assert_eq!(package_line(combined, "lmstudio"), Some(9));
        assert_eq!(package_line(combined, "foo"), None);
    }
}
