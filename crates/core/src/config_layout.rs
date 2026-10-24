//! Detects how the user's Nix configuration is laid out: where Nix App Center
//! writes its files, which file imports them and how to apply. Text scanning,
//! so the front end always shows the result before writing.

use std::path::{Path, PathBuf};

/// The shape of the configuration found on the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutKind {
    /// NixOS flake (`flake.nix` + `nixosConfigurations.<host>`).
    NixosFlake { dir: PathBuf, host: String },
    /// NixOS without flake (`configuration.nix` alone).
    NixosLegacy { dir: PathBuf },
    /// home-manager driven by a flake.
    HmFlake { dir: PathBuf, user: String },
    /// Standalone home-manager (`~/.config/home-manager/home.nix`).
    HmStandalone { dir: PathBuf },
    /// Nothing recognizable: Nix App Center shows a snippet instead of guessing.
    Unknown,
}

/// A scope Nix App Center writes to, and the file importing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeTarget {
    /// The generated file Nix App Center owns.
    pub nix_app_center_file: PathBuf,
    /// The user's file whose `imports` list gets one line.
    pub import_site: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub kind: LayoutKind,
    /// User scope, if any.
    pub home: Option<ScopeTarget>,
    /// System scope; `None` outside NixOS.
    pub system: Option<ScopeTarget>,
    /// The apply argv; empty when unknown.
    pub apply: Vec<String>,
    /// The Home Manager user when Home Manager is a NixOS module.
    pub hm_user: Option<String>,
}

impl Layout {
    /// True when Nix App Center knows where to write and how to apply.
    pub fn is_usable(&self) -> bool {
        !matches!(self.kind, LayoutKind::Unknown)
    }
}

const NIX_APP_CENTER_HOME_FILE: &str = "nix-app-center-home.nix";
const NIX_APP_CENTER_SYSTEM_FILE: &str = "nix-app-center-system.nix";

/// The NixOS configuration folder: `NIX_APP_CENTER_CONFIG_DIR`, the settings,
/// else `/etc/nixos`.
fn nixos_dir() -> PathBuf {
    std::env::var_os("NIX_APP_CENTER_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| crate::settings::load_settings().config_dir)
        .unwrap_or_else(|| PathBuf::from("/etc/nixos"))
}

fn hm_dir() -> PathBuf {
    let home = crate::util::home();
    home.join(".config/home-manager")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Detects the machine layout.
pub fn detect() -> Layout {
    let mut layout = detect_files();
    let custom = std::env::var_os("NIX_APP_CENTER_CONFIG_DIR")
        .is_none()
        .then(|| crate::settings::load_settings().apply_command)
        .flatten()
        .filter(|c| !c.trim().is_empty());
    if let (Some(command), true) = (custom, layout.is_usable()) {
        layout.apply = custom_apply(&command);
    }
    layout
}

/// argv running the user's own apply command; a leading `sudo` asks for
/// administrator rights the graphical way.
fn custom_apply(command: &str) -> Vec<String> {
    match command.trim().strip_prefix("sudo ") {
        Some(rest) => vec![
            "sudo".into(),
            "sh".into(),
            "-c".into(),
            rest.trim().to_string(),
        ],
        None => vec!["sh".into(), "-c".into(), command.trim().to_string()],
    }
}

/// `--override-input` for each local input of the flake in `dir`, so its
/// files are used as they are on disk instead of the locked copy.
pub(crate) fn local_overrides(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("flake.lock"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map(|lock| overrides_from_lock(&lock))
        .unwrap_or_default()
}

/// The folders of the flake in `dir` and of its local inputs, the flake first.
pub(crate) fn flake_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![dir.to_path_buf()];
    for source in local_overrides(dir).chunks(3).filter_map(|c| c.get(2)) {
        if let Some(local) = local_dir(dir, source).filter(|d| !out.contains(d)) {
            out.push(local);
        }
    }
    out
}

/// The folder of a local flake reference (`git+file:///x?ref=y`,
/// `path:./x`), relative ones against `dir`; `None` when it is no folder.
fn local_dir(dir: &Path, source: &str) -> Option<PathBuf> {
    let path = source
        .strip_prefix("git+file://")
        .or_else(|| source.strip_prefix("path:"))?;
    let path = path.split(['?', '#']).next().unwrap_or(path);
    std::fs::canonicalize(dir.join(path))
        .ok()
        .filter(|p| p.is_dir())
}

/// The flakes the user added in Preferences, locked when the list changes,
/// as (locked reference, locked reference): they are no input of the
/// configuration, so the reference is their name.
fn extra_sources() -> Vec<(String, String)> {
    type Locked = (Vec<String>, Vec<(String, String)>);
    static LOCKED: std::sync::Mutex<Option<Locked>> = std::sync::Mutex::new(None);
    let wanted = crate::settings::load_settings().extra_flakes;
    let mut cache = LOCKED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, locked)) = cache.as_ref().filter(|(list, _)| *list == wanted) {
        return locked.clone();
    }
    let locked: Vec<(String, String)> = wanted
        .iter()
        .filter_map(|flake| lock_flake(flake))
        .map(|r| (r.clone(), r))
        .collect();
    *cache = Some((wanted, locked.clone()));
    locked
}

