# LocalRAG1 — Minimal-Tier Retrieval Architecture

## Why this exists

LocalRAG v1 (full Rust + Python sidecar, Tauri UI, ChromaDB, FastEmbed,
cross-encoder reranking, planned Ollama synthesis) was over-engineered for
the actual query patterns that motivated the project. The two canonical
queries it was built for —

- *"where are my bank details, it must be in my 26AS form"*
- *"what design patterns did I use in projects under C:\Repos?"*

— are both **keyword / filename / path problems**, not embedding problems.
A full embedding + reranking + LLM-synthesis stack adds seconds of latency,
hundreds of MB of memory, a Python sidecar process, and a bundled model —
all to do something SQLite FTS5 with a trigram tokenizer handles in
single-digit milliseconds.

LocalRAG1 is the **v1-shaped 80%**: a small Rust binary (single .exe,
no Python, no vector DB, no sidecar) that you launch with `sl` from
any folder. It indexes **the current working directory** (and its
tree), answers filename + keyword queries in <100ms, and falls back
to an optional LLM agent loop only for the long tail of queries that
genuinely require semantic reasoning.

It also borrows one specific lesson from the agentic pattern: **a local
LLM with filesystem access and a small tool surface is itself a
retrieval system.** We keep that as a Tier 3 fallback, not a primary
path, because the cost model is wrong for shipping today but the design
is too useful to abandon.

**Scope model:** every `sl` command is cwd-scoped. The index lives at
`<cwd>/.localrag1/`. `sl` running in two different folders has two
independent indexes, two independent configs. No global "scan my C:
drive" mode.

---

## Target

- **Idle RAM:** <40 MB (single Rust process, no children)
- **Index size:** <5% of indexed corpus (FTS5 + compact metadata)
- **Search latency:** <100ms for Tiers 0–2, 1–5s for Tier 3
- **Installer:** single .exe, no admin, no Python, no model bundled
- **LLM dependency:** **none required** for Tiers 0–2; optional for Tier 3

---

## The four tiers

A query is classified by a cheap pre-filter and dispatched to the
highest-confidence tier that can answer it. Lower tiers are always
cheaper; higher tiers are only invoked when the lower tiers are
unconfident.

```
                     ┌──────────────────────────────┐
  Query ───────────► │ Tier 0 — Filename / path     │  < 1 ms
                     │ exact + fuzzy match          │
                     └──────────────┬───────────────┘
                          weak?    │
                     ┌──────────────▼───────────────┐
                     │ Tier 1 — SQLite FTS5          │  < 50 ms
                     │ (trigram tokenizer, content  │
                     │  + filename + path boosting) │
                     └──────────────┬───────────────┘
                          weak?    │
                     ┌──────────────▼───────────────┐
                     │ Tier 2 — Metadata + filters  │  < 20 ms
                     │ (ext, date, size, project    │
                     │  affinity, recent edits)     │
                     └──────────────┬───────────────┘
                          weak?    │
                     ┌──────────────▼───────────────┐
                     │ Tier 3 — Optional LLM agent  │  1–5 s
                     │ loop (filesystem tools,     │
                     │ ≤ 8 iterations,  local LLM  │
                     │ via Ollama or Bring-Your-   │
                     │ Own)                        │
                     └──────────────────────────────┘
```

The cutoffs between tiers are configurable, with sensible defaults that
mean ~80% of real queries should resolve at Tier 0 or 1.

### Tier 0 — Filename / path match

**What it does:** tokenizes the query and matches it against
**filename** and **parent directory** fields in the index using
exact + edit-distance (Levenshtein ≤ 2) + case-insensitive substring.

**Why it's first:** zero semantic cost, answers a huge class of queries
("26AS", "design patterns", "policy 2024"). Boosts by:
- exact filename match (1.0)
- partial filename match (0.7)
- parent directory token match (0.4)
- extension match (0.2)

**Storage cost:** the index already has `path` and `filename` columns;
this is a `WHERE` clause over those, no new data.

**Threshold to escalate:** top-1 score < 0.5.

### Tier 1 — SQLite FTS5 with trigram

