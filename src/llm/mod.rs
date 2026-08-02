//! Tier 3 — optional LLM agent loop.
//!
//! Wire protocol is intentionally simple:
//!
//! 1. The agent builds a single system+user prompt describing the question,
//!    the available tools, and prior tool-call history.
//! 2. The LLM returns a JSON object like:
//!
//!    { "thought": "I should list the project root",
//!      "action": { "tool": "list_dir", "args": { "path": "." } } }
//!
//!    OR, when the LLM has enough info:
//!
//!    { "thought": "...",
//!      "answer": "the file is at src/old/notes.md" }
//!
//! 3. The agent dispatches tools (read-only), accumulates results, repeats
//!    up to `tier3_max_iterations`. Hard wall-clock cap.
//!
//! Providers:
//!   - "ollama"   → POST {endpoint}/api/chat
//!   - "openai-compatible" → POST {endpoint}/chat/completions
//!
//! No streaming for v1. Synchronous request/response keeps the code short.

pub mod client;
pub mod prompt;
pub mod runner;

use serde::{Deserialize, Serialize};

use crate::config::{LlmConfig, SearchConfig};
use crate::error::Result;
use crate::index::Database;
use crate::query::Query;

pub use runner::{run_agent, AgentStep};

/// What the LLM sent back. Either a tool call or a final answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LlmReply {
    Action {
        thought: Option<String>,
        action: Action,
    },
    Answer {
        thought: Option<String>,
        answer: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub tool: String,
    pub args: serde_json::Value,
}

/// Top-level entry point called by the query pipeline.
pub fn agent(db: &Database, q: &Query) -> Result<String> {
    // Reserved for future: pull config from the database meta table.
    let _ = q;
    let _ = db;
    Err(crate::error::Error::other(
        "Tier 3 LLM not yet wired — set [llm] provider first",
    ))
}

/// Public alias for tests/CLI to invoke the agent loop with an explicit config.
pub fn run(
    db: &Database,
    q: &Query,
    llm: &LlmConfig,
    search: &SearchConfig,
) -> Result<String> {
    if !llm.enabled {
        return Err(crate::error::Error::other("llm disabled"));
    }
    run_agent(db, q, llm, search)
}
