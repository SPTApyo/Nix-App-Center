//! Package model + taxonomy + action model. Pure: zero I/O.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Package {
    pub attr: String,
    pub name: String,
    pub version: String,
    pub description: String,
}

impl Package {
    /// A minimal package from its nixpkgs attribute.
    pub fn from_attr(attr: impl Into<String>) -> Self {
        let attr = attr.into();
        Self {
            name: attr.clone(),
            version: String::new(),
            description: String::new(),
            attr,
        }
    }

    /// Package set of the attr (`haskellPackages`), `None` at top level.
    pub fn package_set(&self) -> Option<&str> {
        self.attr.split_once('.').map(|(prefix, _)| prefix)
    }
}

/// Broad family of a package, for a fallback icon when AppStream has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Game,
    Audio,
    Video,
    Graphics,
    Office,
    Development,
    Terminal,
    Internet,
    Security,
    Science,
    Education,
    System,
    Font,
    Theme,
    Library,
    Generic,
}

/// Keywords per kind; the first match wins, specific words first.
const KIND_WORDS: &[(IconKind, &[&str])] = &[
    (
        IconKind::Game,
        &[
            "game",
            "games",
            "emulator",
            "puzzle",
            "arcade",
            "roguelike",
            "chess",
            "minecraft",
            "steam",
        ],
    ),
    (IconKind::Font, &["font", "fonts", "typeface", "glyphs"]),
    (
        IconKind::Theme,
        &[
            "theme",
            "themes",
            "icon theme",
            "cursor theme",
            "wallpaper",
            "wallpapers",
        ],
    ),
    (
        IconKind::Security,
        &[
            "password",
            "encryption",
            "encrypt",
            "security",
            "firewall",
            "vulnerability",
            "pentest",
            "gpg",
            "keyring",
            "2fa",
        ],
    ),
    (
        IconKind::Audio,
        &[
            "audio",
            "music",
            "sound",
            "synthesizer",
            "podcast",
            "mp3",
            "radio",
            "midi",
            "pulseaudio",
            "pipewire",
        ],
    ),
    (
        IconKind::Video,
        &[
            "video",
            "videos",
            "movie",
            "movies",
            "media player",
            "streaming",
            "webcam",
            "screencast",
            "youtube",
        ],
    ),
    (
        IconKind::Graphics,
        &[
            "image", "images", "photo", "photos", "graphics", "drawing", "paint", "svg", "3d",
            "pixel", "raster", "vector", "cad",
        ],
    ),
    (
        IconKind::Office,
        &[
            "pdf",
            "document",
            "documents",
            "office",
            "spreadsheet",
            "word processor",
            "notes",
            "note-taking",
            "ebook",
            "calendar",
            "presentation",
        ],
    ),
    (
        IconKind::Internet,
        &[
            "browser",
            "web",
            "email",
            "mail",
            "chat",
            "messenger",
            "irc",
            "matrix",
            "xmpp",
            "vpn",
            "torrent",
            "download",
            "downloader",
            "network",
            "http",
            "ssh",
            "dns",
            "proxy",
        ],
    ),
    (
        IconKind::Development,
        &[
            "compiler",
            "programming",
            "ide",
            "debugger",
            "language server",
            "sdk",
            "build system",
            "version control",
            "git",
            "linter",
            "formatter",
            "interpreter",
            "framework",
            "api",
        ],
    ),
    (
        IconKind::Science,
        &[
            "science",
            "scientific",
            "math",
            "mathematics",
            "chemistry",
            "physics",
            "astronomy",
            "statistics",
            "biology",
            "simulation",
        ],
    ),
    (
        IconKind::Education,
        &[
            "education",
            "educational",
            "learn",
            "learning",
            "dictionary",
            "language learning",
            "typing",
        ],
    ),
    (
        IconKind::Terminal,
        &[
            "command-line",
            "command line",
            "cli",
            "terminal",
            "tui",
            "shell",
        ],
    ),
    (
        IconKind::System,
        &[
            "system",
            "disk",
            "monitor",
            "monitoring",
            "kernel",
            "driver",
            "hardware",
            "backup",
            "filesystem",
            "boot",
            "usb",
            "bluetooth",
            "firmware",
            "partition",
        ],
    ),
    (
        IconKind::Library,
        &["library", "bindings", "module", "plugin", "extension"],
    ),
];

