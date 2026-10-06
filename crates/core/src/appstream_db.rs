//! Local AppStream database.
//!
//! Flatpak keeps `~/.local/share/flatpak/appstream/flathub/<arch>/active/appstream.xml.gz`,
//! which holds ALL the Flathub metadata (icons, descriptions, screenshots…) for
//! every package. GNOME Software reads this same file, which is why it is so
//! fast: zero network requests while browsing. Without Flatpak, Onyx downloads
//! the same file from Flathub into its cache directory.
//!
//! Onyx loads it at startup through [`init_background`] (separate, non
//! blocking thread). Once loaded, [`lookup`] is an O(1) HashMap.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::PathBuf,
    sync::{Condvar, LazyLock, Mutex, OnceLock},
};

use flate2::read::GzDecoder;
use quick_xml::{events::Event, Reader};

use crate::package::Package;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub app_id: String,
    pub name: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub developer: Option<String>,
    pub icon_url: Option<String>,
    pub screenshot_urls: Vec<String>,
    /// Caption of each screenshot (parallel to `screenshot_urls`, `None` when missing).
    pub screenshot_captions: Vec<Option<String>>,
    pub homepage: Option<String>,
    pub bugtracker: Option<String>,
    pub help: Option<String>,
    pub donation: Option<String>,
    pub translate: Option<String>,
    pub license: Option<String>,
    pub verified: bool,
    pub releases: Vec<EntryRelease>,
    /// OARS `content_rating` attributes: (id, value) e.g. ("social-chat","intense").
    pub oars: Vec<(String, String)>,
    /// Set for an `addon` component: the app_id of the parent it extends.
    pub extends: Option<String>,
    /// Search keywords (untranslated).
    pub keywords: Vec<String>,
    /// Freedesktop categories (`AudioVideo`, `Game`…).
    pub categories: Vec<String>,
    /// Commands the app installs (`<provides><binary>`).
    pub binaries: Vec<String>,
    /// File types the app opens (`<provides><mediatype>`).
    pub mediatypes: Vec<String>,
    /// Desktop file id (`<launchable type="desktop-id">`).
    pub launchable: Option<String>,
    /// Brand color for a light and a dark background (`<branding>`).
    pub brand_light: Option<String>,
    pub brand_dark: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct EntryRelease {
    pub version: String,
    pub timestamp: i64,
    pub description: Option<String>,
    pub url: Option<String>,
}

/// An add-on/extension of an app (AppStream `type="addon"` component).
#[derive(Debug, Clone)]
pub struct Addon {
    pub name: String,
    pub summary: Option<String>,
    pub icon_url: Option<String>,
}

/// A related app (same developer), for the "Other Apps" section.
#[derive(Debug, Clone)]
pub struct DevApp {
    pub app_id: String,
    pub name: String,
    pub icon_url: Option<String>,
}

// ── Internal DB ───────────────────────────────────────────────────────────────

struct Db {
    entries: Vec<Entry>,
    /// Last app_id segment, lowercase alphanumeric → index
    by_suffix: HashMap<String, usize>,
    /// App name, lowercase alphanumeric → index
    by_name: HashMap<String, usize>,
    /// Full lowercase app_id → index
    by_id: HashMap<String, usize>,
    /// Separate `addon` entries (never in the lookup indexes above).
    addons: Vec<Entry>,
    /// Lowercase parent app_id → indexes of add-ons in `addons`.
    by_extends: HashMap<String, Vec<usize>>,
    /// Lowercase developer → indexes of apps in `entries`.
    by_developer: HashMap<String, Vec<usize>>,
}

impl Db {
    fn lookup(&self, q: &str) -> Option<&Entry> {
        let slug = alnum(q);
        self.by_suffix
            .get(&slug)
            .or_else(|| self.by_name.get(&slug))
            .or_else(|| self.by_id.get(&q.to_lowercase()))
            .and_then(|&i| self.entries.get(i))
    }

