# LocalRAG1 — Project Status

> Start here when resuming work on this project.
> For *why* the project exists and the design decisions, see
> [`Architecture.md`](./Architecture.md). This file focuses on
> **where we are** and **what to do next**.

---

## TL;DR

- Single-crate Rust binary named **`sl`** (`target/release/sl.exe`).
- CLI: `sl init`, `sl index`, `sl search ...`, `sl watch`, `sl config ...`.
- Search pipeline: Tier 0 (filename) → Tier 1 (FTS5 trigram) → Tier 2 (re-rank) → Tier 3 (LLM agent loop, opt-in).
- Storage: SQLite (FTS5) under `<cwd>/.localrag1/index.sqlite3`.
- Index is **incremental** (mtime+size key) and **resumable** (per-batch checkpoints).
- **70 tests pass.** `cargo test` after sourcing `.cargo/build-env.sh`.

---

## What's done (Phases A + B + C of `Architecture.md`)

| Phase | Item | Where |
|---|---|---|
| A | FS crawler w/ `.gitignore` + user `exclude` list | `src/crawler/` |
| A | SQLite + FTS5 schema (trigram tokenizer) | `src/index/schema.rs` |
| A | Incremental ingest, per-batch checkpoints | `src/index/store.rs` |
| A | `fs::watch` driver (Phase A late) | `src/cli/cmd_watch.rs` |
| B | Tier 0: exact + fuzzy filename match | `src/query/tier0.rs` |
| B | Tier 1: SQLite FTS5 over paths + bodies | `src/query/tier1.rs` |
| B | Tier 2: metadata re-rank (recency, size penalty) | `src/query/tier2.rs` |
| B | Tier dispatcher | `src/query/pipeline.rs` |
| C | Read-only FS tools for the agent | `src/tools/mod.rs` |
| C | HTTP client: Ollama + OpenAI-compatible | `src/llm/client.rs` |
| C | System/user prompt builder | `src/llm/prompt.rs` |
| C | Agent loop w/ iter + wall-clock cap | `src/llm/runner.rs` |
| A | Config (TOML, `set`/`show`/`path`) | `src/config/` |
| — | CLI front-end (clap) | `src/cli/` |
| — | Resume state (per-batch checkpoints) | `src/state/` |

## What's left

| Phase | Item | Notes |
|---|---|---|
| C | Tier 3 with live tests | requires running Ollama; see "Testing Tier 3" below |
| D | Tauri UI | explicitly out of v1; gated on CLI usefulness |
| E (v2) | OCR, image embeddings, API-key management | deferred per `Architecture.md` |
| Perf | `cargo bench` slice | not yet written; corpus is small enough |
| Perf | Tighten walker for >100k files | current implementation is `O(N)` single-thread |
| Docs | More doc-comments on `cli::run` arms | patchy in `cmd_watch.rs`, etc. |

---

## How to test

### 0. One-time setup

```bash
# Rust + the toolchain. The toolchain install is already in place, but if
# you ever wipe `%USERPROFILE%/.rustup`, reinstall:
#   winget install Rustlang.Rustup
#   rustup default stable-x86_64-pc-windows-msvc
```

### 1. Build env (every shell session)

```bash
cd C:/repos/LocalRAG1
source .cargo/build-env.sh   # sets MSVC INCLUDE/LIB + PATH
cargo --version              # sanity
```

Why: the local MSVC install is under a non-standard path. The script wires
`INCLUDE`, `LIB`, and `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER` so cargo
finds it. Don't skip this on a fresh shell — `cargo build` will fail otherwise.

### 2. Unit + integration tests

```bash
cargo test                              # everything
cargo test --test crawler               # only `tests/crawler.rs`
cargo test --test index_lifecycle       # only `tests/index_lifecycle.rs`
cargo test --lib                        # only the 62 in-module unit tests
cargo test --doc                        # doctests (currently 0, planned)
```

Expected: **70 passed, 0 failed.**

### 3. Smoke a real folder