/// Prefixes of package sets that only hold libraries (`python314Packages`).
const LIBRARY_SETS: &[&str] = &[
    "python",
    "haskellPackages",
    "perlPackages",
    "nodePackages",
    "rubyPackages",
    "lua",
    "ocamlPackages",
    "rPackages",
    "coqPackages",
    "lispPackages",
    "emacsPackages",
    "vimPlugins",
    "texlivePackages",
    "php",
    "beamPackages",
    "chickenPackages",
    "idrisPackages",
    "akkuPackages",
];

impl Package {
    /// Rough family of the package, to pick a fallback icon.
    pub fn icon_kind(&self) -> IconKind {
        if self
            .package_set()
            .is_some_and(|set| LIBRARY_SETS.iter().any(|p| set.starts_with(p)))
        {
            return IconKind::Library;
        }
        let attr = self.attr.to_lowercase();
        if attr.contains("font") || attr.ends_with("-fonts") {
            return IconKind::Font;
        }
        let text = format!(" {} ", normalize(&self.description));
        KIND_WORDS
            .iter()
            .find(|(_, words)| words.iter().any(|w| text.contains(&format!(" {w} "))))
            .map(|(kind, _)| *kind)
            .unwrap_or(IconKind::Generic)
    }
}

/// Lowercase words separated by single spaces (dashes kept).
fn normalize(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Kind of package, which decides its primary action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Runnable application (has a binary / `meta.mainProgram`): *try-first*.
    App,
    /// Extension, theme, library: *keep-only* (no `nix run`).
    KeepOnly,
}

impl Package {
    /// Kind of the package. Every package is an `App` for now.
    pub fn kind(&self) -> Kind {
        Kind::App
    }

    pub fn is_runnable(&self) -> bool {
        self.kind() == Kind::App
    }
}

/// An action offered for a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Run without installing: `nix run nixpkgs#<attr>`. The headline action.
    Run,
    /// Open a terminal with the package in `PATH`: the try path for CLI tools.
    Shell,
    /// Add to the manifest, so to the generated Nix files.
    Keep,
    /// Copy the `nix run` command (the front end owns the clipboard).
    CopyRunCommand,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::Run => "Run",
            Action::Shell => "Try in a Shell",
            Action::Keep => "Add to Configuration",
            Action::CopyRunCommand => "Copy Command",
        }
    }
}

/// A package's actions: 1 primary + N secondary.
pub struct Actions {
    pub primary: Action,
    pub secondary: Vec<Action>,
}

/// The primary and secondary actions of a package.
pub fn actions_for(pkg: &Package) -> Actions {
    if pkg.is_runnable() {
        Actions {
            primary: Action::Run,
            secondary: vec![Action::Keep, Action::Shell, Action::CopyRunCommand],
        }
    } else {
        Actions {
            primary: Action::Keep,
            secondary: vec![Action::Shell, Action::CopyRunCommand],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(attr: &str, description: &str) -> IconKind {
        Package {
            description: description.into(),
            ..Package::from_attr(attr)
        }
        .icon_kind()
    }

    #[test]
    fn icon_kind_from_description_and_attr() {
        assert_eq!(
            kind("supertux", "Classic 2D jump'n run sidescroller game"),
            IconKind::Game
        );
        assert_eq!(
            kind("ripgrep", "Command-line search tool, like grep"),
            IconKind::Terminal
        );
        assert_eq!(
            kind("python3Packages.requests", "HTTP library for Python"),
            IconKind::Library
        );
        assert_eq!(
            kind("noto-fonts", "Beautiful and free fonts"),
            IconKind::Font
        );
        assert_eq!(
            kind("audacity", "Sound editor with graphical UI"),
            IconKind::Audio
        );
        assert_eq!(
            kind(
                "hello",
                "Program that produces a familiar, friendly greeting"
            ),
            IconKind::Generic
        );
    }

    #[test]
    fn package_set_returns_prefix_for_dotted_attr() {
        assert_eq!(
            Package::from_attr("haskellPackages.aeson").package_set(),
            Some("haskellPackages")
        );
    }

    #[test]
    fn package_set_none_for_top_level_attr() {
        assert_eq!(Package::from_attr("firefox").package_set(), None);
    }

    #[test]
    fn default_kind_is_app_and_runnable() {
        let pkg = Package::from_attr("ripgrep");
        assert_eq!(pkg.kind(), Kind::App);
        assert!(pkg.is_runnable());
    }

    #[test]
    fn actions_for_runnable_has_run_primary() {
        let actions = actions_for(&Package::from_attr("ripgrep"));
        assert_eq!(actions.primary, Action::Run);
        assert_eq!(
            actions.secondary,
            vec![Action::Keep, Action::Shell, Action::CopyRunCommand]
        );
    }
}