**What it does:** trigram-tokenized content index. FTS5's `trigram`
tokenizer gives free typo tolerance and substring matching without
needing `strsim` or an embedding model.

**Why it works for the 26AS query:** "26AS" appears literally in the
filename and (almost certainly) the file content. FTS5 trigram scores
that with high BM25 in a single query.

**Why it works for "Design Patterns":** phrase query `design patterns`
ranks files where that bigram co-occurs. Combined with Tier 0's
filename boosting (files in folders like `repos/`, with names like
`design-patterns.md`), this lands the answer in top-3 without
semantic anything.

**Boosting signals** (applied as FTS5 `bm25()` weights + a `WITH`
post-filter rank):
- match in filename: ×1.5
- match in first/last 500 chars (titles, footers, headers): ×1.3
- match in source code comments vs. identifiers: tunable per file type
- recent mtime: small decay bonus (newer = more likely relevant)

**Threshold to escalate:** top-3 fused score below cutoff, OR the
spread between top-1 and top-3 is too narrow (ambiguous).

**Storage cost:** ~3–5% of indexed file sizes for typical text. PDFs
and images contribute their extracted text only; no image bytes
stored.

### Tier 2 — Metadata filters + structured query

**What it does:** exploits the *implicit* constraints in natural-language
queries. Cheap heuristics, no embeddings:

- **Path hints** — "in C:\Repos", "under my Documents" → scope the
  index scan to that subtree before running FTS5.
- **Extension hints** — "PDFs", "Word docs", "code files" → filter by
  MIME/extension first.
- **Temporal hints** — "last week", "yesterday", "2023" → filter by
  mtime, then rank.
- **Project affinity** — repeated queries about the same project
  re-rank results from that project higher (per-user, persisted).
