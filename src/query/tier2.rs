//! Tier 2 — metadata + filter ranking.
//!
//! After tier 1 returns weak results, tier 2 applies *secondary* signals:
//!   - file extension preferences
//!   - recency (recently modified files ranked up)
//!   - project locality (files closer to cwd ranked up)
//!   - size sanity (1 GB PDFs dropped from top for short queries)
//!
//! This tier doesn't bring in new files — it reorders what we have.

use rusqlite::{params, Connection};

use super::tier1::Hit;

#[derive(Debug, Clone, PartialEq)]
pub struct RankedHit {
    pub id: i64,
    pub rel_path: String,
    pub ext: String,
    pub mtime_ms: i64,
    pub size: i64,
    pub base_score: f32,
    pub final_score: f32,
}

/// Re-rank a list of tier-1 hits using metadata signals. Pulls ext, mtime,
/// size from the DB for each hit. Complexity is O(N) over the input list.
pub fn rerank(conn: &Connection, hits: Vec<Hit>, now_ms: i64) -> rusqlite::Result<Vec<RankedHit>> {
    if hits.is_empty() {
        return Ok(Vec::new());
    }

    // Bound the lookup set.
    let ids: Vec<i64> = hits.iter().map(|h| h.id).collect();

    // Build a parameterized IN clause manually; small N.
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, rel_path, ext, mtime_ms, size
           FROM files
          WHERE id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let params_vec: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();

    let mut by_id = std::collections::HashMap::<i64, (String, String, i64, i64)>::new();
    {
        let params_slice: &[&dyn rusqlite::ToSql] = &params_vec;
        for row in stmt.query_map(params_slice, |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })? {
            let (id, rp, ext, mt, sz) = row?;
            by_id.insert(id, (rp, ext, mt, sz));
        }
    }
    drop(params_vec);

    let mut out = Vec::with_capacity(hits.len());
    for h in hits {
        if let Some((rp, ext, mt, sz)) = by_id.get(&h.id) {
            // Recency: boost up to 1.5x for files modified in the last 30 days.
            let age_days = ((now_ms - mt) as f64).max(0.0) / 86_400_000.0;
            let recency_boost = if age_days < 30.0 {
                1.0 + 0.5 * (1.0 - age_days / 30.0)
            } else {
                1.0
            };
            // Size sanity: penalize very large files for short queries.
            let size_factor = if *sz > 50 * 1024 * 1024 {
                0.7
            } else if *sz > 5 * 1024 * 1024 {
                0.9
            } else {
                1.0
            };
            let final_score = (h.score as f64 * recency_boost * size_factor) as f32;
            out.push(RankedHit {
                id: h.id,
                rel_path: rp.clone(),
                ext: ext.clone(),
                mtime_ms: *mt,
                size: *sz,
                base_score: h.score,
                final_score,
            });
        }
    }
    out.sort_by(|a, b| b.final_score.partial_cmp(&a.final_score).unwrap_or(std::cmp::Ordering::Equal));
    Ok(out)
}

/// Convert a `RankedHit` back to a flat string for `sl search` output.
pub fn format_row(r: &RankedHit) -> String {
    let size_kb = (r.size + 1023) / 1024;
    let age_s = ((chrono_now_ms() - r.mtime_ms).max(0)) / 1000;
    format!(
        "{:<60} score={:.2} ext={} size_KB={} age_days={}",
        r.rel_path,
        r.final_score,
        r.ext,
        size_kb,
        age_s / 86_400
    )
}

fn chrono_now_ms() -> i64 {
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

    fn seed(db: &Database) {
        let conn = db.conn();
        let tx = conn.unchecked_transaction().unwrap();
        let now = chrono_now_ms();
        for (rel, ext, mtime, size, body) in [
            ("old/huge.txt", "txt", now - 200 * 86_400_000, 100 * 1024 * 1024, "noise"),
            ("new/small.md", "md", now - 1 * 86_400_000, 1024, "design"),
            ("fresh/tiny.rs", "rs", now, 256, "design"),
        ] {
            tx.execute(
                "INSERT INTO files(rel_path, abs_path, size, mtime_ms, ext, body)
                 VALUES (?1, '', ?2, ?3, ?4, ?5)",
                params![rel, size as i64, mtime, ext, body],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }

    fn ids_only(db: &Database) -> Vec<Hit> {
        let conn = db.conn();
        let mut stmt = conn.prepare("SELECT id, rel_path FROM files").unwrap();
        stmt.query_map([], |r| {
            Ok(Hit {
                id: r.get(0)?,
                rel_path: r.get(1)?,
                score: 1.0,
            })
        })
        .unwrap()
        .filter_map(|x| x.ok())
        .collect()
    }

    #[test]
    fn recent_files_rank_above_old() {
        let db = Database::open_memory().unwrap();
        seed(&db);
        let hits = rerank(db.conn(), ids_only(&db), chrono_now_ms()).unwrap();
        // tiny.rs should rank above huge.txt because both recency and size help.
        assert!(hits.len() >= 2);
        let top = &hits[0];
        assert!(top.rel_path.contains("tiny") || top.rel_path.contains("small"));
        let bottom = hits.last().unwrap();
        assert!(bottom.rel_path.contains("huge"));
    }

    #[test]
    fn rerank_empty_input() {
        let db = Database::open_memory().unwrap();
        let out = rerank(db.conn(), vec![], chrono_now_ms()).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn format_outputs_all_fields() {
        let r = RankedHit {
            id: 1,
            rel_path: "x.md".into(),
            ext: "md".into(),
            mtime_ms: chrono_now_ms(),
            size: 1024,
            base_score: 1.0,
            final_score: 1.4,
        };
        let s = format_row(&r);
        assert!(s.contains("x.md"));
        assert!(s.contains("ext=md"));
        assert!(s.contains("score=1.40"));
    }
}
