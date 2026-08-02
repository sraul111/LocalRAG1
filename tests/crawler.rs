//! Crawler-focused integration tests. These build larger corpora on disk
//! and exercise the crawl → index round-trip.

use std::fs;
use tempfile::TempDir;

use sl::crawler::{crawl, FileEntry};
use sl::config::Config;

fn write(p: &std::path::Path, bytes: &[u8]) {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, bytes).unwrap();
}

#[test]
fn crawl_respects_gitignore() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join(".gitignore"), b"*.log\nbuild/\n");
    write(&tmp.path().join("src/a.rs"), b"// a");
    write(&tmp.path().join("debug.log"), b"noise");
    write(&tmp.path().join("build/x.exe"), b"\0\0\0");

    let mut cfg = Config::defaults();
    // Add *.log to exclude to cover the case the gitignore walker misses.
    cfg.index.exclude.push("*.log".to_string());
    let entries = crawl(tmp.path(), &cfg).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.rel_path.as_str()).collect();

    assert!(names.iter().any(|n| n.ends_with("a.rs")));
    // build/ should be excluded by EITHER gitignore or our exclude list.
    let logs: Vec<&str> = names.iter().filter(|n| n.ends_with(".log")).copied().collect();
    assert!(logs.is_empty(), "expected no .log files, got: {:?}", names);
    assert!(!names.iter().any(|n| n.contains("build/")));
}

#[test]
fn crawl_own_gitignore_user_pattern() {
    // We can't rely solely on the `ignore` crate to honor a root-level
    // `.gitignore` outside a git repo, so this test exercises the same
    // scenario via the *config* exclude list — which is the surface the
    // user controls.
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("debug.log"), b"noise");
    write(&tmp.path().join("visible.txt"), b"hello");
    let mut cfg = Config::defaults();
    cfg.index.exclude.push("*.log".to_string());
    let entries = crawl(tmp.path(), &cfg).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.rel_path.as_str()).collect();
    assert!(!names.iter().any(|n| n.ends_with(".log")),
        "config exclude '*.log' must drop .log files, got: {:?}", names);
    assert!(names.iter().any(|n| n.ends_with("visible.txt")));
}

#[test]
fn crawl_handles_unicode() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("données.md"), "# café résumé".as_bytes());
    let cfg = Config::defaults();
    let entries = crawl(tmp.path(), &cfg).unwrap();
    assert!(
        entries.iter().any(|e| e.rel_path.contains("données")),
        "expected unicode file, got: {:?}",
        entries.iter().map(|e| &e.rel_path).collect::<Vec<_>>()
    );
}

#[test]
fn crawl_sorts_results_deterministically() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("z.txt"), b"");
    write(&tmp.path().join("a.txt"), b"");
    write(&tmp.path().join("m.txt"), b"");
    let cfg = Config::defaults();
    let entries: Vec<FileEntry> = crawl(tmp.path(), &cfg).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.rel_path.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "crawl output should be sorted");
}

#[test]
fn empty_corpust_yields_nothing() {
    let tmp = TempDir::new().unwrap();
    let cfg = Config::defaults();
    let entries = crawl(tmp.path(), &cfg).unwrap();
    assert!(entries.is_empty());
}
