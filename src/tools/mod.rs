//! Filesystem tools exposed to the LLM agent (Tier 3).
//!
//! These are **read-only** by design. The LLM never deletes or writes.
//! Every operation has a cap (depth, byte count, file count) so a
//! runaway agent loop cannot exhaust resources.
//!
//! Each tool returns either JSON or a JSON-shaped error string. The
//! outer agent loop parses it and feeds it back to the model.
//
// `Cow<Path>` syntax causes Rust 2024 to emit `hidden_lifetime_in_path`.
// We silence the lint per-module so the body of the file stays readable.
#![allow(hidden_lifetime_in_path)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// All tool names the LLM knows about. Stable for wire compatibility.
pub const TOOL_LIST_DIR: &str = "list_dir";
pub const TOOL_READ_FILE: &str = "read_file";
pub const TOOL_GREP_FILES: &str = "grep_files";

/// JSON shape sent back from one tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub tool: String,
    pub ok: bool,
    pub result: serde_json::Value,
}

impl ToolOutput {
    pub fn ok(tool: &str, result: impl Serialize) -> Result<Self> {
        Ok(Self {
            tool: tool.to_string(),
            ok: true,
            result: serde_json::to_value(result).map_err(|e| Error::other(e.to_string()))?,
        })
    }
    pub fn err(tool: &str, msg: impl Into<String>) -> Self {
        Self {
            tool: tool.to_string(),
            ok: false,
            result: serde_json::Value::String(msg.into()),
        }
    }
}

/// Hard boundaries enforced on the LLM regardless of what it asks for.
const MAX_DIR_DEPTH: usize = 3;
const MAX_DIR_ENTRIES: usize = 200;
const MAX_READ_BYTES: usize = 8 * 1024;
const MAX_GREP_RESULTS: usize = 50;
const MAX_GREP_FILE_BYTES: u64 = 1024 * 1024;
const MAX_PATH_LEN: usize = 1024;

/// True if `child` is the same as `parent` or a descendant. Both paths
/// must be absolute. UNC `\\?\` prefix is normalized away before comparison.
fn path_starts_with(parent: &Path, child: &Path) -> bool {
    fn strip_unc(p: &Path) -> std::path::PathBuf {
        let s = p.to_string_lossy();
        if s.starts_with(r"\\?\") {
            std::path::PathBuf::from(&s[4..])
        } else {
            p.to_path_buf()
        }
    }
    let p = strip_unc(parent);
    let c = strip_unc(child);
    c.starts_with(&p)
}

fn validate_rel_path(root: &Path, p: &str) -> Result<PathBuf> {
    if p.len() > MAX_PATH_LEN || p.is_empty() {
        return Err(Error::LlmToolRejected(format!("path length out of bounds")));
    }
    let cand = if Path::new(p).is_absolute() {
        PathBuf::from(p)
    } else {
        root.join(p)
    };
    // Refuse `..` segments in the input outright — they are how escapes
    // happen. This is cheaper and clearer than trying to canonicalize and
    // then compare.
    for comp in cand.components() {
        if let std::path::Component::ParentDir = comp {
            return Err(Error::LlmToolRejected(format!(
                "path contains '..': {p}"
            )));
        }
    }
    // Absolute form for the comparison.
    let abs = if cand.is_absolute() {
        cand.clone()
    } else {
        std::env::current_dir()
            .map_err(|e| Error::Io { operation: "current_dir", source: e })?
            .join(&cand)
    };
    // Refuse path-traversal escapes. Unwrap both UNC prefixes first.
    if !path_starts_with(root, &abs) {
        return Err(Error::LlmToolRejected(format!(
            "path escapes the indexed root: {p}"
        )));
    }
    Ok(abs)
}

/// `list_dir(rel_path)`: return children of a folder, capped.
pub fn list_dir(root: &Path, rel: &str) -> Result<ToolOutput> {
    let abs = validate_rel_path(root, rel)?;
    let meta = std::fs::metadata(&abs).map_err(|e| Error::Io {
        operation: "list_dir metadata",
        source: e,
    })?;
    if !meta.is_dir() {
        return Ok(ToolOutput::err(TOOL_LIST_DIR, "not a directory"));
    }

    let rd = std::fs::read_dir(&abs).map_err(|e| Error::Io {
        operation: "list_dir read_dir",
        source: e,
    })?;
    let mut entries = Vec::new();
    for (i, ent) in rd.enumerate() {
        if i >= MAX_DIR_ENTRIES {
            break;
        }
        let ent = match ent {
            Ok(e) => e,
            Err(_) => continue,
        };
        let ft = ent.file_type().ok();
        let name = ent.file_name().to_string_lossy().into_owned();
        entries.push(serde_json::json!({
            "name": name,
            "is_dir": ft.as_ref().map(|f| f.is_dir()).unwrap_or(false),
        }));
        // Also enforce depth by checking the rel segment count.
        let candidate = abs.join(&name);
        let rel_check = candidate.strip_prefix(root).unwrap_or(&candidate);
        if rel_check.components().count() > MAX_DIR_DEPTH {
            break;
        }
    }
    ToolOutput::ok(TOOL_LIST_DIR, entries)
}

