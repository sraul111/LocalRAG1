//! `sl` — cwd-scoped local search engine.
//!
//! Every subcommand (`init`, `index`, `search`, `watch`, `config`) operates
//! on the current working directory. The index lives at `<cwd>/.localrag1/`,
//! so each folder gets its own independent index.
//!
//! Phase A: `sl init && sl index && sl search "query"` works against any
//! folder, with results coming from Tier 0 (filename) or Tier 1 (FTS5).
//! Phase C: Tier 3 (LLM agent loop, opt-in).

use clap::Parser;

use sl::cli;
use sl::error::Result;

fn main() -> Result<()> {
    // Initialize stderr logging. Set RUST_LOG=debug for verbose output.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .try_init()
        .ok();

    let args = cli::Cli::parse();
    cli::run(args)
}
