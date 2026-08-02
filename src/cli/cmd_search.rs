//! `sl search "..."` — run the query pipeline and pretty-print results.

use crate::config::ProjectContext;
use crate::error::Result;
use crate::index::Database;
use crate::query::{self, Query, TierResult};

pub fn run(ctx: &ProjectContext, raw: &str, tier_override: Option<u8>) -> Result<()> {
    let db = Database::open(&ctx.index_path)?;
    let q = Query::parse(raw);
    let force_tier3 = tier_override == Some(3);
    let result = query::run(&db, &ctx.config, &q, force_tier3)?;
    print_result(&q, &result);
    Ok(())
}

fn print_result(q: &Query, r: &TierResult) {
    match r {
        TierResult::Tier0(hits) => {
            println!("Tier 0 (filename) — {} hit(s) for {}:", hits.len(), q.raw);
            for h in hits {
                println!("  {} (score={:.2})", h.rel_path, h.score);
            }
        }
        TierResult::Tier1(hits) => {
            println!("Tier 1 (FTS5) — {} hit(s) for {}:", hits.len(), q.raw);
            for h in hits {
                println!("  {} (score={:.2})", h.rel_path, h.score);
            }
        }
        TierResult::Tier2(ranked) => {
            println!("Tier 2 (ranked) — {} hit(s) for {}:", ranked.len(), q.raw);
            for r in ranked.iter().take(20) {
                println!("  {}", crate::query::tier2::format_row(r));
            }
        }
        TierResult::Tier3(s) => {
            println!("Tier 3 (LLM):\n{s}");
        }
        TierResult::Empty => {
            println!("No matches for {q}.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use rusqlite::params;

    fn ctx_with_db() -> (tempfile::TempDir, ProjectContext, Database) {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join(".localrag1");
        std::fs::create_dir_all(&data).unwrap();
        let cfg = Config::defaults();
        let cfg_path = data.join("config.toml");
        cfg.save(&cfg_path).unwrap();
        let ctx = ProjectContext::open(&data).unwrap();
        let db_path = data.join("index.sqlite3");
        let db = Database::open(&db_path).unwrap();
        // Seed
        let conn = db.conn();
        let tx = conn.unchecked_transaction().unwrap();
        for (rel, body) in [
            ("README.md", "design patterns in rust"),
            ("docs/design.md", "patterns and architecture"),
        ] {
            tx.execute(
                "INSERT INTO files(rel_path, abs_path, size, mtime_ms, ext, body)
                 VALUES (?1, '', 0, 0, 'md', ?2)",
                params![rel, body],
            ).unwrap();
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
        (t, ctx, db)
    }

    #[test]
    fn search_runs_end_to_end() {
        let (t, ctx, _db) = ctx_with_db();
        let _ = t;
        let r = super::run(&ctx, "design", None);
        assert!(r.is_ok());
    }

    #[test]
    fn tier3_override_without_db_change() {
        let (t, ctx, _db) = ctx_with_db();
        let _ = t;
        // No LLM configured → tier3 path is skipped, but no panic.
        let r = super::run(&ctx, "design", Some(3));
        assert!(r.is_ok());
    }
}
