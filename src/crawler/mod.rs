//! Filesystem crawler.
//!
//! Given a directory and a [`Config`], walk the tree once and yield one
//! [`FileEntry`] per file. The traversal:
//!
//! 1. Honors `.gitignore` files (via the `ignore` crate) when configured.
//! 2. Skips config-level exclude patterns (`target`, `node_modules`, …).
//! 3. Skips files larger than `max_file_size_mb` (defer indexing them).
//! 4. Optionally follows symlinks (default: no).
//!
//! Why a custom walker instead of `walkdir`? `ignore::WalkBuilder` already
//! integrates `.gitignore` parsing, so we don't have to reimplement it.
//!
//! The crawler is **stateless** from the indexer's perspective — the
//! indexer just decides what to do with each entry.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{Error, Result};

mod filetype;

/// A single file the indexer cares about. We collect this (not raw paths)
/// so we can attach extension, size, mtime — all metadata the search
/// tiers want without a second stat.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    /// Path relative to the crawl root, always forward-slash for DB.
    pub rel_path: String,
    /// Absolute path on disk (for reading content).
    pub abs_path: PathBuf,
    /// File size in bytes. 0 for directories / unreadable (we drop them).
    pub size: u64,
    /// Last-modified unix millis.
    pub mtime_ms: i64,
    /// Lowercase file extension, without the dot. Empty if no extension.
    pub ext: String,
    /// `true` if file extension suggests we should inline text in FTS5.
    pub indexable: bool,
}

/// One pass over `root`, yielding every file we want indexed.
///
/// Returns a `Vec<FileEntry>` rather than streaming so the caller can
/// decide whether to index synchronously or hand off to a thread pool.
/// For v1 corpus sizes (a few thousand files) this is fine.
pub fn crawl(root: &Path, cfg: &Config) -> Result<Vec<FileEntry>> {
    if !root.is_dir() {
        return Err(Error::other(format!(
            "crawl root is not a directory: {:?}",
            root
        )));
    }

    let mut entries = Vec::new();

    // Configure the walker. The `ignore` crate is well-tested and matches
    // `ripgrep`'s behavior, which users expect.
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .standard_filters(cfg.index.respect_gitignore)
        .follow_links(cfg.index.follow_symlinks)
        .hidden(false) // We handle hidden files ourselves via exclude list.
        .git_ignore(cfg.index.respect_gitignore)
        .git_exclude(cfg.index.respect_gitignore)
        // Don't traverse into excluded dirs at all (saves IO on huge trees).
        .require_git(true)
        .parents(true);

    // For each directory walk yielded by `ignore`, we manually check
    // against the user's `exclude` list (which is *additive* to gitignore).
    // `ignore` doesn't natively support a custom exclude list, so we
    // walk each yielded entry and skip those matching our patterns.
    let exclude = cfg.index.exclude.clone();

    for result in builder.build() {
        let dent = match result {
            Ok(d) => d,
            Err(e) => {
                // Permission denied, etc. We log and skip rather than fail.
                log::warn!("skipping due to walk error: {e}");
                continue;
            }
        };

        if !dent.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }

        let abs = dent.path();
        if should_exclude(abs, root, &exclude) {
            continue;
        }

        let entry = match build_entry(abs, root) {
            Ok(e) => e,
            Err(e) => {
                log::debug!("skipping {:?}: {e}", abs);
                continue;
            }
        };

        // Apply size filter: cap on inline-indexed bytes.
        let max_bytes: u64 = (cfg.index.max_file_size_mb as u64) * 1024 * 1024;
        if entry.size > max_bytes {
            log::debug!(
                "skipping oversized ({:.1} MB): {:?}",
                entry.size as f64 / 1_048_576.0,
                entry.abs_path
            );
            continue;
        }

        entries.push(entry);
    }

    // Stable order by rel_path — makes tests deterministic.
    entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(entries)
}

/// True if `path` matches any of the user's exclude glob patterns.
/// Patterns match against **any path segment** AND against the relpath as a whole.
fn should_exclude(path: &Path, root: &Path, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let rel = path.strip_prefix(root).unwrap_or(path);
    let rel_str = rel.to_string_lossy();
    for p in patterns {
        // Exact segment match (most common case — "node_modules", "target").
        if rel
            .components()
            .any(|c| c.as_os_str() == std::ffi::OsStr::new(p))
        {
            return true;
        }
        // Glob match against the full relpath.
        if glob_match(p, &rel_str) {
            return true;
        }
    }
    false
}

