//! Where the Nix store space goes: system, user, running programs, old
//! generations, development, tried apps and garbage. Each path counts once,
//! in the first category that keeps it alive.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Share of the store, by priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    System,
    User,
    Running,
    OldGenerations,
    Development,
    TriedApps,
    Garbage,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::System,
        Category::User,
        Category::Running,
        Category::OldGenerations,
        Category::Development,
        Category::TriedApps,
        Category::Garbage,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenProfile {
    System,
    HomeManager,
    User,
}

#[derive(Debug, Clone)]
pub struct Generation {
    pub profile: GenProfile,
    pub number: u32,
    /// Creation time (Unix seconds).
    pub date: u64,
    /// NixOS version of a system generation.
    pub version: Option<String>,
    pub current: bool,
    /// Same store path as the current generation.
    pub same_as_current: bool,
    pub booted: bool,
    /// Bytes only this generation keeps alive.
    pub exclusive: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppOrigin {
    System,
    User,
    Kept,
    Tried,
}

#[derive(Debug, Clone)]
pub struct AppSize {
    pub origin: AppOrigin,
    pub name: String,
    pub desktop_file: Option<PathBuf>,
    /// The package itself.
    pub own: u64,
    /// The package with every dependency.
    pub closure: u64,
}

#[derive(Debug, Clone, Default)]
pub struct StorageReport {
    pub disk_size: u64,
    pub disk_free: u64,
    /// Sum of every store path, before hard link sharing.
    pub store_size: u64,
    pub store_paths: usize,
    pub categories: Vec<(Category, u64)>,
    pub generations: Vec<Generation>,
    pub apps: Vec<AppSize>,
}

/// What a folder of the home holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeKind {
    /// `~/.cache`.
    Cache,
    /// `~/.local/share`, `~/.var/app`.
    Data,
    /// `~/.config`.
    Config,
}

/// One folder an app keeps in the home.
#[derive(Debug, Clone)]
pub struct HomeDir {
    pub kind: HomeKind,
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
}

fn home_bases() -> Vec<(HomeKind, PathBuf)> {
    let home = crate::util::home();
    vec![
        (HomeKind::Cache, home.join(".cache")),
        (HomeKind::Data, home.join(".local/share")),
        (HomeKind::Data, home.join(".var/app")),
        (HomeKind::Config, home.join(".config")),
    ]
}

/// Sizes of the app folders in the home, biggest first. Blocking.
pub fn home_usage() -> Vec<HomeDir> {
    let mut out: Vec<HomeDir> = home_bases()
        .into_iter()
        .filter(|(_, base)| base.is_dir())
        .flat_map(|(kind, base)| {
            let text = Command::new("du")
                .args(["-B1", "-d1", "-x"])
                .arg(&base)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            parse_du(&text, &base, kind)
        })
        .collect();
    out.sort_by_key(|d| std::cmp::Reverse(d.size));
    out
}

/// Children of `base` in `du -d1` output.
fn parse_du(text: &str, base: &Path, kind: HomeKind) -> Vec<HomeDir> {
    text.lines()
        .filter_map(|l| {
            let (size, path) = l.split_once('\t')?;
            let path = PathBuf::from(path);
            if path.parent() != Some(base) {
                return None;
            }
            Some(HomeDir {
                kind,
                name: path.file_name()?.to_string_lossy().into_owned(),
                size: size.trim().parse().ok()?,
                path,
            })
        })
        .collect()
}

/// Deletes a real folder directly under `~/.cache`, nothing else.
pub fn clear_cache(dir: &HomeDir) -> std::io::Result<()> {
    let cache = home_bases()
        .into_iter()
        .find(|(k, _)| *k == HomeKind::Cache)
        .map(|(_, p)| p);
    let is_real_dir = std::fs::symlink_metadata(&dir.path).is_ok_and(|m| m.is_dir());
    if dir.kind != HomeKind::Cache || dir.path.parent() != cache.as_deref() || !is_real_dir {
        return Err(std::io::Error::other("not a cache folder"));
    }
    std::fs::remove_dir_all(&dir.path)
}

/// Every store path as an index, with its size and references.
#[derive(Default)]
pub(crate) struct Graph {
    index: HashMap<String, u32>,
    size: Vec<u64>,
    refs: Vec<Vec<u32>>,
    /// Edges only the GC keep settings add (derivers and their outputs).
    kept: Vec<Vec<u32>>,
}