/// The GitHub flakes search.nixos.org lists, as `github:owner/repo`. Read
/// from its repository, cached a week; empty offline. Blocking.
pub fn suggested_flakes() -> Vec<String> {
    const URL: &str =
        "https://raw.githubusercontent.com/NixOS/nixos-search/main/flakes/manual.toml";
    let path = crate::cache::cache_dir().join("suggested-flakes.toml");
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < 7 * 24 * 3600);
    if !fresh {
        let mut text = String::new();
        let got = crate::http::get(URL, std::time::Duration::from_secs(15))
            .ok()
            .and_then(|mut r| std::io::Read::read_to_string(&mut r, &mut text).ok());
        if got.is_some() {
            let _ = crate::paths::write_atomic(&path, text.as_bytes());
        }
    }
    parse_suggested(&std::fs::read_to_string(&path).unwrap_or_default())
}

/// `github:owner/repo` of each GitHub source of nixos-search's flake list.
fn parse_suggested(toml: &str) -> Vec<String> {
    let value = |line: &str, key: &str| {
        line.trim()
            .strip_prefix(key)?
            .trim()
            .strip_prefix('=')?
            .trim()
            .trim_matches('"')
            .to_string()
            .into()
    };
    let mut out = Vec::new();
    for block in toml.split("[[sources]]").skip(1) {
        let field = |key: &str| block.lines().find_map(|l| value(l, key));
        if field("type").as_deref() != Some("github") {
            continue;
        }
        if let (Some(owner), Some(repo)) = (field("owner"), field("repo")) {
            out.push(format!("github:{owner}/{repo}"));
        }
    }
    out
}

/// `github:owner/repo` with its current revision; `None` for other forms.
fn lock_flake(flake: &str) -> Option<String> {
    let rest = flake.trim().strip_prefix("github:")?;
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    let safe = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    if !safe(owner) || !safe(repo) {
        return None;
    }
    let out = crate::util::nix()
        .args([
            "flake",
            "metadata",
            "--json",
            &format!("github:{owner}/{repo}"),
        ])
        .output()
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let rev = v["revision"]
        .as_str()
        .filter(|r| crate::versions::is_rev(r))?;
    Some(format!("github:{owner}/{repo}/{rev}"))
}

/// Flake inputs of the configuration and of its local inputs that may hold
/// packages, as (name, locked reference), then the flakes added in
/// Preferences. nixpkgs and plumbing are left out.
pub fn flake_sources() -> Vec<(String, String)> {
    let mut out = config_sources();
    out.extend(extra_sources());
    out
}

