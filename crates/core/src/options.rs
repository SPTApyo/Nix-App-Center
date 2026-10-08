//! NixOS option search, from the `options.json` the NixOS channel publishes
//! for the branch the config follows (the same data as search.nixos.org).
//! Downloaded once a week (about 1 MB, brotli), searched in memory.

use std::io::Read;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

const URL: &str = "https://channels.nixos.org/{branch}/options.json.br";
const TTL: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NixOption {
    pub name: String,
    pub description: String,
    pub type_: String,
    pub default: Option<String>,
    pub example: Option<String>,
    /// Module files declaring the option, relative to nixpkgs.
    pub declarations: Vec<String>,
}

impl NixOption {
    /// A line to paste in a configuration, from the example or the default.
    pub fn snippet(&self) -> String {
        let value = self
            .example
            .as_deref()
            .or(self.default.as_deref())
            .filter(|v| !v.contains('\n'))
            .unwrap_or("…");
        format!("{} = {value};", self.name)
    }
}

static OPTIONS: OnceLock<Vec<NixOption>> = OnceLock::new();

/// The options of the config's NixOS branch, loaded once. Blocking.
fn options() -> &'static [NixOption] {
    OPTIONS.get_or_init(|| {
        let branch = crate::current_source()
            .branch
            .filter(|b| b.starts_with("nixos-"))
            .unwrap_or_else(|| "nixos-unstable".into());
        load(&branch).unwrap_or_default()
    })
}

fn cache_path(branch: &str) -> PathBuf {
    crate::cache::cache_dir().join(format!("options-{branch}.json"))
}

fn load(branch: &str) -> Option<Vec<NixOption>> {
    let path = cache_path(branch);
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < TTL);
    if !fresh {
        if let Some(json) = download(branch) {
            let _ = crate::paths::write_atomic(&path, &json);
        }
    }
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some(parse(&v))
}

fn download(branch: &str) -> Option<Vec<u8>> {
    let resp = ureq::get(&URL.replace("{branch}", branch))
        .timeout(Duration::from_secs(60))
        .call()
        .ok()?;
    let mut json = Vec::new();
    brotli_decompressor::Decompressor::new(resp.into_reader(), 4096)
        .take(200 * 1024 * 1024)
        .read_to_end(&mut json)
        .ok()?;
    Some(json)
}