```bash
mkdir -p /tmp/sl-demo && cd /tmp/sl-demo
mkdir src && echo "// fun" > src/main.rs && echo "# patterns" > README.md
C:/repos/LocalRAG1/target/release/sl.exe init
C:/repos/LocalRAG1/target/release/sl.exe index
C:/repos/LocalRAG1/target/release/sl.exe search "fun"
# → "Tier 2 (ranked) — 1 hit(s) for fun: src/main.rs score=..."
```

### 4. Pause / resume a long crawl

For a folder with thousands of files, the indexer checkpoints every
`BATCH_SIZE` files (default 2000). If the process is killed mid-crawl, the
next `sl index` finishes from the last successful batch — no full restart.

```bash
sl index --batch-size 500    # smaller = more frequent checkpoints
sl status                    # "50% complete, 3000/6000 files (batch 2 of 3)"
```

(The UI for `--batch-size` and `sl status` is wired in Phase A; coverage in tests
is currently light. See `src/cli/cmd_status.rs` for what's there.)

### 5. Testing Tier 3 (LLM)

Tier 3 needs a live HTTP endpoint. With Ollama running locally:

```bash
# Terminal 1 (outside the project):
ollama serve

# Terminal 2:
sl config set llm.enabled true
sl config set llm.model qwen2.5:7b
sl search "that thing I wrote last year" --tier 3
```

If you don't have Ollama, `sl search --tier 3` falls back to the
`"couldn't determine an answer"` string — that's by design (transport
failures never panic).

---

## How to resume after a break

```bash
cd C:/repos/LocalRAG1
git log --oneline -10          # see recent commits
cat STATUS.md                  # ← you are here
cargo build --release          # confirm binary still compiles
cargo test                     # confirm 70/70 still green
```

If `cargo test` is broken on a fresh shell, you forgot the build env:

```bash
source .cargo/build-env.sh && cargo test
```

If `cargo test` is broken even after that, look at `cargo test 2>&1 |
tail -50` and then check `Architecture.md` for the file you're touching.

---

## File map (what's where)

```
LocalRAG1/
├── Cargo.toml               # crate + bin + lib config
├── README.md                # user-facing intro
├── Architecture.md          # design (read first)
├── STATUS.md                # this file — read second
├── .cargo/
│   ├── build-env.sh         # source before cargo
│   └── config.toml          # static cargo config (linker path)
├── src/
│   ├── main.rs              # `fn main` — thin CLI entrypoint
│   ├── lib.rs               # re-exports modules for tests/
│   ├── cli/                 # clap surface + per-subcommand handlers
│   ├── config/              # TOML config + defaults
│   ├── crawler/             # walking the filesystem
│   ├── error.rs             # crate-wide `Error` enum
│   ├── index/               # SQLite + FTS5 layer
│   ├── llm/                 # Phase C: Ollama/OpenAI-compatible agent loop
│   ├── query/               # Tier 0/1/2 + dispatcher
│   ├── tools/               # LLM-callable read-only FS ops
│   └── util/                # small helpers (paths, etc.)
├── tests/                   # integration tests
│   ├── crawler.rs
│   ├── end_to_end.rs        # spawns the built binary
│   └── index_lifecycle.rs
└── skills/                  # 🆕 guidance docs for future AI sessions
    ├── README.md            # entry point
    ├── coding-style.md
    ├── testing.md
    ├── resumability.md
    ├── phase-roadmap.md
    └── traps-and-pitfalls.md
```

If you only read **two** of the `skills/` files before coding, read
`skills/traps-and-pitfalls.md` and `skills/testing.md`.

---

## Conventions

- Single crate, modules inside `src/`. Single binary `sl`.
- Every public function gets a `///` doc comment.
- Every module gets a top `//!` module doc.
- One commit per logical change. Conventional-ish messages
  (`Phase A: ...`, `Fix: ...`).
- Config keys are dotted (`llm.enabled`, `index.max_file_size_mb`).
- Errors: `thiserror` in `src/error.rs`, surfaced as `crate::error::Error`.
- Logging: `log` crate; visible with `RUST_LOG=debug`.
- Indentation: 4 spaces, rustfmt defaults.

See `skills/coding-style.md` for the full list.
