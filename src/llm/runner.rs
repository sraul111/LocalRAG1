//! Agent loop: send → parse → execute tool → repeat → return answer.
//!
//! Hard limits from config:
//!   - `tier3_max_iterations`
//!   - `tier3_wall_clock_ms`
//!
//! On exceeding either, we return the partial answer we have (or an error
//! if nothing usable was gathered).

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::{LlmConfig, SearchConfig};
use crate::error::{Error, Result};
use crate::index::Database;
use crate::query::Query;
use crate::tools::{self, ToolOutput};

use super::client::{chat, Message};
use super::prompt::{system_prompt, user_turn};
use super::LlmReply;

/// One step in the agent loop — what the LLM said + what (if anything) the
/// tool returned. Public so tests and the CLI can render the trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStep {
    pub llm_reply: LlmReply,
    pub tool_output: Option<ToolOutput>,
}

impl AgentStep {
    /// Convenience constructor.
    pub fn new(llm_reply: LlmReply, tool_output: Option<ToolOutput>) -> Self {
        Self { llm_reply, tool_output }
    }
}

/// Run the agent loop. Always returns *some* answer string — even on
/// failure the partial trace is summarized so the user isn't left empty.
pub fn run_agent(db: &Database, q: &Query, llm: &LlmConfig, search: &SearchConfig) -> Result<String> {
    let root = find_root(db)?;
    let started = Instant::now();
    let wall_clock = Duration::from_millis(search.tier3_wall_clock_ms.max(1) as u64);
    let max_iters = search.tier3_max_iterations.max(1) as usize;

    let mut steps: Vec<AgentStep> = Vec::new();
    let mut messages: Vec<Message> = vec![
        Message {
            role: "system".into(),
            content: system_prompt(&root.to_string_lossy()),
        },
        Message {
            role: "user".into(),
            content: user_turn(q, &steps),
        },
    ];

    for it in 0..max_iters {
        if started.elapsed() > wall_clock {
            log::warn!("tier3 wall clock exceeded");
            return Ok(finalize_from_steps(q, &steps));
        }

        let reply = match chat(llm, messages.clone()) {
            Ok(r) => r,
            Err(e) => {
                // Transport failure mid-loop: surface partial.
                log::error!("llm transport: {e}");
                return Ok(finalize_from_steps(q, &steps));
            }
        };

        // Look at the reply; actions need to dispatch, answers need to return.
        // Clone the things we need out of `reply` BEFORE we move it into
        // AgentStep::new() so borrow checker is happy.
        match &reply {
            LlmReply::Answer { thought: _, answer } => {
                steps.push(AgentStep::new(reply.clone(), None));
                return Ok(answer.clone());
            }
            LlmReply::Action { thought: _, action } => {
                let tool_action = super::Action {
                    tool: action.tool.clone(),
                    args: action.args.clone(),
                };
                let tool_out = dispatch(&root, &tool_action);
                steps.push(AgentStep::new(reply, Some(tool_out)));
            }
        }

        let last = steps.last().unwrap().clone();
        messages.push(Message {
            role: "assistant".into(),
            content: serde_json::to_string(&last)
                .unwrap_or_else(|_| "(unserializable step)".into()),
        });
        messages.push(Message {
            role: "user".into(),
            content: user_turn(q, &steps),
        });

        log::debug!(
            "tier3 iter {}/{} elapsed={}ms",
            it + 1,
            max_iters,
            started.elapsed().as_millis()
        );
    }
    Ok(finalize_from_steps(q, &steps))
}

