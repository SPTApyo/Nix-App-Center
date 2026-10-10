//! What is installed on the machine, from every source Nix Software reads:
//! Nix profiles, Flatpak and AppImages. One index answers "is this package
//! installed, where and how" for tiles, rows and app pages.

use std::collections::HashSet;

use crate::profiles::Scope;

/// Where an installed app comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    System,
    Home,
    Flatpak,
    AppImage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// Lowercase names it answers to: pname, desktop file name, app id.
    pub names: Vec<String>,
    pub version: String,
    pub origin: Origin,
    /// Set up through a module (`X.enable`).
    pub module: bool,
    /// What removing it needs: "installation:app id" for Flatpak, the
    /// desktop file for an AppImage, the pname otherwise.
    pub handle: String,
}

impl From<Scope> for Origin {
    fn from(scope: Scope) -> Self {
        match scope {
            Scope::System => Origin::System,
            Scope::User => Origin::Home,
        }
    }
}

/// Everything installed. Blocking: reads profiles, files and Flatpak.
pub fn installed_index() -> Vec<Installed> {
    let modules = crate::enabled::enabled_modules();
    let mut out: Vec<Installed> = Vec::new();
    for a in crate::profiles::profile_apps() {
        let stem = a
            .desktop_file
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        out.push(entry(
            vec![a.pname.to_lowercase(), stem],
            a.version,
            a.scope.into(),
            &modules,
        ));
    }
    for p in crate::profiles::profile_packages() {
        out.push(entry(
            vec![p.pname.to_lowercase()],
            p.version,
            p.scope.into(),
            &modules,
        ));
    }
    out.extend(crate::foreign::flatpaks());
    out.extend(crate::foreign::appimages());
    out
}

fn entry(
    names: Vec<String>,
    version: String,
    origin: Origin,
    modules: &HashSet<String>,
) -> Installed {
    let module = names.iter().any(|n| modules.contains(n));
    Installed {
        handle: names[0].clone(),
        names,
        version,
        origin,
        module,
    }
}

/// The entry for a package: by attribute name, or Flathub app id. Nested
/// attributes (haskellPackages.X) must match the version too.
pub fn find<'a>(
    index: &'a [Installed],
    attr: &str,
    version: &str,
    app_id: Option<&str>,
) -> Option<&'a Installed> {
    let pname = attr.rsplit('.').next().unwrap_or(attr).to_lowercase();
    let id = app_id.map(str::to_lowercase);
    let nested = attr.contains('.');
    index.iter().find(|e| {
        let by_name =
            e.names.contains(&pname) && (!nested || version.is_empty() || e.version == version);
        let by_id = id.as_ref().is_some_and(|id| e.names.contains(id));
        by_name || by_id
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(names: &[&str], version: &str, origin: Origin) -> Installed {
        Installed {
            names: names.iter().map(|n| n.to_string()).collect(),
            version: version.into(),
            origin,
            module: false,
            handle: String::new(),
        }
    }

    #[test]
    fn finds_by_name_id_and_version() {
        let index = vec![
            e(&["steam-unwrapped", "steam"], "1.0", Origin::System),
            e(&["obsidian"], "1.13", Origin::Home),
            e(&["org.videolan.vlc"], "3.0", Origin::Flatpak),
        ];
        assert_eq!(
            find(&index, "steam", "", None).unwrap().origin,
            Origin::System
        );
        assert!(find(&index, "haskellPackages.Obsidian", "0.1", None).is_none());
        assert!(find(&index, "obsidian", "9.9", None).is_some());
        assert_eq!(
            find(&index, "vlc", "", Some("org.videolan.VLC"))
                .unwrap()
                .origin,
            Origin::Flatpak
        );
        assert!(find(&index, "gimp", "", None).is_none());
    }
}
