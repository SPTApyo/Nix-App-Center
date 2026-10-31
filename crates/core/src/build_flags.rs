//! Boolean arguments of a package recipe (`withQt5 ? true`), read from the
//! recipe file since Nix does not expose their defaults.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildFlag {
    pub name: String,
    pub default: bool,
}

/// Flags of the recipe at `position` (`file:line`).
pub fn build_flags(position: &str) -> Vec<BuildFlag> {
    let file = position.rsplit_once(':').map_or(position, |(f, _)| f);
    std::fs::read_to_string(file)
        .map(|src| parse_flags(&src))
        .unwrap_or_default()
}

/// `name ? true|false` pairs before the argument set closes.
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

/// `(<base>.override { … })`, or `base` when nothing changes.
pub(crate) fn with_flags(base: &str, changes: &[(String, bool)]) -> String {
    if changes.is_empty() {
        return base.to_string();
    }
    let pairs: Vec<String> = changes.iter().map(|(n, v)| format!("{n} = {v};")).collect();
    format!("({base}.override {{ {} }})", pairs.join(" "))
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
    fn override_writes_the_changed_flags() {
        let changes = vec![
            ("withQt5".to_string(), false),
            ("jackSupport".to_string(), true),
        ];
        let pkg = with_flags("pkgs.vlc", &changes);
        assert_eq!(
            pkg,
            "(pkgs.vlc.override { withQt5 = false; jackSupport = true; })"
        );
        assert_eq!(with_flags("pkgs.vlc", &[]), "pkgs.vlc");
    }
}