- **Size hints** — "that big file", "small notes" → filter.
- **Owner hints** — "my notes" (vs. someone else's) → apply on
  Windows via file owner when available.

This is a **post-filter or pre-filter** on Tiers 0/1's result set. It
typically narrows 50K candidates to 200, then Tier 1 re-ranks.

**Threshold to escalate:** if filtering eliminated everything (the
hint was a false lead), or the filtered top-3 still don't look
right after boosting.

### Tier 3 — Optional LLM agent loop (the v2 fallback)

**What it does:** spawns a local LLM (Ollama, or a BYO endpoint
configured at install time) and gives it a small tool surface:

- `search(query, path_filter?, ext_filter?, mtime_filter?)` → returns
  top-20 from FTS5
- `grep(pattern, path_filter?, ext_filter?)` → returns matching lines
  with file:line references
- `read(path, offset?, limit?)` → returns file content
- `list_dir(path)` → returns directory contents
- `done(results, reasoning)` → emits final answer

The LLM gets the original query, the (weak) Tier 0–2 results as
context, and is asked to either confirm the existing top-3 or iterate
up to 8 tool calls before returning. Every tool call is bounded and
logged. A wall-clock and iteration cap both hard-stop the loop.

**Why optional:** this tier is the *only* part of the system that
needs a model. Everything else is a single Rust binary. The default
install ships without Ollama; users opt in. The CLI / API surface
stays identical.

**Why limited to 8 iterations:** the agent-loop pattern (the one
*I* use when answering your queries) is powerful but expensive. At
~500ms per local LLM call, 8 iterations is 4s — which is the
"still feels responsive" ceiling. Past that, users bounce.

**Why not always-on:** every query at Tier 3 burns ~1–5s and ~1GB
of RAM during the call. Firing it on every search (which is what
"chat with your files" products do) is what makes those products
feel slow and hot. Tiers 0–2 are the daily-driver.

---

## Ingestion pipeline

Same as v1 in spirit, much simpler in practice.

```
File detected (notify / initial crawl)
   │
   ├─ skip if matches exclude list (default + per-repo .gitignore)
   ├─ skip if > max_size (default 200 MB)
   ├─ skip if binary with no extractable text (configurable)
   │
   ├─ extract text:
   │     - text/code/markdown: raw UTF-8
   │     - PDF: lightweight extractor (lopdf or pdf-extract)
   │     - docx/xlsx/pptx: zip + xml parse
   │     - images: skip for v1; Phase 6 OCR in v2
   │
   ├─ compute:
   │     - path, filename, parent_dir tokens
   │     - extension, mime
   │     - mtime, size, owner
   │     - first 500 chars + last 500 chars (boost signal)
   │
   └─ upsert into SQLite:
         - files table:     id, path, filename, ext, mtime, size, owner
         - files_fts:       FTS5 virtual table over content + filename
                            with trigram tokenizer
```

**No chunking for v1.** FTS5 is happy to index a 200-page PDF as a
single document; trigram scoring still works. Chunking is a v2
optimization for the case where Tier 3 wants to feed specific
sections to the LLM without dumping the whole doc into its context.

**No embeddings, no vector store, no model files.** Period.

---

## Rust ↔ "LLM" boundary

Tier 3 is the only place Python or an LLM shows up. Two designs,
pickable at install time:

1. **Ollama sidecar** — if Ollama is installed and the user opts in,
   the Rust process issues HTTP POSTs to `http://localhost:11434`.
   No Python, no PyInstaller. The Rust process is the only one
   the user sees.

2. **BYO endpoint** — user configures an OpenAI-compatible endpoint
   (OpenAI, Anthropic via proxy, Groq, local llama-server, etc.).
   Same HTTP interface from Rust's side.

The agent loop runs **in the Rust process** — we just feed the LLM
JSON tool descriptions and parse its JSON tool calls. The LLM never
touches the filesystem directly; the Rust process is the executor.
This is safer than letting the LLM run shell commands and matches
the spawn-per-query model from v1 (stdin/stdout) without the Python
overhead.

---

## Configuration surface

A single `config/localrag1.toml` (and the same hardcoded-defaults
file the v1 CLAUDE.md specifies, just trimmed):

```toml
[index]
data_dir = "%LOCALAPPDATA%/LocalRAG1"
exclude = ["node_modules", ".git", "venv", "__pycache__",
           "Windows", "System32", "ProgramData", "AppData/Local/Temp",
           ".cache", "bin", "obj", "dist", "target"]
max_file_size_mb = 200
respect_gitignore = true

[search]
tier0_min_score = 0.5
tier1_min_score = 0.3
tier3_max_iterations = 8
tier3_wall_clock_ms = 5000

[llm]                  # all optional
enabled = false
provider = "ollama"     # or "openai-compatible"
model = "qwen2.5:7b"
endpoint = "http://localhost:11434"
```

The exclude list is the **user-editable** part. Everything else has
sensible defaults.

---

## Build phases (v1 scope)

1. **Phase A — Rust crawler + SQLite FTS5 index**
   Drive detection (`sysinfo`), `ignore` for exclude lists, `notify`
   for file watching, `rusqlite` with `bundled` for FTS5. Unit tests
   against a fixed corpus with known answer files (26AS.pdf,
   design-patterns.md in a fake `C:\Repos`).

2. **Phase B — Tier 0/1/2 query pipeline**
   Single `localrag1 search "26AS"` CLI command. Should answer the
   two canonical queries correctly without any LLM involvement. This
   is the ship-able v1.

3. **Phase C — Tier 3 LLM agent loop**
   Opt-in. Ollama sidecar or BYO endpoint. Bounded iterations,
   logged tool calls. The "chat with your files" mode that v1
   wanted to be the whole product.

4. **Phase D — Tauri UI** (only if the CLI is useful)
   Same model as v1: system tray, search box, result list with
   snippet highlighting. Tier 3 is a toggle, not on by default.

5. **Phase E (v2) — OCR, image embeddings, BYO API key management**

---

## What we explicitly are NOT doing in v1

- ❌ ChromaDB / vector store
- ❌ FastEmbed / ONNX runtime / bundled models
- ❌ Cross-encoder reranker
- ❌ Python sidecar / PyInstaller
- ❌ Chunking (deferred to Tier 3 for v2)
- ❌ Ollama bundled with the installer (Phase 5 was right to push this out)
- ❌ LLM synthesis on every query

If benchmarks on a real-world C:\ corpus show any of these are
necessary to hit >90% recall on representative queries, revisit. Until
then, ship the 80% solution.
