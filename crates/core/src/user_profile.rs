//! `nix profile` installs, for users without a managed configuration.

use std::process::Command;

/// One installed element of `~/.nix-profile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileElement {
    pub name: String,
    /// `legacyPackages.<system>.<attr>`, when installed from a flake.
    pub attr_path: Option<String>,
    pub store_paths: Vec<String>,
}

impl ProfileElement {
    /// Home Manager's element: removing it wipes the setup.
    pub fn is_home_manager(&self) -> bool {
        self.name == "home-manager-path"
    }

    pub fn is_attr(&self, attr: &str) -> bool {
        self.attr_path
            .as_deref()
            .is_some_and(|p| p.ends_with(&format!(".{attr}")))
    }
}

/// Elements of the user profile; empty when unusable.
pub fn profile_elements() -> Vec<ProfileElement> {
    let out = crate::util::nix()
        .args(["profile", "list", "--json"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            parse_elements(&serde_json::from_slice(&o.stdout).unwrap_or_default())
        }
        _ => Vec::new(),
    }
}

fn parse_elements(v: &serde_json::Value) -> Vec<ProfileElement> {
    v["elements"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(name, e)| ProfileElement {
                    name: name.clone(),
                    attr_path: e["attrPath"].as_str().map(str::to_string),
                    store_paths: e["storePaths"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|p| p.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Installs `attr` in the user profile. Blocking.
pub fn profile_add(attr: &str) -> std::io::Result<()> {
    run(Command::new("env").args([
        "NIXPKGS_ALLOW_UNFREE=1",
        "nix",
        "profile",
        "add",
        "--impure",
        &format!("{}#{attr}", crate::nixpkgs_ref()),
        "--extra-experimental-features",
        "nix-command flakes",
    ]))
}

/// Removes element `name` from the user profile.
pub fn profile_remove(name: &str) -> std::io::Result<()> {
    run(crate::util::nix().args(["profile", "remove", name]))
}

/// pnames installed with nix-env, the older profile tool; empty when the
/// profile is managed by nix profile.
pub fn nix_env_names() -> Vec<String> {
    if !crate::util::home()
        .join(".nix-profile/manifest.nix")
        .exists()
    {
        return Vec::new();
    }
    Command::new("nix-env")
        .arg("--query")
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| crate::profiles::split_name(l.trim()).0)
                .filter(|n| !n.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Uninstalls `pname` from a nix-env profile.
pub fn nix_env_remove(pname: &str) -> std::io::Result<()> {
    run(Command::new("nix-env").args(["--uninstall", pname]))
}

fn run(cmd: &mut Command) -> std::io::Result<()> {
    let out = cmd.output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(crate::util::last_error_line(
            &out.stderr,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_profile_list() {
        let v = serde_json::json!({ "version": 3, "elements": {
            "hello": { "attrPath": "legacyPackages.x86_64-linux.hello", "storePaths": ["/nix/store/a-hello-2.12"] },
            "home-manager-path": { "attrPath": null, "storePaths": ["/nix/store/b-home-manager-path"] }
        }});
        let els = parse_elements(&v);
        let hello = els.iter().find(|e| e.name == "hello").unwrap();
        assert!(hello.is_attr("hello"));
        assert!(!hello.is_attr("ello"));
        assert!(els.iter().any(|e| e.is_home_manager()));
    }
}
