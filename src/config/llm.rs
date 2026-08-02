//! LLM provider configuration (Tier 3, opt-in).

use serde::{Deserialize, Serialize};

/// Backend identifier. Either `"ollama"` or `"openai-compatible"`.
/// We store it as a string so the user can extend it without recompile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Provider(String);

impl Provider {
    /// Construct from any `Into<String>`. Used by config setters + tests.
    pub fn new(s: impl Into<String>) -> Self {
        Provider(s.into())
    }
}

impl Default for Provider {
    fn default() -> Self {
        Provider("ollama".into())
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Tier 3 config block. Every field has a default so partial TOML works.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LlmConfig {
    /// Master switch. When false, Tier 3 is skipped entirely.
    pub enabled: bool,
    /// Provider key (`ollama` / `openai-compatible`).
    #[serde(default)]
    pub provider: Provider,
    /// Model name (e.g. `qwen2.5:7b`, `gpt-4o-mini`).
    #[serde(default = "default_model")]
    pub model: String,
    /// HTTP endpoint. Default points at Ollama's local server.
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    /// Optional API key for `openai-compatible` providers. Never logged.
    #[serde(default)]
    pub api_key: Option<String>,
}

fn default_model() -> String {
    "qwen2.5:7b".to_string()
}

fn default_endpoint() -> String {
    "http://localhost:11434".to_string()
}

impl LlmConfig {
    /// All-default config (LLM tier off).
    pub fn defaults() -> Self {
        Self {
            enabled: false,
            provider: Provider::default(),
            model: default_model(),
            endpoint: default_endpoint(),
            api_key: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_llm_off() {
        let c = LlmConfig::defaults();
        assert!(!c.enabled);
        assert_eq!(c.provider.to_string(), "ollama");
        assert_eq!(c.endpoint, "http://localhost:11434");
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let s = r#"
            enabled = true
            model   = "llama3.2:3b"
        "#;
        let c: LlmConfig = toml::from_str(s).unwrap();
        assert!(c.enabled);
        assert_eq!(c.model, "llama3.2:3b");
        assert_eq!(c.provider.to_string(), "ollama"); // default
        assert_eq!(c.endpoint, "http://localhost:11434"); // default
    }
}