/// GC settings that keep more than references: `keep-derivations` keeps a
/// live output's derivation, `keep-outputs` a live derivation's outputs.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Keep {
    pub derivations: bool,
    pub outputs: bool,
}

impl Keep {
    /// The settings of this machine's Nix.
    fn of_nix() -> Self {
        let on = |name: &str| {
            crate::util::nix()
                .args(["config", "show", name])
                .output()
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
        };
        Keep {
            derivations: on("keep-derivations"),
            outputs: on("keep-outputs"),
        }
    }
}

impl Graph {
    /// The graph, with the edges the GC follows under `keep`.
    pub(crate) fn from_path_info(v: &serde_json::Value, keep: Keep) -> Graph {
        let infos = crate::tried::parse_path_info(v);
        let mut g = Graph::default();
        for p in infos.keys() {
            let i = g.size.len() as u32;
            g.index.insert(p.clone(), i);
            g.size.push(0);
            g.refs.push(Vec::new());
            g.kept.push(Vec::new());
        }
        for (p, info) in &infos {
            let i = g.index[p] as usize;
            g.size[i] = info.nar_size;
            g.refs[i] = info
                .references
                .iter()
                .filter_map(|r| g.index.get(r).copied())
                .filter(|r| *r as usize != i)
                .collect();
        }
        for (p, info) in &infos {
            let Some(drv) = info.deriver.as_ref().and_then(|d| g.index.get(d).copied()) else {
                continue;
            };
            let out = g.index[p];
            if keep.derivations {
                g.kept[out as usize].push(drv);
            }
            if keep.outputs {
                g.kept[drv as usize].push(out);
            }
        }
        g
    }

    fn id(&self, path: &str) -> Option<u32> {
        self.index.get(path).copied()
    }

    /// Closure of `roots`, not entering the `stop` paths.
    fn closure(&self, roots: &[u32], stop: &HashSet<u32>) -> HashSet<u32> {
        let mut seen = HashSet::new();
        let mut stack: Vec<u32> = roots.to_vec();
        while let Some(p) = stack.pop() {
            if stop.contains(&p) || !seen.insert(p) {
                continue;
            }
            stack.extend(self.refs[p as usize].iter().copied());
        }
        seen
    }

    /// What the GC keeps from `roots`: references and keep settings edges.
    fn alive(&self, roots: &[u32]) -> HashSet<u32> {
        let mut seen = HashSet::new();
        let mut stack: Vec<u32> = roots.to_vec();
        while let Some(p) = stack.pop() {
            if !seen.insert(p) {
                continue;
            }
            stack.extend(self.refs[p as usize].iter().copied());
            stack.extend(self.kept[p as usize].iter().copied());
        }
        seen
    }

    fn sum(&self, set: &HashSet<u32>) -> u64 {
        set.iter().map(|&p| self.size[p as usize]).sum()
    }
}

/// A GC root: the link that holds it and the store path it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Root {
    pub link: String,
    pub target: String,
}

/// GC roots from `nix-store --gc --print-roots`.
pub(crate) fn parse_roots(text: &str) -> Vec<Root> {
    text.lines()
        .filter_map(|l| {
            let (link, target) = l.split_once(" -> ")?;
            Some(Root {
                link: link.trim().trim_matches('"').to_string(),
                target: target.trim().to_string(),
            })
        })
        .collect()
}

/// `system-34-link` gives `("system", 34)`.
fn generation_of(link: &str) -> Option<(String, u32)> {
    let name = Path::new(link).file_name()?.to_str()?;
    let rest = name.strip_suffix("-link")?;
    let (profile, n) = rest.rsplit_once('-')?;
    Some((profile.to_string(), n.parse().ok()?))
}

/// Machine facts the report needs.
#[derive(Debug, Default)]
pub(crate) struct Machine {
    pub roots: Vec<Root>,
    pub system_current: Option<String>,
    pub system_booted: Option<String>,
    /// Store paths of the user's environments.
    pub user_envs: Vec<String>,
    /// The user's profile folders.
    pub user_profile_dirs: Vec<String>,
    pub tried: Vec<String>,
    /// Generation links the profiles point to now.
    pub current_links: Vec<String>,
}