    fn addons_for(&self, app_id: &str) -> Vec<Addon> {
        self.by_extends
            .get(&app_id.to_lowercase())
            .map(|idxs| {
                // GIMP 2 and 3 ship the same plug-ins under distinct ids.
                let mut seen = std::collections::HashSet::new();
                idxs.iter()
                    .filter_map(|&i| self.addons.get(i))
                    .filter(|e| seen.insert(e.name.to_lowercase()))
                    .map(|e| Addon {
                        name: e.name.clone(),
                        summary: e.summary.clone(),
                        icon_url: e.icon_url.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn apps_by_developer(&self, developer: &str, exclude_id: &str) -> Vec<DevApp> {
        self.by_developer
            .get(&developer.to_lowercase())
            .map(|idxs| {
                idxs.iter()
                    .filter_map(|&i| self.entries.get(i))
                    .filter(|e| e.app_id != exclude_id)
                    .map(|e| DevApp {
                        app_id: e.app_id.clone(),
                        name: e.name.clone(),
                        icon_url: e.icon_url.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Keeps only alphanumeric characters, lowercased.
fn alnum(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

// ── Singleton ─────────────────────────────────────────────────────────────────

static DB: OnceLock<Db> = OnceLock::new();
/// Startup guard: `OnceLock::set` is atomic, unlike a bare
/// `if DB.get().is_some()` (two concurrent `init_background` calls before the
/// first load ends would both pass that test and each start their own XML
/// decompression/parse).
static INIT_STARTED: OnceLock<()> = OnceLock::new();

/// Load state. Telling "in progress" from "impossible" matters: a load in
/// progress can be *waited for* (a few seconds), a load that will never
/// finish cannot (machine without any AppStream file).
#[derive(Clone, Copy, PartialEq)]
enum LoadState {
    /// `init_background` never called (CLI case): nothing to wait for.
    NotStarted,
    Loading,
    Ready,
    /// No AppStream XML available (or empty parse): nothing to wait for.
    Unavailable,
}

static STATE: LazyLock<(Mutex<LoadState>, Condvar)> =
    LazyLock::new(|| (Mutex::new(LoadState::NotStarted), Condvar::new()));

fn set_state(s: LoadState) {
    let (m, cv) = &*STATE;
    *m.lock().unwrap_or_else(|e| e.into_inner()) = s;
    cv.notify_all();
}

/// `true` once no load is in progress (ready, unavailable or never started):
/// an `AppMeta` computed now will not get richer, so it can be memoized
/// without poisoning the session with a poorer listing.
/// Waits up to `timeout` for the AppStream load, if one is running.
pub fn wait_appstream(timeout: std::time::Duration) {
    wait_until_settled(timeout);
}

pub fn is_settled() -> bool {
    let (m, _) = &*STATE;
    *m.lock().unwrap_or_else(|e| e.into_inner()) != LoadState::Loading
}

/// Waits (bounded) for the load IN PROGRESS to end. Returns at once when the
/// load has not started, succeeded, or is impossible; never starts a load.
/// Real bug: with an instant home page (fixed disk cache), opening a details
/// page during the XML parse (~4 s) fell back on REST without add-ons or
/// developer apps, memoized for the whole session.
pub(crate) fn wait_until_settled(timeout: std::time::Duration) {
    let (m, cv) = &*STATE;
    let deadline = std::time::Instant::now() + timeout;
    let mut st = m.lock().unwrap_or_else(|e| e.into_inner());
    while *st == LoadState::Loading {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        let (guard, _) = cv
            .wait_timeout(st, deadline - now)
            .unwrap_or_else(|e| e.into_inner());
        st = guard;
    }
}

/// Starts loading the AppStream file in the background. Non blocking.
/// Safe to call several times (idempotent): only the first call really
/// starts a loading thread.
pub fn init_background() {
    if INIT_STARTED.set(()).is_err() {
        return; // already started (or running) by an earlier call
    }
    set_state(LoadState::Loading);
    std::thread::spawn(|| match load() {
        Some(db) => {
            let _ = DB.set(db);
            set_state(LoadState::Ready);
        }
        None => set_state(LoadState::Unavailable),
    });
}

/// Instant lookup. Returns `None` while the DB is not ready (loading) or when
/// the package is unknown.
pub fn lookup(name: &str) -> Option<&'static Entry> {
    DB.get()?.lookup(name)
}

/// Apps whose name, summary or id holds every term, best first. Local and
/// instant; empty while the database loads.
pub fn search_local(terms: &[String], limit: usize) -> Vec<LocalHit> {
    DB.get()
        .map(|db| rank_local(&db.entries, terms, limit))
        .unwrap_or_default()
}

fn rank_local(entries: &[Entry], terms: &[String], limit: usize) -> Vec<LocalHit> {
    let terms: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
    let mut hits: Vec<(i32, &Entry)> = entries
        .iter()
        .filter_map(|e| {
            let name = e.name.to_lowercase();
            let rest = format!(
                "{} {} {} {} {}",
                e.summary.as_deref().unwrap_or(""),
                e.app_id,
                e.keywords.join(" "),
                e.binaries.join(" "),
                e.categories.join(" ")
            )
            .to_lowercase();
            if !terms.iter().all(|t| name.contains(t) || rest.contains(t)) {
                return None;
            }
            let mut score = 10 * terms.iter().filter(|t| name.contains(t.as_str())).count() as i32;
            if terms.first().is_some_and(|t| name.starts_with(t.as_str())) {
                score += 5;
            }
            Some((score, e))
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
    hits.into_iter()
        .take(limit)
        .map(|(_, e)| LocalHit {
            app_id: e.app_id.clone(),
            name: e.name.clone(),
            summary: e.summary.clone().unwrap_or_default(),
        })
        .collect()
}

/// True when `mediatype` is the file type named by `query` (`pdf` for
/// `application/pdf`, `mp4` for `video/mp4`, `x-` prefixes ignored).
fn opens(mediatype: &str, query: &str) -> bool {
    let sub = mediatype.rsplit('/').next().unwrap_or(mediatype);
    let sub = sub.strip_prefix("x-").unwrap_or(sub);
    mediatype.eq_ignore_ascii_case(query) || sub.eq_ignore_ascii_case(query)
}

/// Apps that open files of type `query`, resolved to nixpkgs. Runs a
/// (cached) `nix search`: call it from a thread.
pub fn apps_for_file_type(query: &str, limit: usize) -> Vec<(Package, Option<String>)> {
    let q = query.trim();
    if q.len() < 2 || q.contains(char::is_whitespace) {
        return Vec::new();
    }
    let Some(db) = DB.get() else {
        return Vec::new();
    };
    let apps: Vec<DevApp> = db
        .entries
        .iter()
        .filter(|e| e.mediatypes.iter().any(|m| opens(m, q)))
        .take(limit * 3)
        .map(|e| DevApp {
            app_id: e.app_id.clone(),
            name: e.name.clone(),
            icon_url: e.icon_url.clone(),
        })
        .collect();
    let mut out = crate::categories::resolve_dev_apps(&apps, "");
    out.truncate(limit);
    out
}

/// Brand colors `(light, dark)` of a package's AppStream app, if declared.
/// Only well formed `#rrggbb` colors: the value ends up in CSS.
pub fn brand_colors(p: &Package) -> Option<(Option<String>, Option<String>)> {
    let e = lookup_package(p)?;
    let (light, dark) = (valid_color(&e.brand_light), valid_color(&e.brand_dark));
    (light.is_some() || dark.is_some()).then_some((light, dark))
}

fn valid_color(c: &Option<String>) -> Option<String> {
    c.as_ref()
        .filter(|c| {
            c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
        })
        .cloned()
}

/// One app of the local database, by its id.
pub fn local_app(app_id: &str) -> Option<LocalHit> {
    let e = lookup(app_id).filter(|e| e.app_id.eq_ignore_ascii_case(app_id))?;
    Some(LocalHit {
        app_id: e.app_id.clone(),
        name: e.name.clone(),
        summary: e.summary.clone().unwrap_or_default(),
    })
}

/// An app found by [`search_local`].
#[derive(Debug, Clone)]
pub struct LocalHit {
    pub app_id: String,
    pub name: String,
    pub summary: String,
}

/// The nixpkgs package of an AppStream app, when nixpkgs has it. Runs a
/// (cached) `nix search`: call it from a thread.
pub fn package_for_app(app_id: &str, name: &str) -> Option<Package> {
    let app = DevApp {
        app_id: app_id.to_string(),
        name: name.to_string(),
        icon_url: None,
    };
    crate::categories::resolve_dev_apps(&[app], "")
        .into_iter()
        .next()
        .map(|(p, _)| p)
}

/// `true` once the DB is loaded and ready (lookups will not hit the network).
pub fn is_ready() -> bool {
    DB.get().is_some()
}

/// Finds the local AppStream entry of a nixpkgs package: by name (`pname`)
/// first, then by attr (some packages only match one of them). `Db::lookup`
/// already tries suffix/name/id for EACH key, so trying both keys raises
/// recall without any extra false positive risk.
///
/// The curated overlay wins both ways: an attr it maps goes to that app, and
/// an app it pins to another attr is never matched by name (`zoom`, the
/// Z-code player, is not `us.zoom.Zoom`, which is `zoom-us`).
pub(crate) fn lookup_package(p: &Package) -> Option<&'static Entry> {
    if crate::resolve::is_unmatched(&p.attr) {
        return None;
    }
    if let Some(e) = crate::resolve::resolve_app_id(&p.attr).and_then(|id| lookup(&id)) {
        return Some(e);
    }
    let free = |e: &&Entry| {
        crate::resolve::attr_for_app_id(&e.app_id).is_none_or(|a| a.eq_ignore_ascii_case(&p.attr))
    };
    lookup(&p.name)
        .filter(free)
        .or_else(|| lookup(&p.attr).filter(free))
}

/// Add-ons/extensions extending the app with the given `app_id`.
pub(crate) fn addons_for(app_id: &str) -> Vec<Addon> {
    DB.get().map(|db| db.addons_for(app_id)).unwrap_or_default()
}

/// Other apps by the same developer (excluding `exclude_id`).
pub(crate) fn apps_by_developer(developer: &str, exclude_id: &str) -> Vec<DevApp> {
    DB.get()
        .map(|db| db.apps_by_developer(developer, exclude_id))
        .unwrap_or_default()
}

// ── Loading ───────────────────────────────────────────────────────────────────

fn flatpak_xml_path() -> Option<PathBuf> {
    let arch = arch()?;

    let bases: Vec<PathBuf> = [
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".local/share/flatpak")),
        Some(PathBuf::from("/var/lib/flatpak")),
    ]
    .into_iter()
    .flatten()
    .collect();

    for base in bases {
        let xml = base.join(format!("appstream/flathub/{arch}/active/appstream.xml.gz"));
        if xml.exists() {
            return Some(xml);
        }
    }
    None
}

/// Flathub publishes the same file Flatpak caches locally.
const FLATHUB_APPSTREAM_URL: &str = "https://dl.flathub.org/repo/appstream/{arch}/appstream.xml.gz";

/// How long a downloaded copy stays fresh before a refresh is attempted.
const DOWNLOAD_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

fn arch() -> Option<&'static str> {
    if cfg!(target_arch = "x86_64") {
        Some("x86_64")
    } else if cfg!(target_arch = "aarch64") {
        Some("aarch64")
    } else {
        None
    }
}

/// Fallback for machines without Flatpak (most NixOS installs): keep our own
/// copy of the Flathub AppStream file in the cache directory. A stale copy is
/// used at once and refreshed in the background for the next start.
fn downloaded_xml_path() -> Option<PathBuf> {
    let path = crate::cache::cache_dir().join("appstream.xml.gz");
    let url = FLATHUB_APPSTREAM_URL.replace("{arch}", arch()?);
    if !path.exists() {
        refresh(&url, &path);
        return path.exists().then_some(path);
    }
    let stale = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|age| age >= DOWNLOAD_TTL);
    if stale {
        let target = path.clone();
        std::thread::spawn(move || refresh(&url, &target));
    }
    Some(path)
}

fn refresh(url: &str, path: &std::path::Path) {
    if let Some(bytes) = download(url) {
        let _ = crate::paths::write_atomic(path, &bytes);
    }
}

fn download(url: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(120))
        .call()
        .ok()?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(200 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    // Reject truncated or non-gzip payloads.
    bytes.starts_with(&[0x1f, 0x8b]).then_some(bytes)
}

fn load() -> Option<Db> {
    let path = flatpak_xml_path().or_else(downloaded_xml_path)?;
    let file = std::fs::File::open(&path).ok()?;
    let gz = GzDecoder::new(BufReader::new(file));
    let entries = parse(BufReader::new(gz));

    if entries.is_empty() {
        return None;
    }

    Some(build_db(entries))
}

/// Builds the lookup indexes from parsed entries. Shared by `load()` and the
/// tests (`make_db`) so the indexing policy never diverges between the real
/// code and what the tests check.
fn build_db(all: Vec<Entry>) -> Db {
    // Split apps (lookup) and add-ons (indexed by their `extends` parent).
    let mut entries: Vec<Entry> = Vec::new();
    let mut addons: Vec<Entry> = Vec::new();
    for e in all {
        if e.extends.is_some() {
            addons.push(e);
        } else {
            entries.push(e);
        }
    }

    let mut by_suffix: HashMap<String, usize> = HashMap::with_capacity(entries.len());
    let mut by_name: HashMap<String, usize> = HashMap::with_capacity(entries.len());
    let mut by_id: HashMap<String, usize> = HashMap::with_capacity(entries.len());
    let mut by_developer: HashMap<String, Vec<usize>> = HashMap::new();
    // One suffix (last app id segment, e.g. "player"/"editor") may be shared
    // by several distinct apps in the full Flathub catalog. Matching the first
    // one seen would be a false positive (wrong icon/description), worse than
    // no match. So any suffix seen more than once is removed from the
    // HashMap. Same policy for NAMES: two apps with the same name, neither
    // matches by name.
    let mut ambiguous_suffixes: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut ambiguous_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (i, e) in entries.iter().enumerate() {
        let suffix = alnum(e.app_id.split('.').next_back().unwrap_or(&e.app_id));
        if !suffix.is_empty() {
            match by_suffix.entry(suffix) {
                std::collections::hash_map::Entry::Occupied(e) => {
                    ambiguous_suffixes.insert(e.key().clone());
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(i);
                }
            }
        }
        let name_slug = alnum(&e.name);
        if !name_slug.is_empty() {
            match by_name.entry(name_slug) {
                std::collections::hash_map::Entry::Occupied(o) => {
                    ambiguous_names.insert(o.key().clone());
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(i);
                }
            }
        }
        by_id.entry(e.app_id.to_lowercase()).or_insert(i);
        if let Some(dev) = e.developer.as_ref().filter(|d| !d.trim().is_empty()) {
            by_developer.entry(dev.to_lowercase()).or_default().push(i);
        }
    }
    for s in &ambiguous_suffixes {
        by_suffix.remove(s);
    }
    for s in &ambiguous_names {
        by_name.remove(s);
    }

    let mut by_extends: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, a) in addons.iter().enumerate() {
        if let Some(parent) = a.extends.as_ref() {
            by_extends.entry(parent.to_lowercase()).or_default().push(i);
        }
    }

    Db {
        entries,
        by_suffix,
        by_name,
        by_id,
        addons,
        by_extends,
        by_developer,
    }
}

// ── XML parser ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Collect {
    None,
    Id,
    Name,
    DevName,
    Summary,
    License,
    Icon,
    ImageUrl,
    Url,
    ReleaseUrl,
    CustomVal,
    Extends,
    ContentAttr,
    Caption,
    Keyword,
    Category,
    Binary,
    MediaType,
    Launchable,
    Color,
}

/// Appends markup text the way HTML flows it: whitespace runs (XML
/// indentation included) become one space, none at a line start.
fn push_flowed(buf: &mut String, text: &str) {
    for (i, word) in text.split_whitespace().enumerate() {
        let at_line_start = buf.is_empty() || buf.ends_with('\n') || buf.ends_with("• ");
        let glued = i == 0 && !text.starts_with(char::is_whitespace);
        if !at_line_start && !glued {
            buf.push(' ');
        }
        buf.push_str(word);
    }
    if text.ends_with(char::is_whitespace) && !text.trim().is_empty() {
        buf.push(' ');
    }
}

fn end_line(buf: &mut String) {
    buf.truncate(buf.trim_end_matches(' ').len());
    buf.push('\n');
}

fn parse<R: BufRead>(reader: R) -> Vec<Entry> {
    let mut xml = Reader::from_reader(reader);
    // Without this, quick-xml emits a single `Event::Empty` (not handled by
    // the loop below, which only processes Start/End/Text/Eof) for
    // self-closing tags (`<release version="…" timestamp="…" />`, very common
    // in the real Flathub appstream.xml.gz for releases without notes).
    // Asking for their expansion makes them arrive as a normal Start+End and
    // go through exactly the same logic, with zero code duplication.
    xml.config_mut().expand_empty_elements = true;
    let mut buf = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();

    // ── Media base URL (prefix for relative URLs) ────────────────────────────
    let mut media_base = String::new();

    // ── Depth & skip ──────────────────────────────────────────────────────────
    let mut depth: usize = 0;
    // When > 0: inside a subtree to ignore.
    // Every Start increments it, every End decrements it down to 0.
    let mut skip: usize = 0;

    // ── Current entry ─────────────────────────────────────────────────────────
    let mut cur: Option<Entry> = None;
    let mut comp_depth: usize = 0;

    // ── Text collection ───────────────────────────────────────────────────────
    let mut collect = Collect::None;

    // ── Main description ──────────────────────────────────────────────────────
    let mut in_desc = false;
    let mut desc_depth: usize = 0;
    let mut desc_buf = String::new();

    // ── Release description ───────────────────────────────────────────────────
    let mut in_rel_desc = false;
    let mut rel_desc_depth: usize = 0;
    let mut rel_desc_buf = String::new();

    // ── Structural flags ──────────────────────────────────────────────────────
    let mut in_screenshots = false;
    let mut in_screenshot = false;
    let mut in_releases = false;
    let mut in_release = false;
    let mut in_developer = false; // <developer> tag (AppStream 1.0+ format)

    // ── Field context ─────────────────────────────────────────────────────────
    let mut icon_remote = false;
    let mut icon_w: u32 = 0;
    let mut icon_scale: u32 = 1;
    let mut color_scheme = String::new();
    let mut entry_icon_px: u32 = 0;
    let mut url_kind = String::new();
    let mut custom_key = String::new();
    let mut cur_rel = EntryRelease::default();
    let mut content_attr_id = String::new();
    let mut cur_caption: Option<String> = None;

    loop {
        buf.clear();
        let ev = match xml.read_event_into(&mut buf) {
            Ok(ev) => ev,
            Err(_) => break,
        };

        match ev {
            // ── Opening ───────────────────────────────────────────────────────
            Event::Start(ref e) => {
                depth += 1;

                if skip > 0 {
                    skip += 1;
                    continue;
                }

                let qname = e.name();
                let tag = match std::str::from_utf8(qname.as_ref()) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let has_lang = e
                    .attributes()
                    .flatten()
                    .any(|a| a.key.as_ref() == b"xml:lang");

                // Inside a description: collect the raw text
                if in_desc && tag != "description" {
                    if has_lang {
                        skip = 1; // skip the translated paragraph
                    } else if tag == "li" {
                        desc_buf.push_str("• ");
                    }
                    // Otherwise let the text through via Event::Text
                    continue;
                }
                if in_rel_desc && tag != "description" {
                    if has_lang {
                        skip = 1;
                    } else if tag == "li" {
                        rel_desc_buf.push_str("• ");
                    }
                    continue;
                }

                // ── Root / structural elements ────────────────────────────────
                match tag {
                    "components" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"media_baseurl" {
                                media_base = String::from_utf8_lossy(&attr.value).into_owned();
                            }
                        }
                    }
                    "component" => {
                        let comp_type: String = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"type")
                            .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                            .unwrap_or_default();
                        match comp_type.as_str() {
                            "desktop-application" | "desktop" | "addon" => {
                                cur = Some(Entry::default());
                                entry_icon_px = 0;
                                comp_depth = depth;
                            }
                            _ => {
                                skip = 1; // ignore fonts, codecs, etc.
                            }
                        }
                    }

                    // ── Simple fields (only inside a component) ───────────────
                    _ if cur.is_none() => {}

                    "id" if !has_lang => {
                        collect = Collect::Id;
                    }
                    "name" if !in_developer && !has_lang => {
                        collect = Collect::Name;
                    }
                    "name" if in_developer && !has_lang => {
                        collect = Collect::DevName;
                    }
                    "name" if has_lang => {
                        skip = 1;
                    }
                    "summary" if !has_lang => {
                        collect = Collect::Summary;
                    }
                    "summary" if has_lang => {
                        skip = 1;
                    }
                    "developer_name" if !has_lang => {
                        collect = Collect::DevName;
                    }
                    "developer_name" if has_lang => {
                        skip = 1;
                    }
                    "developer" => {
                        in_developer = true;
                    }

                    "description" if !in_desc && !in_releases && !has_lang => {
                        in_desc = true;
                        desc_depth = depth;
                        desc_buf.clear();
                    }
                    "description" if !in_rel_desc && in_releases && in_release && !has_lang => {
                        in_rel_desc = true;
                        rel_desc_depth = depth;
                        rel_desc_buf.clear();
                    }
                    "description" => {
                        skip = 1; // lang or duplicate
                    }

                    "icon" => {
                        let mut itype = String::new();
                        let mut w: u32 = 0;
                        icon_scale = 1;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"type" => {
                                    itype = String::from_utf8_lossy(&attr.value).into_owned()
                                }
                                b"width" => {
                                    w = String::from_utf8_lossy(&attr.value).parse().unwrap_or(0)
                                }
                                b"scale" => {
                                    icon_scale =
                                        String::from_utf8_lossy(&attr.value).parse().unwrap_or(1)
                                }
                                _ => {}
                            }
                        }
                        if itype == "remote" {
                            icon_remote = true;
                            icon_w = w;
                            collect = Collect::Icon;
                        } else {
                            icon_remote = false;
                            skip = 1; // local cached icons are not used
                        }
                    }

                    // These tags may hold an <id> referencing ANOTHER component
                    // (e.g. <recommends><id>com.valvesoftware.Steam</id>
                    // </recommends> in the Protontricks listing): without this
                    // skip, the nested <id> overwrites the current component's
                    // and corrupts its entry (right name, another package's app_id).
                    "requires" | "recommends" | "suggests" | "supports" => {
                        skip = 1;
                    }

                    "extends" if !has_lang => {
                        collect = Collect::Extends;
                    }
                    "keyword" if !has_lang => {
                        collect = Collect::Keyword;
                    }
                    "category" => {
                        collect = Collect::Category;
                    }
                    "binary" => {
                        collect = Collect::Binary;
                    }
                    "mediatype" => {
                        collect = Collect::MediaType;
                    }
                    "launchable" => {
                        let desktop = e.attributes().flatten().any(|a| {
                            a.key.as_ref() == b"type" && a.value.as_ref() == b"desktop-id"
                        });
                        if desktop {
                            collect = Collect::Launchable;
                        }
                    }
                    "color" => {
                        let attr = |k: &[u8]| {
                            e.attributes()
                                .flatten()
                                .find(|a| a.key.as_ref() == k)
                                .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                                .unwrap_or_default()
                        };
                        if attr(b"type") == "primary" {
                            color_scheme = attr(b"scheme_preference");
                            collect = Collect::Color;
                        }
                    }

                    "content_attribute" => {
                        content_attr_id = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"id")
                            .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                            .unwrap_or_default();
                        collect = Collect::ContentAttr;
                    }

                    "screenshots" => {
                        in_screenshots = true;
                    }
                    "screenshot" if in_screenshots => {
                        in_screenshot = true;
                        cur_caption = None;
                    }
                    "caption" if in_screenshot && !has_lang => {
                        collect = Collect::Caption;
                    }
                    "caption" if in_screenshot && has_lang => {
                        skip = 1;
                    }
                    "image" if in_screenshot => {
                        let is_source = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"type")
                            .map(|a| a.value.as_ref() == b"source")
                            .unwrap_or(false);
                        if is_source {
                            collect = Collect::ImageUrl;
                        } else {
                            skip = 1;
                        }
                    }

                    "url" if !in_release => {
                        url_kind = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"type")
                            .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                            .unwrap_or_default();
                        collect = Collect::Url;
                    }

                    "project_license" if !has_lang => {
                        collect = Collect::License;
                    }

                    "releases" => {
                        in_releases = true;
                    }
                    "release" if in_releases => {
                        in_release = true;
                        cur_rel = EntryRelease::default();
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"version" => {
                                    cur_rel.version =
                                        String::from_utf8_lossy(&attr.value).into_owned()
                                }
                                b"timestamp" => {
                                    cur_rel.timestamp =
                                        String::from_utf8_lossy(&attr.value).parse().unwrap_or(0)
                                }
                                _ => {}
                            }
                        }
                    }
                    "url" if in_release => {
                        let t = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"type")
                            .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                            .unwrap_or_default();
                        if t == "details" {
                            collect = Collect::ReleaseUrl;
                        }
                    }

                    "value" if !has_lang => {
                        custom_key = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"key")
                            .map(|a| String::from_utf8_lossy(&a.value).into_owned())
                            .unwrap_or_default();
                        collect = Collect::CustomVal;
                    }

                    _ => {}
                }
            }

            // ── Closing ───────────────────────────────────────────────────────
            Event::End(ref e) => {
                if skip > 0 {
                    skip -= 1;
                    depth -= 1;
                    continue;
                }

                let qname = e.name();
                let tag = match std::str::from_utf8(qname.as_ref()) {
                    Ok(s) => s,
                    Err(_) => {
                        depth -= 1;
                        continue;
                    }
                };

                // End of a child inside a description
                if in_desc && depth > desc_depth {
                    if matches!(tag, "p" | "li") {
                        end_line(&mut desc_buf);
                    }
                    depth -= 1;
                    continue;
                }
                if in_rel_desc && depth > rel_desc_depth {
                    if matches!(tag, "p" | "li") {
                        end_line(&mut rel_desc_buf);
                    }
                    depth -= 1;
                    continue;
                }

                match tag {
                    "component" if depth == comp_depth => {
                        if let Some(mut entry) = cur.take() {
                            if !entry.app_id.is_empty() {
                                if entry.name.is_empty() {
                                    entry.name = entry.app_id.clone();
                                }
                                entries.push(entry);
                            }
                        }
                        // Reset all per-component state
                        in_desc = false;
                        in_screenshots = false;
                        in_screenshot = false;
                        in_releases = false;
                        in_release = false;
                        in_rel_desc = false;
                        in_developer = false;
                        collect = Collect::None;
                        icon_remote = false;
                    }
                    "developer" => {
                        in_developer = false;
                    }
                    "description" if in_desc && depth == desc_depth => {
                        in_desc = false;
                        if let Some(e) = cur.as_mut() {
                            let t = desc_buf.trim().to_string();
                            if !t.is_empty() {
                                e.description = Some(t);
                            }
                        }
                        desc_buf.clear();
                        collect = Collect::None;
                    }
                    "description" if in_rel_desc && depth == rel_desc_depth => {
                        in_rel_desc = false;
                        let t = rel_desc_buf.trim().to_string();
                        if !t.is_empty() {
                            cur_rel.description = Some(t);
                        }
                        rel_desc_buf.clear();
                        collect = Collect::None;
                    }
                    "release" if in_release => {
                        in_release = false;
                        if let Some(e) = cur.as_mut() {
                            if !cur_rel.version.is_empty() && e.releases.len() < 10 {
                                e.releases.push(cur_rel.clone());
                            }
                        }
                        cur_rel = EntryRelease::default();
                    }
                    "releases" => {
                        in_releases = false;
                    }
                    "screenshots" => {
                        in_screenshots = false;
                    }
                    "screenshot" => {
                        in_screenshot = false;
                    }
                    _ => {
                        collect = Collect::None;
                    }
                }

                depth -= 1;
            }

            // ── Text ──────────────────────────────────────────────────────────
            Event::Text(ref e) => {
                if skip > 0 {
                    continue;
                }

                let cow = match e.unescape() {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                if in_desc {
                    push_flowed(&mut desc_buf, &cow);
                    continue;
                }
                if in_rel_desc {
                    push_flowed(&mut rel_desc_buf, &cow);
                    continue;
                }
                let text = cow.trim();
                if text.is_empty() {
                    continue;
                }

                let entry = match cur.as_mut() {
                    Some(e) => e,
                    None => continue,
                };

                let abs = |s: &str| -> String {
                    if s.starts_with("http") {
                        s.to_string()
                    } else {
                        // `trim_end_matches` alone does NOT guarantee a single '/'
                        // separator: concatenating "…/media" (trimmed) directly to
                        // "org/…" gave "…/mediaorg/…" (bug covered by the tests
                        // parser_icon_prepends_media_baseurl_for_relative_url and
                        // parser_screenshots_prepend_media_baseurl).
                        format!(
                            "{}/{}",
                            media_base.trim_end_matches('/'),
                            s.trim_start_matches('/')
                        )
                    }
                };

                match collect {
                    // Only the FIRST <id> (the component's): an <id> nested in
                    // <provides> reads "…Steam.desktop" and would overwrite the
                    // real app_id otherwise.
                    Collect::Id if entry.app_id.is_empty() => {
                        entry.app_id = text.to_string();
                    }
                    Collect::Name if entry.name.is_empty() => {
                        entry.name = text.to_string();
                    }
                    Collect::DevName if entry.developer.is_none() => {
                        entry.developer = Some(text.to_string());
                    }
                    Collect::Summary if entry.summary.is_none() => {
                        entry.summary = Some(text.to_string());
                    }
                    Collect::License if entry.license.is_none() => {
                        entry.license = Some(text.to_string());
                    }
                    Collect::Icon if icon_remote => {
                        let url = abs(text);
                        // Keep the sharpest; fetching falls back to smaller ones.
                        let px = icon_w * icon_scale;
                        if px > entry_icon_px || entry.icon_url.is_none() {
                            entry_icon_px = px;
                            entry.icon_url = Some(url);
                        }
                    }
                    Collect::ImageUrl if entry.screenshot_urls.len() < 5 => {
                        entry.screenshot_urls.push(abs(text));
                        entry.screenshot_captions.push(cur_caption.take());
                    }
                    Collect::Caption => {
                        cur_caption = Some(text.to_string());
                    }
                    Collect::Extends if entry.extends.is_none() => {
                        entry.extends = Some(text.to_string());
                    }
                    Collect::Keyword => entry.keywords.push(text.to_string()),
                    Collect::Category => entry.categories.push(text.to_string()),
                    Collect::Binary => entry.binaries.push(text.to_string()),
                    Collect::MediaType => entry.mediatypes.push(text.to_string()),
                    Collect::Launchable if entry.launchable.is_none() => {
                        entry.launchable = Some(text.to_string());
                    }
                    Collect::Color => {
                        let color = Some(text.to_string());
                        if color_scheme == "dark" {
                            entry.brand_dark = color;
                        } else {
                            entry.brand_light = color;
                        }
                    }
                    Collect::ContentAttr if !content_attr_id.is_empty() => {
                        entry.oars.push((content_attr_id.clone(), text.to_string()));
                    }
                    Collect::Url => match url_kind.as_str() {
                        "homepage" => entry.homepage = Some(text.to_string()),
                        "bugtracker" => entry.bugtracker = Some(text.to_string()),
                        "help" => entry.help = Some(text.to_string()),
                        "donation" => entry.donation = Some(text.to_string()),
                        "translate" => entry.translate = Some(text.to_string()),
                        _ => {}
                    },
                    Collect::ReleaseUrl => {
                        cur_rel.url = Some(text.to_string());
                    }
                    Collect::CustomVal if custom_key == "flathub::verification::verified" => {
                        entry.verified = text == "true";
                    }
                    _ => {}
                }
                collect = Collect::None;
            }

            Event::Eof => break,
            _ => {}
        }
    }

    entries
}