/// Reads `options.json`: `{ "<name>": { description, type, default, … } }`.
fn parse(v: &serde_json::Value) -> Vec<NixOption> {
    let text = |x: &serde_json::Value| -> Option<String> {
        x["text"]
            .as_str()
            .map(str::to_string)
            .or_else(|| x.as_str().map(str::to_string))
            .or_else(|| (!x.is_null()).then(|| x.to_string()))
    };
    let Some(map) = v.as_object() else {
        return Vec::new();
    };
    let mut out: Vec<NixOption> = map
        .iter()
        .map(|(name, o)| NixOption {
            name: name.clone(),
            description: o["description"].as_str().unwrap_or("").trim().to_string(),
            type_: o["type"].as_str().unwrap_or("").to_string(),
            default: o.get("default").and_then(text),
            example: o.get("example").and_then(text),
            declarations: o["declarations"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|d| d.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Options matching every word of `query`, best first. Blocking on the first
/// call (download and parse).
pub fn search_options(query: &str, limit: usize) -> Vec<NixOption> {
    rank(options(), query, limit)
}

/// The `enable` option of the NixOS module that sets up a package, such as
/// programs.steam.enable for steam. Blocking on the first call.
pub fn module_for(attr: &str) -> Option<NixOption> {
    module_in(options(), attr)
}

/// A module counts when its `package` option defaults to the package, or
/// when it is named after it. The shallowest one wins: programs.steam
/// over services.wivrn.steam.
fn module_in(all: &[NixOption], attr: &str) -> Option<NixOption> {
    let pname = attr.rsplit('.').next().unwrap_or(attr);
    let default = format!("pkgs.{pname}");
    let by_package = all
        .iter()
        .filter(|o| o.default.as_deref() == Some(default.as_str()))
        .filter_map(|o| o.name.strip_suffix(".package").map(str::to_string));
    let by_name = ["programs", "services", "virtualisation", "hardware"]
        .iter()
        .map(|root| format!("{root}.{pname}"));
    by_package
        .chain(by_name)
        .filter_map(|prefix| {
            let enable = format!("{prefix}.enable");
            let i = all
                .binary_search_by(|o| o.name.as_str().cmp(&enable))
                .ok()?;
            Some(&all[i])
        })
        .min_by_key(|o| o.name.matches('.').count())
        .cloned()
}

fn rank(all: &[NixOption], query: &str, limit: usize) -> Vec<NixOption> {
    let q = query.trim().to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(i32, &NixOption)> = all
        .iter()
        .filter_map(|o| {
            let name = o.name.to_lowercase();
            let desc = o.description.to_lowercase();
            if !words.iter().all(|w| name.contains(w) || desc.contains(w)) {
                return None;
            }
            let mut score = 0;
            if name == q {
                score += 1000;
            }
            score += 100 * words.iter().filter(|w| name.contains(*w)).count() as i32;
            if words
                .iter()
                .any(|w| name.ends_with(&format!(".{w}")) || name.split('.').any(|p| p == *w))
            {
                score += 50;
            }
            // The switch of a module is what people look for first.
            if name.ends_with(".enable") {
                score += 20;
            }
            score -= name.matches('.').count() as i32;
            Some((score, o))
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
    hits.into_iter()
        .take(limit)
        .map(|(_, o)| o.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<NixOption> {
        parse(&serde_json::json!({
            "services.openssh.enable": {
                "description": "Whether to enable the OpenSSH secure shell daemon.",
                "type": "boolean",
                "default": { "_type": "literalExpression", "text": "false" },
                "declarations": ["nixos/modules/services/networking/ssh/sshd.nix"]
            },
            "services.openssh.settings.PermitRootLogin": {
                "description": "Whether the root user can login using ssh.",
                "type": "string",
                "default": { "_type": "literalExpression", "text": "\"prohibit-password\"" },
                "example": { "_type": "literalExpression", "text": "\"no\"" }
            },
            "programs.ssh.startAgent": {
                "description": "Whether to start the OpenSSH agent.",
                "type": "boolean"
            }
        }))
    }

    fn modules() -> Vec<NixOption> {
        parse(&serde_json::json!({
            "programs.steam.enable": { "type": "boolean" },
            "programs.steam.package": {
                "default": { "_type": "literalExpression", "text": "pkgs.steam" }
            },
            "services.wivrn.steam.enable": { "type": "boolean" },
            "services.wivrn.steam.package": {
                "default": { "_type": "literalExpression", "text": "pkgs.steam" }
            },
            "virtualisation.virtualbox.host.enable": { "type": "boolean" },
            "virtualisation.virtualbox.host.package": {
                "default": { "_type": "literalExpression", "text": "pkgs.virtualbox" }
            },
            "programs.gamemode.enable": { "type": "boolean" },
            "programs.gamemode.settings": { "type": "attribute set" }
        }))
    }

    #[test]
    fn finds_the_module_of_a_package() {
        let all = modules();
        let name = |attr| module_in(&all, attr).map(|o| o.name);
        assert_eq!(name("steam").as_deref(), Some("programs.steam.enable"));
        assert_eq!(
            name("virtualbox").as_deref(),
            Some("virtualisation.virtualbox.host.enable"),
            "found through its package option"
        );
        assert_eq!(
            name("gamemode").as_deref(),
            Some("programs.gamemode.enable")
        );
        assert_eq!(name("vlc"), None);
    }

    #[test]
    fn ranks_name_matches_first() {
        let hits = rank(&sample(), "openssh", 10);
        assert_eq!(hits[0].name, "services.openssh.enable");
        assert_eq!(hits.len(), 3, "the agent matches by description");
        assert!(rank(&sample(), "openssh root", 10).len() == 1);
    }

    #[test]
    fn snippet_prefers_the_example() {
        let all = sample();
        let root = all
            .iter()
            .find(|o| o.name.ends_with("PermitRootLogin"))
            .unwrap();
        assert_eq!(
            root.snippet(),
            "services.openssh.settings.PermitRootLogin = \"no\";"
        );
        let ssh = all
            .iter()
            .find(|o| o.name == "services.openssh.enable")
            .unwrap();
        assert_eq!(ssh.snippet(), "services.openssh.enable = false;");
    }
}