fn config_sources() -> Vec<(String, String)> {
    let dir = match detect().kind {
        LayoutKind::NixosFlake { dir, .. } | LayoutKind::HmFlake { dir, .. } => dir,
        _ => return Vec::new(),
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for d in flake_dirs(&dir) {
        let Some(lock) = std::fs::read_to_string(d.join("flake.lock"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        else {
            continue;
        };
        for (name, source) in sources_from_lock(&lock) {
            if !out.iter().any(|(n, _)| *n == name) {
                out.push((name, source));
            }
        }
    }
    out
}

fn sources_from_lock(lock: &serde_json::Value) -> Vec<(String, String)> {
    const PLUMBING: [&str; 8] = [
        "nixpkgs",
        "home-manager",
        "flake-utils",
        "flake-parts",
        "systems",
        "flake-compat",
        "nix-darwin",
        "treefmt-nix",
    ];
    let root = lock["root"].as_str().unwrap_or("root");
    let Some(inputs) = lock["nodes"][root]["inputs"].as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, node) in inputs {
        let Some(node) = node.as_str() else { continue };
        let locked = &lock["nodes"][node]["locked"];
        let (Some(kind @ ("github" | "gitlab" | "sourcehut")), Some(owner), Some(repo), Some(rev)) = (
            locked["type"].as_str(),
            locked["owner"].as_str(),
            locked["repo"].as_str(),
            locked["rev"].as_str(),
        ) else {
            continue;
        };
        if PLUMBING.iter().any(|p| repo == *p || name.starts_with(p)) {
            continue;
        }
        let safe = |s: &str| {
            s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        };
        let owner_ok = safe(owner.strip_prefix('~').unwrap_or(owner));
        if safe(name) && owner_ok && safe(repo) && crate::versions::is_rev(rev) {
            out.push((name.clone(), format!("{kind}:{owner}/{repo}/{rev}")));
        }
    }
    out
}

fn overrides_from_lock(lock: &serde_json::Value) -> Vec<String> {
    let root = lock["root"].as_str().unwrap_or("root");
    let Some(inputs) = lock["nodes"][root]["inputs"].as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, node) in inputs {
        let Some(node) = node.as_str() else { continue };
        let original = &lock["nodes"][node]["original"];
        let source = match original["type"].as_str() {
            Some("git") => original["url"]
                .as_str()
                .filter(|u| u.starts_with("file://"))
                .map(|u| format!("git+{u}")),
            Some("path") => original["path"].as_str().map(|p| format!("path:{p}")),
            _ => None,
        };
        if let Some(source) = source {
            out.extend(["--override-input".to_string(), name.clone(), source]);
        }
    }
    out
}

fn detect_files() -> Layout {
    let nixos = nixos_dir();
    let hm = hm_dir();

    let flake = nixos.join("flake.nix");
    if flake.is_file() {
        let src = read(&flake);
        // The environment variable beats the settings.
        let chosen = std::env::var_os("NIX_APP_CENTER_CONFIG_DIR")
            .is_none()
            .then(|| crate::settings::load_settings().host)
            .flatten();
        if let Some(host) = chosen.or_else(|| first_nixos_host(&src)) {
            let hm_user = hm_user_in_dir(&nixos);
            return nixos_flake_layout(nixos, host, hm_user);
        }
    }

    if nixos.join("configuration.nix").is_file() {
        let hm_user = hm_user_in_dir(&nixos);
        return nixos_legacy_layout(nixos, hm_user);
    }

    if hm.join("flake.nix").is_file() {
        let user = current_user();
        return hm_flake_layout(hm, user);
    }

    if hm.join("home.nix").is_file() {
        return hm_standalone_layout(hm);
    }

    Layout {
        kind: LayoutKind::Unknown,
        home: None,
        system: None,
        apply: Vec::new(),
        hm_user: None,
    }
}

/// What Nix reports about a flake layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// Configurations the flake really outputs.
    pub hosts: Vec<String>,
    /// Home Manager users of the host, or why the host did not evaluate.
    pub hm_users: Result<Vec<String>, String>,
}

/// Checks the scan with Nix by evaluating the flake. Blocking.
///
/// # Errors
/// The last line Nix printed.
pub fn verify(layout: &Layout) -> Result<Verified, String> {
    let overrides = match &layout.kind {
        LayoutKind::NixosFlake { dir, .. } | LayoutKind::HmFlake { dir, .. } => {
            local_overrides(dir)
        }
        _ => Vec::new(),
    };
    let names = |expr: String| -> Result<Vec<String>, String> {
        let out = crate::util::nix()
            .args(["eval", "--json", &expr, "--apply", "builtins.attrNames"])
            .args(&overrides)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(crate::util::last_error_line(&out.stderr));
        }
        serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
    };
    match &layout.kind {
        LayoutKind::NixosFlake { dir, host } => {
            let flake = dir.display();
            let hosts = names(format!("{flake}#nixosConfigurations"))?;
            let hm_users = names(format!(
                "{flake}#nixosConfigurations.\"{host}\".config.home-manager.users"
            ))
            .or_else(|e| {
                if e.contains("'home-manager' missing") {
                    Ok(Vec::new())
                } else {
                    Err(e)
                }
            });
            Ok(Verified { hosts, hm_users })
        }
        LayoutKind::HmFlake { dir, .. } => Ok(Verified {
            hosts: names(format!("{}#homeConfigurations", dir.display()))?,
            hm_users: Ok(Vec::new()),
        }),
        _ => Ok(Verified {
            hosts: Vec::new(),
            hm_users: Ok(Vec::new()),
        }),
    }
}

