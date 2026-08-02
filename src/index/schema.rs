//! DDL for the SQLite database. Run once at startup.
//!
//! We use FTS5 with the **trigram** tokenizer so substring matching works
//! out of the box — no FTS5 query syntax required from the user.
//!
//! `files` is the canonical metadata table.
//! `fts_content` holds inlined file bodies (NULL for binary or oversized).
//! `fts_path` is a contentless FTS table holding only file paths, used
//!   for filename/folder ranking via SQL.
//! `meta` is a small key/value table for schema_version, last_crawl_ms, etc.

/// Schema version. Bumped if we ever need a migration.
pub const SCHEMA_VERSION: i64 = 1;

pub const DDL: &[&str] = &[
    // --- Core metadata ---------------------------------------------------
    r#"
    CREATE TABLE IF NOT EXISTS files (
        id        INTEGER PRIMARY KEY,
        rel_path  TEXT NOT NULL UNIQUE,
        abs_path  TEXT NOT NULL,
        size      INTEGER NOT NULL,
        mtime_ms  INTEGER NOT NULL,
        ext       TEXT NOT NULL,
        body      TEXT  -- inlined text for FTS; NULL = skip
    );
    "#,
    // Index on ext for tier-2 filters.
    r#"
    CREATE INDEX IF NOT EXISTS files_ext_idx ON files(ext);
    "#,
    // Index on mtime for "recent edits" filters.
    r#"
    CREATE INDEX IF NOT EXISTS files_mtime_idx ON files(mtime_ms DESC);
    "#,
    // --- FTS5 filename --------------------------------------------------
    // `content=` lets us mirror `rel_path` into a contentless FTS table
    // without keeping a second body column.
    r#"
    CREATE VIRTUAL TABLE IF NOT EXISTS fts_path USING fts5(
        rel_path,
        tokenize = 'trigram'
    );
    "#,
    // --- FTS5 content ---------------------------------------------------
    r#"
    CREATE VIRTUAL TABLE IF NOT EXISTS fts_content USING fts5(
        body,
        tokenize = 'trigram'
    );
    "#,
    // --- Metadata kv ----------------------------------------------------
    r#"
    CREATE TABLE IF NOT EXISTS meta (
        key   TEXT PRIMARY KEY,
        value TEXT
    );
    "#,
];

/// Apply every DDL statement in a single transaction.
pub fn create(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN;")?;
    for stmt in DDL {
        conn.execute_batch(stmt)?;
    }
    // Record schema version. INSERT OR IGNORE in case the migration isn't fresh.
    conn.execute(
        "INSERT OR IGNORE INTO meta(key, value) VALUES ('schema_version', ?1)",
        rusqlite::params![SCHEMA_VERSION.to_string()],
    )?;
    conn.execute_batch("COMMIT;")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn create_then_query_meta() {
        let conn = Connection::open_in_memory().unwrap();
        create(&conn).unwrap();
        let v: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION.to_string());
    }

    #[test]
    fn schema_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        create(&conn).unwrap();
        create(&conn).unwrap(); // calling twice must not error
        let v: i64 = conn
            .query_row("SELECT count(*) FROM files", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 0);
    }
}
