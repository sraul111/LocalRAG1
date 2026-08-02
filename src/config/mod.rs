//! Configuration model and persistence.
//!
//! The config is a single TOML file at `.localrag1/config.toml`. Every
//! field has a sensible default — the file is fully optional. Public
//! surface is the [`Config`] struct plus a [`load`] / [`save`] pair.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub mod defaults;
pub mod llm;

pub use llm::LlmConfig;

/// Top-level config. Mirrors the TOML schema in `Architecture.md`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Indexer behavior.
    pub index: IndexConfig,
    /// Search + tier-cutoff behavior.
    pub search: SearchConfig,
    /// LLM provider options (Tier 3).
    pub llm: LlmConfig,
}

/// Indexer settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IndexConfig {
    /// Glob/folder patterns to skip, in addition to `.gitignore` (if enabled).
    #[serde(default = "defaults::exclude")]
    pub exclude: Vec<String>,
    /// Hard cap on per-file bytes that will be hashed/inlined into the FTS table.
    pub max_file_size_mb: u32,
    /// Whether to parse `.gitignore` files inside the indexed tree.
    pub respect_gitignore: bool,
    /// Whether to follow symbolic links during crawl. Default false (safer).
    #[serde(default)]
    pub follow_symlinks: bool,
}

/// Search-cutoff settings. These decide which tier handles a given query.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SearchConfig {
    /// Minimum confidence for Tier 0 (filename) to short-circuit.
    pub tier0_min_score: f32,
    /// Minimum confidence for Tier 1 (FTS5) to short-circuit.
    pub tier1_min_score: f32,
    /// Max LLM agent-loop iterations.
    pub tier3_max_iterations: u32,
    /// Hard wall-clock cap for Tier 3.
    pub tier3_wall_clock_ms: u32,
}

impl Config {
    /// Build a config with all defaults (no file on disk).
    pub fn defaults() -> Self {
        Self {
            index: IndexConfig {
                exclude: defaults::exclude(),
                max_file_size_mb: 200,
                respect_gitignore: true,
                follow_symlinks: false,
            },
            search: SearchConfig {
                tier0_min_score: 0.5,
                tier1_min_score: 0.3,
                tier3_max_iterations: 8,
                tier3_wall_clock_ms: 5000,
            },
            llm: LlmConfig::defaults(),
        }
    }

    /// Load a config from `path`. If the file doesn't exist, return defaults.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::defaults());
        }
        let text = std::fs::read_to_string(path).map_err(|e| Error::Io {
            operation: "read config",
            source: e,
        })?;
        let cfg: Config = toml::from_str(&text).map_err(|e| Error::InvalidConfig {
            path: path.to_path_buf(),
            source: e,
        })?;
        Ok(cfg)
    }

    /// Serialize + write to `path`, creating parent dirs if needed.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Io {
                operation: "create_dir_all (config parent)",
                source: e,
            })?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| Error::Other(e.to_string()))?;
        std::fs::write(path, text).map_err(|e| Error::Io {
            operation: "write config",
            source: e,
        })?;
        Ok(())
    }

    /// Set a dotted key like `"index.max_file_size_mb" = 200`.
    /// Returns the new config and a flag indicating whether anything changed.
    pub fn set(&mut self, key: &str, value: &str) -> Result<bool> {
        let mut parts = key.split('.');
        let section = parts
            .next()
            .ok_or_else(|| Error::other("config key requires a section, e.g. 'index.foo'"))?;
        let field = parts
            .next()
            .ok_or_else(|| Error::other("config key requires a field, e.g. 'index.foo'"))?;
        if parts.next().is_some() {
            return Err(Error::other("config keys are at most two levels deep"));
        }

        match (section, field) {
            ("index", "max_file_size_mb") => {
                let v: u32 = value
                    .parse()
                    .map_err(|_| Error::other("expected u32 for max_file_size_mb"))?;
                self.index.max_file_size_mb = v;
            }
            ("index", "respect_gitignore") => {
                let v: bool = value
                    .parse()
                    .map_err(|_| Error::other("expected true/false for respect_gitignore"))?;
                self.index.respect_gitignore = v;
            }
            ("index", "follow_symlinks") => {
                let v: bool = value
                    .parse()
                    .map_err(|_| Error::other("expected true/false for follow_symlinks"))?;
                self.index.follow_symlinks = v;
            }
            ("llm", "enabled") => {
                let v: bool = value
                    .parse()
                    .map_err(|_| Error::other("expected true/false for llm.enabled"))?;
                self.llm.enabled = v;
            }
            ("llm", "model") => {
                self.llm.model = value.to_string();
            }
            ("llm", "endpoint") => {
                self.llm.endpoint = value.to_string();
            }
            ("llm", "provider") => {
                self.llm.provider = crate::config::llm::Provider::new(value);
            }
            _ => {
                return Err(Error::other(format!(
                    "unknown config key '{key}' — run `sl config` for available keys"
                )));
            }
        }
        Ok(true)
    }
}

/// Where config + index data lives for the currently-running invocation.
#[derive(Debug, Clone)]
pub struct ProjectContext {
    /// Absolute path to `.localrag1/`.
    pub data_dir: PathBuf,
    /// Absolute path to the SQLite index inside the data dir.
    pub index_path: PathBuf,
    /// Absolute path to the config file inside the data dir.
    pub config_path: PathBuf,
    /// Loaded config (always populated; falls back to defaults).
    pub config: Config,
}

impl ProjectContext {
    /// Open the project at `data_dir` (must already exist). Loads config.
    pub fn open(data_dir: &Path) -> Result<Self> {
        let cfg_path = crate::util::config_path(data_dir);
        let cfg = Config::load(&cfg_path)?;
        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            index_path: crate::util::index_path(data_dir),
            config_path: cfg_path,
            config: cfg,
        })
    }

    /// Persist any in-memory config edits to disk.
    pub fn save_config(&self) -> Result<()> {
        self.config.save(&self.config_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn defaults_round_trip() {
        let cfg = Config::defaults();
        let s = toml::to_string(&cfg).unwrap();
        let cfg2: Config = toml::from_str(&s).unwrap();
        assert_eq!(cfg, cfg2);
    }

    #[test]
    fn load_missing_returns_defaults() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nope.toml");
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg, Config::defaults());
    }

    #[test]
    fn set_unknown_key_errors() {
        let mut cfg = Config::defaults();
        let r = cfg.set("nope.foo", "bar");
        assert!(r.is_err());
    }

    #[test]
    fn set_then_get_back() {
        let mut cfg = Config::defaults();
        cfg.set("llm.enabled", "true").unwrap();
        cfg.set("llm.model", "qwen2.5:7b").unwrap();
        cfg.set("index.max_file_size_mb", "42").unwrap();
        assert!(cfg.llm.enabled);
        assert_eq!(cfg.llm.model, "qwen2.5:7b");
        assert_eq!(cfg.index.max_file_size_mb, 42);
    }

    #[test]
    fn save_then_load_reproduces() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("c.toml");
        let cfg = Config::defaults();
        cfg.save(&p).unwrap();
        let cfg2 = Config::load(&p).unwrap();
        assert_eq!(cfg, cfg2);
    }
}