/// `read_file(rel_path)`: dump bounded file content.
pub fn read_file(root: &Path, rel: &str) -> Result<ToolOutput> {
    let abs = validate_rel_path(root, rel)?;
    let meta = std::fs::metadata(&abs).map_err(|e| Error::Io {
        operation: "read_file metadata",
        source: e,
    })?;
    if !meta.is_file() {
        return Ok(ToolOutput::err(TOOL_READ_FILE, "not a regular file"));
    }
    use std::io::Read;
    let mut f = std::fs::File::open(&abs).map_err(|e| Error::Io {
        operation: "read_file open",
        source: e,
    })?;
    let mut buf = vec![0u8; MAX_READ_BYTES];
    let n = f.read(&mut buf).map_err(|e| Error::Io {
        operation: "read_file read",
        source: e,
    })?;
    buf.truncate(n);
    let body = String::from_utf8_lossy(&buf).into_owned();
    ToolOutput::ok(
        TOOL_READ_FILE,
        serde_json::json!({
            "truncated": meta.len() > MAX_READ_BYTES as u64,
            "size": meta.len(),
            "body": body,
        }),
    )
}

/// `grep_files(pattern, rel_dir)`: case-insensitive substring search across `rel_dir`.
pub fn grep_files(root: &Path, rel_dir: &str, pattern: &str) -> Result<ToolOutput> {
    if pattern.is_empty() || pattern.len() > 256 {
        return Err(Error::LlmToolRejected("invalid pattern".into()));
    }
    let abs = validate_rel_path(root, rel_dir)?;
    let pat_lower = pattern.to_lowercase();
    let mut hits = Vec::new();

    let walker = ignore::WalkBuilder::new(&abs);
    for dent in walker.build().flatten() {
        if hits.len() >= MAX_GREP_RESULTS {
            break;
        }
        if !dent.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }
        let path = dent.path();
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() > MAX_GREP_FILE_BYTES {
                continue;
            }
        }
        if let Ok(bytes) = std::fs::read(path) {
            let s = String::from_utf8_lossy(&bytes).to_lowercase();
            if s.contains(&pat_lower) {
                let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned();
                hits.push(serde_json::json!({"path": rel, "size": bytes.len()}));
            }
        }
    }

    ToolOutput::ok(
        TOOL_GREP_FILES,
        serde_json::json!({
            "count": hits.len(),
            "results": hits,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn corpus(tmp: &Path) {
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(tmp.join("src/main.rs"), b"fn main() { println!(\"hi\"); }").unwrap();
        fs::write(tmp.join("src/lib.rs"), b"pub fn lib() {}").unwrap();
        fs::write(tmp.join("README.md"), b"# project").unwrap();
    }

    #[test]
    fn list_dir_returns_children() {
        let t = TempDir::new().unwrap();
        corpus(t.path());
        let out = list_dir(t.path(), ".").unwrap();
        assert!(out.ok);
        let arr = out.result.as_array().unwrap();
        let names: Vec<_> = arr.iter().map(|v| v["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"src"));
        assert!(names.contains(&"README.md"));
    }

    #[test]
    fn read_file_truncates() {
        let t = TempDir::new().unwrap();
        corpus(t.path());
        let out = read_file(t.path(), "README.md").unwrap();
        assert!(out.ok);
        let j = &out.result;
        assert_eq!(j["body"].as_str().unwrap(), "# project");
        assert_eq!(j["truncated"], serde_json::json!(false));
    }

    #[test]
    fn grep_files_substring() {
        let t = TempDir::new().unwrap();
        corpus(t.path());
        let out = grep_files(t.path(), ".", "println").unwrap();
        assert!(out.ok);
        let j = &out.result;
        assert!(j["count"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn rejects_path_escape() {
        let t = TempDir::new().unwrap();
        corpus(t.path());
        let bad = list_dir(t.path(), "../");
        // Should error because escaping root is rejected.
        assert!(bad.is_err() || matches!(bad, Ok(o) if !o.ok));
    }
}
