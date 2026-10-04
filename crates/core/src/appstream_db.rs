//! Base AppStream locale issue du cache Flatpak.
//!
//! Flatpak maintient `~/.local/share/flatpak/appstream/flathub/<arch>/active/appstream.xml.gz`
//! qui contient TOUTES les métadonnées Flathub (icônes, descriptions, screenshots…)
//! pour tous les paquets. GNOME Software lit ce même fichier — c'est pourquoi il est
//! si rapide : zéro requête réseau pour la navigation.
//!
//! Onyx le charge au démarrage via [`init_background`] (thread séparé, non-bloquant).
//! Dès que le chargement est terminé, [`lookup`] est un HashMap O(1).

use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::PathBuf,
    sync::{Condvar, LazyLock, Mutex, OnceLock},
};

use flate2::read::GzDecoder;
use quick_xml::{Reader, events::Event};

use crate::package::Package;

// ── Types publics ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub app_id: String,
    pub name: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub developer: Option<String>,
    pub icon_url: Option<String>,
    pub screenshot_urls: Vec<String>,
    /// Légende de chaque screenshot (parallèle à `screenshot_urls`, `None` si absente).
    pub screenshot_captions: Vec<Option<String>>,
    pub homepage: Option<String>,
    pub bugtracker: Option<String>,
    pub help: Option<String>,
    pub donation: Option<String>,
    pub translate: Option<String>,
    pub license: Option<String>,
    pub verified: bool,
    pub releases: Vec<EntryRelease>,
    /// Attributs OARS `content_rating` : (id, valeur) ex. ("social-chat","intense").
    pub oars: Vec<(String, String)>,
    /// Renseigné pour un composant `addon` : l'app_id du parent qu'il étend.
    pub extends: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct EntryRelease {
    pub version: String,
    pub timestamp: i64,
    pub description: Option<String>,
    pub url: Option<String>,
}

/// Un addon/extension d'une app (composant AppStream `type="addon"`).
#[derive(Debug, Clone)]
pub struct Addon {
    pub name: String,
    pub summary: Option<String>,
    pub icon_url: Option<String>,
}

/// Une app apparentée (même développeur), pour la section « Autres applications ».
#[derive(Debug, Clone)]
pub struct DevApp {
    pub app_id: String,
    pub name: String,
    pub icon_url: Option<String>,
}

// ── DB interne ────────────────────────────────────────────────────────────────

