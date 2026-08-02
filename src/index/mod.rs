//! SQLite + FTS5 indexing layer.
//!
//! Schema overview (see [`schema`] for the canonical CREATE statements):
//!
//! ```text
//! files(id PK, rel_path UNIQUE, abs_path, size, mtime_ms, ext, hash)
//! fts_files(rowid=fk, content=rel_path, ...contentless)
//! fts_content(rowid=fk, content=body)
//! ```
//!
//! The split lets us query filename and content with different weights.
//! `files` is the source of truth; `fts_*` tables are derived and rebuilt
//! cheaply on reindex.

pub mod db;
pub mod schema;
pub mod store;

pub use db::Database;
pub use store::{Indexed, Indexer};
