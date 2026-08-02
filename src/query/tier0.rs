//! Tier 0 — exact + normalized filename/path match.
//!
//! Fast path: one SQL query against `files` with LIKE on the lowercased
//! rel_path. If anything matches the user's first token exactly, we
//! short-circuit and never open FTS5.

use rusqlite::{params, Connection};

use super::query::Query;

/// Result rows from a tier-0 run.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: i64,
    pub rel_path: String,
    pub score: f32,
}

/// Try to find rows whose path contains the first token verbatim (case-insensitive).
/// Returns an empty vec if nothing matches — caller moves to tier 1.
pub fn run(conn: &Connection, q: &Query, min_score: f32) -> rusqlite::Result<Vec<Hit>> {
    let token = q.first_token();
    if token.is_empty() {
        return Ok(Vec::new());
    }
    let like = format!("%{}%", token.to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT id, rel_path
           FROM files
          WHERE lower(rel_path) LIKE ?1
          ORDER BY
            -- exact basename match scores 1.0
            CASE WHEN lower(rel_path) = lower(?2) THEN 1.0
                 WHEN lower(rel_path) LIKE lower(?3) THEN 0.8
                 ELSE 0.5 END DESC,
            length(rel_path) ASC
          LIMIT 20",
    )?;

    let eq = format!("{}", q.raw.to_lowercase());
    let suffix = format!("/{}", token);
    let rows = stmt.query_map(params![like, eq, suffix], |r| {
        let id: i64 = r.get(0)?;
        let rp: String = r.get(1)?;
        let lower = rp.to_lowercase();
        let score: f32 = if lower == q.raw.to_lowercase() {
            1.0
        } else if lower.ends_with(&suffix) {
            0.8
        } else {
            0.5
        };
        Ok(Hit { id, rel_path: rp, score })
    })?;

    Ok(rows
        .filter_map(|r| r.ok())
        .filter(|h| h.score >= min_score)
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Database;

    fn seed(db: &Database) {
        let conn = db.conn();
        let tx = conn.unchecked_transaction().unwrap();
        for (rel, ext, body) in [
            ("README.md", "md", "# hello"),
            ("src/main.rs", "rs", "fn main(){}"),
            ("notes/design.md", "md", "design notes"),
            ("docs/rust-book.md", "md", "the rust book"),
        ] {
            tx.execute(
                "INSERT INTO files(rel_path, abs_path, size, mtime_ms, ext, body)
                 VALUES (?1, '', 0, 0, ?2, ?3)",
                params![rel, ext, body],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }

    #[test]
    fn exact_filename_wins_with_high_score() {
        let db = Database::open_memory().unwrap();
        seed(&db);
        let q = Query::parse("README.md");
        let hits = run(db.conn(), &q, 0.5).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].rel_path, "README.md");
        assert!(hits[0].score >= 0.8);
    }

    #[test]
    fn token_match_returns_partial() {
        let db = Database::open_memory().unwrap();
        seed(&db);
        let q = Query::parse("rust");
        let hits = run(db.conn(), &q, 0.4).unwrap();
        let paths: Vec<_> = hits.iter().map(|h| h.rel_path.clone()).collect();
        assert!(paths.iter().any(|p| p.contains("rust")));
    }

    #[test]
    fn no_match_yields_empty() {
        let db = Database::open_memory().unwrap();
        seed(&db);
        let q = Query::parse("nonexistent_xyz");
        let hits = run(db.conn(), &q, 0.0).unwrap();
        assert!(hits.is_empty());
    }
}
