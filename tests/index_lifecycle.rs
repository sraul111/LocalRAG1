//! End-to-end index lifecycle tests. Drives:
//!   crawler -> indexer store -> SQLite -> search pipeline
//!
//! The goal is to make sure the round-trip is correct: a file written to
//! disk shows up in search results, a deleted file disappears, a modified
//! file is re-indexed, and a `.localrag1/` from a different cwd doesn't
//! accidentally answer queries.

use sl::config::Config;
use sl::crawler;
use sl::index::{store::ingest, Database};
use sl::query::{self, Query, TierResult};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn make_entry(tmp: &Path, rel: &str, body: &[u8]) -> sl::crawler::FileEntry {
    let p = tmp.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&p, body).unwrap();
    let meta = fs::metadata(&p).unwrap();
    sl::crawler::FileEntry {
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
fn lifecycle_create_update_remove() {
    let tmp = TempDir::new().unwrap();
    let cfg = Config::defaults();

    // First crawl: two files.
    let entries_a = vec![
        make_entry(tmp.path(), "src/main.rs", b"fn main() {}"),
        make_entry(tmp.path(), "README.md", b"# hello world"),
    ];
    let db_path = tmp.path().join("index.sqlite3");
    let db = Database::open(&db_path).unwrap();
    ingest(&db, tmp.path(), &entries_a).unwrap();

    // Search should find "hello" (body), in README.md.
    let q = Query::parse("hello");
    let r = query::run(&db, &cfg, &q, false).unwrap();
    assert!(
        matches!(r, TierResult::Tier2(_) | TierResult::Tier1(_)),
        "expected hello to be found via FTS5, got {r:?}"
    );

    // Second crawl: drop main.rs, change README body.
    let entries_b = vec![make_entry(
        tmp.path(),
        "README.md",
        b"# updated content, redesigned",
    )];
    // Wait at least 1 ms so mtime differs.
    std::thread::sleep(std::time::Duration::from_millis(5));
    ingest(&db, tmp.path(), &entries_b).unwrap();

    // Old query no longer hits anything.
    let r = query::run(&db, &cfg, &Query::parse("hello"), false).unwrap();
    assert_eq!(r, TierResult::Empty, "old content shouldn't still match");

    // New query hits.
    let r = query::run(&db, &cfg, &Query::parse("redesigned"), false).unwrap();
    assert!(
        matches!(r, TierResult::Tier2(_) | TierResult::Tier1(_)),
        "expected redesigned to be found, got {r:?}"
    );

    // main.rs row should be gone.
    let count: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM files WHERE rel_path = ?", ["src/main.rs"], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "removed file still in DB");
}

#[test]
fn different_cwds_have_separate_indexes() {
    // Two folders, each with `.localrag1/` indexes, must not bleed state.
    let tmp = TempDir::new().unwrap();
    let cwd_a = tmp.path().join("a");
    let cwd_b = tmp.path().join("b");
    fs::create_dir_all(&cwd_a).unwrap();
    fs::create_dir_all(&cwd_b).unwrap();

    let db_a_path = cwd_a.join(".localrag1").join("index.sqlite3");
    let db_b_path = cwd_b.join(".localrag1").join("index.sqlite3");
    fs::create_dir_all(db_a_path.parent().unwrap()).unwrap();
    fs::create_dir_all(db_b_path.parent().unwrap()).unwrap();

    // Index cwd_a with one file, cwd_b with another.
    let entries_a = vec![make_entry(&cwd_a, "alpha.md", b"# alpha only")];
    let entries_b = vec![make_entry(&cwd_b, "beta.md", b"# beta only")];

    // Drive from inside cwd_a so the FTS path is `alpha.md`.
    let cfg = Config::defaults();
    let _ = crawler::crawl(&cwd_a, &cfg).unwrap();
    let db_a = Database::open(&db_a_path).unwrap();
    ingest(&db_a, &cwd_a, &entries_a).unwrap();
    let db_b = Database::open(&db_b_path).unwrap();
    ingest(&db_b, &cwd_b, &entries_b).unwrap();

    let ra = query::run(&db_a, &cfg, &Query::parse("alpha"), false).unwrap();
    let rb = query::run(&db_b, &cfg, &Query::parse("alpha"), false).unwrap();
    assert!(
        matches!(ra, TierResult::Tier2(_) | TierResult::Tier1(_) | TierResult::Tier0(_)),
        "alpha should be in db_a, got {ra:?}"
    );
    assert_eq!(rb, TierResult::Empty, "alpha must NOT leak into db_b");
}
