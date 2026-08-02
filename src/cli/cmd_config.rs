//! `sl config ...` — view or modify the config.

use clap::Subcommand;

use crate::config::ProjectContext;
use crate::error::Result;

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    /// Show the current effective config.
    Show,
    /// Set a dotted config key, e.g. `sl config set llm.enabled true`.
    Set {
        /// Dotted key like `llm.enabled`, `index.max_file_size_mb`.
        key: String,
        /// Value to set (parsed by type of the key).
        value: String,
    },
    /// Print the path of the active config file.
    Path,
}

pub fn run(ctx: &ProjectContext, cmd: ConfigCmd) -> Result<()> {
    match cmd {
        ConfigCmd::Show => {
            let s = toml::to_string_pretty(&ctx.config)
                .map_err(|e| crate::error::Error::Other(e.to_string()))?;
            print!("{s}");
        }
        ConfigCmd::Set { key, value } => {
            let mut new_cfg = ctx.config.clone();
            new_cfg.set(&key, &value)?;
            new_cfg.save(&ctx.config_path)?;
            println!("Set {key} = {value}");
        }
        ConfigCmd::Path => println!("{}", ctx.config_path.display()),
    }
    Ok(())
}
