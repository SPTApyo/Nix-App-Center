//! Répertoire de données Onyx (`$XDG_DATA_HOME/onyx`), partagé par `manifest`
//! (`kept.json`) et `tried` (`tried.json`).

use std::path::PathBuf;

pub(crate) fn data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".local/share")
        });
    base.join("onyx")
}

/// Écriture atomique : fichier temporaire voisin puis `rename` (atomique sur un
/// même système de fichiers). Un crash mi-écriture laissait sinon un JSON
/// tronqué, que `unwrap_or_default()` remplaçait silencieusement par un état
/// vide au chargement suivant — perte du manifeste/journal.
pub(crate) fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("onyx"),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_content_and_leaves_no_tmp() {
        // Pas d'env muté → pas besoin d'env_lock.
        let dir = std::env::temp_dir().join(format!("onyx-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("kept.json");

        write_atomic(&path, b"un").unwrap();
        write_atomic(&path, b"deux").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"deux");

        let tmp_left = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains(".tmp"));
        assert!(!tmp_left, "aucun fichier temporaire ne doit rester");
    }
}
