//! Command-line interface. `clap` derive on `Cli`, manual dispatch in
//! [`run`]. Each subcommand is a small function in a sibling module so
//! the entry point file stays readable.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::ProjectContext;
use crate::error::Result;
use crate::util::paths;

pub mod cmd_config;
pub mod cmd_index;
pub mod cmd_init;
pub mod cmd_search;
pub mod cmd_watch;

/// Top-level CLI surface. Use `--help` to see current subcommands.
#[derive(Debug, Parser)]
#[command(name = "sl", version, about = "Cwd-scoped local search engine")]
pub struct Cli {
    /// Path to the `.localrag1/` directory (auto-discovered if omitted).
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Initialize `.localrag1/` in the current directory (idempotent).
    Init,
    /// Build / refresh the index.
    Index,
    /// Watch the cwd and incrementally update the index.
    Watch,
    /// Run a search.
    Search {
        /// The query string. Examples: `"26AS bank"`, `"design patterns"`.
        query: String,
        /// Force a particular tier (0..3). Default: walk tiers.
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=3))]
        tier: Option<u8>,
    },
    /// Get or set configuration values.
    Config {
        #[command(subcommand)]
        action: cmd_config::ConfigCmd,
    },
}

/// Top-level dispatcher.
pub fn run(args: Cli) -> Result<()> {
    match args.cmd {
        Cmd::Init => cmd_init::run(args.data_dir.as_deref()),
        Cmd::Index => cmd_index::run(args.data_dir.as_deref()),
        Cmd::Watch => cmd_watch::run(args.data_dir.as_deref()),
        Cmd::Search { query, tier } => {
            let ctx = ensure_project(args.data_dir.as_deref())?;
            cmd_search::run(&ctx, &query, tier)
        }
        Cmd::Config { action } => {
            let ctx = ensure_project(args.data_dir.as_deref())?;
            cmd_config::run(&ctx, action)
        }
    }
}

/// Either find an existing `.localrag1/` (search/init) or error out.
fn ensure_project(provided: Option<&std::path::Path>) -> Result<ProjectContext> {
    let data = paths::find_data_dir(provided)?;
    ProjectContext::open(&data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_search_subcommand() {
        let cli = Cli::try_parse_from(["sl", "search", "26AS bank"]).unwrap();
        match cli.cmd {
            Cmd::Search { query, tier } => {
                assert_eq!(query, "26AS bank");
                assert!(tier.is_none());
            }
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn parses_config_subcommand() {
        let cli = Cli::try_parse_from(["sl", "config", "show"]).unwrap();
        assert!(matches!(cli.cmd, Cmd::Config { .. }));
    }
}
