//! `sl init` — create `.localrag1/` in the current directory (or given path).
//!
//! Idempotent: re-running is a no-op. Writes a default config if none exists.

use crate::config::Config;
use crate::error::Result;
use crate::util::paths;

pub fn run(provided: Option<&std::path::Path>) -> Result<()> {
    let data = paths::ensure_data_dir(provided)?;
    let cfg_path = paths::config_path(&data);
    if !cfg_path.exists() {
        Config::defaults().save(&cfg_path)?;
    }
    log::info!("data dir ready at {}", data.display());
    println!("Created (or already had) {}", data.display());
    println!("Run `sl index` next, then `sl search \"...\"`.");
    Ok(())
}
