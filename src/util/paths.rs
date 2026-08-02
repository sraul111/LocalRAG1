//! Filesystem-path helpers. All paths are absolute, canonicalized when
//! possible. Windows backslashes and Unix slashes are normalized via
//! [`std::path::Path`] so callers don't have to care.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Name of the per-folder data directory. Hidden on Unix, dot-prefixed everywhere.
pub const DATA_DIR_NAME: &str = ".localrag1";

/// Name of the SQLite index file inside the data dir.
pub const INDEX_FILE: &str = "index.sqlite3";

/// Name of the human-editable config file inside the data dir.
pub const CONFIG_FILE: &str = "config.toml";

/// Locate the `.localrag1/` directory.
///
/// The strategy:
/// 1. If a path is given, use it.
/// 2. Otherwise, search from `cwd` upward — the FIRST directory that
///    contains `.localrag1/` is the project root. This matches the
///    intuition that `sl` should "just work" in nested folders of an
///    initialized project.
/// 3. If no such directory is found, return [`Error::NotInitialized`].
pub fn find_data_dir(provided: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = provided {
        let abs = absolutize(p)?;
        if abs.is_dir() {
            return Ok(abs);
        }
        return Err(Error::NotInitialized(p.to_path_buf()));
    }

    let cwd = std::env::current_dir()
        .map_err(|e| Error::Io { operation: "current_dir", source: e })?;

    let mut here: &Path = &cwd;
    loop {
        let candidate = here.join(DATA_DIR_NAME);
        if candidate.is_dir() {
            return Ok(candidate);
        }
        // Stop at filesystem root.
        match here.parent() {
            Some(p) if p != here => here = p,
            _ => return Err(Error::NotInitialized(cwd)),
        }
    }
}

/// Resolve a `.localrag1/` dir at `cwd` (creating it), or under `provided`
/// if given. `provided` is the project root — the `.localrag1/` dir is
/// placed inside it.
pub fn ensure_data_dir(provided: Option<&Path>) -> Result<PathBuf> {
    let root = if let Some(p) = provided {
        absolutize(p)?
    } else {
        std::env::current_dir()
            .map_err(|e| Error::Io { operation: "current_dir", source: e })?
    };
    let dir = root.join(DATA_DIR_NAME);
    std::fs::create_dir_all(&dir)
        .map_err(|e| Error::Io { operation: "create_dir_all", source: e })?;
    Ok(dir)
}

/// Absolute path to the SQLite index file inside a data dir.
pub fn index_path(data_dir: &Path) -> PathBuf {
    data_dir.join(INDEX_FILE)
}

/// Absolute path to the config file inside a data dir.
pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CONFIG_FILE)
}

/// Convert any path to an absolute, canonicalized form.
///
/// We use `canonicalize` when the path exists so symlinks are resolved,
/// falling back to `current_dir() + path` for not-yet-existing paths.
pub fn absolutize(p: &Path) -> Result<PathBuf> {
    if p.exists() {
        return p.canonicalize().map_err(|e| Error::Io {
            operation: "canonicalize",
            source: e,
        });
    }
    let cwd = std::env::current_dir()
        .map_err(|e| Error::Io { operation: "current_dir", source: e })?;
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    Ok(joined)
}

/// Compute the path *relative to* the project root for display purposes.
/// Falls back to the absolute path if conversion fails.
pub fn to_relative_or_abs(root: &Path, absolute: &Path) -> String {
    match absolute.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => absolute.to_string_lossy().into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn ensure_data_dir_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a/b/c");
        fs::create_dir_all(&nested).unwrap();

        let created = ensure_data_dir(Some(&nested)).unwrap();
        assert!(created.is_dir());
        // exactly `.localrag1` inside the given path
        assert_eq!(created.file_name().unwrap(), DATA_DIR_NAME);
    }

    #[test]
    fn find_data_dir_walks_upward() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let deep = root.join("a/b/c");
        fs::create_dir_all(&deep).unwrap();
        fs::create_dir_all(root.join(DATA_DIR_NAME)).unwrap();

        // Simulate being inside `<root>/a/b/c` and looking for `.localrag1`.
        let mut here: &Path = &deep;
        let mut found = None;
        loop {
            if here.join(DATA_DIR_NAME).is_dir() {
                found = Some(here.join(DATA_DIR_NAME));
                break;
            }
            match here.parent() {
                Some(p) if p != here => here = p,
                _ => break,
            }
        }
        assert!(found.is_some());
        assert_eq!(
            found.unwrap().canonicalize().unwrap(),
            root.join(DATA_DIR_NAME).canonicalize().unwrap()
        );
    }

    #[test]
    fn to_relative_falls_back_to_absolute() {
        let root = Path::new("/a/b");
        let out_of_tree = Path::new("/x/y/z");
        let rel = to_relative_or_abs(root, out_of_tree);
        assert_eq!(rel, "/x/y/z");
    }

    #[test]
    fn to_relative_strips_prefix() {
        let root = Path::new("/a/b");
        let inside = Path::new("/a/b/c/d.txt");
        let rel = to_relative_or_abs(root, inside);
        assert_eq!(rel, "c/d.txt");
    }
}
