use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

/// Validates that a file looks like a Minecraft mod JAR.
pub fn validate_mod_jar(path: &Path) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::InvalidMod {
            path: path.to_path_buf(),
            reason: "missing file name".into(),
        })?;

    if file_name.starts_with('.') {
        return Err(AppError::InvalidMod {
            path: path.to_path_buf(),
            reason: "hidden files are not allowed".into(),
        });
    }

    let lower = file_name.to_ascii_lowercase();
    if !lower.ends_with(".jar") {
        return Err(AppError::InvalidMod {
            path: path.to_path_buf(),
            reason: "mods must be .jar files".into(),
        });
    }

    if lower.ends_with("-sources.jar") || lower.ends_with("-dev.jar") {
        return Err(AppError::InvalidMod {
            path: path.to_path_buf(),
            reason: "source/dev jars are not runtime mods".into(),
        });
    }

    Ok(())
}

/// Handles a single directory entry while scanning for mod JARs.
pub fn handle_dir_entry(dir: &Path, entry: io::Result<fs::DirEntry>) -> Result<Option<PathBuf>> {
    let entry = entry.map_err(|e| AppError::Io {
        path: dir.to_path_buf(),
        message: e.to_string(),
    })?;
    let path = entry.path();
    if path.is_file() {
        validate_mod_jar(&path)?;
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

/// Scans a directory and validates every mod JAR found.
pub fn validate_mods_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(dir).map_err(|e| AppError::Io {
        path: dir.to_path_buf(),
        message: e.to_string(),
    })?;

    let mut mods = Vec::new();
    for entry in entries {
        if let Some(path) = handle_dir_entry(dir, entry)? {
            mods.push(path);
        }
    }

    mods.sort();
    Ok(mods)
}

/// Returns the container mount path for mods based on loader type.
pub fn mods_mount_path() -> &'static str {
    "/data/mods"
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn accepts_valid_jar() {
        validate_mod_jar(Path::new("jei-1.20.1.jar")).unwrap();
    }

    #[test]
    fn rejects_non_jar() {
        let err = validate_mod_jar(Path::new("readme.txt")).unwrap_err();
        assert!(matches!(err, AppError::InvalidMod { .. }));
    }

    #[test]
    fn rejects_sources_jar() {
        let err = validate_mod_jar(Path::new("mod-1.0-sources.jar")).unwrap_err();
        assert!(err.to_string().contains("source/dev"));
    }

    #[test]
    fn rejects_hidden_file() {
        let err = validate_mod_jar(Path::new(".secret.jar")).unwrap_err();
        assert!(err.to_string().contains("hidden"));
    }

    #[test]
    fn validate_mods_dir_empty_missing() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope");
        assert!(validate_mods_dir(&missing).unwrap().is_empty());
    }

    #[test]
    fn validate_mods_dir_collects_jars() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.jar"), b"x").unwrap();
        std::fs::write(dir.path().join("b.jar"), b"y").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"z").unwrap();

        let err = validate_mods_dir(dir.path()).unwrap_err();
        assert!(matches!(err, AppError::InvalidMod { .. }));

        std::fs::remove_file(dir.path().join("notes.txt")).unwrap();
        let mods = validate_mods_dir(dir.path()).unwrap();
        assert_eq!(mods.len(), 2);
    }

    #[test]
    fn mods_mount_path_constant() {
        assert_eq!(mods_mount_path(), "/data/mods");
    }

    #[test]
    fn rejects_path_without_file_name() {
        let err = validate_mod_jar(Path::new("")).unwrap_err();
        assert!(err.to_string().contains("missing file name"));
    }

    #[test]
    fn handle_dir_entry_io_error() {
        let dir = TempDir::new().unwrap();
        let err = handle_dir_entry(dir.path(), Err(io::Error::other("boom"))).unwrap_err();
        assert!(matches!(err, AppError::Io { .. }));
    }

    #[test]
    fn handle_dir_entry_skips_directories() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        let mods = validate_mods_dir(dir.path()).unwrap();
        assert!(mods.is_empty());
    }

    #[test]
    fn validate_mods_dir_read_errors() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        assert!(validate_mods_dir(&file).is_err());
    }
}
