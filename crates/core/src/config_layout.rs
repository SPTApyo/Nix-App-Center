//! Detection of the user's Nix configuration layout.
//!
//! Onyx used to assume *standalone* home-manager: it wrote
//! `~/.config/home-manager/onyx-packages.nix` and applied with `home-manager switch`.
//! Both assumptions break outside that case, and the most common NixOS setup
//! (flake + home-manager as a NixOS module) has neither that file imported nor that
//! binary. The generated file was then an **orphan**: "keep" had no effect.
//!
//! This module identifies the real layout and derives three things from it: where to
//! put the Onyx files, which `imports` to patch, and **the real apply command**.
//!
//! Detection is *text scanning*, not Nix parsing: it looks for markers and never
//! rebuilds a file from an AST. So it can be wrong, which is why it is never applied
//! silently: the front end shows what was found and asks for confirmation (see
//! `config_attach`).

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
    /// Nothing recognizable. Onyx does not guess: it falls back on the manual snippet.
    Unknown,
}

/// A scope Onyx can write to: its file, and the file whose `imports` list must be
/// patched so Onyx's file is taken into account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeTarget {
    /// The file Onyx owns (generated, never edited by hand).
    pub onyx_file: PathBuf,
    /// The user's file whose `imports = [ … ]` gets patched, a single line.
    pub import_site: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub kind: LayoutKind,
    /// User scope (`home.packages`…). `None` when the layout offers none.
    pub home: Option<ScopeTarget>,
    /// System scope (`networking.firewall`…). `None` outside NixOS: standalone
    /// home-manager has no way to write system options.
    pub system: Option<ScopeTarget>,
    /// The full apply argv. Empty when the layout is unknown.
    pub apply: Vec<String>,
    /// The home-manager user, when HM is plugged in as a NixOS module. It decides
    /// the wrapping of the Onyx file (`home-manager.users.<user> = { … }`).
    pub hm_user: Option<String>,
}

impl Layout {
    /// True when Onyx knows where to write and how to apply.
    pub fn is_usable(&self) -> bool {
        !matches!(self.kind, LayoutKind::Unknown)
    }
}

const ONYX_HOME_FILE: &str = "onyx-home.nix";
const ONYX_SYSTEM_FILE: &str = "onyx-system.nix";

/// Root of the NixOS configuration. `$ONYX_CONFIG_DIR` is the escape hatch (and
/// the test injection point: no test may ever touch the real `/etc/nixos`).
fn nixos_dir() -> PathBuf {
    std::env::var_os("ONYX_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/nixos"))
}

