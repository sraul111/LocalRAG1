//! File-type heuristic for "should we inline this in FTS5?".
//!
//! Binary content (images, compiled artifacts) gets filename + metadata
//! indexed but no body content. Text-ish formats get inlined. The list
//! is intentionally conservative — we can always broaden later.

/// True if files with extension `ext` and `size` bytes should have their
/// content inlined into the FTS5 table. Pure metadata + filename matching
/// is always available regardless of this flag.
pub fn is_indexable(ext: &str, size: u64) -> bool {
    // 8 MB hard cap on what we'll actually read. Config's max_file_size_mb
    // already filters at the crawler, but we double-check here in case the
    // indexer is called directly (e.g. tests, or a future incremental flow).
    const READ_LIMIT: u64 = 8 * 1024 * 1024;
    if size > READ_LIMIT {
        return false;
    }

    matches!(
        ext,
        // Plain text / markup
        | "txt"
        | "md"
        | "markdown"
        | "rst"
        | "adoc"
        | "org"
        // Source code (common languages)
        | "rs"
        | "py"
        | "js"
        | "jsx"
        | "ts"
        | "tsx"
        | "go"
        | "java"
        | "kt"
        | "swift"
        | "c"
        | "h"
        | "cpp"
        | "cc"
        | "hpp"
        | "cs"
        | "rb"
        | "php"
        | "sh"
        | "bash"
        | "ps1"
        | "lua"
        | "scala"
        | "pl"
        // Config / data
        | "json"
        | "yaml"
        | "yml"
        | "toml"
        | "ini"
        | "cfg"
        | "conf"
        | "properties"
        | "xml"
        | "csv"
        | "tsv"
        | "sql"
        | "graphql"
        | "proto"
        // Shell/script-adjacent
        | "html"
        | "css"
        | "scss"
        | "less"
        | "vue"
        | "svelte"
        | "elm"
        // Docs
        | "tex"
        | "bib"
        | "log"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_indexable() {
        assert!(is_indexable("md", 100));
        assert!(is_indexable("rs", 100));
        assert!(is_indexable("json", 100));
    }

    #[test]
    fn binaries_are_not() {
        assert!(!is_indexable("png", 100));
        assert!(!is_indexable("exe", 100));
        assert!(!is_indexable("o", 100));
        assert!(!is_indexable("so", 100));
        assert!(!is_indexable("dll", 100));
        assert!(!is_indexable("zip", 100));
    }

    #[test]
    fn huge_files_dropped() {
        // Even a .txt larger than 8 MB won't be inlined.
        assert!(!is_indexable("txt", 9 * 1024 * 1024));
        assert!(is_indexable("txt", 1024));
    }
}
