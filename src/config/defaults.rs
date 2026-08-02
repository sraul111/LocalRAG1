//! Default values used by [`super::Config`]. Centralized so the surface
//! is grep-able and tests can assert against a single source of truth.

/// Default exclude patterns. These are merged with `.gitignore` content
/// (when `respect_gitignore` is true) at crawl time.
///
/// We deliberately keep this short — the user can edit it.
pub fn exclude() -> Vec<String> {
    vec![
        // Version control / IDE
        ".git".into(),
        ".hg".into(),
        ".svn".into(),
        ".idea".into(),
        ".vscode".into(),
        // Build outputs
        "target".into(),
        "node_modules".into(),
        "dist".into(),
        "build".into(),
        "__pycache__".into(),
        ".venv".into(),
        "venv".into(),
        // Our own data dir (never index the index)
        ".localrag1".into(),
    ]
}