// ── Tests ─────────────────────────────────────────────────────────────────────
//
// Three independent layers are tested:
//   1. `alnum()`: name normalization
//   2. `parse()`: XML extraction (structure, ignored translations, media, …)
//   3. `Db::lookup()`: matching strategy from nix name to AppStream entry
//
// The test XML covers every real case seen in the Flathub appstream.xml.gz.
#[cfg(test)]
mod tests {
    use super::*;

    // A minimal but representative AppStream file.
    // Covers: media_baseurl, xml:lang, old/new developer formats,
    // 64 vs 128 icon, source vs thumbnail screenshots, releases, custom/verified,
    // URL types (homepage, bugtracker), ignored addon and font.
    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<components version="0.14" origin="flathub" media_baseurl="https://dl.flathub.org/media/">

  <component type="desktop-application">
    <id>org.gnome.Gedit</id>
    <name>gedit</name>
    <name xml:lang="fr">gedit (FR)</name>
    <summary>A text editor</summary>
    <summary xml:lang="fr">Un éditeur</summary>
    <developer_name>The GNOME Project</developer_name>
    <developer_name xml:lang="fr">Projet GNOME</developer_name>
    <icon type="remote" width="64" height="64">org/gnome/Gedit/icon64.png</icon>
    <icon type="remote" width="128" height="128">org/gnome/Gedit/icon128.png</icon>
    <screenshots>
      <screenshot type="default">
        <caption>The main window</caption>
        <image type="source" width="1600" height="900">org/gnome/Gedit/shot1.png</image>
        <image type="thumbnail" width="400" height="225">org/gnome/Gedit/thumb1.png</image>
      </screenshot>
      <screenshot>
        <image type="source" width="1600" height="900">org/gnome/Gedit/shot2.png</image>
      </screenshot>
    </screenshots>
    <url type="homepage">https://wiki.gnome.org/Apps/Gedit</url>
    <url type="bugtracker">https://gitlab.gnome.org/GNOME/gedit/-/issues</url>
    <project_license>GPL-2.0+</project_license>
    <content_rating type="oars-1.1">
      <content_attribute id="social-chat">intense</content_attribute>
      <content_attribute id="money-purchasing">intense</content_attribute>
    </content_rating>
    <releases>
      <release version="46.2" timestamp="1712000000">
        <description><p>Bug fixes.</p></description>
      </release>
      <release version="46.1" timestamp="1710000000" />
    </releases>
    <custom>
      <value key="flathub::verification::verified">true</value>
    </custom>
    <description>
      <p>gedit is a text editor.</p>
      <p xml:lang="fr">gedit est un éditeur.</p>
    </description>
  </component>