/// Store size per category, and every generation.
pub(crate) fn analyse(g: &Graph, m: &Machine) -> (Vec<(Category, u64)>, Vec<Generation>) {
    let ids = |paths: &[&String]| -> Vec<u32> { paths.iter().filter_map(|p| g.id(p)).collect() };
    let user_env_ids: HashSet<u32> = m.user_envs.iter().filter_map(|p| g.id(p)).collect();
    let is_system_gen = |r: &Root| r.link.starts_with("/nix/var/nix/profiles/system-");
    let is_user_gen = |r: &Root| {
        m.user_profile_dirs
            .iter()
            .any(|d| r.link.starts_with(d.as_str()))
            && generation_of(&r.link).is_some()
    };
    let current: Vec<&String> = m
        .system_current
        .iter()
        .chain(m.system_booted.iter())
        .collect();

    let system = g.closure(&ids(&current), &user_env_ids);
    let user = g.closure(
        &user_env_ids.iter().copied().collect::<Vec<_>>(),
        &HashSet::new(),
    );
    let running: Vec<&String> = m
        .roots
        .iter()
        .filter(|r| r.link.starts_with("/proc/"))
        .map(|r| &r.target)
        .collect();
    let gens: Vec<&String> = m
        .roots
        .iter()
        .filter(|r| is_system_gen(r) || is_user_gen(r))
        .map(|r| &r.target)
        .collect();
    let other: Vec<&String> = m
        .roots
        .iter()
        .filter(|r| {
            !r.link.starts_with("/proc/")
                && !is_system_gen(r)
                && !is_user_gen(r)
                && !r.link.starts_with("{censored}")
                && !current.contains(&&r.target)
        })
        .map(|r| &r.target)
        .collect();
    let none = HashSet::new();
    let sets = [
        (Category::System, system),
        (Category::User, user),
        (Category::Running, g.closure(&ids(&running), &none)),
        (Category::OldGenerations, g.closure(&ids(&gens), &none)),
        (Category::Development, g.closure(&ids(&other), &none)),
        (
            Category::TriedApps,
            g.closure(&ids(&m.tried.iter().collect::<Vec<_>>()), &none),
        ),
    ];
    let mut taken: HashSet<u32> = HashSet::new();
    let mut categories = Vec::new();
    for (cat, set) in sets {
        let own: HashSet<u32> = set.difference(&taken).copied().collect();
        categories.push((cat, g.sum(&own)));
        taken.extend(own);
    }
    let roots: Vec<&String> = m.roots.iter().map(|r| &r.target).chain(current).collect();
    let alive = g.alive(&ids(&roots));
    let build_inputs: HashSet<u32> = alive.difference(&taken).copied().collect();
    if let Some(dev) = categories
        .iter_mut()
        .find(|(c, _)| *c == Category::Development)
    {
        dev.1 += g.sum(&build_inputs);
    }
    taken.extend(build_inputs);
    let garbage: u64 = (0..g.size.len() as u32)
        .filter(|p| !taken.contains(p))
        .map(|p| g.size[p as usize])
        .sum();
    categories.push((Category::Garbage, garbage));

    (categories, generations(g, m, &is_system_gen, &is_user_gen))
}

