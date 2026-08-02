//! Indexer — turn a crawl result into rows in `files`, `fts_path`, `fts_content`.
//!
//! Strategy:
//! 1. Wrap the whole crawl in one transaction (one fsync, atomic from the
//!    user's perspective: if we crash mid-index, the previous index is still
//!    on disk until `reindex` finalizes the new state).
//! 2. **Incremental**: for each entry, compare mtime+size against the
//!    existing row. Skip if unchanged. Replace if newer. Insert if new.
//! 3. After ingestion, prune files that no longer exist (left over from a
//!    prior crawl whose file was deleted).
//!
//! Inline body reads are bounded — we read at most 1 MiB per file even
//! inside the crawler-cleared set, so a giant blob never hangs the indexer.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Transaction};

use crate::crawler::FileEntry;
use crate::error::Result;

use super::Database;

/// Per-file indexer statistics.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Indexed {
    /// Number of existing rows whose mtime+size matched.
    pub unchanged: usize,
    /// Number of rows updated (mtime changed or content re-read).
    pub updated: usize,
    /// Number of brand new rows inserted.
    pub inserted: usize,
    /// Number of rows deleted because the file no longer exists.
    pub removed: usize,
    /// Number of files encountered (row total after indexing).
    pub total: usize,
}

/// Write a whole crawl into the database. Incremental — only does work
/// for changed files. Synchronous; returns when all rows are written.
pub fn ingest(db: &Database, root: &Path, entries: &[FileEntry]) -> Result<Indexed> {
    let mut stats = Indexed::default();
    let conn = db.conn();
    let tx = conn.unchecked_transaction()?;

    // 1. Existing rows keyed by rel_path → (id, mtime_ms, size).
    let mut existing: HashMap<String, (i64, i64, i64)> = HashMap::new();
    {
        let mut stmt = tx.prepare("SELECT id, rel_path, mtime_ms, size FROM files")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        for row in rows {
            let (id, rel, mtime, size) = row?;
            existing.insert(rel, (id, mtime, size));
        }
    }

    let mut seen: Vec<String> = Vec::with_capacity(entries.len());

    for entry in entries {
        if existing.contains_key(&entry.rel_path) {
            let (id, prev_mtime, prev_size) = existing[&entry.rel_path];
            if prev_mtime == entry.mtime_ms && prev_size == entry.size as i64 {
                // No-op, the FTS rows are still valid.
                stats.unchanged += 1;
                seen.push(entry.rel_path.clone());
                continue;
            }
            update_row(&tx, id, entry)?;
            stats.updated += 1;
        } else {
            insert_row(&tx, entry)?;
            stats.inserted += 1;
        }
        seen.push(entry.rel_path.clone());
    }

    // 2. Remove rows that weren't seen this crawl.
    {
        // Build a parameter list dynamically — SQLite caps at 999 params.
        // For our scale (a few thousand files per cwd) this is fine.
        if !existing.is_empty() {
            let mut stmt = tx.prepare("DELETE FROM files WHERE id = ?1")?;
            for (rel, (id, _, _)) in &existing {
                if !seen.contains(rel) {
                    stmt.execute(params![*id])?;
                    stats.removed += 1;
                }
            }
        }
    }

    // 3. Update meta.
    tx.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES ('last_index_ms', ?1)",
        params![chrono_now_ms().to_string()],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES ('root', ?1)",
        params![root.to_string_lossy()],
    )?;

    tx.commit()?;
    stats.total = entries.len();
    Ok(stats)
}

/// Upsert helper for new files.
fn insert_row(tx: &Transaction<'_>, entry: &FileEntry) -> Result<()> {
    let body = if entry.indexable {
        Some(read_bounded(&entry.abs_path, entry.size)?)
    } else {
        None
    };

    tx.execute(
        "INSERT INTO files(rel_path, abs_path, size, mtime_ms, ext, body)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            entry.rel_path,
            entry.abs_path.to_string_lossy(),
            entry.size as i64,
            entry.mtime_ms,
            entry.ext,
            body.as_deref(),
        ],
    )?;
    let id = tx.last_insert_rowid();

    // Mirror into FTS tables.
    tx.execute(
        "INSERT INTO fts_path(rowid, rel_path) VALUES (?1, ?2)",
        params![id, entry.rel_path],
    )?;
    if let Some(b) = body {
        tx.execute(
            "INSERT INTO fts_content(rowid, body) VALUES (?1, ?2)",
            params![id, b],
        )?;
    }
    Ok(())
}

