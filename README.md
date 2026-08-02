# LocalRAG1

A minimal, single-binary local search engine for your C: drive.

**What's in v1:** a Rust process that crawls your drives, builds a
SQLite FTS5 trigram index over file content + filenames, and answers
search queries in <100ms using filename match + keyword search +
metadata filters — no Python, no vector DB, no bundled model, no
sidecar.

**What's optional in v1:** a Tier-3 LLM agent loop, for the long tail
of queries that genuinely need semantic reasoning. Off by default,
on with a one-line config change.

See [`Architecture.md`](./Architecture.md) for the full design.

## Quick start (when built)

```bash
# Build
cargo build --release

# Index your C: drive (one-time, then incremental)
localrag1 index

# Search
localrag1 search "26AS bank details"
localrag1 search "design patterns repos"

# Opt into the LLM tier (requires Ollama or a BYO endpoint)
localrag1 config set llm.enabled true
localrag1 search "that thing about inheritance I wrote last year" --tier 3
```

## Status

Architecture-only repo as of this commit. No code yet. See
`Architecture.md` § "Build phases" for the planned order.
