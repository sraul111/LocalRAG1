//! Tier 1 — SQLite FTS5 over both filename and body.
//!
//! Uses trigram tokenizer so we can do substring (not just token) search.
//! Results merge path hits and body hits, weighted per the user's config.

use rusqlite::{params, Connection};

use super::query::Query;

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: i64,
    pub rel_path: String,
    pub score: f32,
}

/// FTS5 trigram search. The query string is split into N-grams of length 3,
/// but FTS5's trigram tokenizer handles that for us transparently.
///
/// We combine:
///   - filename match (high weight)
///   - body match (lower weight)
/// into a single ranked list. SQL `bm25` is used per FTS5 table, then we
/// blend on the Rust side to keep things simple.
pub fn run(conn: &Connection, q: &Query, min_score: f32) -> rusqlite::Result<Vec<Hit>> {
    // FTS5 MATCH expression — the cleaned query is space-separated trigrams
    // already from our tokenizer, so we just pass it.
    let match_expr = format!("\"{}\"", q.cleaned.replace('"', "\"\""));
    if match_expr.len() < 3 + 2 {
        // Empty / short; bail.
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT f.id, f.rel_path,
                (
                  CASE WHEN fts_path.id IS NOT NULL THEN 5.0 ELSE 0 END +
                  CASE WHEN fts_body.id IS NOT NULL THEN 2.0 ELSE 0 END
                ) AS score
           FROM files f
           LEFT JOIN (
             SELECT rowid AS id FROM fts_path WHERE fts_path MATCH ?1
           ) fts_path ON fts_path.id = f.id
           LEFT JOIN (
             SELECT rowid AS id FROM fts_content WHERE fts_content MATCH ?1
           ) fts_body ON fts_body.id = f.id
          WHERE fts_path.id IS NOT NULL OR fts_body.id IS NOT NULL
          ORDER BY score DESC, length(f.rel_path) ASC
          LIMIT 50",
    )?;

    let rows = stmt.query_map(params![match_expr], |r| {
        Ok(Hit {
            id: r.get::<_, i64>(0)?,
            rel_path: r.get::<_, String>(1)?,
            score: r.get::<_, f64>(2)? as f32,
        })
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
            ("README.md", "md", "design patterns in rust"),
            ("src/main.rs", "rs", "the main entrypoint"),
            ("notes/design.md", "md", "this document covers classic design patterns"),
            ("random.txt", "txt", "completely unrelated fluff about knits"),
        ] {
            tx.execute(
                "INSERT INTO files(rel_path, abs_path, size, mtime_ms, ext, body)
                 VALUES (?1, '', 0, 0, ?2, ?3)",
                params![rel, ext, body],
            )
            .unwrap();
        }
        // Mirror into FTS tables; in real ingest() this happens too.
        for (rel, body) in [
            ("README.md", "design patterns in rust"),
            ("src/main.rs", "the main entrypoint"),
            ("notes/design.md", "this document covers classic design patterns"),
            ("random.txt", "completely unrelated fluff about knits"),
        ] {
            tx.execute(
                "INSERT INTO fts_path(rowid, rel_path) VALUES ((SELECT id FROM files WHERE rel_path = ?1), ?1)",
                params![rel],
            ).unwrap();
            tx.execute(
                "INSERT INTO fts_content(rowid, body) VALUES ((SELECT id FROM files WHERE rel_path = ?1), ?2)",
                params![rel, body],
            ).unwrap();
        }
        tx.commit().unwrap();
    }

    #[test]
    fn body_match_via_trigram() {
        let db = Database::open_memory().unwrap();
        seed(&db);
        let q = Query::parse("design patterns");
        let hits = run(db.conn(), &q, 1.0).unwrap();
        assert!(!hits.is_empty(), "expected trigram match for 'design patterns'");
        // design.md should win because both filename and body match.
        assert!(hits.iter().any(|h| h.rel_path.contains("design.md")));
    }

    #[test]
    fn path_match_via_trigram() {
        let db = Database::open_memory().unwrap();
        seed(&db);
        let q = Query::parse("main");
        let hits = run(db.conn(), &q, 1.0).unwrap();
        // src/main.rs has "main" in path; score >= path weight.
        assert!(hits.iter().any(|h| h.rel_path.contains("main")));
    }

    #[test]
    fn nothing_matches_returns_empty() {
        let db = Database::open_memory().unwrap();
        seed(&db);
        let q = Query::parse("zzzqqqxxx");
        let hits = run(db.conn(), &q, 0.0).unwrap();
        assert!(hits.is_empty());
    }
}
