//! The logs of past applies, updates and other tasks, so a failure can be
//! read again later. The newest twenty are kept.

use std::path::{Path, PathBuf};

const KEPT: usize = 20;

/// A saved log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLog {
    pub path: PathBuf,
    /// When it ended (Unix seconds).
    pub at: u64,
    /// Its first line: the task and how it ended.
    pub title: String,
}

fn dir() -> PathBuf {
    crate::paths::data_dir().join("logs")
}

/// Saves a log whose first line is `title`, then drops the oldest beyond
/// twenty.
pub fn save_task_log(title: &str, body: &str) -> std::io::Result<()> {
    save_in(&dir(), crate::now_unix(), title, body)
}

fn save_in(dir: &Path, at: u64, title: &str, body: &str) -> std::io::Result<()> {
    let mut n = 0;
    let path = loop {
        let p = dir.join(format!("{at}-{n}.log"));
        if !p.exists() {
            break p;
        }
        n += 1;
    };
    let first = title.lines().next().unwrap_or_default();
    crate::paths::write_atomic(&path, format!("{first}\n{body}").as_bytes())?;
    for old in logs_in(dir).into_iter().skip(KEPT) {
        let _ = std::fs::remove_file(old.path);
    }
    Ok(())
}

/// The saved logs, newest first.
pub fn task_logs() -> Vec<TaskLog> {
    logs_in(&dir())
}

fn logs_in(dir: &Path) -> Vec<TaskLog> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(u64, u64, TaskLog)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?.to_string();
            let (at, n) = stem.split_once('-')?;
            let (at, n) = (at.parse().ok()?, n.parse().ok()?);
            let title = std::fs::read_to_string(&path)
                .ok()?
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            Some((at, n, TaskLog { path, at, title }))
        })
        .collect();
    out.sort_by_key(|(at, n, _)| std::cmp::Reverse((*at, *n)));
    out.into_iter().map(|(_, _, log)| log).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_twenty_newest_first() {
        let dir = std::env::temp_dir().join(format!("nix-app-center-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for i in 0..25u64 {
            save_in(&dir, 1000 + i, &format!("Updating · task {i}"), "line\n").unwrap();
        }
        save_in(&dir, 1024, "Same second", "x").unwrap();
        let logs = logs_in(&dir);
        assert_eq!(logs.len(), KEPT);
        assert_eq!(logs[0].title, "Same second");
        assert_eq!(logs[1].title, "Updating · task 24");
        assert_eq!(logs[0].at, 1024);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