  <component type="desktop-application">
    <id>org.gnome.gedit.Plugins</id>
    <name>gedit Plugins</name>
    <developer_name>The GNOME Project</developer_name>
    <icon type="remote" width="128" height="128">org/gnome/plugins/icon.png</icon>
  </component>

  <component type="desktop-application">
    <id>io.github.extremetuxracer.ETR</id>
    <name>Extreme Tux Racer</name>
    <summary>Race down a mountain</summary>
    <developer>
      <name>ETR Team</name>
    </developer>
    <icon type="remote" width="128" height="128">etr/icon128.png</icon>
  </component>

  <component type="addon">
    <id>org.gnome.Gedit.Highlighter</id>
    <name>Syntax Highlighter</name>
    <summary>Adds syntax highlighting</summary>
    <extends>org.gnome.Gedit</extends>
  </component>

  <component type="font">
    <id>org.example.SomeFont</id>
    <name>Ignored Font</name>
  </component>

</components>"#;

    fn parse_sample() -> Vec<Entry> {
        parse(std::io::Cursor::new(SAMPLE_XML.as_bytes()))
    }

    fn make_db(entries: Vec<Entry>) -> Db {
        build_db(entries)
    }

    // ── alnum() ───────────────────────────────────────────────────────────────

    #[test]
    fn alnum_strips_spaces() {
        assert_eq!(alnum("Extreme Tux Racer"), "extremetuxracer");
    }

