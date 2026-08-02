//! Normalization of a user query.

/// A user query, normalized in three ways for cheap + easy matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Original string verbatim. Used for display.
    pub raw: String,
    /// Lowercased with punctuation separated by spaces. Used for tier 0.
    pub normalized: String,
    /// Space-trimmed, ASCII-only. Used for FTS5 substring matching.
    pub cleaned: String,
}

impl Query {
    /// Build a query from raw text. Empty input is preserved verbatim so
    /// callers can decide what to do.
    pub fn parse(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let lower = raw.to_lowercase();
        // Insert spaces around non-alphanumeric so "26AS" and "26 AS" both
        // match the same rows. Keep ASCII letters/digits/+/#/./_ as word chars.
        let mut normalized = String::with_capacity(lower.len() + 8);
        let mut prev_alnum = false;
        for c in lower.chars() {
            let is_alnum =
                c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '.' | '_' | '-');
            if is_alnum {
                if !prev_alnum && !normalized.is_empty() {
                    normalized.push(' ');
                }
                normalized.push(c);
                prev_alnum = true;
            } else {
                prev_alnum = false;
            }
        }

        let cleaned = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
        Self {
            raw,
            normalized,
            cleaned,
        }
    }

    /// First word/token — used by tier 0 exact-match path.
    pub fn first_token(&self) -> &str {
        self.cleaned
            .split_whitespace()
            .next()
            .unwrap_or(self.cleaned.as_str())
    }
}

impl std::fmt::Display for Query {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lowercases() {
        let q = Query::parse("26AS Bank-Details");
        assert!(q.normalized.contains("26as"));
        assert!(q.normalized.contains("bank"));
        assert!(q.normalized.contains("details"));
    }

    #[test]
    fn parse_inserts_spaces_around_punct() {
        let q = Query::parse("hello,world!");
        assert!(q.normalized.contains("hello"));
        assert!(q.normalized.contains(" "));
        assert!(q.normalized.contains("world"));
    }

    #[test]
    fn first_token_handles_leading_whitespace() {
        let q = Query::parse("   Rust programming");
        assert_eq!(q.first_token(), "rust");
    }

    #[test]
    fn empty_query_does_not_panic() {
        let q = Query::parse("");
        assert!(q.cleaned.is_empty());
        assert_eq!(q.first_token(), "");
    }
}
