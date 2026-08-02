//! `sl index` — crawl cwd + write to the SQLite index.

use crate::config::ProjectContext;
use crate::crawler;
use crate::error::Result;
use crate::index::{store::ingest, Database};
use crate::util::paths;

pub fn run(provided: Option<&std::path::Path>) -> Result<()> {
    let data = paths::ensure_data_dir(provided)?;
    let ctx = ProjectContext::open(&data)?;
    run_inner(&ctx)
}

fn run_inner(ctx: &ProjectContext) -> Result<()> {
    let cwd = std::env::current_dir().map_err(|e| crate::error::Error::Io {
        operation: "current_dir",
        source: e,
    })?;
    let db = Database::open(&ctx.index_path)?;
    let entries = crawler::crawl(&cwd, &ctx.config)?;
    log::info!("crawler found {} candidate files", entries.len());
    let stats = ingest(&db, &cwd, &entries)?;
    println!(
        "Indexed {} files (inserted={}, updated={}, unchanged={}, removed={})",
        stats.total, stats.inserted, stats.updated, stats.unchanged, stats.removed
    );
    Ok(())
}