    #[test]
    fn alnum_strips_dots_and_hyphens() {
        assert_eq!(alnum("org.gnome.Gedit"), "orggnomegedit");
        assert_eq!(alnum("vlc-media-player"), "vlcmediaplayer");
    }

    #[test]
    fn alnum_lowercases() {
        assert_eq!(alnum("LibreOffice"), "libreoffice");
        assert_eq!(alnum("GIMP"), "gimp");
        assert_eq!(alnum("Firefox"), "firefox");
    }

    #[test]
    fn alnum_empty() {
        assert_eq!(alnum(""), "");
    }

    // ── Parser: structure ─────────────────────────────────────────────────────

    #[test]
    fn parser_excludes_fonts_but_keeps_apps_and_addons() {
        // 3 desktop apps + 1 addon = 4; the font is excluded.
        let entries = parse_sample();
        assert_eq!(entries.len(), 4);
        assert!(!entries.iter().any(|e| e.app_id.contains("Font")));
    }

    #[test]
    fn parser_captures_addon_extends() {
        let addon = parse_sample()
            .into_iter()
            .find(|e| e.app_id == "org.gnome.Gedit.Highlighter")
            .unwrap();
        assert_eq!(addon.extends.as_deref(), Some("org.gnome.Gedit"));
    }

    #[test]
    fn parser_captures_content_rating_oars() {
        let e = parse_sample()
            .into_iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert!(e
            .oars
            .contains(&("social-chat".to_string(), "intense".to_string())));
        assert!(e
            .oars
            .contains(&("money-purchasing".to_string(), "intense".to_string())));
    }