/// Every generation with what only it keeps alive.
fn generations(
    g: &Graph,
    m: &Machine,
    is_system_gen: &dyn Fn(&Root) -> bool,
    is_user_gen: &dyn Fn(&Root) -> bool,
) -> Vec<Generation> {
    // Live roots reaching each path.
    let mut targets: Vec<&String> = m.roots.iter().map(|r| &r.target).collect();
    targets.extend(m.system_current.iter().chain(m.system_booted.iter()));
    targets.extend(m.user_envs.iter());
    targets.sort();
    targets.dedup();
    let mut reach: HashMap<u32, u32> = HashMap::new();
    let mut closures: HashMap<&String, HashSet<u32>> = HashMap::new();
    for t in targets {
        let Some(id) = g.id(t) else { continue };
        let c = g.closure(&[id], &HashSet::new());
        for p in &c {
            *reach.entry(*p).or_default() += 1;
        }
        closures.insert(t, c);
    }

    let mut out: Vec<Generation> = m
        .roots
        .iter()
        .filter(|r| is_system_gen(r) || is_user_gen(r))
        .filter_map(|r| {
            let (profile_name, number) = generation_of(&r.link)?;
            let profile = if is_system_gen(r) {
                GenProfile::System
            } else if profile_name == "home-manager" {
                GenProfile::HomeManager
            } else {
                GenProfile::User
            };
            let exclusive = closures
                .get(&r.target)
                .map(|c| {
                    c.iter()
                        .filter(|p| reach.get(p) == Some(&1))
                        .map(|&p| g.size[p as usize])
                        .sum()
                })
                .unwrap_or(0);
            let link = Path::new(&r.link);
            Some(Generation {
                profile,
                number,
                date: std::fs::symlink_metadata(link)
                    .and_then(|md| md.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                version: std::fs::read_to_string(Path::new(&r.target).join("nixos-version"))
                    .ok()
                    .map(|v| v.trim().to_string()),
                current: m.current_links.contains(&r.link),
                same_as_current: !m.current_links.contains(&r.link)
                    && m.system_current.as_ref() == Some(&r.target),
                booted: m.system_booted.as_ref() == Some(&r.target),
                exclusive,
            })
        })
        .collect();
    out.sort_by_key(|g| (g.profile as u8, std::cmp::Reverse(g.number)));
    out
}

fn canonical(path: impl AsRef<Path>) -> Option<String> {
    std::fs::canonicalize(path)
        .ok()
        .map(|p| p.display().to_string())
        .filter(|p| p.starts_with("/nix/store/"))
}

fn machine() -> Machine {
    let roots = Command::new("nix-store")
        .args(["--gc", "--print-roots"])
        .output()
        .map(|o| parse_roots(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    let home = crate::util::home();
    let user = crate::config_layout::current_user();
    let user_envs = [
        PathBuf::from("/etc/profiles/per-user").join(&user),
        home.join(".nix-profile"),
        home.join(".local/state/home-manager/gcroots/current-home"),
    ]
    .iter()
    .filter_map(canonical)
    .collect();
    Machine {
        roots,
        system_current: canonical("/run/current-system"),
        system_booted: canonical("/run/booted-system"),
        user_envs,
        current_links: current_links(&[
            PathBuf::from("/nix/var/nix/profiles"),
            home.join(".local/state/nix/profiles"),
            PathBuf::from(format!("/nix/var/nix/profiles/per-user/{user}")),
        ]),
        user_profile_dirs: vec![
            home.join(".local/state/nix/profiles").display().to_string(),
            format!("/nix/var/nix/profiles/per-user/{user}"),
        ],
        tried: crate::load_tried()
            .tried
            .into_iter()
            .flat_map(|e| e.out_paths)
            .collect(),
    }
}

/// The current generation link of each profile in `dirs`.
fn current_links(dirs: &[PathBuf]) -> Vec<String> {
    dirs.iter()
        .filter_map(|d| std::fs::read_dir(d).ok().map(|e| (d, e)))
        .flat_map(|(d, entries)| {
            entries.filter_map(Result::ok).filter_map(move |e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let target = std::fs::read_link(e.path()).ok()?;
                (!name.ends_with("-link")).then(|| d.join(target).display().to_string())
            })
        })
        .collect()
}

/// (size, free) of the store's file system.
fn disk() -> (u64, u64) {
    let out = Command::new("df")
        .args(["-B1", "--output=size,avail", "/nix/store"])
        .output();
    let text = out
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let nums: Vec<u64> = text
        .lines()
        .nth(1)
        .unwrap_or("")
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    (
        nums.first().copied().unwrap_or(0),
        nums.get(1).copied().unwrap_or(0),
    )
}

/// Sizes of the apps Nix Software shows.
fn apps(g: &Graph) -> Vec<AppSize> {
    let none = HashSet::new();
    let sized = |paths: &[String]| -> (u64, u64) {
        let ids: Vec<u32> = paths.iter().filter_map(|p| g.id(p)).collect();
        let own = ids.iter().map(|&i| g.size[i as usize]).sum();
        (own, g.sum(&g.closure(&ids, &none)))
    };
    let kept: HashSet<String> = crate::load_manifest()
        .packages()
        .into_iter()
        .map(|p| p.attr.to_lowercase())
        .collect();
    // One row per package, not per desktop file.
    let mut seen = HashSet::new();
    let mut out: Vec<AppSize> = crate::profile_apps()
        .into_iter()
        .filter(|a| seen.insert(a.store_path.clone()))
        .map(|a| {
            let (own, closure) = sized(&[a.store_path.display().to_string()]);
            let origin = if kept.contains(&a.pname.to_lowercase()) {
                AppOrigin::Kept
            } else if a.scope == crate::Scope::System {
                AppOrigin::System
            } else {
                AppOrigin::User
            };
            AppSize {
                origin,
                name: a.pname,
                desktop_file: Some(a.desktop_file),
                own,
                closure,
            }
        })
        .collect();
    for e in crate::load_tried().tried {
        let (own, closure) = sized(&e.out_paths);
        out.push(AppSize {
            origin: AppOrigin::Tried,
            name: if e.package.name.is_empty() {
                e.package.attr
            } else {
                e.package.name
            },
            desktop_file: None,
            own,
            closure,
        });
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.closure));
    out
}

/// The storage report. Reads the whole store database: blocking.
pub fn storage_report() -> std::io::Result<StorageReport> {
    let out = crate::util::nix()
        .args(["path-info", "--all", "--json"])
        .output()?;
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(std::io::Error::other)?;
    let g = Graph::from_path_info(&v, Keep::of_nix());
    drop(v);
    let (categories, generations) = analyse(&g, &machine());
    let (disk_size, disk_free) = disk();
    Ok(StorageReport {
        disk_size,
        disk_free,
        store_size: g.size.iter().sum(),
        store_paths: g.size.len(),
        categories,
        generations,
        apps: apps(&g),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Store: system (sys→libc), user env (env→app→libc), an old generation
    /// (old→oldlib), a dev shell (shell), a tried app (tried) and garbage.
    fn graph() -> Graph {
        let info =
            |refs: &[&str], size: u64| serde_json::json!({ "references": refs, "narSize": size });
        Graph::from_path_info(
            &serde_json::json!({
                "/nix/store/sys": info(&["/nix/store/libc", "/nix/store/env"], 10),
                "/nix/store/libc": info(&[], 100),
                "/nix/store/env": info(&["/nix/store/app"], 1),
                "/nix/store/app": info(&["/nix/store/libc"], 20),
                "/nix/store/old": info(&["/nix/store/oldlib", "/nix/store/libc"], 5),
                "/nix/store/oldlib": info(&[], 50),
                "/nix/store/shell": info(&[], 7),
                "/nix/store/tried": info(&[], 3),
                "/nix/store/junk": info(&[], 2),
            }),
            Keep::default(),
        )
    }

    #[test]
    fn keep_settings_keep_derivations_and_outputs_alive() {
        let v = serde_json::json!({
            "/nix/store/app": { "references": [], "narSize": 1, "deriver": "/nix/store/app.drv" },
            "/nix/store/app.drv": { "references": ["/nix/store/src"], "narSize": 1 },
            "/nix/store/src": { "references": [], "narSize": 1 },
            "/nix/store/built": { "references": [], "narSize": 1, "deriver": "/nix/store/tool.drv" },
            "/nix/store/tool.drv": { "references": [], "narSize": 1 },
        });
        let ids = |g: &Graph, roots: &[&str]| -> HashSet<u32> {
            let r: Vec<u32> = roots.iter().filter_map(|p| g.id(p)).collect();
            g.alive(&r)
        };
        let plain = Graph::from_path_info(&v, Keep::default());
        assert_eq!(ids(&plain, &["/nix/store/app"]).len(), 1);
        let kept = Graph::from_path_info(
            &v,
            Keep {
                derivations: true,
                outputs: true,
            },
        );
        assert_eq!(
            ids(&kept, &["/nix/store/app"]).len(),
            3,
            "drv and its source"
        );
        assert!(
            ids(&kept, &["/nix/store/tool.drv"]).contains(&kept.id("/nix/store/built").unwrap())
        );
    }

    #[test]
    fn kept_build_inputs_are_development_not_garbage() {
        let v = serde_json::json!({
            "/nix/store/sys": { "references": [], "narSize": 10, "deriver": "/nix/store/sys.drv" },
            "/nix/store/sys.drv": { "references": ["/nix/store/gcc"], "narSize": 1 },
            "/nix/store/gcc": { "references": [], "narSize": 100 },
            "/nix/store/junk": { "references": [], "narSize": 2 },
        });
        let g = Graph::from_path_info(
            &v,
            Keep {
                derivations: true,
                outputs: true,
            },
        );
        let m = Machine {
            roots: vec![root("/nix/var/nix/profiles/system-1-link", "sys")],
            system_current: Some("/nix/store/sys".into()),
            current_links: vec!["/nix/var/nix/profiles/system-1-link".into()],
            ..Default::default()
        };
        let (cats, _) = analyse(&g, &m);
        let size = |c| cats.iter().find(|(k, _)| *k == c).unwrap().1;
        assert_eq!(size(Category::System), 10, "runtime closure only");
        assert_eq!(size(Category::Development), 101);
        assert_eq!(size(Category::Garbage), 2);
    }

    fn root(link: &str, target: &str) -> Root {
        Root {
            link: link.into(),
            target: format!("/nix/store/{target}"),
        }
    }

    fn machine() -> Machine {
        Machine {
            roots: vec![
                root("/nix/var/nix/profiles/system-2-link", "sys"),
                root("/nix/var/nix/profiles/system-1-link", "old"),
                root("/home/u/project/.direnv/flake-profile-1-link", "shell"),
            ],
            system_current: Some("/nix/store/sys".into()),
            system_booted: None,
            user_envs: vec!["/nix/store/env".into()],
            user_profile_dirs: vec!["/home/u/.local/state/nix/profiles".into()],
            tried: vec!["/nix/store/tried".into()],
            current_links: vec!["/nix/var/nix/profiles/system-2-link".into()],
        }
    }

    #[test]
    fn categories_count_each_path_once() {
        let (cats, _) = analyse(&graph(), &machine());
        let size = |c: Category| cats.iter().find(|(k, _)| *k == c).unwrap().1;
        assert_eq!(size(Category::System), 110, "sys + libc, not the user env");
        assert_eq!(size(Category::User), 21, "env + app, libc already counted");
        assert_eq!(size(Category::OldGenerations), 55);
        assert_eq!(size(Category::Development), 7);
        assert_eq!(size(Category::TriedApps), 3);
        assert_eq!(size(Category::Garbage), 2);
        assert_eq!(cats.iter().map(|c| c.1).sum::<u64>(), 198);
    }

    #[test]
    fn old_generation_frees_only_what_it_alone_keeps() {
        let (_, gens) = analyse(&graph(), &machine());
        let old = gens.iter().find(|g| g.number == 1).unwrap();
        assert_eq!(old.exclusive, 55, "libc is shared with the current system");
        let cur = gens.iter().find(|g| g.number == 2).unwrap();
        assert!(cur.current);
        assert_eq!(gens[0].number, 2, "newest first");
    }

    #[test]
    fn parse_du_keeps_only_the_children() {
        let text = "120\t/h/.cache/spotify\n40\t/h/.cache/uv/sub\n200\t/h/.cache\n";
        let dirs = parse_du(text, Path::new("/h/.cache"), HomeKind::Cache);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "spotify");
        assert_eq!(dirs[0].size, 120);
    }

    #[test]
    fn clear_cache_refuses_anything_but_a_cache_child() {
        let data = HomeDir {
            kind: HomeKind::Data,
            name: "x".into(),
            path: PathBuf::from("/nonexistent/x"),
            size: 0,
        };
        assert!(clear_cache(&data).is_err());
        let outside = HomeDir {
            kind: HomeKind::Cache,
            ..data
        };
        assert!(clear_cache(&outside).is_err());
    }

    #[test]
    fn parses_roots_and_generation_links() {
        let roots = parse_roots("\"/nix/var/nix/profiles/system-34-link\" -> /nix/store/abc\n{censored} -> /nix/store/x\n");
        assert_eq!(roots[0].link, "/nix/var/nix/profiles/system-34-link");
        assert_eq!(roots[0].target, "/nix/store/abc");
        assert_eq!(generation_of(&roots[0].link), Some(("system".into(), 34)));
        assert_eq!(
            generation_of("/home/u/.local/state/nix/profiles/home-manager-7-link"),
            Some(("home-manager".into(), 7))
        );
    }
}
