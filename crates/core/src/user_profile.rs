//! `nix profile` for users without a managed configuration: Onyx then
//! installs for the user only, imperatively, like nix-software-center does.

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
    /// Home Manager's own element: removing it would wipe the whole setup.
    pub fn is_home_manager(&self) -> bool {
        self.name == "home-manager-path"
    }

    pub fn is_attr(&self, attr: &str) -> bool {
        self.attr_path
            .as_deref()
            .is_some_and(|p| p.ends_with(&format!(".{attr}")))
    }
}

/// Elements of the user profile, empty when `nix profile` is unusable.
pub fn profile_elements() -> Vec<ProfileElement> {
    let out = Command::new("nix")
        .args([
            "profile",
            "list",
            "--json",
            "--extra-experimental-features",
            "nix-command flakes",
        ])
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

/// Installs `attr` into the user profile. Downloads: call it from a thread.
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

/// Removes the element `name` from the user profile.
pub fn profile_remove(name: &str) -> std::io::Result<()> {
    run(Command::new("nix").args([
        "profile",
        "remove",
        name,
        "--extra-experimental-features",
        "nix-command flakes",
    ]))
}

fn run(cmd: &mut Command) -> std::io::Result<()> {
    let out = cmd.output()?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(std::io::Error::other(
        err.lines()
            .rev()
            .find(|l| l.trim_start().starts_with("error"))
            .unwrap_or("nix profile failed")
            .trim()
            .to_string(),
    ))
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