    #[test]
    fn parser_captures_screenshot_caption_aligned() {
        let e = parse_sample()
            .into_iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert_eq!(e.screenshot_urls.len(), 2);
        assert_eq!(e.screenshot_captions.len(), 2);
        assert_eq!(e.screenshot_captions[0].as_deref(), Some("The main window"));
        assert_eq!(e.screenshot_captions[1], None);
    }

    #[test]
    fn db_addons_and_developer_apps() {
        let db = make_db(parse_sample());
        let addons = db.addons_for("org.gnome.Gedit");
        assert_eq!(addons.len(), 1);
        assert_eq!(addons[0].name, "Syntax Highlighter");
        // gedit and gedit Plugins share "The GNOME Project".
        let devapps = db.apps_by_developer("The GNOME Project", "org.gnome.Gedit");
        assert_eq!(devapps.len(), 1);
        assert_eq!(devapps[0].app_id, "org.gnome.gedit.Plugins");
    }

    #[test]
    fn addons_with_the_same_name_are_listed_once() {
        let addon = |id: &str| Entry {
            app_id: id.into(),
            name: "G'MIC".into(),
            extends: Some("org.gimp.GIMP".into()),
            ..Entry::default()
        };
        let app = Entry {
            app_id: "org.gimp.GIMP".into(),
            name: "GIMP".into(),
            ..Entry::default()
        };
        let db = build_db(vec![
            app,
            addon("org.gimp.GIMP.Plugin.GMic"),
            addon("org.gimp.GIMP.Plugin.GMic3"),
        ]);
        assert_eq!(db.addons_for("org.gimp.GIMP").len(), 1);
    }

    // ── Parser: text (translations ignored) ───────────────────────────────────