/// Upsert helper for changed files.
fn update_row(tx: &Transaction<'_>, id: i64, entry: &FileEntry) -> Result<()> {
    let body = if entry.indexable {
        Some(read_bounded(&entry.abs_path, entry.size)?)
    } else {
        None
    };
    tx.execute(
        "UPDATE files
           SET abs_path = ?2, size = ?3, mtime_ms = ?4, ext = ?5, body = ?6
         WHERE id = ?1",
        params![
            id,
            entry.abs_path.to_string_lossy(),
            entry.size as i64,
            entry.mtime_ms,
            entry.ext,
            body.as_deref(),
        ],
    )?;
    // Re-mirror: SQLite FTS5 doesn't support UPDATE-with-row-replace in one
    // call, so delete + insert.
    tx.execute("DELETE FROM fts_path WHERE rowid = ?1", params![id])?;
    tx.execute(
        "INSERT INTO fts_path(rowid, rel_path) VALUES (?1, ?2)",
        params![id, entry.rel_path],
    )?;
    tx.execute("DELETE FROM fts_content WHERE rowid = ?1", params![id])?;
    if let Some(b) = body {
        tx.execute(
            "INSERT INTO fts_content(rowid, body) VALUES (?1, ?2)",
            params![id, b],
        )?;
    }
    Ok(())
}

/// Read at most `expected_size` bytes from `path` (capped at 1 MiB).
/// Lossy: non-UTF8 files yield empty string. We accept that for v1.
fn read_bounded(path: &Path, expected_size: u64) -> Result<String> {
    const HARD_CAP: usize = 1024 * 1024;
    let to_read = std::cmp::min(HARD_CAP, expected_size as usize);
    let mut f = std::fs::File::open(path).map_err(|e| crate::error::Error::Io {
        operation: "open file for indexing",
        source: e,
    })?;
    let mut buf = vec![0u8; to_read];
    use std::io::Read;
    let n = f.read(&mut buf).map_err(|e| crate::error::Error::Io {
        operation: "read file for indexing",
        source: e,
    })?;
    buf.truncate(n);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn chrono_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// Re-export so callers `use crate::index::Indexer` works.
pub use self::ingest as ingest_index;
/// Convenience wrapper: type-aliased indexer reference (= the DB itself).
pub type Indexer = Database;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Database;
    use std::fs;
    use tempfile::TempDir;

    fn make_entry(tmp: &Path, rel: &str, body: &str) -> FileEntry {
        let p = tmp.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, body).unwrap();
        let meta = fs::metadata(&p).unwrap();
        FileEntry {
            rel_path: rel.replace('\\', "/"),
            abs_path: p,
            size: meta.len(),
            mtime_ms: meta
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            ext: rel
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_lowercase(),
            indexable: true,
        }
    }

    #[test]
    fn first_index_inserts_all() {
        let tmp = TempDir::new().unwrap();
        let entries = vec![
            make_entry(tmp.path(), "src/main.rs", "fn main() {}"),
            make_entry(tmp.path(), "README.md", "# hello world"),
        ];
        let db = Database::open_memory().unwrap();
        let stats = ingest(&db, tmp.path(), &entries).unwrap();
        assert_eq!(stats.inserted, 2);
        assert_eq!(stats.unchanged, 0);

        let count: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn second_index_with_no_changes_unchanged() {
        let tmp = TempDir::new().unwrap();
        let entries = vec![make_entry(tmp.path(), "README.md", "# hello world")];
        let db = Database::open_memory().unwrap();
        ingest(&db, tmp.path(), &entries).unwrap();
        let stats2 = ingest(&db, tmp.path(), &entries).unwrap();
        assert_eq!(stats2.unchanged, 1);
        assert_eq!(stats2.inserted, 0);
    }

    #[test]
    fn removed_file_is_pruned() {
        let tmp = TempDir::new().unwrap();
        let entries = vec![
            make_entry(tmp.path(), "README.md", "# hello"),
            make_entry(tmp.path(), "src/lib.rs", "// lib"),
        ];
        let db = Database::open_memory().unwrap();
        ingest(&db, tmp.path(), &entries).unwrap();

        // Re-crawl with just one file; lib.rs must be deleted.
        let reduced = vec![make_entry(tmp.path(), "README.md", "# hello")];
        let s = ingest(&db, tmp.path(), &reduced).unwrap();
        assert_eq!(s.removed, 1);
        let count: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn body_inlined_only_when_indexable() {
        let tmp = TempDir::new().unwrap();
        let mut txt = make_entry(tmp.path(), "x.md", "lorem ipsum");
        txt.indexable = false;
        let db = Database::open_memory().unwrap();
        ingest(&db, tmp.path(), &[txt]).unwrap();
        let body: Option<String> = db
            .conn()
            .query_row("SELECT body FROM files LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert!(body.is_none(), "non-indexable should not inline, got: {body:?}");
    }
}