struct Db {
    entries: Vec<Entry>,
    /// Dernier segment de l'app_id alphanumérique minuscule → index
    by_suffix: HashMap<String, usize>,
    /// Nom de l'app alphanumérique minuscule → index
    by_name: HashMap<String, usize>,
    /// app_id complet minuscule → index
    by_id: HashMap<String, usize>,
    /// Entrées `addon` séparées (jamais dans les index de lookup ci-dessus).
    addons: Vec<Entry>,
    /// app_id parent minuscule → index d'addons dans `addons`.
    by_extends: HashMap<String, Vec<usize>>,
    /// Développeur minuscule → index d'apps dans `entries`.
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

/// Conserve uniquement les caractères alphanumériques en minuscule.
fn alnum(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

// ── Singleton ─────────────────────────────────────────────────────────────────

static DB: OnceLock<Db> = OnceLock::new();
/// Garde-fou de démarrage : `OnceLock::set` est atomique, contrairement au
/// `if DB.get().is_some()` seul (deux appels concurrents à `init_background`
/// avant la fin du 1er chargement passeraient tous les deux ce test et
/// lanceraient chacun leur propre décompression/parse du XML, en double).
static INIT_STARTED: OnceLock<()> = OnceLock::new();

/// État du chargement. Distinguer « en cours » de « impossible » est vital :
/// on peut *attendre* la fin d'un chargement en cours (quelques secondes),
/// jamais un chargement qui n'aboutira pas (machine sans cache Flatpak).
#[derive(Clone, Copy, PartialEq)]
enum LoadState {
    /// `init_background` jamais appelé (cas CLI) — rien à attendre.
    NotStarted,
    Loading,
    Ready,
    /// Pas de XML Flatpak sur la machine (ou parse vide) — rien à attendre.
    Unavailable,
}

static STATE: LazyLock<(Mutex<LoadState>, Condvar)> =
    LazyLock::new(|| (Mutex::new(LoadState::NotStarted), Condvar::new()));

fn set_state(s: LoadState) {
    let (m, cv) = &*STATE;
    *m.lock().unwrap_or_else(|e| e.into_inner()) = s;
    cv.notify_all();
}

/// `true` quand plus aucun chargement n'est en cours (prêt, indisponible ou
/// jamais lancé) : un `AppMeta` calculé maintenant ne s'enrichira plus →
/// mémoïsable sans risque d'empoisonner la session avec une fiche appauvrie.
pub(crate) fn is_settled() -> bool {
    let (m, _) = &*STATE;
    *m.lock().unwrap_or_else(|e| e.into_inner()) != LoadState::Loading
}

/// Attend (borné) la fin du chargement EN COURS. Retour immédiat si le
/// chargement n'a pas commencé, a abouti, ou est impossible — ne déclenche
/// jamais de chargement. Bug vécu : l'accueil devenu instantané (cache disque
/// réparé), on ouvrait une page détail pendant le parse XML (~4 s) → repli
/// REST sans addons/apps-du-développeur, mémoïsé pour toute la session.
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

/// Lance le chargement du fichier AppStream en arrière-plan. Non bloquant.
/// Sûr à appeler plusieurs fois (idempotent) : seul le premier appel démarre
/// réellement un thread de chargement.
pub fn init_background() {
    if INIT_STARTED.set(()).is_err() {
        return; // déjà démarré (ou en cours) par un appel précédent
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

/// Lookup instantané. Retourne `None` si la DB n'est pas encore prête
/// (chargement en cours) ou si le paquet est inconnu.
pub fn lookup(name: &str) -> Option<&'static Entry> {
    DB.get()?.lookup(name)
}

/// `true` quand la DB est chargée et prête (les lookups ne feront plus de réseau).
pub fn is_ready() -> bool {
    DB.get().is_some()
}

/// Cherche l'entrée AppStream locale d'un paquet nixpkgs : par nom (`pname`)
/// d'abord, puis par attr (certains paquets ne matchent que par l'un des deux —
/// `Db::lookup` essaie déjà suffix/nom/id pour CHAQUE clé donnée, donc essayer
/// les deux clés augmente encore le rappel sans risquer de faux positif
/// supplémentaire).
pub(crate) fn lookup_package(p: &Package) -> Option<&'static Entry> {
    lookup(&p.name).or_else(|| lookup(&p.attr))
}

/// Addons/extensions qui étendent l'app d'`app_id` donné.
pub(crate) fn addons_for(app_id: &str) -> Vec<Addon> {
    DB.get().map(|db| db.addons_for(app_id)).unwrap_or_default()
}

/// Autres apps du même développeur (hors `exclude_id`).
pub(crate) fn apps_by_developer(developer: &str, exclude_id: &str) -> Vec<DevApp> {
    DB.get()
        .map(|db| db.apps_by_developer(developer, exclude_id))
        .unwrap_or_default()
}

// ── Chargement ────────────────────────────────────────────────────────────────

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
/// still used when the refresh fails (offline start).
fn downloaded_xml_path() -> Option<PathBuf> {
    let path = crate::cache::cache_dir().join("appstream.xml.gz");
    let fresh = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < DOWNLOAD_TTL);
    if !fresh {
        let url = FLATHUB_APPSTREAM_URL.replace("{arch}", arch()?);
        if let Some(bytes) = download(&url) {
            let _ = crate::paths::write_atomic(&path, &bytes);
        }
    }
    path.exists().then_some(path)
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

/// Construit les index de lookup à partir des entrées parsées. Partagé entre
/// `load()` et les tests (`make_db`) pour que la politique d'indexation ne
/// diverge jamais entre le code réel et ce qui est vérifié par les tests.
fn build_db(all: Vec<Entry>) -> Db {
    // Sépare apps (lookup) et addons (indexés par leur parent `extends`).
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
    // Un même suffixe (dernier segment d'app-id, ex. "player"/"editor") peut être
    // partagé par plusieurs apps distinctes dans le catalogue Flathub complet.
    // Matcher au hasard le 1er apparu serait un faux positif (mauvaise icône/
    // description affichée) — pire qu'une absence de correspondance. On retire
    // donc du HashMap tout suffixe rencontré plus d'une fois. Même politique
    // pour les NOMS : deux apps homonymes → aucune ne matche par nom.
    let mut ambiguous_suffixes: std::collections::HashSet<String> = std::collections::HashSet::new();
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

    Db { entries, by_suffix, by_name, by_id, addons, by_extends, by_developer }
}

// ── Parser XML ────────────────────────────────────────────────────────────────

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
    // Sans ça, quick-xml émet un unique `Event::Empty` (non géré par la boucle
    // ci-dessous, qui ne traite que Start/End/Text/Eof) pour les tags
    // auto-fermants (`<release version="…" timestamp="…" />`, très courants
    // dans le vrai appstream.xml.gz de Flathub pour les releases sans notes).
    // En demandant leur expansion, ils arrivent comme un Start+End normal et
    // retraversent exactement la même logique — zéro duplication de code.
    xml.config_mut().expand_empty_elements = true;
    let mut buf = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();

    // ── URL de base des médias (préfixe pour les URL relatives) ──────────────
    let mut media_base = String::new();

    // ── Profondeur & skip ─────────────────────────────────────────────────────
    let mut depth: usize = 0;
    // Quand > 0 : on est dans un sous-arbre à ignorer.
    // Chaque Start l'incrémente, chaque End le décrémente jusqu'à 0.
    let mut skip: usize = 0;

    // ── Entrée courante ───────────────────────────────────────────────────────
    let mut cur: Option<Entry> = None;
    let mut comp_depth: usize = 0;

    // ── Collecte de texte ─────────────────────────────────────────────────────
    let mut collect = Collect::None;

    // ── Description principale ────────────────────────────────────────────────
    let mut in_desc = false;
    let mut desc_depth: usize = 0;
    let mut desc_buf = String::new();

    // ── Description de release ────────────────────────────────────────────────
    let mut in_rel_desc = false;
    let mut rel_desc_depth: usize = 0;
    let mut rel_desc_buf = String::new();

    // ── Drapeaux structurels ──────────────────────────────────────────────────
    let mut in_screenshots = false;
    let mut in_screenshot = false;
    let mut in_releases = false;
    let mut in_release = false;
    let mut in_developer = false; // balise <developer> (format AppStream 1.0+)

    // ── Contexte de champ ─────────────────────────────────────────────────────
    let mut icon_remote = false;
    let mut icon_w: u32 = 0;
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
            // ── Ouverture ─────────────────────────────────────────────────────
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

                let has_lang =
                    e.attributes().flatten().any(|a| a.key.as_ref() == b"xml:lang");

                // Intérieur d'une description : on collecte le texte brut
                if in_desc && tag != "description" {
                    if has_lang {
                        skip = 1; // saute le paragraphe traduit
                    } else if tag == "li" {
                        desc_buf.push_str("• ");
                    }
                    // Sinon : on laisse le texte passer via Event::Text
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

                // ── Éléments racine / structurels ─────────────────────────────
                match tag {
                    "components" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"media_baseurl" {
                                media_base =
                                    String::from_utf8_lossy(&attr.value).into_owned();
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
                                comp_depth = depth;
                            }
                            _ => {
                                skip = 1; // ignore fonts, codecs, etc.
                            }
                        }
                    }

                    // ── Champs simples (seulement si dans un composant) ────────
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
                        skip = 1; // lang ou doublon
                    }

