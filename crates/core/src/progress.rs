//! Progress of a Nix command run with `--log-format internal-json`: counts of
//! copied paths and built derivations, and the readable lines for the log.

use std::collections::HashMap;

/// Activity types of the internal-json protocol.
const COPY_PATHS: u64 = 103;
const BUILDS: u64 = 104;
/// Result types.
const PROGRESS: u64 = 105;
const BUILD_LOG_LINE: u64 = 101;

/// What the command is doing now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Downloading { done: u64, expected: u64 },
    Building { done: u64, expected: u64 },
    Activating,
}

/// What a running command reports.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// A readable log line.
    Line(String),
    /// Share done (`None` while unknown) and phase.
    Progress(Option<f64>, Phase),
}

#[derive(Debug, Default)]
pub struct Tracker {
    kinds: HashMap<u64, u64>,
    /// (done, expected) per activity id.
    counts: HashMap<u64, (u64, u64)>,
    copy: (u64, u64),
    build: (u64, u64),
    activating: bool,
    error: Option<String>,
    /// (done, expected) read from plain text logs, for commands without JSON.
    text_copy: (u64, u64),
    text_build: (u64, u64),
}

/// The count of "these 12 paths will be fetched" or "this path will be…".
fn announced(line: &str) -> Option<u64> {
    if line.starts_with("this ") {
        return Some(1);
    }
    line.strip_prefix("these ")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// `text` without terminal color codes.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl Tracker {
    /// Reads one output line; returns the text it adds to the log, if any.
    pub fn feed(&mut self, line: &str) -> Option<String> {
        let Some(json) = line.strip_prefix("@nix ") else {
            self.read_text(line);
            return Some(line.to_string());
        };
        let v: serde_json::Value = serde_json::from_str(json).ok()?;
        let id = v["id"].as_u64().unwrap_or_default();
        match v["action"].as_str()? {
            "start" => {
                self.kinds
                    .insert(id, v["type"].as_u64().unwrap_or_default());
                let text = v["text"].as_str().filter(|t| !t.is_empty())?;
                (v["level"].as_u64().unwrap_or(9) <= 3).then(|| plain(text))
            }
            "msg" => {
                let text = plain(v["msg"].as_str()?);
                let level = v["level"].as_u64().unwrap_or(9);
                if level == 0 {
                    let last = text
                        .lines()
                        .map(str::trim)
                        .rfind(|l| l.starts_with("error:"));
                    if let Some(line) = last {
                        self.error = Some(line.to_string());
                    }
                }
                (level <= 3).then_some(text)
            }
            "result" => {
                let fields = v["fields"].as_array()?;
                match v["type"].as_u64()? {
                    PROGRESS => {
                        let n = |i: usize| {
                            fields
                                .get(i)
                                .and_then(serde_json::Value::as_u64)
                                .unwrap_or(0)
                        };
                        let kind = self.kinds.get(&id).copied();
                        if matches!(kind, Some(COPY_PATHS | BUILDS)) {
                            self.counts.insert(id, (n(0), n(1)));
                            let total = |k: u64| {
                                self.counts
                                    .iter()
                                    .filter(|(i, _)| self.kinds.get(i) == Some(&k))
                                    .fold((0, 0), |a, (_, c)| (a.0 + c.0, a.1 + c.1))
                            };
                            self.copy = total(COPY_PATHS);
                            self.build = total(BUILDS);
                        }
                        None
                    }
                    BUILD_LOG_LINE => fields.first()?.as_str().map(plain),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Counts what plain Nix logs announce and report.
    fn read_text(&mut self, line: &str) {
        let text = plain(line);
        let l = text.trim();
        if l.contains("activating the configuration") {
            self.activating = true;
        } else if l.contains("will be fetched") {
            self.text_copy.1 += announced(l).unwrap_or(0);
        } else if l.contains("will be built") {
            self.text_build.1 += announced(l).unwrap_or(0);
        } else if l.starts_with("copying path '") {
            self.text_copy.0 += 1;
        } else if l.starts_with("building '") {
            self.text_build.0 += 1;
        } else if l.starts_with("error:") {
            self.error = Some(l.to_string());
        }
    }

    /// The counts of JSON events, else those of plain text logs.
    fn counts(&self) -> ((u64, u64), (u64, u64)) {
        if self.copy.1 + self.build.1 > 0 {
            (self.copy, self.build)
        } else {
            let cap = |(d, e): (u64, u64)| (d.min(e), e);
            (cap(self.text_copy), cap(self.text_build))
        }
    }

    /// The last error Nix reported, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Share done, from 0 to 1; `None` while nothing is counted yet.
    pub fn fraction(&self) -> Option<f64> {
        if self.activating {
            return Some(1.0);
        }
        let (copy, build) = self.counts();
        let expected = copy.1 + build.1;
        (expected > 0).then(|| (copy.0 + build.0) as f64 / expected as f64)
    }

    pub fn phase(&self) -> Phase {
        let (copy, build) = self.counts();
        if self.activating {
            Phase::Activating
        } else if build.1 > build.0 && copy.0 >= copy.1 {
            Phase::Building {
                done: build.0,
                expected: build.1,
            }
        } else if copy.1 > 0 {
            Phase::Downloading {
                done: copy.0,
                expected: copy.1,
            }
        } else if build.1 > 0 {
            Phase::Building {
                done: build.0,
                expected: build.1,
            }
        } else {
            Phase::Starting
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_downloads_and_builds() {
        let mut t = Tracker::default();
        assert_eq!(t.fraction(), None);
        t.feed(r#"@nix {"action":"start","id":1,"level":0,"parent":0,"text":"","type":103}"#);
        t.feed(r#"@nix {"action":"start","id":2,"level":0,"parent":0,"text":"","type":104}"#);
        t.feed(r#"@nix {"action":"result","fields":[1,4,0,0],"id":1,"type":105}"#);
        t.feed(r#"@nix {"action":"result","fields":[0,1,0,0],"id":2,"type":105}"#);
        assert_eq!(t.fraction(), Some(0.2));
        assert_eq!(
            t.phase(),
            Phase::Downloading {
                done: 1,
                expected: 4
            }
        );
        t.feed(r#"@nix {"action":"result","fields":[4,4,0,0],"id":1,"type":105}"#);
        assert_eq!(
            t.phase(),
            Phase::Building {
                done: 0,
                expected: 1
            }
        );
        assert_eq!(
            t.feed("activating the configuration..."),
            Some("activating the configuration...".into())
        );
        assert_eq!(t.phase(), Phase::Activating);
        assert_eq!(t.fraction(), Some(1.0));
    }

    #[test]
    fn keeps_the_real_error_of_a_trace() {
        let mut t = Tracker::default();
        let msg = r#"@nix {"action":"msg","level":0,"msg":"\u001b[31;1merror:\u001b[0m\n … while calling\n\n       error: The option `my.ai.models' does not exist."}"#;
        t.feed(msg);
        assert_eq!(
            t.error(),
            Some("error: The option `my.ai.models' does not exist.")
        );
    }

    #[test]
    fn counts_plain_text_logs_too() {
        let mut t = Tracker::default();
        t.feed("these 3 paths will be fetched (1.2 MiB download, 4 MiB unpacked):");
        t.feed("this derivation will be built:");
        t.feed("copying path '/nix/store/a' from 'https://cache.nixos.org'...");
        assert_eq!(t.fraction(), Some(0.25));
        assert_eq!(
            t.phase(),
            Phase::Downloading {
                done: 1,
                expected: 3
            }
        );
        t.feed("copying path '/nix/store/b' from 'https://cache.nixos.org'...");
        t.feed("copying path '/nix/store/c' from 'https://cache.nixos.org'...");
        t.feed("building '/nix/store/x.drv'...");
        assert_eq!(t.fraction(), Some(1.0));
        t.feed("error: builder for '/nix/store/x.drv' failed");
        assert_eq!(
            t.error(),
            Some("error: builder for '/nix/store/x.drv' failed")
        );
    }

    #[test]
    fn keeps_readable_lines_for_the_log() {
        let mut t = Tracker::default();
        let msg = r#"@nix {"action":"msg","level":3,"msg":"these 2 paths will be fetched"}"#;
        assert_eq!(
            t.feed(msg).as_deref(),
            Some("these 2 paths will be fetched")
        );
        let noisy = r#"@nix {"action":"msg","level":6,"msg":"debug"}"#;
        assert_eq!(t.feed(noisy), None);
        let log = r#"@nix {"action":"result","fields":["make: done"],"id":7,"type":101}"#;
        assert_eq!(t.feed(log).as_deref(), Some("make: done"));
        assert_eq!(
            t.feed("building the system configuration...").as_deref(),
            Some("building the system configuration...")
        );
    }
}
