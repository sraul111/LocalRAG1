//! HTTP client for LLM providers.
//!
//! v1 supports two providers:
//! - `"ollama"` → POST {endpoint}/api/chat
//! - `"openai-compatible"` → POST {endpoint}/chat/completions
//!
//! The request and response shapes are slightly different; we always
//! normalize to one internal shape: `LlmReply`.

use serde::{Deserialize, Serialize};

use crate::config::LlmConfig;
use crate::error::{Error, Result};

use super::LlmReply;

/// One message in the LLM conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String, // "system" | "user" | "assistant" | "tool"
    pub content: String,
}

/// Send a chat completion request and return the parsed reply.
///
/// `endpoint` is e.g. `http://localhost:11434`, `messages` is the full
/// conversation so far. Returns [`Error::LlmTransport`] on network/IO
/// failure, [`Error::LlmResponse`] on malformed responses.
pub fn chat(llm: &LlmConfig, messages: Vec<Message>) -> Result<LlmReply> {
    match llm.provider.to_string().as_str() {
        "ollama" => chat_ollama(llm, messages),
        "openai-compatible" => chat_openai_compat(llm, messages),
        other => Err(Error::other(format!("unknown llm provider '{other}'"))),
    }
}

// ---- Ollama --------------------------------------------------------

fn chat_ollama(llm: &LlmConfig, messages: Vec<Message>) -> Result<LlmReply> {
    let url = format!("{}/api/chat", llm.endpoint.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": llm.model,
        "stream": false,
        "messages": messages.iter().map(|m| {
            serde_json::json!({"role": m.role, "content": m.content})
        }).collect::<Vec<_>>(),
        "format": "json",
    });

    let resp: OllamaResponse = send(&url, &body, llm.api_key.as_deref())?;
    let content = resp
        .message
        .map(|m| m.content)
        .ok_or_else(|| Error::LlmResponse("missing message content".into()))?;
    parse_reply(&content)
}

// ---- OpenAI-compatible ---------------------------------------------

fn chat_openai_compat(llm: &LlmConfig, messages: Vec<Message>) -> Result<LlmReply> {
    let url = format!("{}/chat/completions", llm.endpoint.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": llm.model,
        "stream": false,
        "response_format": { "type": "json_object" },
        "messages": messages.iter().map(|m| {
            serde_json::json!({"role": m.role, "content": m.content})
        }).collect::<Vec<_>>(),
    });
    let resp: OpenAIResponse = send(&url, &body, llm.api_key.as_deref())?;
    let content = resp
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| Error::LlmResponse("no choices in response".into()))?;
    parse_reply(&content)
}

// ---- shared transport + parse -------------------------------------

fn send<T: for<'de> Deserialize<'de>>(
    url: &str,
    body: &serde_json::Value,
    api_key: Option<&str>,
) -> Result<T> {
    let mut req = ureq::post(url)
        .set("Content-Type", "application/json");
    if let Some(k) = api_key {
        req = req.set("Authorization", &format!("Bearer {k}"));
    }
    let resp = req
        .send_json(body)
        .map_err(|e| Error::LlmTransport(e.to_string()))?;
    let txt = resp
        .into_string()
        .map_err(|e| Error::LlmTransport(e.to_string()))?;
    serde_json::from_str(&txt).map_err(|e| Error::LlmResponse(e.to_string()))
}

/// The LLM's `content` field is a string containing JSON. Parse it into our
/// internal [`LlmReply`]. If parsing fails, surface as LlmResponse — callers
/// can decide whether to retry or give up.
fn parse_reply(content: &str) -> Result<LlmReply> {
    // Strip any markdown fences (some models wrap JSON in ```json ... ```).
    let trimmed = strip_fences(content);
    serde_json::from_str(&trimmed).map_err(|e| Error::LlmResponse(e.to_string()))
}

fn strip_fences(s: &str) -> String {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("```") {
        if let Some(rest) = inner.strip_suffix("```") {
            // Skip the optional first line (e.g. "json").
            return rest.lines().skip(1).collect::<Vec<_>>().join("\n");
        }
    }
    s.to_string()
}

// ---- response shapes (provider-specific) --------------------------

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: Option<OllamaMsg>,
}
#[derive(Debug, Deserialize)]
struct OllamaMsg {
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}
#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}
#[derive(Debug, Deserialize)]
struct OpenAIMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_fences_handles_json_block() {
        let s = "```json\n{\"a\":1}\n```";
        let inner = strip_fences(s);
        assert!(inner.contains("\"a\":1"));
    }

    #[test]
    fn strip_fences_returns_input_when_no_fence() {
        let s = "{\"a\":1}";
        assert_eq!(strip_fences(s), s);
    }

    #[test]
    fn unknown_provider_errors() {
        let cfg = LlmConfig {
            enabled: true,
            provider: crate::config::llm::Provider::new("wat"),
            model: "x".into(),
            endpoint: "http://x".into(),
            api_key: None,
        };
        let r = chat(&cfg, vec![Message {
            role: "user".into(),
            content: "{}".into()
        }]);
        assert!(r.is_err());
    }
}