                    "icon" => {
                        let mut itype = String::new();
                        let mut w: u32 = 0;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"type" => {
                                    itype =
                                        String::from_utf8_lossy(&attr.value).into_owned()
                                }
                                b"width" => {
                                    w = String::from_utf8_lossy(&attr.value)
                                        .parse()
                                        .unwrap_or(0)
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
                            skip = 1; // on n'utilise pas les icônes cachées locales
                        }
                    }

                    // Ces balises peuvent contenir un <id> qui référence un AUTRE
                    // composant (ex. <recommends><id>com.valvesoftware.Steam</id>
                    // </recommends> dans la fiche Protontricks) : sans ce skip, ce
                    // <id> imbriqué écrase celui du composant en cours et corrompt
                    // son entrée (nom correct, app_id d'un autre paquet).
                    "requires" | "recommends" | "suggests" | "supports" => {
                        skip = 1;
                    }

                    "extends" if !has_lang => {
                        collect = Collect::Extends;
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
                                        String::from_utf8_lossy(&attr.value)
                                            .parse()
                                            .unwrap_or(0)
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

            // ── Fermeture ─────────────────────────────────────────────────────
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

                // Fin d'un enfant à l'intérieur d'une description
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
                        // Réinitialise tout l'état par composant
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

            // ── Texte ─────────────────────────────────────────────────────────
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
                        // `trim_end_matches` seul ne garantit PAS un unique '/' de
                        // séparation : concaténer "…/media" (trimmé) directement à
                        // "org/…" donnait "…/mediaorg/…" (bug vérifié par les tests
                        // parser_icon_prepends_media_baseurl_for_relative_url et
                        // parser_screenshots_prepend_media_baseurl).
                        format!(
                            "{}/{}",
                            media_base.trim_end_matches('/'),
                            s.trim_start_matches('/')
                        )
                    }
                };

                match collect {
                    // Seulement le PREMIER <id> (celui du composant) : un <id>
                    // imbriqué dans <provides> vaut « …Steam.desktop » et écraserait
                    // le vrai app_id sinon.
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
                        // Préfère 128×128 ; sinon prend la première disponible.
                        if icon_w == 128 || entry.icon_url.is_none() {
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
// On teste trois couches indépendantes :
//   1. `alnum()` — normalisation des noms
//   2. `parse()` — extraction XML (structure, traductions ignorées, médias, …)
//   3. `Db::lookup()` — stratégie de correspondance nom nix → entrée AppStream
//
// Le XML de test couvre tous les cas réels observés dans appstream.xml.gz Flathub.
#[cfg(test)]
mod tests {
    use super::*;

    // Un fichier AppStream minimal mais représentatif.
    // Couvre : media_baseurl, xml:lang, formats developer ancien/nouveau,
    // icône 64 vs 128, screenshots source vs thumbnail, releases, custom/verified,
    // types d'URL (homepage, bugtracker), addon et font ignorés.
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

    // ── Parser : structure ────────────────────────────────────────────────────

    #[test]
    fn parser_excludes_fonts_but_keeps_apps_and_addons() {
        // 3 apps desktop + 1 addon = 4 ; la font est exclue.
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
        let e = parse_sample().into_iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        assert!(e.oars.contains(&("social-chat".to_string(), "intense".to_string())));
        assert!(e.oars.contains(&("money-purchasing".to_string(), "intense".to_string())));
    }

    #[test]
    fn parser_captures_screenshot_caption_aligned() {
        let e = parse_sample().into_iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
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
        // gedit et gedit Plugins partagent « The GNOME Project ».
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
        let app = Entry { app_id: "org.gimp.GIMP".into(), name: "GIMP".into(), ..Entry::default() };
        let db = build_db(vec![app, addon("org.gimp.GIMP.Plugin.GMic"), addon("org.gimp.GIMP.Plugin.GMic3")]);
        assert_eq!(db.addons_for("org.gimp.GIMP").len(), 1);
    }

    // ── Parser : texte (traductions ignorées) ─────────────────────────────────

    #[test]
    fn parser_name_takes_en_only() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        assert_eq!(e.name, "gedit");
        assert_ne!(e.name, "gedit (FR)", "la version xml:lang ne doit pas écraser la EN");
    }

    #[test]
    fn parser_summary_takes_en_only() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        assert_eq!(e.summary.as_deref(), Some("A text editor"));
    }

    #[test]
    fn description_text_flows_like_html() {
        let xml = r#"<components><component type="desktop-application"><id>a.b</id><name>A</name>
<description><p>GIMP is
      an <em>image</em> editor.</p><ul><li>Fast</li><li>Free</li></ul></description>
</component></components>"#;
        let e = parse(std::io::Cursor::new(xml)).remove(0);
        assert_eq!(e.description.as_deref(), Some("GIMP is an image editor.\n• Fast\n• Free"));
    }

    #[test]
    fn parser_description_en_only() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        let desc = e.description.as_deref().unwrap_or("");
        assert!(desc.contains("gedit is a text editor"), "description EN absente");
        assert!(!desc.contains("éditeur"), "traduction FR ne doit pas apparaître dans la description");
    }

    #[test]
    fn parser_developer_old_format() {
        // <developer_name> = format AppStream 0.x
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        assert_eq!(e.developer.as_deref(), Some("The GNOME Project"));
    }

    #[test]
    fn parser_developer_new_format() {
        // <developer><name> = format AppStream 1.0+
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.name == "Extreme Tux Racer").unwrap();
        assert_eq!(e.developer.as_deref(), Some("ETR Team"));
    }

    #[test]
    fn parser_nested_id_in_recommends_does_not_corrupt_app_id() {
        // Régression réelle : la fiche Protontricks contient
        // <recommends><id>com.valvesoftware.Steam</id></recommends>. Sans le
        // skip sur ces balises, ce <id> imbriqué écrasait celui du composant,
        // et lookup("steam") renvoyait "Protontricks" sous l'app_id de Steam.
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
        let steam = db.lookup("steam").expect("steam doit être trouvé");
        assert_eq!(steam.name, "Steam", "lookup(\"steam\") ne doit pas renvoyer Protontricks");
    }

    #[test]
    fn parser_license() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        assert_eq!(e.license.as_deref(), Some("GPL-2.0+"));
    }

    #[test]
    fn parser_homepage_url() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        assert_eq!(e.homepage.as_deref(), Some("https://wiki.gnome.org/Apps/Gedit"));
    }

    #[test]
    fn parser_bugtracker_url() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        assert!(e.bugtracker.as_deref().unwrap_or("").contains("gitlab.gnome.org"));
    }

    // ── Parser : champ verified ───────────────────────────────────────────────

    #[test]
    fn parser_verified_true() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        assert!(e.verified);
    }

    #[test]
    fn parser_not_verified_by_default() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.name == "Extreme Tux Racer").unwrap();
        assert!(!e.verified, "sans clé verified la valeur doit être false");
    }

    // ── Parser : icônes ───────────────────────────────────────────────────────

    #[test]
    fn parser_prefers_128px_over_64px() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        let icon = e.icon_url.as_deref().expect("icône manquante");
        assert!(icon.contains("icon128"), "doit préférer icon128 sur icon64");
        assert!(!icon.contains("icon64"));
    }

    #[test]
    fn parser_icon_prepends_media_baseurl_for_relative_url() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        let icon = e.icon_url.as_deref().expect("icône manquante");
        assert!(
            icon.starts_with("https://dl.flathub.org/media/"),
            "URL relative doit être préfixée par media_baseurl, obtenu : {icon}"
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
            "une URL déjà absolue ne doit pas être doublée avec media_baseurl"
        );
    }

    // ── Parser : screenshots ──────────────────────────────────────────────────

    #[test]
    fn parser_screenshots_source_only_no_thumbnails() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        assert_eq!(e.screenshot_urls.len(), 2, "deux screenshots source attendus");
        for url in &e.screenshot_urls {
            assert!(!url.contains("thumb"), "les thumbnails ne doivent pas figurer");
        }
    }

    #[test]
    fn parser_screenshots_prepend_media_baseurl() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        for url in &e.screenshot_urls {
            assert!(
                url.starts_with("https://dl.flathub.org/media/"),
                "screenshot URL doit commencer par media_baseurl : {url}"
            );
        }
    }

    // ── Parser : releases ─────────────────────────────────────────────────────

    #[test]
    fn parser_releases_version_and_timestamp() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        assert_eq!(e.releases.len(), 2);
        assert_eq!(e.releases[0].version, "46.2");
        assert_eq!(e.releases[0].timestamp, 1712000000);
    }

    #[test]
    fn parser_release_description() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        let desc = e.releases[0].description.as_deref().unwrap_or("");
        assert!(desc.contains("Bug fixes"));
    }

    #[test]
    fn parser_release_without_description_is_none() {
        let entries = parse_sample();
        let e = entries.iter().find(|e| e.app_id == "org.gnome.Gedit").unwrap();
        assert_eq!(e.releases[1].version, "46.1");
        assert!(e.releases[1].description.is_none());
    }

    // ── Db::lookup() — stratégie de correspondance ────────────────────────────

    #[test]
    fn lookup_by_app_id_suffix() {
        // "gedit" = alnum(dernier segment de "org.gnome.Gedit")
        let db = make_db(parse_sample());
        let found = db.lookup("gedit");
        assert!(found.is_some(), "gedit doit être trouvé par suffix");
        assert_eq!(found.unwrap().app_id, "org.gnome.Gedit");
    }

    #[test]
    fn lookup_by_name_slug_extremetuxracer() {
        // Régression historique : suffix("io.github.extremetuxracer.ETR") = "etr"
        // ne correspond pas à "extremetuxracer", mais by_name le trouve via
        // alnum("Extreme Tux Racer") = "extremetuxracer".
        let db = make_db(parse_sample());
        let found = db.lookup("extremetuxracer");
        assert!(found.is_some(), "extremetuxracer doit être trouvé par nom");
        assert_eq!(found.unwrap().name, "Extreme Tux Racer");
    }

    #[test]
    fn lookup_by_full_app_id() {
        let db = make_db(parse_sample());
        let found = db.lookup("org.gnome.Gedit");
        assert!(found.is_some(), "lookup par app_id complet doit fonctionner");
    }

    #[test]
    fn lookup_by_full_app_id_case_insensitive() {
        let db = make_db(parse_sample());
        let found = db.lookup("org.gnome.gedit");
        assert!(found.is_some(), "by_id doit être insensible à la casse");
    }

    #[test]
    fn lookup_suffix_case_insensitive() {
        // "GEDIT" → alnum = "gedit" → trouvé par suffix
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
        // "etr" matche le suffix de "io.github.extremetuxracer.ETR"
        // ce test vérifie que by_suffix a priorité sur by_name
        let db = make_db(parse_sample());
        let found = db.lookup("etr");
        assert!(found.is_some(), "ETR doit être trouvé par suffix 'etr'");
        assert_eq!(found.unwrap().app_id, "io.github.extremetuxracer.ETR");
    }

    // ── Db::lookup — collision de suffixe (faux positif potentiel) ───────────

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
        // "Foo Player" et "Bar Player" partagent le suffixe d'app-id "player" :
        // une recherche par ce seul suffixe est ambiguë, on ne doit RIEN
        // retourner (icône générique) plutôt que la mauvaise app au hasard.
        let db = build_db(two_apps_sharing_suffix());
        assert!(
            db.lookup("player").is_none(),
            "un suffixe partagé par 2 apps ne doit matcher ni l'une ni l'autre"
        );
    }

    #[test]
    fn lookup_ambiguous_suffix_falls_back_to_full_name() {
        // Le nom complet de chaque app reste unique et donc trouvable via by_name,
        // même quand leur suffixe d'app-id commun a été neutralisé.
        let db = build_db(two_apps_sharing_suffix());
        assert_eq!(db.lookup("fooplayer").unwrap().app_id, "org.example.foo.Player");
        assert_eq!(db.lookup("barplayer").unwrap().app_id, "org.example.bar.Player");
    }

    // ── État de chargement : wait_until_settled / is_settled ─────────────────
    // Un seul test manipule STATE (statique process-global) pour éviter toute
    // course entre tests parallèles sur cet état partagé.
    #[test]
    fn wait_until_settled_blocks_only_while_loading() {
        use std::time::{Duration, Instant};

        // NotStarted (défaut, cas CLI qui n'appelle jamais init) : immédiat.
        let t0 = Instant::now();
        wait_until_settled(Duration::from_secs(5));
        assert!(t0.elapsed() < Duration::from_millis(500), "NotStarted ne doit pas attendre");

        // Loading : attend la fin du chargement (notifiée ici après ~100 ms),
        // PAS le timeout complet.
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
        assert!(dt >= Duration::from_millis(80), "doit attendre la fin du chargement");
        assert!(dt < Duration::from_secs(5), "ne doit pas attendre le timeout complet");
        assert!(is_settled());

        // Ready : immédiat.
        set_state(LoadState::Ready);
        let t0 = Instant::now();
        wait_until_settled(Duration::from_secs(5));
        assert!(t0.elapsed() < Duration::from_millis(500), "Ready ne doit pas attendre");
        assert!(is_settled());
    }

    #[test]
    fn lookup_ambiguous_name_matches_neither() {
        // Deux apps distinctes portant le MÊME nom : matcher la première apparue
        // serait un faux positif (même politique que les suffixes ambigus).
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
            "un nom partagé par 2 apps ne doit matcher ni l'une ni l'autre"
        );
        // Les app_id complets restent résolvables.
        assert!(db.lookup("org.example.AAA").is_some());
        assert!(db.lookup("org.example.BBB").is_some());
    }
}