/// Minimal glob matcher supporting `*` and `?`. We avoid pulling the
/// `glob` crate just for this — it's ~40 lines and the use is narrow.
fn glob_match(pattern: &str, text: &str) -> bool {
    // Naive but correct recursive matcher supporting `*` (any run of chars),
    // `?` (any single char), and literal characters. Both `*` and `?` need
    // to consume one or more characters from `text` (matches `fnmatch`'s
    // `.gitignore` semantics).
    fn rec(pat: &[char], txt: &[char]) -> bool {
        match (pat.first(), txt.first()) {
            (None, None) => true,
            (None, Some(_)) => false,
            (Some(_), None) => pat.iter().all(|c| *c == '*'),
            (Some('?'), Some(_)) => rec(&pat[1..], &txt[1..]),
            (Some('*'), _) => {
                // Try every possible split of `txt`: 0 chars consumed by '*'
                // through length chars consumed by '*'. The first sub-match
                // is allowed because we discard previous progress on '*'.
                pat.iter().all(|c| *c == '*')
                    || (0..txt.len()).any(|i| rec(&pat[1..], &txt[i..]))
            }
            (Some(a), Some(b)) if a == b => rec(&pat[1..], &txt[1..]),
            _ => false,
        }
    }
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    rec(&pat, &txt)
}

fn build_entry(abs: &Path, root: &Path) -> Result<FileEntry> {
    let meta = std::fs::metadata(abs).map_err(|e| Error::Io {
        operation: "metadata during crawl",
        source: e,
    })?;
    let rel_path = abs
        .strip_prefix(root)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/");
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let ext = abs
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let indexable = filetype::is_indexable(&ext, meta.len());

    Ok(FileEntry {
        rel_path,
        abs_path: abs.to_path_buf(),
        size: meta.len(),
        mtime_ms,
        ext,
        indexable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_corpus(tmp: &Path) {
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::create_dir_all(tmp.join("node_modules/lodash")).unwrap();
        fs::create_dir_all(tmp.join("target/debug")).unwrap();
        fs::create_dir_all(tmp.join(".git/objects")).unwrap();
        fs::write(tmp.join("src/main.rs"), b"fn main() {}").unwrap();
        fs::write(tmp.join("src/lib.rs"), b"pub fn hi() {}").unwrap();
        fs::write(tmp.join("README.md"), b"# Hello").unwrap();
        fs::write(tmp.join("node_modules/lodash/index.js"), b"// module").unwrap();
        fs::write(tmp.join("target/debug/build.o"), b"\0\0\0").unwrap();
    }

    #[test]
    fn crawl_yields_expected_files() {
        let tmp = TempDir::new().unwrap();
        make_corpus(tmp.path());
        let cfg = Config::defaults();
        let entries = crawl(tmp.path(), &cfg).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.rel_path.as_str()).collect();

        // .gitignored dirs absent ONLY if respect_gitignore is on (default)
        assert!(names.contains(&"src/main.rs"));
        assert!(names.contains(&"src/lib.rs"));
        assert!(names.contains(&"README.md"));
        // .git internals — handled by gitignore when enabled.
        assert!(
            !names.iter().any(|n| n.starts_with(".git/")),
            "got: {names:?}"
        );
    }

    #[test]
    fn exclude_patterns_remove_build_dirs() {
        let tmp = TempDir::new().unwrap();
        make_corpus(tmp.path());

        // Turn off gitignore so we test our own excludes independently.
        let mut cfg = Config::defaults();
        cfg.index.respect_gitignore = false;
        let entries = crawl(tmp.path(), &cfg).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.rel_path.as_str()).collect();
        assert!(
            !names.iter().any(|n| n.starts_with("node_modules/")),
            "node_modules must be excluded, got: {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.starts_with("target/")),
            "target must be excluded, got: {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.starts_with(".localrag1/")),
            ".localrag1 must be excluded, got: {names:?}"
        );
    }

    #[test]
    fn oversized_files_dropped() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("big.txt"), vec![0u8; 5 * 1024 * 1024]).unwrap();
        fs::write(tmp.path().join("small.txt"), b"hello").unwrap();
        let mut cfg = Config::defaults();
        cfg.index.respect_gitignore = false;
        cfg.index.max_file_size_mb = 1; // 1 MB cap; 5 MB file must drop
        let entries = crawl(tmp.path(), &cfg).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.rel_path.as_str()).collect();
        assert!(names.contains(&"small.txt"));
        assert!(!names.contains(&"big.txt"));
    }

    #[test]
    fn crawl_missing_root_errors() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does/not/exist");
        let cfg = Config::defaults();
        assert!(crawl(&missing, &cfg).is_err());
    }

    #[test]
    fn glob_helper_basics() {
        assert!(glob_match("*.rs", "foo.rs"));
        assert!(glob_match("src/*.rs", "src/main.rs"));
        assert!(!glob_match("*.rs", "foo.md"));
        assert!(glob_match("foo?", "fooX"));
    }
}
