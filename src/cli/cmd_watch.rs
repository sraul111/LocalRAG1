//! `sl watch` — keep the index fresh.

use notify::{Config as NConfig, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc;
use std::time::Duration;

use crate::error::{Error, Result};

pub fn run(provided: Option<&std::path::Path>) -> Result<()> {
    // We keep the watch command lightweight: log changes, suggest a re-index.
    // A full incremental pipeline is Phase C polish, but the harness is
    // here so the CLI is wired end-to-end now.
    let cwd = std::env::current_dir().map_err(|e| Error::Io {
        operation: "current_dir",
        source: e,
    })?;
    let _ = provided;

    let (tx, rx) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        NConfig::default().with_poll_interval(Duration::from_secs(2)),
    )
    .map_err(|e| Error::Other(e.to_string()))?;

    watcher
        .watch(&cwd, RecursiveMode::Recursive)
        .map_err(|e| Error::Other(e.to_string()))?;

    println!("Watching {} (Ctrl-C to stop). Re-run `sl index` to absorb changes.", cwd.display());
    for res in rx {
        match res {
            Ok(e) => log::debug!("fs event: {:?}", e),
            Err(e) => log::warn!("watch error: {e}"),
        }
    }
    Ok(())
}
