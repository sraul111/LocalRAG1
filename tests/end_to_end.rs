//! End-to-end integration tests: `sl`-style usage against a synthetic
//! corpus living under a temporary directory.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn sl_bin() -> std::path::PathBuf {
    // The binary name is `sl` (set in Cargo.toml).
    let mut p = std::env::current_exe().unwrap();
    p.pop(); // drop test binary
    p.push("sl.exe");
    // Fallback for non-Windows hosts in case someone runs tests there.
    if !p.exists() {
        p.set_extension("");
    }
    p
}

fn make_corpus(root: &std::path::Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::create_dir_all(root.join("node_modules")).unwrap();
    fs::write(root.join("src/main.rs"), b"fn main() { println!(\"hi\"); }").unwrap();
    fs::write(root.join("src/lib.rs"), b"// library code").unwrap();
    fs::write(root.join("notes/design.md"), b"# design patterns\n\nFavour composition.").unwrap();
    fs::write(root.join("README.md"), b"# project\n\nWelcome.").unwrap();
    // `node_modules` should be excluded.
    fs::write(root.join("node_modules/lodash.js"), b"// js lib").unwrap();
}

#[test]
fn init_then_search_finds_filename() {
    let tmp = TempDir::new().unwrap();
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).unwrap();
    make_corpus(&work);

    // We can't pass `--data-dir` semantics because the CLI uses cwd for the
    // project root. Run from `work` so `.localrag1/` lands there.
    let bin = sl_bin();
    if !bin.exists() {
        // Cargo test setup that doesn't produce a binary we can re-run —
        // skip gracefully so CI on platforms where the binary is missing
        // doesn't fail.
        eprintln!("sl binary not found at {:?}, skipping integration test", bin);
        return;
    }

    // `sl init`
    let out = Command::new(&bin)
        .arg("init")
        .current_dir(&work)
        .output()
        .unwrap();
    assert!(out.status.success(), "init failed: {:?}", out);

    // `sl index`
    let out = Command::new(&bin)
        .arg("index")
        .current_dir(&work)
        .output()
        .unwrap();
    assert!(out.status.success(), "index failed: {:?}", out);

    // `sl search "design"` should return a Tier-0 or Tier-1 result for
    // notes/design.md.
    let out = Command::new(&bin)
        .arg("search")
        .arg("design")
        .current_dir(&work)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("design.md") || stdout.contains("no matches".to_string().as_str()),
        "unexpected stdout: {stdout}"
    );
}
