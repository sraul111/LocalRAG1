//! Prompt construction for the Tier-3 agent loop.
//!
//! The system prompt establishes:
//!   - tools and their JSON schemas
//!   - safety constraints (no shell, no writes)
//!   - exact reply shape
//!
//! User turns carry: the question, prior steps, and any tool results.

use serde_json::json;

use crate::query::Query;

use super::{Action, AgentStep, LlmReply};

/// System prompt — sent once at the start of every agent run.
pub fn system_prompt(cwd_brief: &str) -> String {
    format!(
        r#"You are `sl`, a read-only filesystem agent that helps the user
find files. You run inside the folder: {cwd_brief}.

Hard constraints:
- You may only call the tools listed below. No shell, no Python, no writes.
- Stop as soon as you have enough evidence to answer. Don't exhaust iterations.
- When you know the answer, reply with the final `answer` shape only.

Available tools (call them by emitting JSON in this exact shape):

{{ "thought": "one short sentence about your plan",
  "action": {{
    "tool": "list_dir",
    "args": {{ "path": "." }}
  }}
}}

{{ "thought": "...",
  "action": {{
    "tool": "read_file",
    "args": {{ "path": "relative/path/from/cwd" }}
  }}
}}

{{ "thought": "...",
  "action": {{
    "tool": "grep_files",
    "args": {{ "path": ".", "pattern": "substring" }}
  }}
}}

When you have enough to answer:

{{ "thought": "...", "answer": "the answer in plain English" }}

Output a single JSON object per reply. No prose before or after."#
    )
}

/// User-prompt builder for one turn. Includes the original question plus
/// the running history (thoughts, tool calls, results).
pub fn user_turn(q: &Query, steps: &[AgentStep]) -> String {
    let mut s = format!("Question: {}\n\n", q.raw);
    if !steps.is_empty() {
        s.push_str("Steps so far:\n");
        for (i, st) in steps.iter().enumerate() {
            let llm_json = match &st.llm_reply {
                LlmReply::Action { thought, action } => json!({
                    "thought": thought,
                    "tool": action.tool,
                    "args": action.args,
                })
                .to_string(),
                LlmReply::Answer { thought, answer } => {
                    json!({"thought": thought, "answer": answer}).to_string()
                }
            };
            let tool_json = match &st.tool_output {
                Some(o) => serde_json::to_string(o).unwrap_or_else(|_| "<unserializable>".into()),
                None => "null".into(),
            };
            s.push_str(&format!(
                "  Step {i}: llm={llm_json} tool_result={tool_json}\n"
            ));
        }
        s.push('\n');
    }
    s.push_str("Respond with one JSON object only.");
    s
}

/// Cheap pre-extraction of `tool + args` from raw text. Used as a fallback
/// when the LLM returns malformed JSON: we look for `{ "tool": "..." }`.
pub fn extract_action_from_text(text: &str) -> Option<Action> {
    // Naive scan; OK as a last-ditch fallback.
    let needle = "\"tool\"";
    let i = text.find(needle)?;
    let rest = &text[i..];
    // Find the `"tool": "..."` substring.
    let colon = rest.find(':')?;
    let after = &rest[colon + 1..];
    let q1 = after.find('"')?;
    let q2 = after[q1 + 1..].find('"')?;
    let tool = after[q1 + 1..q1 + 1 + q2].to_string();
    Some(Action {
        tool,
        args: serde_json::Value::Object(Default::default()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Action;

    #[test]
    fn system_prompt_mentions_tools() {
        let s = system_prompt("C:/x");
        assert!(s.contains("list_dir"));
        assert!(s.contains("read_file"));
        assert!(s.contains("grep_files"));
    }

    #[test]
    fn user_turn_includes_question() {
        let q = Query::parse("where is 26AS");
        let s = user_turn(&q, &[]);
        assert!(s.contains("where is 26AS"));
        assert!(s.contains("Steps so far:") == false);
    }

    #[test]
    fn user_turn_with_steps() {
        let q = Query::parse("hi");
        let step = AgentStep::new(
            LlmReply::Action {
                thought: Some("look".into()),
                action: Action {
                    tool: "list_dir".into(),
                    args: json!({"path": "."}),
                },
            },
            None,
        );
        let s = user_turn(&q, &[step]);
        assert!(s.contains("Steps so far:"));
        assert!(s.contains("list_dir"));
    }

    #[test]
    fn extract_action_fallback() {
        let txt = "well I'll call {\"tool\": \"read_file\", \"args\": {}}";
        let a = extract_action_from_text(txt).unwrap();
        assert_eq!(a.tool, "read_file");
    }
}