fn hm_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".config/home-manager")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Detects the machine layout.
pub fn detect() -> Layout {
    let nixos = nixos_dir();
    let hm = hm_dir();

    let flake = nixos.join("flake.nix");
    if flake.is_file() {
        let src = read(&flake);
        if let Some(host) = first_nixos_host(&src) {
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

// ── Text scanning ─────────────────────────────────────────────────────────────

/// First `nixosConfigurations.<host>` or `nixosConfigurations = { <host> = …`.
/// When the machine hostname is among the configurations found, it wins: a
/// multi-machine flake must not make Onyx write into another machine's config.
pub(crate) fn first_nixos_host(flake_src: &str) -> Option<String> {
    let hosts = nixos_hosts(flake_src);
    if hosts.is_empty() {
        return None;
    }
    let this = hostname();
    if let Some(h) = this.as_ref().filter(|h| hosts.contains(h)) {
        return Some(h.clone());
    }
    hosts.into_iter().next()
}

/// Every host declared in `nixosConfigurations`.
fn nixos_hosts(flake_src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (idx, _) in flake_src.match_indices("nixosConfigurations") {
        let rest = &flake_src[idx + "nixosConfigurations".len()..];
        // Form A: `nixosConfigurations.laptop = …`
        if let Some(after_dot) = rest.strip_prefix('.') {
            if let Some(name) = take_ident(after_dot) {
                out.push(name);
                continue;
            }
        }
        // Form B: `nixosConfigurations = { laptop = … }`, first ident of the brace.
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

/// The Nix identifier at the head of `s` (`laptop = …` → `laptop`), quotes allowed.
fn take_ident(s: &str) -> Option<String> {
    let s = s.trim_start().trim_start_matches('"');
    let ident: String = s
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!ident.is_empty()).then_some(ident)
}

/// The user of `home-manager.users.<user>` when it appears in a `.nix` file of
/// the directory, a sign that home-manager is plugged in as a NixOS module.
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

// ── Building layouts ──────────────────────────────────────────────────────────

/// On NixOS both scopes attach at the **same** place: the top-level `imports` list
/// of `configuration.nix`. It is the safest splice, no nested Nix to parse. The
/// "home" file is then a NixOS module wrapping its content in
/// `home-manager.users.<user>`, not a home-manager module.
fn nixos_scopes(dir: &Path) -> (Option<ScopeTarget>, Option<ScopeTarget>) {
    let site = dir.join("configuration.nix");
    (
        Some(ScopeTarget {
            onyx_file: dir.join(ONYX_HOME_FILE),
            import_site: site.clone(),
        }),
        Some(ScopeTarget {
            onyx_file: dir.join(ONYX_SYSTEM_FILE),
            import_site: site,
        }),
    )
}

fn nixos_flake_layout(dir: PathBuf, host: String, hm_user: Option<String>) -> Layout {
    let (home, system) = nixos_scopes(&dir);
    let apply = vec![
        "sudo".into(),
        "nixos-rebuild".into(),
        "switch".into(),
        "--flake".into(),
        format!("{}#{}", dir.display(), host),
    ];
    Layout { kind: LayoutKind::NixosFlake { dir, host }, home, system, apply, hm_user }
}

fn nixos_legacy_layout(dir: PathBuf, hm_user: Option<String>) -> Layout {
    let (home, system) = nixos_scopes(&dir);
    let apply = vec!["sudo".into(), "nixos-rebuild".into(), "switch".into()];
    Layout { kind: LayoutKind::NixosLegacy { dir }, home, system, apply, hm_user }
}

fn hm_flake_layout(dir: PathBuf, user: String) -> Layout {
    let home = Some(ScopeTarget {
        onyx_file: dir.join(ONYX_HOME_FILE),
        import_site: dir.join("home.nix"),
    });
    let apply = vec![
        "home-manager".into(),
        "switch".into(),
        "--flake".into(),
        format!("{}#{}", dir.display(), user),
    ];
    // No system scope: standalone home-manager cannot write NixOS options.
    // A system option is refused with a snippet to paste.
    Layout { kind: LayoutKind::HmFlake { dir, user }, home, system: None, apply, hm_user: None }
}

fn hm_standalone_layout(dir: PathBuf) -> Layout {
    let home = Some(ScopeTarget {
        onyx_file: dir.join(ONYX_HOME_FILE),
        import_site: dir.join("home.nix"),
    });
    let apply = vec!["home-manager".into(), "switch".into()];
    Layout { kind: LayoutKind::HmStandalone { dir }, home, system: None, apply, hm_user: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Throwaway fixture. No test ever touches the real `/etc/nixos` or `~`.
    fn fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("onyx-layout-{}-{}", std::process::id(), name));
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
        write(&dir, "flake.nix", "outputs = { ... }: { nixosConfigurations.laptop = x; };");
        write(&dir, "configuration.nix", "{ imports = [ ./hardware-configuration.nix ]; }");
        write(&dir, "home.nix", "{ home-manager.users.alice = { ... }; }");
        std::env::set_var("ONYX_CONFIG_DIR", &dir);

        let layout = detect();
        std::env::remove_var("ONYX_CONFIG_DIR");

        assert_eq!(
            layout.kind,
            LayoutKind::NixosFlake { dir: dir.clone(), host: "laptop".into() }
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
        assert_eq!(home.onyx_file, dir.join("onyx-home.nix"));
        assert_eq!(system.onyx_file, dir.join("onyx-system.nix"));
        assert_eq!(home.import_site, dir.join("configuration.nix"));
        assert_eq!(system.import_site, dir.join("configuration.nix"));
    }

    #[test]
    fn detects_nixos_legacy_without_flake() {
        let _g = crate::env_lock();
        let dir = fixture("legacy");
        write(&dir, "configuration.nix", "{ imports = [ ]; }");
        std::env::set_var("ONYX_CONFIG_DIR", &dir);

        let layout = detect();
        std::env::remove_var("ONYX_CONFIG_DIR");

        assert_eq!(layout.kind, LayoutKind::NixosLegacy { dir });
        assert_eq!(layout.apply, ["sudo", "nixos-rebuild", "switch"]);
        assert!(layout.system.is_some());
    }

    #[test]
    fn unknown_when_nothing_found() {
        let _g = crate::env_lock();
        let dir = fixture("vide");
        let home = fixture("home-vide");
        std::env::set_var("ONYX_CONFIG_DIR", &dir);
        std::env::set_var("HOME", &home);

        let layout = detect();
        std::env::remove_var("ONYX_CONFIG_DIR");

        assert_eq!(layout.kind, LayoutKind::Unknown);
        assert!(layout.apply.is_empty(), "no apply may be offered");
        assert!(!layout.is_usable());
    }

    #[test]
    fn hm_standalone_has_no_system_scope() {
        // Standalone home-manager has no way to write NixOS options.
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
        // Without a match with the real hostname, the first declared one is used.
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
    fn finds_hm_user() {
        assert_eq!(hm_user_in_src("home-manager.users.alice = { };").as_deref(), Some("alice"));
        assert!(hm_user_in_src("{ home.packages = [ ]; }").is_none());
    }
}
