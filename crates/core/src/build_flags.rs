//! Build options of a package: the boolean arguments of its recipe, such as
//! `withQt5 ? true` in vlc, switched through `pkgs.<attr>.override`. Read
//! from the recipe file itself (meta.position): Nix exposes the argument
//! names but not their defaults.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildFlag {
    pub name: String,
    pub default: bool,
}

/// Flags of the recipe at `position` (`/nix/store/…/package.nix:315`).
pub fn build_flags(position: &str) -> Vec<BuildFlag> {
    let file = position.rsplit_once(':').map_or(position, |(f, _)| f);
    std::fs::read_to_string(file)
        .map(|src| parse_flags(&src))
        .unwrap_or_default()
}

/// `name ? true|false` pairs of the argument set, before its closing `}:`.
fn parse_flags(src: &str) -> Vec<BuildFlag> {
    let header = src.split("}:").next().unwrap_or_default();
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut out: Vec<BuildFlag> = Vec::new();
    for (i, _) in header.match_indices('?') {
        let before = header[..i].trim_end();
        let name: String = before
            .chars()
            .rev()
            .take_while(|c| is_ident(*c))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let value: String = header[i + 1..]
            .trim_start()
            .chars()
            .take_while(|c| is_ident(*c))
            .collect();
        let default = match value.as_str() {
            "true" => true,
            "false" => false,
            _ => continue,
        };
        if !name.is_empty() && !out.iter().any(|f| f.name == name) {
            out.push(BuildFlag { name, default });
        }
    }
    out
}

/// The flags a saved block overrides, read back from
/// `pkgs.<attr>.override { withQt5 = false; }`.
pub fn overridden_flags(block: &str, attr: &str) -> Vec<(String, bool)> {
    let Some(rest) = block.split_once(&format!("pkgs.{attr}.override {{")) else {
        return Vec::new();
    };
    let body = rest.1.split('}').next().unwrap_or_default();
    body.split(';')
        .filter_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            let value = match value.trim() {
                "true" => true,
                "false" => false,
                _ => return None,
            };
            Some((name.trim().to_string(), value))
        })
        .collect()
}

/// `pkgs.<attr>.override { … }`, or `pkgs.<attr>` when nothing changes.
pub fn overridden_package(attr: &str, changes: &[(String, bool)]) -> String {
    if changes.is_empty() {
        return format!("pkgs.{attr}");
    }
    let pairs: Vec<String> = changes.iter().map(|(n, v)| format!("{n} = {v};")).collect();
    format!("(pkgs.{attr}.override {{ {} }})", pairs.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_boolean_arguments_only() {
        let src = "# vlc\n{\n  lib,\n  stdenv,\n  chromecastSupport ? true,\n  jackSupport ? false,\n  withGtk ? stdenv.isLinux,\n  extraFlags ? [ ],\n}:\nstdenv.mkDerivation { x = a ? b; }";
        let flags = parse_flags(src);
        assert_eq!(
            flags,
            [
                BuildFlag {
                    name: "chromecastSupport".into(),
                    default: true
                },
                BuildFlag {
                    name: "jackSupport".into(),
                    default: false
                },
            ]
        );
    }

    #[test]
    fn override_round_trips() {
        let changes = vec![
            ("withQt5".to_string(), false),
            ("jackSupport".to_string(), true),
        ];
        let pkg = overridden_package("vlc", &changes);
        assert_eq!(
            pkg,
            "(pkgs.vlc.override { withQt5 = false; jackSupport = true; })"
        );
        let block = format!("home.packages = [ {pkg} ];");
        assert_eq!(overridden_flags(&block, "vlc"), changes);
        assert_eq!(overridden_package("vlc", &[]), "pkgs.vlc");
        assert!(overridden_flags("home.packages = [ pkgs.vlc ];", "vlc").is_empty());
    }
}
