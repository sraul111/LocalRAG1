//! Thin [`rusqlite::Connection`] wrapper with the pragmas we want always on.

use std::path::Path;

use crate::error::Result;

use super::schema;

/// Wrapped SQLite connection. Cloning is cheap (it's a `RefCell` inside
/// rusqlite); the database itself is thread-safe via `check_same_thread=false`.
#[derive(Debug)]
pub struct Database {
    conn: rusqlite::Connection,
}

impl Database {
    /// Open or create a database at `path`, run schema migrations.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path).map_err(crate::error::Error::Sqlite)?;
        // Pragmas: WAL for crash-safety + concurrent reads; NORMAL sync is
        // good enough for a local indexer. foreign_keys is irrelevant.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;
             PRAGMA mmap_size = 268435456; -- 256 MB",
        )?;
        schema::create(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory database with the schema applied — for tests.
    pub fn open_memory() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory().map_err(crate::error::Error::Sqlite)?;
        conn.execute_batch(
            "PRAGMA journal_mode = MEMORY;
             PRAGMA synchronous = OFF;",
        )?;
        schema::create(&conn)?;
        Ok(Self { conn })
    }

    /// Borrow the underlying connection. Methods on `Indexer` / query
    /// pipeline need this to execute SQL.
    pub fn conn(&self) -> &rusqlite::Connection {
        &self.conn
    }
}