/// The host to use from a flake: this machine's hostname when declared,
/// else the first `nixosConfigurations` entry.
pub(crate) fn first_nixos_host(flake_src: &str) -> Option<String> {
    pick_host(nixos_hosts(flake_src), hostname().as_deref())
}

/// The host named like this machine, ignoring case, else the first one.
fn pick_host(hosts: Vec<String>, this: Option<&str>) -> Option<String> {
    let same = |h: &&String| this.is_some_and(|t| h.eq_ignore_ascii_case(t));
    hosts
        .iter()
        .find(same)
        .cloned()
        .or_else(|| hosts.into_iter().next())
}

/// Every host declared in `nixosConfigurations`.
fn nixos_hosts(flake_src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, _) in flake_src.match_indices("nixosConfigurations") {
        let rest = &flake_src[idx + "nixosConfigurations".len()..];
        // `nixosConfigurations.laptop = …`
        if let Some(after_dot) = rest.strip_prefix('.') {
            if let Some(name) = take_ident(after_dot) {
                out.push(name);
                continue;
            }
        }
        // `nixosConfigurations = { laptop = … }`
        if let Some(brace) = rest.find('{') {
            let inner = rest[brace + 1..].trim_start();
            if let Some(name) = take_ident(inner) {
                out.push(name);
            }
        }
    }
    out.dedup();
    out
}

/// The Nix identifier at the start of `s`, quotes allowed.
fn take_ident(s: &str) -> Option<String> {
    let s = s.trim_start().trim_start_matches('"');
    let ident: String = s
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!ident.is_empty()).then_some(ident)
}

/// The `home-manager.users.<user>` user found in the folder's files.
fn hm_user_in_dir(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "nix") {
            if let Some(user) = hm_user_in_src(&read(&path)) {
                return Some(user);
            }
        }
    }
    None
}

pub(crate) fn hm_user_in_src(src: &str) -> Option<String> {
    let idx = src.find("home-manager.users.")?;
    take_ident(&src[idx + "home-manager.users.".len()..])
}

fn hostname() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
}

pub(crate) fn current_user() -> String {
    std::env::var("USER").unwrap_or_else(|_| "root".to_string())
}

/// NixOS scopes: both files attach to the top-level `imports` list; the
/// home file wraps its content in `home-manager.users.<user>`.
fn nixos_scopes(dir: &Path) -> (Option<ScopeTarget>, Option<ScopeTarget>) {
    // Without configuration.nix, the flake modules list.
    let config = dir.join("configuration.nix");
    let site = if config.is_file() || !dir.join("flake.nix").is_file() {
        config
    } else {
        dir.join("flake.nix")
    };
    (
        Some(ScopeTarget {
            nix_app_center_file: dir.join(NIX_APP_CENTER_HOME_FILE),
            import_site: site.clone(),
        }),
        Some(ScopeTarget {
            nix_app_center_file: dir.join(NIX_APP_CENTER_SYSTEM_FILE),
            import_site: site,
        }),
    )
}

fn nixos_flake_layout(dir: PathBuf, host: String, hm_user: Option<String>) -> Layout {
    let (home, system) = nixos_scopes(&dir);
    let mut apply = vec![
        "sudo".into(),
        "nixos-rebuild".into(),
        "switch".into(),
        "--flake".into(),
        format!("{}#{}", dir.display(), host),
    ];
    apply.extend(local_overrides(&dir));
    Layout {
        kind: LayoutKind::NixosFlake { dir, host },
        home,
        system,
        apply,
        hm_user,
    }
}

fn nixos_legacy_layout(dir: PathBuf, hm_user: Option<String>) -> Layout {
    let (home, system) = nixos_scopes(&dir);
    let apply = vec!["sudo".into(), "nixos-rebuild".into(), "switch".into()];
    Layout {
        kind: LayoutKind::NixosLegacy { dir },
        home,
        system,
        apply,
        hm_user,
    }
}