    #[test]
    fn parser_name_takes_en_only() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert_eq!(e.name, "gedit");
        assert_ne!(
            e.name, "gedit (FR)",
            "the xml:lang version must not overwrite the EN one"
        );
    }

    #[test]
    fn parser_summary_takes_en_only() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert_eq!(e.summary.as_deref(), Some("A text editor"));
    }

    #[test]
    fn description_text_flows_like_html() {
        let xml = r#"<components><component type="desktop-application"><id>a.b</id><name>A</name>
<description><p>GIMP is
      an <em>image</em> editor.</p><ul><li>Fast</li><li>Free</li></ul></description>
</component></components>"#;
        let e = parse(std::io::Cursor::new(xml)).remove(0);
        assert_eq!(
            e.description.as_deref(),
            Some("GIMP is an image editor.\n• Fast\n• Free")
        );
    }

    #[test]
    fn parser_description_en_only() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        let desc = e.description.as_deref().unwrap_or("");
        assert!(
            desc.contains("gedit is a text editor"),
            "description EN absente"
        );
        assert!(
            !desc.contains("éditeur"),
            "the FR translation must not appear in the description"
        );
    }

    #[test]
    fn parser_developer_old_format() {
        // <developer_name> = AppStream 0.x format
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert_eq!(e.developer.as_deref(), Some("The GNOME Project"));
    }

    #[test]
    fn parser_developer_new_format() {
        // <developer><name> = AppStream 1.0+ format
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.name == "Extreme Tux Racer")
            .unwrap();
        assert_eq!(e.developer.as_deref(), Some("ETR Team"));
    }

    #[test]
    fn parser_nested_id_in_recommends_does_not_corrupt_app_id() {
        // Real regression: the Protontricks listing contains
        // <recommends><id>com.valvesoftware.Steam</id></recommends>. Without the
        // skip on these tags, the nested <id> overwrote the component's, and
        // lookup("steam") returned "Protontricks" under Steam's app_id.
        let xml = r#"<?xml version="1.0"?>
<components>
  <component type="desktop-application">
    <id>com.github.Matoking.protontricks</id>
    <name>Protontricks</name>
    <recommends>
      <id>com.valvesoftware.Steam</id>
    </recommends>
    <requires>
      <id>org.freedesktop.Platform</id>
      <display_length>360</display_length>
    </requires>
  </component>
  <component type="desktop-application">
    <id>com.valvesoftware.Steam</id>
    <name>Steam</name>
  </component>
</components>"#;
        let entries = parse(std::io::Cursor::new(xml.as_bytes()));
        let proton = entries.iter().find(|e| e.name == "Protontricks").unwrap();
        assert_eq!(proton.app_id, "com.github.Matoking.protontricks");

        let db = build_db(entries);
        let steam = db.lookup("steam").expect("steam must be found");
        assert_eq!(
            steam.name, "Steam",
            "lookup(\"steam\") must not return Protontricks"
        );
    }

    #[test]
    fn parser_license() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert_eq!(e.license.as_deref(), Some("GPL-2.0+"));
    }

    #[test]
    fn parser_homepage_url() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert_eq!(
            e.homepage.as_deref(),
            Some("https://wiki.gnome.org/Apps/Gedit")
        );
    }

    #[test]
    fn parser_bugtracker_url() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert!(e
            .bugtracker
            .as_deref()
            .unwrap_or("")
            .contains("gitlab.gnome.org"));
    }

    // ── Parser: verified field ────────────────────────────────────────────────

    #[test]
    fn parser_verified_true() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert!(e.verified);
    }

    #[test]
    fn parser_not_verified_by_default() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.name == "Extreme Tux Racer")
            .unwrap();
        assert!(
            !e.verified,
            "without a verified key the value must be false"
        );
    }

    // ── Parser: icons ─────────────────────────────────────────────────────────

    #[test]
    fn parser_prefers_128px_over_64px() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        let icon = e.icon_url.as_deref().expect("missing icon");
        assert!(icon.contains("icon128"), "must prefer icon128 over icon64");
        assert!(!icon.contains("icon64"));
    }

    #[test]
    fn parser_icon_prepends_media_baseurl_for_relative_url() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        let icon = e.icon_url.as_deref().expect("missing icon");
        assert!(
            icon.starts_with("https://dl.flathub.org/media/"),
            "a relative URL must be prefixed with media_baseurl, got: {icon}"
        );
    }

    #[test]
    fn parser_reads_keywords_provides_launchable_and_branding() {
        let xml = r#"<components origin="flathub"><component type="desktop-application">
    <id>org.videolan.VLC</id><name>VLC</name>
    <launchable type="desktop-id">org.videolan.VLC.desktop</launchable>
    <categories><category>AudioVideo</category></categories>
    <keywords><keyword>player</keyword><keyword xml:lang="fr">lecteur</keyword></keywords>
    <provides><binary>vlc</binary><mediatype>video/mp4</mediatype></provides>
    <branding><color type="primary" scheme_preference="light">#ff8800</color>
      <color type="primary" scheme_preference="dark">#552200</color></branding>
  </component></components>"#;
        let e = &parse(xml.as_bytes())[0];
        assert_eq!(e.keywords, ["player"]);
        assert_eq!(e.categories, ["AudioVideo"]);
        assert_eq!(e.binaries, ["vlc"]);
        assert_eq!(e.mediatypes, ["video/mp4"]);
        assert_eq!(e.launchable.as_deref(), Some("org.videolan.VLC.desktop"));
        assert_eq!(e.brand_light.as_deref(), Some("#ff8800"));
        assert_eq!(e.brand_dark.as_deref(), Some("#552200"));
    }

    #[test]
    fn only_hex_colors_reach_css() {
        let ok = Some("#1E3A8A".to_string());
        assert_eq!(valid_color(&ok), ok);
        for bad in ["#;}*{a:", "red", "#12345", "#12345é"] {
            assert_eq!(valid_color(&Some(bad.to_string())), None, "{bad}");
        }
    }

    #[test]
    fn opens_matches_the_subtype_only() {
        assert!(opens("application/pdf", "pdf"));
        assert!(opens("application/x-bittorrent", "bittorrent"));
        assert!(opens("video/mp4", "video/mp4"));
        assert!(!opens("application/pdfa", "pdf"));
    }

    #[test]
    fn rank_local_needs_every_term_and_prefers_names() {
        let e = |id: &str, name: &str, summary: &str| Entry {
            app_id: id.into(),
            name: name.into(),
            summary: Some(summary.into()),
            ..Entry::default()
        };
        let entries = [
            e("org.a.Player", "Player", "Plays videos"),
            e("org.b.Vlc", "VLC", "Media player"),
            e("org.c.Edit", "Editor", "Text"),
        ];
        let terms = |q: &str| vec![q.to_string()];
        let hits = rank_local(&entries, &terms("player"), 10);
        assert_eq!(
            hits.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(),
            ["Player", "VLC"]
        );
        assert!(rank_local(&entries, &["media".into(), "text".into()], 10).is_empty());
    }

    #[test]
    fn parser_icon_prefers_the_sharpest() {
        let xml = r#"<components origin="flathub"><component type="desktop-application">
    <id>org.videolan.VLC</id><name>VLC</name>
    <icon type="remote" width="128" height="128">https://x/128x128/vlc.png</icon>
    <icon type="remote" width="128" height="128" scale="2">https://x/128x128@2/vlc.png</icon>
  </component></components>"#;
        let entries = parse(xml.as_bytes());
        assert_eq!(
            entries[0].icon_url.as_deref(),
            Some("https://x/128x128@2/vlc.png")
        );
    }

    #[test]
    fn parser_icon_absolute_url_not_doubled() {
        let xml = r#"<?xml version="1.0"?>
<components media_baseurl="https://base/">
  <component type="desktop-application">
    <id>com.example.App</id><name>App</name>
    <icon type="remote" width="128" height="128">https://cdn.example.com/icon.png</icon>
  </component>
</components>"#;
        let entries = parse(std::io::Cursor::new(xml.as_bytes()));
        assert_eq!(
            entries[0].icon_url.as_deref(),
            Some("https://cdn.example.com/icon.png"),
            "an already absolute URL must not be doubled with media_baseurl"
        );
    }

    // ── Parser: screenshots ───────────────────────────────────────────────────

    #[test]
    fn parser_screenshots_source_only_no_thumbnails() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert_eq!(
            e.screenshot_urls.len(),
            2,
            "deux screenshots source attendus"
        );
        for url in &e.screenshot_urls {
            assert!(!url.contains("thumb"), "thumbnails must not be listed");
        }
    }

    #[test]
    fn parser_screenshots_prepend_media_baseurl() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        for url in &e.screenshot_urls {
            assert!(
                url.starts_with("https://dl.flathub.org/media/"),
                "screenshot URL must start with media_baseurl: {url}"
            );
        }
    }

    // ── Parser: releases ──────────────────────────────────────────────────────

    #[test]
    fn parser_releases_version_and_timestamp() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert_eq!(e.releases.len(), 2);
        assert_eq!(e.releases[0].version, "46.2");
        assert_eq!(e.releases[0].timestamp, 1712000000);
    }

    #[test]
    fn parser_release_description() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        let desc = e.releases[0].description.as_deref().unwrap_or("");
        assert!(desc.contains("Bug fixes"));
    }

    #[test]
    fn parser_release_without_description_is_none() {
        let entries = parse_sample();
        let e = entries
            .iter()
            .find(|e| e.app_id == "org.gnome.Gedit")
            .unwrap();
        assert_eq!(e.releases[1].version, "46.1");
        assert!(e.releases[1].description.is_none());
    }

    // ── Db::lookup(): matching strategy ───────────────────────────────────────

    #[test]
    fn lookup_by_app_id_suffix() {
        // "gedit" = alnum(last segment of "org.gnome.Gedit")
        let db = make_db(parse_sample());
        let found = db.lookup("gedit");
        assert!(found.is_some(), "gedit must be found by suffix");
        assert_eq!(found.unwrap().app_id, "org.gnome.Gedit");
    }

    #[test]
    fn lookup_by_name_slug_extremetuxracer() {
        // Historical regression: suffix("io.github.extremetuxracer.ETR") = "etr"
        // does not match "extremetuxracer", but by_name finds it through
        // alnum("Extreme Tux Racer") = "extremetuxracer".
        let db = make_db(parse_sample());
        let found = db.lookup("extremetuxracer");
        assert!(found.is_some(), "extremetuxracer must be found by name");
        assert_eq!(found.unwrap().name, "Extreme Tux Racer");
    }

    #[test]
    fn lookup_by_full_app_id() {
        let db = make_db(parse_sample());
        let found = db.lookup("org.gnome.Gedit");
        assert!(found.is_some(), "lookup by full app_id must work");
    }

    #[test]
    fn lookup_by_full_app_id_case_insensitive() {
        let db = make_db(parse_sample());
        let found = db.lookup("org.gnome.gedit");
        assert!(found.is_some(), "by_id must be case insensitive");
    }

    #[test]
    fn lookup_suffix_case_insensitive() {
        // "GEDIT" → alnum = "gedit" → found by suffix
        let db = make_db(parse_sample());
        assert!(db.lookup("GEDIT").is_some());
    }

    #[test]
    fn lookup_unknown_returns_none() {
        let db = make_db(parse_sample());
        assert!(db.lookup("ripgrep").is_none());
        assert!(db.lookup("").is_none());
        assert!(db.lookup("libreoffice").is_none());
    }

    #[test]
    fn lookup_suffix_wins_over_name_slug() {
        // "etr" matches the suffix of "io.github.extremetuxracer.ETR";
        // this test checks that by_suffix wins over by_name
        let db = make_db(parse_sample());
        let found = db.lookup("etr");
        assert!(found.is_some(), "ETR must be found by suffix 'etr'");
        assert_eq!(found.unwrap().app_id, "io.github.extremetuxracer.ETR");
    }

    // ── Db::lookup: suffix collision (potential false positive) ──────────────

    fn two_apps_sharing_suffix() -> Vec<Entry> {
        let xml = r#"<?xml version="1.0"?>
<components>
  <component type="desktop-application">
    <id>org.example.foo.Player</id>
    <name>Foo Player</name>
  </component>
  <component type="desktop-application">
    <id>org.example.bar.Player</id>
    <name>Bar Player</name>
  </component>
</components>"#;
        parse(std::io::Cursor::new(xml.as_bytes()))
    }

    #[test]
    fn lookup_ambiguous_suffix_matches_neither() {
        // "Foo Player" and "Bar Player" share the app id suffix "player": a
        // search by that suffix alone is ambiguous and must return NOTHING
        // (generic icon) rather than the wrong app at random.
        let db = build_db(two_apps_sharing_suffix());
        assert!(
            db.lookup("player").is_none(),
            "a suffix shared by 2 apps must match neither"
        );
    }

    #[test]
    fn lookup_ambiguous_suffix_falls_back_to_full_name() {
        // Each app's full name stays unique and thus findable through by_name,
        // even when their common app id suffix was neutralized.
        let db = build_db(two_apps_sharing_suffix());
        assert_eq!(
            db.lookup("fooplayer").unwrap().app_id,
            "org.example.foo.Player"
        );
        assert_eq!(
            db.lookup("barplayer").unwrap().app_id,
            "org.example.bar.Player"
        );
    }

    // ── Load state: wait_until_settled / is_settled ──────────────────────────
    // Only one test touches STATE (process global static) to avoid any race
    // between parallel tests on that shared state.
    #[test]
    fn wait_until_settled_blocks_only_while_loading() {
        use std::time::{Duration, Instant};

        // NotStarted (default, CLI case that never calls init): immediate.
        let t0 = Instant::now();
        wait_until_settled(Duration::from_secs(5));
        assert!(
            t0.elapsed() < Duration::from_millis(500),
            "NotStarted must not wait"
        );

        // Loading: waits for the end of the load (notified here after ~100 ms),
        // NOT the full timeout.
        set_state(LoadState::Loading);
        assert!(!is_settled());
        let h = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            set_state(LoadState::Unavailable);
        });
        let t0 = Instant::now();
        wait_until_settled(Duration::from_secs(10));
        h.join().unwrap();
        let dt = t0.elapsed();
        assert!(
            dt >= Duration::from_millis(80),
            "must wait for the load to end"
        );
        assert!(
            dt < Duration::from_secs(5),
            "must not wait for the full timeout"
        );
        assert!(is_settled());

        // Ready: immediate.
        set_state(LoadState::Ready);
        let t0 = Instant::now();
        wait_until_settled(Duration::from_secs(5));
        assert!(
            t0.elapsed() < Duration::from_millis(500),
            "Ready must not wait"
        );
        assert!(is_settled());
    }

    #[test]
    fn lookup_ambiguous_name_matches_neither() {
        // Two distinct apps with the SAME name: matching the first one seen
        // would be a false positive (same policy as ambiguous suffixes).
        let xml = r#"<?xml version="1.0"?>
<components>
  <component type="desktop-application">
    <id>org.example.AAA</id>
    <name>Player</name>
  </component>
  <component type="desktop-application">
    <id>org.example.BBB</id>
    <name>Player</name>
  </component>
</components>"#;
        let db = build_db(parse(std::io::Cursor::new(xml.as_bytes())));
        assert!(
            db.lookup("player").is_none(),
            "a name shared by 2 apps must match neither"
        );
        // Full app_ids stay resolvable.
        assert!(db.lookup("org.example.AAA").is_some());
        assert!(db.lookup("org.example.BBB").is_some());
    }
}
