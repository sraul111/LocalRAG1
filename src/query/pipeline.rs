//! Top-level query pipeline. Walks tiers 0→2 (and optionally 3) and
//! returns the first non-empty result.

use crate::error::Result;

use super::{query::Query, tier0, tier1, tier2};
use crate::config::Config;
use crate::index::Database;

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: i64,
    pub rel_path: String,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TierResult {
    Tier0(Vec<Hit>),
    Tier1(Vec<Hit>),
    Tier2(Vec<tier2::RankedHit>),
    Tier3(String),
    Empty,
}

/// Run the pipeline.
///
/// Order:
/// 1. Tier 0 — filename match.
/// 2. Tier 1 — FTS5 over filename + body.
/// 3. Tier 2 — re-rank tier-1 hits by metadata.
/// 4. Tier 3 — if `llm.enabled` and we force tier3, invoke the agent loop.
pub fn run(db: &Database, cfg: &Config, q: &Query, force_tier3: bool) -> Result<TierResult> {
    let q0 = tier0::run(db.conn(), q, cfg.search.tier0_min_score)
        .map_err(crate::error::Error::Sqlite)?;
    if !q0.is_empty() {
        let hits = q0
            .into_iter()
            .map(|h| Hit { id: h.id, rel_path: h.rel_path, score: h.score })
            .collect();
        return Ok(TierResult::Tier0(hits));
    }

    let q1 = tier1::run(db.conn(), q, cfg.search.tier1_min_score)
        .map_err(crate::error::Error::Sqlite)?;
    if !q1.is_empty() {
        // Move q1 into rerank. If rerank yields nothing, return Empty —
        // the FTS hits we cared about got consumed and there is nothing
        // useful to surface.
        let ranked = tier2::rerank(db.conn(), q1, now_ms())
            .map_err(crate::error::Error::Sqlite)?;
        if !ranked.is_empty() {
            return Ok(TierResult::Tier2(ranked));
        }
        return Ok(TierResult::Empty);
    }

    if force_tier3 && cfg.llm.enabled {
        let txt = crate::llm::run(db, q, &cfg.llm, &cfg.search)?;
        return Ok(TierResult::Tier3(txt));
    }

    Ok(TierResult::Empty)
}

/// Aggregate answer handed to CLI/print layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub query: String,
    pub result: TierResult,
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Database;
    use rusqlite::params;

    fn seed(db: &Database) {
        let conn = db.conn();
        let tx = conn.unchecked_transaction().unwrap();
        for (rel, body) in [
            ("README.md", "welcome to the project"),
            ("src/main.rs", "fn main() {}"),
            ("design.md", "design patterns and notes"),
        ] {
            tx.execute(
                "INSERT INTO files(rel_path, abs_path, size, mtime_ms, ext, body)
                 VALUES (?1, '', 0, 0, 'md', ?2)",
                params![rel, body],
            )
            .unwrap();
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
    fn short_circuits_at_tier0() {
        let db = Database::open_memory().unwrap();
        seed(&db);
        let cfg = Config::defaults();
        let q = Query::parse("README.md");
        let r = run(&db, &cfg, &q, false).unwrap();
        match r {
            TierResult::Tier0(hits) => assert_eq!(hits.len(), 1),
            other => panic!("expected Tier0, got {other:?}"),
        }
    }

    #[test]
    fn falls_through_to_tier2() {
        // Tier-1 trigram search over seeded files. We use a query that
        // does NOT appear exactly in any path so tier-0 stays out, leaving
        // the pipeline at tier-1/2.
        let db = Database::open_memory().unwrap();
        seed(&db);
        let mut cfg = Config::defaults();
        cfg.search.tier1_min_score = 0.0;
        let q = Query::parse("patterns"); // substring lives in body, not path
        let r = run(&db, &cfg, &q, false).unwrap();
        assert!(
            matches!(r, TierResult::Tier2(_) | TierResult::Tier1(_)),
            "expected tier2/tier1 hit, got {r:?}"
        );
    }

    #[test]
    fn empty_when_nothing_matches() {
        let db = Database::open_memory().unwrap();
        seed(&db);
        let cfg = Config::defaults();
        let q = Query::parse("qqqxxxxnothing");
        let r = run(&db, &cfg, &q, false).unwrap();
        assert_eq!(r, TierResult::Empty);
    }
}