fn hm_flake_layout(dir: PathBuf, user: String) -> Layout {
    let home = Some(ScopeTarget {
        nix_app_center_file: dir.join(NIX_APP_CENTER_HOME_FILE),
        import_site: dir.join("home.nix"),
    });
    let mut apply = vec![
        "home-manager".into(),
        "switch".into(),
        "--flake".into(),
        format!("{}#{}", dir.display(), user),
    ];
    apply.extend(local_overrides(&dir));
    // Standalone Home Manager has no system scope.
    Layout {
        kind: LayoutKind::HmFlake { dir, user },
        home,
        system: None,
        apply,
        hm_user: None,
    }
}

fn hm_standalone_layout(dir: PathBuf) -> Layout {
    let home = Some(ScopeTarget {
        nix_app_center_file: dir.join(NIX_APP_CENTER_HOME_FILE),
        import_site: dir.join("home.nix"),
    });
    let apply = vec!["home-manager".into(), "switch".into()];
    Layout {
        kind: LayoutKind::HmStandalone { dir },
        home,
        system: None,
        apply,
        hm_user: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_inputs_resolve_to_their_folder() {
        let dir = std::env::temp_dir().join(format!("nix-app-center-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real")).unwrap();
        let real = std::fs::canonicalize(dir.join("real")).unwrap();
        let abs = format!("git+file://{}?ref=main", real.display());
        assert_eq!(local_dir(&dir, &abs), Some(real.clone()));
        assert_eq!(local_dir(&dir, "path:./real"), Some(real));
        assert_eq!(local_dir(&dir, "path:./missing"), None);
        assert_eq!(local_dir(&dir, "github:o/r"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_the_suggested_flakes() {
        let toml = "[[sources]]\ntype = \"git\"\nurl = \"x\"\n\n[[sources]]\ntype = \"github\"\nowner = \"fufexan\"\nrepo = \"nix-gaming\"\n";
        assert_eq!(parse_suggested(toml), vec!["github:fufexan/nix-gaming"]);
    }

    #[test]
    fn package_sources_skip_nixpkgs_and_plumbing() {
        let lock = serde_json::json!({ "root": "root", "nodes": {
            "root": { "inputs": { "nixpkgs": "n", "nix-gaming": "g", "flake-utils": "u", "local": "l" } },
            "n": { "locked": { "type": "github", "owner": "NixOS", "repo": "nixpkgs", "rev": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" } },
            "g": { "locked": { "type": "github", "owner": "fufexan", "repo": "nix-gaming", "rev": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" } },
            "u": { "locked": { "type": "github", "owner": "numtide", "repo": "flake-utils", "rev": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" } },
            "l": { "locked": { "type": "path", "path": "/x" } }
        }});
        assert_eq!(
            sources_from_lock(&lock),
            vec![(
                "nix-gaming".to_string(),
                "github:fufexan/nix-gaming/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()
            )]
        );
    }

    #[test]
    fn local_inputs_are_used_as_they_are_on_disk() {
        let lock = serde_json::json!({
            "root": "root",
            "nodes": {
                "root": { "inputs": { "nixos-config": "nixos-config", "nixpkgs": "nixpkgs", "local": "local" } },
                "nixos-config": { "original": { "type": "git", "url": "file:///home/u/nixos-config" } },
                "nixpkgs": { "original": { "type": "github", "owner": "NixOS", "repo": "nixpkgs" } },
                "local": { "original": { "type": "path", "path": "/home/u/mods" } }
            }
        });
        let args = overrides_from_lock(&lock);
        assert!(args.windows(3).any(|w| w
            == [
                "--override-input",
                "nixos-config",
                "git+file:///home/u/nixos-config"
            ]));
        assert!(args
            .windows(3)
            .any(|w| w == ["--override-input", "local", "path:/home/u/mods"]));
        assert!(
            !args.iter().any(|a| a == "nixpkgs"),
            "remote inputs stay locked"
        );
    }

    #[test]
    fn a_custom_command_asks_for_rights_only_with_sudo() {
        assert_eq!(
            custom_apply("sudo nixos-rebuild switch"),
            ["sudo", "sh", "-c", "nixos-rebuild switch"]
        );
        assert_eq!(
            custom_apply("  home-manager switch "),
            ["sh", "-c", "home-manager switch"]
        );
    }

    /// Throwaway fixture. No test ever touches the real `/etc/nixos` or `~`.
    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nix-app-center-layout-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn detects_nixos_flake_with_hm_module() {
        let _g = crate::env_lock();
        let dir = fixture("flake");
        write(
            &dir,
            "flake.nix",
            "outputs = { ... }: { nixosConfigurations.laptop = x; };",
        );
        write(
            &dir,
            "configuration.nix",
            "{ imports = [ ./hardware-configuration.nix ]; }",
        );
        write(&dir, "home.nix", "{ home-manager.users.alice = { ... }; }");
        std::env::set_var("NIX_APP_CENTER_CONFIG_DIR", &dir);

        let layout = detect();
        std::env::remove_var("NIX_APP_CENTER_CONFIG_DIR");

        assert_eq!(
            layout.kind,
            LayoutKind::NixosFlake {
                dir: dir.clone(),
                host: "laptop".into()
            }
        );
        assert_eq!(layout.hm_user.as_deref(), Some("alice"));
        assert_eq!(
            layout.apply,
            [
                "sudo",
                "nixos-rebuild",
                "switch",
                "--flake",
                &format!("{}#laptop", dir.display())
            ]
        );
        // Both scopes attach in the same configuration.nix.
        let home = layout.home.unwrap();
        let system = layout.system.unwrap();
        assert_eq!(
            home.nix_app_center_file,
            dir.join("nix-app-center-home.nix")
        );
        assert_eq!(
            system.nix_app_center_file,
            dir.join("nix-app-center-system.nix")
        );
        assert_eq!(home.import_site, dir.join("configuration.nix"));
        assert_eq!(system.import_site, dir.join("configuration.nix"));
    }

    #[test]
    fn detects_nixos_legacy_without_flake() {
        let _g = crate::env_lock();
        let dir = fixture("legacy");
        write(&dir, "configuration.nix", "{ imports = [ ]; }");
        std::env::set_var("NIX_APP_CENTER_CONFIG_DIR", &dir);

        let layout = detect();
        std::env::remove_var("NIX_APP_CENTER_CONFIG_DIR");

        assert_eq!(layout.kind, LayoutKind::NixosLegacy { dir });
        assert_eq!(layout.apply, ["sudo", "nixos-rebuild", "switch"]);
        assert!(layout.system.is_some());
    }

    #[test]
    fn unknown_when_nothing_found() {
        let _g = crate::env_lock();
        let dir = fixture("vide");
        let home = fixture("home-vide");
        std::env::set_var("NIX_APP_CENTER_CONFIG_DIR", &dir);
        std::env::set_var("HOME", &home);

        let layout = detect();
        std::env::remove_var("NIX_APP_CENTER_CONFIG_DIR");

        assert_eq!(layout.kind, LayoutKind::Unknown);
        assert!(layout.apply.is_empty(), "no apply may be offered");
        assert!(!layout.is_usable());
    }

    #[test]
    fn hm_standalone_has_no_system_scope() {
        // Standalone Home Manager has no system scope.
        let dir = PathBuf::from("/tmp/hm");
        let layout = hm_standalone_layout(dir);
        assert!(layout.system.is_none());
        assert_eq!(layout.apply, ["home-manager", "switch"]);
    }

    #[test]
    fn host_prefers_this_machine_over_first_declared() {
        let src = "nixosConfigurations.Autre = a; nixosConfigurations.laptop = b;";
        let hosts = nixos_hosts(src);
        assert_eq!(hosts, ["Autre", "laptop"]);
        // Unknown hostname: the first host.
        assert!(first_nixos_host(src).is_some());
    }

    #[test]
    fn parses_attrset_form_of_nixos_configurations() {
        let src = "nixosConfigurations = { laptop = nixpkgs.lib.nixosSystem { }; };";
        assert_eq!(nixos_hosts(src), ["laptop"]);
    }

    #[test]
    fn no_host_when_flake_has_no_nixos_configurations() {
        assert!(first_nixos_host("outputs = { }: { packages.x = y; };").is_none());
    }

    #[test]
    fn the_host_matches_the_machine_whatever_the_case() {
        let hosts = vec!["laptop".to_string(), "desktop-01".to_string()];
        assert_eq!(
            pick_host(hosts.clone(), Some("Desktop-01")).as_deref(),
            Some("desktop-01")
        );
        assert_eq!(pick_host(hosts, Some("other")).as_deref(), Some("laptop"));
        assert_eq!(pick_host(Vec::new(), Some("x")), None);
    }

    #[test]
    fn finds_hm_user() {
        assert_eq!(
            hm_user_in_src("home-manager.users.alice = { };").as_deref(),
            Some("alice")
        );
        assert!(hm_user_in_src("{ home.packages = [ ]; }").is_none());
    }
}
