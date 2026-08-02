# LocalRAG1

A minimal, single-binary local search engine for any folder you're
sitting in. Launched with `sl`.

**What's in v1:** a Rust binary that initializes on the **current working
directory**, crawls that folder's tree, builds a SQLite FTS5 trigram
index over file content + filenames, and answers search queries in
<100ms using filename match + keyword search + metadata filters —
no Python, no vector DB, no bundled model, no sidecar.

**What's optional in v1:** a Tier-3 LLM agent loop, for the long tail
of queries that genuinely need semantic reasoning. Off by default,
on with a one-line config change.

`sl` is cwd-scoped: every command operates on the folder it was typed
in (the index lives under that folder's `.localrag1/`). You can have
`sl` running in ten different folders with ten independent indexes.

See [`Architecture.md`](./Architecture.md) for the full design.

## Quick start (when built)

```bash
# cd into any folder, then:
sl init                  # one-time: create .localrag1/ here
sl index                 # build the FTS5 index of this folder
sl search "26AS bank details"
sl search "design patterns repos"
sl watch                 # incremental updates on file changes (Phase A end)

# Opt into the LLM tier (requires Ollama or a BYO endpoint)
sl config set llm.enabled true
sl search "that thing about inheritance I wrote last year" --tier 3
```

## Status

Architecture-only repo as of this commit. No code yet. See
`Architecture.md` § "Build phases" for the planned order.