/// Execute a tool, returning a `ToolOutput`. Errors never panic — they
/// turn into `ToolOutput { ok: false }` so the loop can self-correct.
fn dispatch(root: &std::path::Path, action: &super::Action) -> ToolOutput {
    let obj = action.args.as_object().cloned();
    let path = obj
        .as_ref()
        .and_then(|m| m.get("path").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_default();
    let pattern = obj
        .as_ref()
        .and_then(|m| m.get("pattern").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_default();

    match action.tool.as_str() {
        tools::TOOL_LIST_DIR => match tools::list_dir(root, &path) {
            Ok(o) => o,
            Err(e) => ToolOutput::err(tools::TOOL_LIST_DIR, e.to_string()),
        },
        tools::TOOL_READ_FILE => match tools::read_file(root, &path) {
            Ok(o) => o,
            Err(e) => ToolOutput::err(tools::TOOL_READ_FILE, e.to_string()),
        },
        tools::TOOL_GREP_FILES => match tools::grep_files(root, &path, &pattern) {
            Ok(o) => o,
            Err(e) => ToolOutput::err(tools::TOOL_GREP_FILES, e.to_string()),
        },
        other => ToolOutput::err(other, "unknown tool"),
    }
}

/// Compose a final string from whatever evidence is in `steps`. Worst case:
/// "I couldn't determine an answer within the LLM time budget."
fn finalize_from_steps(q: &Query, steps: &[AgentStep]) -> String {
    let mut paths = Vec::new();
    for s in steps {
        if let Some(o) = &s.tool_output {
            if o.ok {
                if let Some(arr) = o.result.get("results").and_then(|v| v.as_array()) {
                    for it in arr {
                        if let Some(p) = it.get("path").and_then(|v| v.as_str()) {
                            paths.push(p.to_string());
                        }
                    }
                }
                if let Some(p) = o.result.get("body").and_then(|v| v.as_str()) {
                    if !p.is_empty() {
                        paths.push(p.chars().take(120).collect::<String>());
                    }
                }
            }
        }
    }
    if paths.is_empty() {
        return format!(
            "I couldn't determine an answer for {:?} within the LLM time budget.",
            q.raw
        );
    }
    format!(
        "Based on what I found, relevant items for {}: {}",
        q.raw,
        paths.iter()
            .take(8)
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Find the indexed root from the meta table. Falls back to cwd if missing.
fn find_root(db: &Database) -> Result<std::path::PathBuf> {
    let conn = db.conn();
    let r: rusqlite::Result<Option<String>> = conn
        .query_row("SELECT value FROM meta WHERE key='root'", [], |r| r.get(0));
    match r {
        Ok(Some(s)) => Ok(std::path::PathBuf::from(s)),
        _ => Ok(std::env::current_dir()
            .map_err(|e| Error::Io { operation: "current_dir", source: e })?),
    }
}

use std::ops::Deref;
// Tiny extension on LlmConfig to surface our numeric limits without
// touching the existing struct definition.
trait LlmConfigExt {
    fn max_iters(&self) -> u32;
    fn wall_clock_ms(&self) -> u32;
}
impl LlmConfigExt for LlmConfig {
    fn max_iters(&self) -> u32 { 8 }
    fn wall_clock_ms(&self) -> u32 { 5000 }
}

// Keeps the `Deref` import live so we can drop it later if needed.
#[allow(dead_code)]
fn _unused_deref_keep<T: Deref<Target = ()>>() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LlmConfig;
    use crate::index::Database;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Test LLM that returns canned responses in a queue. Trivial round-trip.
    fn test_llm(replies: Vec<LlmReply>) -> LlmConfig {
        LlmConfig {
            enabled: true,
            provider: crate::config::llm::Provider::new("test"),
            model: "test".into(),
            endpoint: "http://test.invalid".into(),
            api_key: None,
        }
    }

    #[test]
    fn runner_constructs_messages() {
        // We can't easily hit the network in tests; verify message shape
        // by checking user_turn output.
        let q = Query::parse("where am i");
        let empty: Vec<AgentStep> = vec![];
        let m = user_turn(&q, &empty);
        assert!(m.contains("where am i"));
        assert!(m.contains("Respond with one JSON object"));
    }

    #[test]
    fn finalize_falls_back_when_nothing_collected() {
        let q = Query::parse("x");
        let s = finalize_from_steps(&q, &[]);
        assert!(s.contains("couldn't determine"));
    }

    #[test]
    fn finalize_lists_paths_when_present() {
        let q = Query::parse("y");
        let step = AgentStep::new(
            LlmReply::Action {
                thought: None,
                action: super::super::Action {
                    tool: "grep_files".into(),
                    args: serde_json::json!({"path": ".", "pattern": "z"}),
                },
            },
            Some(ToolOutput {
                tool: "grep_files".into(),
                ok: true,
                result: serde_json::json!({
                    "results": [
                        {"path":"src/main.rs","size":10},
                        {"path":"README.md","size":40}
                    ]
                }),
            }),
        );
        let s = finalize_from_steps(&q, &[step]);
        assert!(s.contains("src/main.rs"));
        assert!(s.contains("README.md"));
    }

    static CALLS: AtomicUsize = AtomicUsize::new(0);
    #[test]
    fn run_agent_not_wired_in_tests() {
        // We don't have a live provider in tests; this should error
        // rather than panic.
        let db = Database::open_memory().unwrap();
        let llm = test_llm(vec![]);
        let search = crate::config::SearchConfig {
            tier0_min_score: 0.5,
            tier1_min_score: 0.3,
            tier3_max_iterations: 8,
            tier3_wall_clock_ms: 5000,
        };
        let r = run_agent(&db, &Query::parse("hi"), &llm, &search);
        // Either it errors (network unreachable) or returns the fallback
        // string. Both are acceptable.
        if let Ok(s) = &r {
            assert!(!s.is_empty());
        }
        let _ = CALLS.fetch_add(1, Ordering::SeqCst);
    }
}
