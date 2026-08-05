# TODO — Pending design decisions and features

Capture here so we don't lose them between sessions. Check off / move to
`STATUS.md` when done.

---

## REPL / interactive mode for `sl`

**Status:** Not started. Design only.

**Motivation.** Today `sl` behaves like a one-shot CLI: every invocation
opens the DB, runs one query, prints, exits. That mirrors `cmd.exe` and
forces the user to retype context for every follow-up. The user wants a
REPL — once results are returned, the next question should naturally
build on them, the way an interactive assistant (or this terminal) does.

Examples of the desired behavior:

```
sl [C:\Repos\LocalRAG1]>
  init
  index
sl [C:\Repos\LocalRAG1] indexed, 1284 files> search design patterns
  ...20 results...
sl [C:\Repos\LocalRAG1] ctx=last-query> search that thing about chubb
  ...uses prior context to disambiguate...
sl [C:\Repos\LocalRAG1] ctx=last-query> open 3
  ...opens 3rd result in $EDITOR...
sl [C:\Repos\LocalRAG1] ctx=last-query> clear
sl [C:\Repos\LocalRAG1]> exit
```

**Scope of v1 (the "small version" we agreed to do first):**

- New entry point: `sl` with no subcommand → drop into REPL.
  `sl search "..."` and other subcommands keep working as one-shots
  (so scripts / CI / `cargo test` don't break).
- Use `rustyline` for line editing, persistent history file, and
  completion. New dep in `Cargo.toml`.
- Persistent `Database` connection opened once at REPL start, reused
  for every command in the session.
- Session-scoped `Context` struct holding:
  - `last_query: String`
  - `last_hits: Vec<Hit>` (paths only, lightweight)
  - `last_tier: TierResult`
  - `history: Vec<(query, result)>` for `↑` / Ctrl-R
- Slash-style commands vs. natural language: commands like
  `search`, `open N`, `clear`, `exit` are explicit; anything else
  is treated as a search query. (Same model as `psql`, `litecli`,
  `redis-cli`.)
- Context propagation rules (conservative default):
  - Use prior context when the new query is short (< 4 words), OR
    contains a pronoun (`it` / `them` / `that` / `this` / `these` /
    `those`), OR starts with `more` / `narrow` / `also` / `what about`.
  - `!search ...` prefix forces a fresh query, ignoring context.
- Session is per-folder. REPL starts in cwd, indexes cwd, prompt shows
  the folder. No mid-session `cd` — process already opened the DB.
- `open N` opens the Nth result from `last_hits` in `$EDITOR`
  (fall back to `$VISUAL`, then `code`, then `notepad`, then just
  print the path).

**v2 (after v1 is solid):**

- Tier 3 (LLM) wired into the REPL with full conversation history.
  The `Context` struct from v1 feeds tier 3 directly so it can
  resolve "show me the one about inheritance" against the prior
  5 turns. This is where the REPL makes tier 3 dramatically more
  useful than single-shot tier 3 ever was.
- Heavyweight context option: store retrieved snippets (not just
  paths) so tier 3 can reason across actual content, not just
  filenames.

**Out of scope:**

- Multi-folder / global REPL that follows `cd`.
- Networking / remote indexes.
- TUI widgets (just plain text prompt for v1).

**Approx size:** ~300 LOC of REPL plumbing + tests. No changes to the
existing tier 0/1/2/3 logic — REPL is a new module that wraps
`query::run` and `cmd_search::run`.

---

## Open questions to resolve when we pick this back up

1. Entry point: `sl` (no args) → REPL, **or** explicit `sl repl`?
   Plan: support both — no subcommand and `repl` both enter REPL.
2. Per-folder REPL only, or allow a global session that follows
   `cd` between folders? Plan: per-folder only for v1.
3. Context rules — conservative (short / pronoun only) or
   aggressive (always use last query unless `!`)? Plan: conservative
   with `!` escape hatch.
4. Should `sl search "..."` still work as a one-shot when REPL is
   the default? Plan: yes. Scripts / CI must not break.
5. `open N` editor precedence: `$EDITOR` → `$VISUAL` → `code` →
   `notepad` → print path. Confirm with user.

---

## Related — tier 0 false positives (separate concern, captured here)

While discussing the REPL, we noticed that the user's query
`"design patterns n chubb project"` returned 20 tier-0 hits, all at
score 0.5, including chubb files like `data-access-designer.md`
because `"design"` is a substring of `"designer"`. This is **not** a
REPL problem and **not** a tier-routing problem — it's a tier-0
quality issue. The REPL design above does not fix it.

Likely fix (separate task, see STATUS when we start it):

- Tier 0 only runs when the query has exactly 1 word; multi-word
  queries skip directly to tier 1.
- OR: raise `tier0_min_score` default from 0.5 to 0.7 so substring
  matches (0.5) are filtered out and only `ends-with /token` (0.8)
  and exact matches (1.0) pass.

Decide when we tackle the REPL — they're independent and small enough
to do in the same PR.

---

## Index bootstrap on REPL startup (snapshot-diff, lazy cost)

**Status:** Not implemented. Design only.

**Motivation.** Users shouldn't have to run `sl init` followed
by `sl index` before any search works. Two steps of friction is
too many — they'll skip it, then complain `sl` doesn't find
their files. The right behavior: `sl` alone, in a folder, opens
a REPL that's already useful and gets more useful as the index
fills in.

**Design principle (load-bearing invariant).** Indexing is an
*optimization*, not a prerequisite. The search path should
always take the cheapest route that can answer:

1. Indexed data exists for the relevant files → answer from
   the index (tier 0/1/2, <100 ms).
2. Index is partial or stale → answer from what we have; only
   escalate to tier 3 if what we have is *weak*.
3. Index is empty or unavailable → fall back to tier 3 with
   whatever folder context we *did* collect. The user gets an
   answer instead of an "indexing, please wait" wall.

This invariant outlives every implementation detail in this
section. If we ever ship a behavior that violates it (e.g.,
"wait for full index before opening REPL"), we've regressed.

**Trigger: REPL startup, not first search.** The previous
version of this section ("auto-init on first search") placed
the indexing work on the first search path. That was wrong.
Moving it to startup means:

- The REPL prompt shows real stats on day one: `sl
  [C:\Repos\LocalRAG1] indexed 8122/8124, pending 2>` instead
  of pretending the corpus is empty until the first keystroke.
- Tier 0 doesn't have to handle "is the index even up yet?" at
  search time — it can assume the on-startup snapshot is the
  freshest known state.
- First search becomes pure read; cost is bounded and
  predictable.
- "Initialize" is no longer a thing the user types. The REPL
  opens → the index exists at the freshness we could afford
  → the user searches.

**Mechanism: diff against last snapshot, index only deltas.**

Treat the FTS5 index as a **derived view** of the folder. On
every REPL startup:

1. **Load previous snapshot** (if any) from
   `<cwd>/.localrag1/`:
   - `files` table rows: `(path, mtime, size, content_hash,
     indexed_at)`.
   - `pending` rows: paths that were queued but not committed
     in the previous session.
   - `stats`: `total_files`, `indexed_files`, `pending_count`,
     `last_indexed_at`, `last_index_duration_ms`.
2. **Walk the cwd tree.** Honor the exclude list (system paths
   non-overridable in v1 — see Scope lock section). Emit one
   event per file: `{ path, mtime, size, content_hash }`.
   Hash is `blake3` over content, lazy-computed only for files
   whose `mtime`+`size` differs from the snapshot row.
3. **Reconcile.**
   - File in walk ∩ snapshot, hash unchanged → skip.
   - File in walk ∩ snapshot, hash changed → upsert (FTS5
     `INSERT OR REPLACE`). One row.
   - File in walk, not in snapshot → insert. One row.
   - File in snapshot, not in walk → delete row (or mark
     `deleted_at` — see open question 3 below).
   - File in `pending` from previous session and not yet
     committed → re-enqueue for indexing.
4. **Write updated snapshot.** Persist the new
   `stats` row so the next startup has a baseline.

This is the React-DOM analogy in startup-time clothing: the
"index" is just the diff between on-disk state and what we
last believed. We don't try to keep two trees in sync via
events; we reconcile on boundary (startup), then search reads.

**Why not `notify` / inotify / file watcher.** We considered
it. Rejected because:

- A background watcher adds shutdown complexity (kill the
  worker cleanly on REPL exit, on Ctrl+C, on terminal close).
- Watcher reliability on Windows is uneven across network
  shares, virtualized folders, and certain mounts — the
  failure mode is *silent staleness*, which is worse than
  paying ~50–200 ms of diff cost on next startup.
- Auto-init and incremental indexing collapse into the same
  code path under snapshot-diff. Two features, one mechanism.
- The snapshot-diff approach handles abrupt shutdowns
  naturally: worst case, one missed startup's worth of
  deltas, recovered on next startup. A watcher that dies
  silently leaves the index drifting forever.

**Size guard — "don't block the user."** Compare the walk's
byte/file totals against the configured thresholds *before*
running reconciliation:

- **Small corpus (under threshold, default ~500 MB /
  ~10 000 files):** Run reconcile synchronously. Show
  progress as a single-line overwrite (`\r`) so the user
  sees movement: `initializing…` → `indexing… 412 / 8124
  files (12 MB / 1.3 GB)` → `ready. indexed 8122/8124,
  pending 2`. Then open REPL. Repo-sized folders finish in
  seconds; the progress line earns its keep.
- **Large corpus (over threshold):** Run reconcile in a
  background thread. Open REPL immediately with whatever
  rows already exist (often thousands from a previous
  session). Background thread emits stats to a status line
  the REPL can render on next prompt. User searches against
  partial index from the first keystroke; tier 1 results
  improve visibly as the indexer catches up.
- **Way-above ceiling (configurable, default ~10 GB /
  ~200 000 files), user must opt in:** Skip tier 1 for this
  session. Route everything through tier 0 + tier 3. The
  hardcoded upper bound on top of the config value means
  even `max=∞` can't OOM the process.

**Progress UI (small-corpus path).**

- Single-line overwrite, not a TUI. Avoid ncurses / crossterm
  for v1 — text + `\r` is enough.
- Spinner only during DB open + schema check ("initializing…").
  Once we start the actual reconcile, show real progress.
- After completion, print a one-line "ready" summary, then
  drop into REPL.

**REPL integration.**

- REPL prompt shows real stats on every prompt: `sl
  [C:\Repos\LocalRAG1] indexed 8122/8124, pending 2>`
  (large-corpus path) or `sl [C:\Repos\LocalRAG1]
  indexed 8124/8124>` (small-corpus path).
- Inside the REPL, the snapshot-diff doesn't repeat. Per-search
  cost is bounded to FTS5 query + a cheap cwd-`stat()` to
  detect "folder changed since last known state" → if changed,
  trigger a small diff (just the changed subtree) before
  answering. This is the "auto-refresh on idle" we considered
  — a tier-0-cost check that only escalates when the user's
  prompt implies staleness.
- On graceful REPL exit (`exit` / Ctrl+D): flush
  `pending` → `files`, write final `stats`, close DB.
  On abrupt exit (terminal close, Ctrl+C beyond first): any
  rows already committed to SQLite (WAL = durable) survive;
  any rows still in `pending` are re-queued on next startup.

**Tier-3 fallback when index build fails.**

Carried over from the previous version, unchanged in spirit:

- If reconciliation fails (corrupt DB, exclude list blew up
  the walk, permission error in cwd that we can't recover
  from): don't surface the error as a wall.
- Bundle the user's raw query plus whatever folder context
  we *did* collect (cwd, file count, top-level layout,
  extension histogram, recent mtimes) and hand it to the LLM
  as a tier-3 fallback.
- The user gets a meaningful answer regardless of index
  state.

This is also the fallback when the index exists but is
stale / partial — see the Tier 1 → Tier 3 contract section
for the escalation rule in the partial-index case.

**Implementation notes.**

- Index reconciliation runs in the CLI startup phase, before
  rustyline takes the prompt. Synchronous in small-corpus
  path; background thread in large-corpus path. Same code,
  different executor.
- The folder snapshot used as tier-3 fallback context is the
  same metadata tier 2 already computes — no new pipeline,
  just a new consumer of `Tier2Output`.
- `stats` row is *the* UX surface for the REPL prompt and
  for the `--status` CLI subcommand. Treat it as a public
  contract.

**Note on tier routing.** The word-vs-sentence distinction
in the previous version of this section is *not* how tiers
actually route (per Architecture.md, all four tiers can run
on any query). The distinction that's *actually* useful:
single-word queries can be served by tier 0 alone without
waiting for the FTS5 index, so the startup path is most
aggressive about getting tier 0 rows quickly. Tier routing
itself is unchanged.

**Open questions:**

1. `--no-auto-index` flag for users who want to wait for a
   full index before REPL opens? Plan: yes, parity with the
   current explicit `init` / `index` flow.
2. Size guard: config, hardcoded, or both? Plan: both —
   config knob plus a hardcoded ceiling to bound worst case.
3. Delete policy: hard-delete missing rows, or soft-delete
   with `deleted_at`? Plan: hard-delete for v1. If a user
   renames a file and immediately searches, the diff catches
   the new path on the next snapshot and re-adds it. The
   "stale FTS5 row for the old path for one search cycle"
   is acceptable; tier 0's filename match won't find it
   anyway once the path's gone, and tier 1 will return 0
   hits for an empty path.
4. Large-corpus background thread lifecycle: who owns it?
   Plan: a struct `IndexReconciler` owned by the REPL
   session, exposing `pending() -> &Snapshot`, dropped on
   REPL exit (which flushes pending rows first).

---

## Tier 1 → Tier 3 contract (meaningful-answer policy)

**Status:** Not implemented. To be implemented when tier 3 lands.

**Motivation.** Tier 1 returns a *ranked list of files* — it
doesn't know if those files actually answer the user's query or
if the user typed a coherent phrase. That's fine: FTS5 is fast
*because* it doesn't try to be smart. But it leaves a gap the
user feels: tier 1 hands back 20 files for
`"design patterns chubb project"` and the user has to eyeball
which 2 are real. Tier 3 should close that gap — but only if it's
clear about what tier 1 *can* and *cannot* tell it.

**Decision: keep tier 1 dumb. Let tier 3 judge.**

Two principles to lock in early so we don't drift:

1. **Tier 1 returns evidence, not verdicts.** Output shape is
   `Vec<{ path, matched_terms, phrase_match_score, snippet, bm25 }>`
   — structured enough that tier 3 can see *which* terms
   matched, *where* they matched, and *how strongly*. The LLM
   needs the granularity; a flat "top 10 paths" loses the signal
   that tells "this matched `"design"` because it's a substring
   of `"designer"`" from "this matched because the file is
   about design patterns."

2. **Phrase coherence is a tier-3 judgment, not a tier-1
   feature.** "Did the user type a real question or a bag of
   words?" is a semantic question an LLM can answer and a
   trigram tokenizer cannot. Tier 1 must not attempt it —
   bolting semantics onto FTS5 defeats the speed premise and
   produces a worse heuristic than the LLM anyway.

**Tier 3 policy on top of tier-1 evidence:**

- Given the query + tier 1's ranked evidence, tier 3 reasons:
  *"do these matched files actually answer the user's question,
  or is this a pile of files that happen to share those words?"*
- **If yes:** synthesize a concise, file-grounded answer. Cite
  the top 2–3 paths with one-line snippets. Don't dump all 20.
- **If no (matched terms don't cohere, top hits are
  substring-noise like `"design"` ⊂ `"designer"`, scores are
  flat / ambiguous):** ask **one** clarifying question before
  answering. Not a stream of them. Format:
  `"by 'chubb project' do you mean [A] or [B]?"` — anchored
  in evidence, not generic.
- **If tier-1 confidence is high** (clear phrase match, strong
  BM25, top hits agree): tier 3 should answer *without*
  re-running searches. The agent loop is for the hard cases, not
  every query. This is the gating from the REPL motivation:
  don't burn 1–5s and ~1 GB RAM when FTS5 already nailed it.

**Partial-index case (the indexing-in-progress state).**

The "snapshot-diff on REPL startup" pattern means the index is
*frequently* partial: a folder that's mid-reconcile after a
large-corpus startup, a brand-new folder where only the most
recent few files made it into the snapshot, a user who just
added 50 files and the small-corpus path is still running
when they search. The right escalation is **not** "forward
everything to tier 3" — it's the same tier-3 rule as above,
just with `index_coverage` as one more input signal:

- **Run tier 1 against whatever rows are already in the
  index.** Don't pre-emptively widen the search to "anything
  tier 3 might find" — that just makes every search 1–5s
  during indexing, which is the regression we're avoiding.
- **Treat `index_coverage < full` exactly like weak tier-1
  confidence** — it's one more reason tier 1's results may
  be incomplete, not a license to skip tier 1.
- **If the combined tier 1+2 evidence is weak**, escalate to
  tier 3 with the partial-index context: the
  `folder_snapshot` (cwd + tree + extension histogram +
  recent mtimes) is what tier 3 uses to find files
  tier 1 couldn't see yet. The user gets an answer; tier 3
  fills the gaps; future searches benefit from the rows
  landing in the index.
- **If `index_coverage` is near full and tier 1 is still
  weak**, that's an actual tier-1-confidence issue, not an
  indexing issue. Handle the same as today's no-coverage
  path.

The plain reading of "data not indexed yet → tier 3" sounds
helpful but produces a tool that's slower the more files it
has. The right reading is **"search what we have; escalate
only if what we have is weak."** Mechanically identical to
the no-index case; the only difference is that tier 1 has
*something* to search, and tier 3's prompt gets the
`folder_snapshot` rather than starting blind.

**Implementation pointer (when this gets built):**

- This is a tier-3 prompt-template and tool-design decision, not
  a tier-1 change. The prompt should receive:
  - `query: String`
  - `tier1_evidence: Vec<Evidence>` (the structured shape above)
  - `tier2_metadata: Tier2Output` (extension / path / temporal
    hints from the existing tier 2)
  - `folder_snapshot: Option<FolderSnapshot>` (from the
    auto-init section, when index isn't ready)
- Tool definitions: keep tier 3's toolset as scoped as today
  (`search`, `grep`, `read`, `list_dir`, `done`). Do **not** add
  a "judge phrase coherence" tool — the LLM already judges that
  inline given the evidence.
- "Ask one clarifying question" is a *response shape*, not a
  distinct tool. Tier 3 emits a `Clarify` payload with one
  question; the REPL formats it and re-prompts.

**Why capture this now (and not when we build tier 3):**

- The tier-1 evidence schema gets locked in early so tier 3
  doesn't have to retrofit. If we ship tier 1 first with a
  flat `Vec<Path>` return type, tier 3 will have to re-query
  tier 1 to recover the matched-terms info — wasteful and
  silently loses signal.
- The "keep tier 1 dumb" rule prevents a future contributor
  from adding phrase-coherence heuristics to tier 1 "to help
  tier 3 out." Document it now, find it later.

---

## Scope lock — search/analysis only, no write capability

**Status:** Decision recorded. Not implemented (because it's the
absence of a feature, but worth pinning down).

**Decision.** `localrag1` (`sl`) is a **fast search and file
analysis tool.** It does not write, edit, or execute files. Tier 3,
when enabled, *reads* and *narrates* — it does not mutate.
"Create me a Python project that does X" is **out of scope** for
this binary.

A separate Pi-style agentic tool — Rust, four basic tools
(`read`, `write`, `edit`, `bash`), its own design discussion —
will exist as its own project. That tool can consume
`localrag1`'s FTS5 results over IPC or as a library to get the
retrieval benefits without inheriting this binary's safety
constraints.

**Why we made this call (the indexing walk + the Windows
incident).**

While exploring `sl` on `C:\`, the startup directory walk
tried to stat / read protected system paths (`C:\Windows`,
`C:\Program Files`, etc.) and surfaced `ACCESS_DENIED` errors
into the user-visible output. **The indexer was not yet
honoring a system-path exclude list, and tier 3 was not
involved at all** — tier 3 is still unconfigured on this
machine and would have returned its own failure, not an
access-denied error.

Two real consequences of that incident:

1. **The exclude list must work, silently, by default.**
   System paths must be filtered before stat, not after — so
   the user never sees `ACCESS_DENIED` lines for paths they
   didn't ask about. In v1 these are *non-overridable* in
   the default config; a user who really needs to index them
   can flip an explicit "I take responsibility" flag.
2. **The argument for keeping write capability out of this
   binary is real but hypothetical here, not from incident.**
   The Windows walk hit read errors, not write errors — we
   don't have an observed case of tier-0 substring-match
   having put a tool on the path to trashing a system file.
   The argument is preventive, not forensic: adding
   `write_file` / `edit_file` to a tool that already
   substring-matches filenames against system DLLs would
   create the failure mode, not have exploited it. Move write
   capability to a sibling project that has its own
   permission model and its own audit trail — don't bolt it
   onto this binary.

**Concrete safety constraints this implies:**

- Indexer never writes outside `<cwd>/.localrag1/`. Period. If
  the index can't be created there, it errors — it does **not**
  fall back to `%LOCALAPPDATA%` or `%TEMP%` silently.
- The default exclude list adds the Windows system paths that
  produced access-denied noise (`C:\Windows`,
  `C:\Program Files`, `C:\Program Files (x86)`,
  `C:\ProgramData`, `C:\System Volume Information`). These
  should be **non-overridable** in v1 — a user who really
  wants to index them can flip a "I take responsibility"
  config flag, but the safe default is unreachable.
- Tier 3's toolset stays read-only: `search`, `grep`, `read`,
  `list_dir`, `done`. **No** `write_file`, `edit_file`,
  `mkdir`, `run`, `delete`. If a user asks tier 3 to "create"
  or "modify" something, tier 3 should respond with "this tool
  doesn't do that; use [the agentic sibling] for it."
  **This invariant holds during indexing-in-progress too.**
  The partial-index path is more likely to lean on tier 3
  (because tier 1 is weaker when the index is incomplete),
  which is exactly the moment a sloppy implementation would
  start to "helpfully" widen tier 3's toolset to compensate.
  Don't. The toolset is decided by safety, not by how often
  tier 3 fires.
- The `llm.enabled = false` invariant from the REPL section
  still holds: a user with no LLM configured gets a fully
  usable search + REPL tool. Tier 3's `Clarify` shape (asking
  a question) is allowed without write capabilities.

**Forward pointer — "agentic shell with FTS5-for-token-economy"
framing.**

The reason FTS5 is worth the engineering in `localrag1` is
*not* just that it's faster than embeddings. It's that an agent
operating on a large local corpus can spend the bulk of each
turn *re-deriving context* (re-reading, re-grepping, re-listing)
unless the retrieval layer hands back **structured evidence**
(see the tier-1 → tier-3 contract section above). An agent that
calls into FTS5 first and gets back ranked, evidence-shaped
results uses a small fraction of the tokens per turn compared
to one that scans the filesystem raw.

So even though we're not building the agentic shell in this
project, the FTS5 + structured-evidence design in this project
is **deliberately consumable** by a future agentic tool that
needs to read lots of files cheaply. Treat the tier-1 schema
as a public contract, not an internal detail — it's the
handshake between "fast retrieval" (this project) and "bounded
agency" (the future sibling project).

**What this rule does *not* mean:**

- It does not mean tier 3 can never narrate *how* to do
  something. "To create that Python project, run
  `mkdir foo && cd foo && python -m venv .venv`..." is a
  narration, not an execution. Fine.
- It does not mean no caching writes. Tier 1/2 indexes,
  per-user project affinity prefs, last-query context — those
  are local writes to `.localrag1/` and are clearly in scope.
- It does not mean tier 3 cannot *suggest* code. Suggesting
  "here's what the file should contain" is narration. The line
  is whether the tool **executes** the change, not whether it
  **proposes** it.

---

## Tier 3 token-economy knobs — provider-aware parameter mapping

**Status:** Not started. **More discussion required** before
implementation. We've sketched the shape but haven't pinned the
defaults or the provider list. Resolve before writing code.

**Motivation.** Tier 3 (LLM agent loop) is the only path in
`sl` that costs money or RAM. Without per-call caps a single
agent turn can return a multi-thousand-token monologue; without
a cap on internal "thinking," reasoning-capable models
(Qwen3, Gemini 2.5, Claude with extended thinking, OpenAI
o1/o3) burn tokens before producing anything useful. Token
economy is not a nice-to-have — it's the difference between
"tier 3 is free" and "tier 3 is a liability."

**The key insight: one config field, one wire format per
provider.** The user writes one TOML block. Three adapter
functions translate it into whatever the model on the other
side actually accepts. No `if provider == "x"` branching
outside the adapters.

**Three knobs, all in `LlmConfig`:**

| Field | What it caps | Default |
|---|---|---|
| `max_output_tokens` | final answer length | `300` |
| `thinking_budget` | internal reasoning tokens (0 = off) | `0` |
| `temperature` | randomness (0 = deterministic) | `0.0` |

**Adapter translation matrix** (which JSON field each provider
honors each knob in):

| Concept | Ollama (`/api/chat`) | OpenAI-compat (`/chat/completions`) | Gemini REST |
|---|---|---|---|
| Output cap | `options.num_predict` | `max_tokens` | `generationConfig.maxOutputTokens` |
| Disable thinking | omit, or `think: false` for qwen3+ | `reasoning_effort: "low"` or omit | `generationConfig.thinkingConfig.thinkingBudget: 0` |
| Force JSON | `"format": "json"` | `response_format: {type: "json_object"}` | `generationConfig.responseMimeType: "application/json"` |
| Temperature | `options.temperature` | `temperature` | `generationConfig.temperature` |
| System prompt | a `{role:"system"}` message in the array | same | **separate** `systemInstruction.parts[].text` field |

**Notable provider quirks (capture so we don't relearn them):**

- **Gemini is the odd one out.** The system prompt isn't a
  message in the array — it's a top-level `systemInstruction`
  field with its own schema. Adapter must partition the
  messages array before building the body.
- **Ollama silently ignores unknown options** unless a
  specific model says otherwise. Safer to emit and let the
  model filter than to branch on model name.
- **`thinking_budget` is only meaningful for reasoning-
  capable models.** Anthropic, Gemini 2.5, OpenAI o-series,
  Qwen3 honor it; everything else drops it silently. Don't
  error on `thinking_budget > 0` for a non-reasoning model —
  just let the provider ignore it. Log once at startup so
  the user knows it didn't take effect.
- **`max_output_tokens` of 0 is invalid on most providers.**
  Clamp to ≥ 16 at the adapter boundary.

**Unknowns — explicitly not resolved yet:**

1. **Which providers does v1 actually need?** Confirmed:
   Ollama (`minimax-m3:cloud` running locally for the user).
   The user has Gemini access and may want Qwen. Do we ship
   three adapters in v1, or just Ollama + a generic
   `openai-compatible` shim that *happens* to talk to
   Gemini's OpenAI-compatible endpoint?
2. **If Gemini, do we use Gemini REST or Gemini's OpenAI-
   compatible endpoint?** The OpenAI-compatible endpoint
   hides the `systemInstruction` quirk but adds another
   layer of "is this field mapped?" to debug.
3. **What's the right `max_output_tokens` default for tier 3
   specifically?** The agent returns filenames + one-line
   explanations; an answer rarely exceeds 100 tokens. 300 is
   safe but maybe 150 is better — saves ~50% on the typical
   turn. **Need user input.**
4. **`thinking_budget` default of 0 is conservative, but is
   it too conservative?** For Ollama with `minimax-m3:cloud`
   specifically, no — that model doesn't reason. For Qwen3
   via Ollama, the user might *want* reasoning enabled
   selectively. **Need user input on "off by default with
   per-query override" vs. "config-driven, fixed at startup."**
5. **Where does `temperature` live in the REPL?** Today it's
   a startup-time config. Maybe it should be a per-query flag
   (`sl search "..." --temp 0.0`)? **Out of scope for v1,
   revisit after REPL ships.**
6. **Per-call `max_tokens` vs. agent-loop caps.** Per-call
   cap stops one reply from being a 10k-token monologue. The
   existing agent-loop caps in `run_agent` (`tier3_max_iterations`,
   `tier3_wall_clock_ms`) stop the *loop* from making 50 HTTP
   calls. These are complementary, not redundant. Ship both.
7. **Token-budget telemetry?** Should `sl search --tier 3`
   print tokens-used at the end? Useful for the user to
   calibrate their config. **Out of scope for v1, cheap to
   add later.**

**Proposed implementation order once decisions are made:**

1. Add `max_output_tokens` / `thinking_budget` / `temperature`
   to `LlmConfig` with defaults (after step 3 above).
2. Add internal `RequestOptions` struct + `From<&LlmConfig>`
   conversion in `llm/client.rs`.
3. Refactor existing `chat_ollama` and `chat_openai_compat`
   into body-builder functions + a thin `send_and_parse` shell.
4. Add `chat_gemini` (genuinely different URL + system-prompt
   handling) if step 1 says we need it.
5. Fix the existing `unknown_provider_errors` test in
   `client.rs` — it uses a struct literal that won't compile
   after step 1.
6. Add one `body_options_are_passed_through` test per
   provider — assert `max_output_tokens: 300` produces
   `"num_predict": 300` in the Ollama body, `"max_tokens": 300`
   in the OpenAI body, `"maxOutputTokens": 300` in the Gemini
   body, etc.

**Why capture this now (and not when we build tier 3):**

- The decision affects `LlmConfig`'s stable surface. Once
  users write TOML against it, changing the field names or
  defaults is a compat break.
- The provider list is a one-way door: adding adapters later
  is fine, but deciding *now* which wire formats `sl` owns
  keeps the adapter layer from sprawling.
- The "one config, three adapters" pattern needs to be
  agreed before any code is written, or we'll refactor it
  twice.

**Next action:** resolve unknowns 1, 3, and 4 (provider
list, output-cap default, thinking-budget default + override
mechanism) before touching `LlmConfig`.

---

## Tier 3 empty-result escalation policy (empty-result → clarify)

**Status:** Not implemented. **More discussion required** on the
phrase heuristic and the clarifying-question shape, but the
overall direction is settled.

**Motivation.** Today `pipeline::run` short-circuits at the
first non-empty tier and falls through to `TierResult::Empty`
when tier 0/1/2 all return nothing. Tier 3 is only reachable
via the explicit `--tier 3` flag. So for a query like
`"design patterns"` against a repo where the phrase is nowhere
in any file body, the user gets the **"no results" wall** —
no clarifying question, no escalation, no recovery. The fix is
a cheap escalation rule: *when tiers 0-2 are empty and the
query looks phrase-shaped, give tier 3 one shot at asking the
user what they meant*.

**Concrete user-visible behavior we want:**

1. `sl search "design patterns"` → tier 0/1/2 return empty →
   tier 3 fires → tier 3 sees no evidence and emits a
   clarifying question → CLI/REPL prints it instead of the
   "no results" wall.
2. `sl search "qqqxxxxnothing"` (nonsense) → tier 0/1/2 empty →
   tier 3 fires → tier 3 also sees no evidence, asks one
   clarifying question. (Even nonsense gets the question —
   cheaper than debating whether the query "deserves" tier 3.)
3. `sl search "README"` → tier 0 short-circuits → tier 3
   never asked. (No escalation needed when tier 0 nailed it.)
4. `sl search "factory"` with `llm.enabled = false` → tier
   0/1/2 empty → return `Empty` exactly as today. No silent
   fallback to a tier the user hasn't opted into.
5. `sl search "factory"` with `llm.enabled = true` but
   `tier3_fallback_on_empty = false` → return `Empty`. The
   user has the LLM but doesn't want fallback inference.
6. `sl search "factory pattern implementation"` (multi-token,
   phrase-shaped) → empty tier 0/1/2 → tier 3 fires → tier 3
   has a real question to disambiguate (impl vs. notes vs.
   discussion).

**Pipeline change (`query/pipeline.rs`).**

After the existing tier 0/1/2 logic, before returning
`TierResult::Empty`, add an escalation block:

```rust
// After tier 0/1/2 all return empty:
if cfg.llm.enabled
   && cfg.search.tier3_fallback_on_empty
   && looks_like_phrase(q)
{
    let txt = crate::llm::run(db, q, &cfg.llm, &cfg.search)?;
    return Ok(TierResult::Tier3(txt));
}
Ok(TierResult::Empty)
```

**`looks_like_phrase(q)` — the heuristic. More discussion
required.** A query is "phrase-shaped" if it has more than one
whitespace-separated token AND doesn't look like a bare keyword
dump. Working rules:

- **Trivially phrase:** two or more tokens → trigger tier 3.
- **Trivially keyword:** single token → don't trigger. (Tier
  3 can't help with "show me `factory`" without knowing what
  the user means by factory.)
- **Edge cases needing more thought:**
  - All pronoun-only (`"it"`, `"them"`, `"those"`) →
     usually means REPL follow-up; in one-shot search mode
     this is a typo and *should* trigger tier 3 because the
     user clearly meant something contextual that's now lost.
  - Stopword-heavy but multi-token (`"of the"` ) →
     probably a typo or paste mistake; trigger tier 3.
  - Single token with high token-entropy (looks like a
     random string / hash) → don't trigger. Cheaper to
     return "no results" than to burn a tier-3 call.

**Config knobs to add** to `SearchConfig`:

```rust
/// When all tiers 0-2 return empty AND llm.enabled is true,
/// escalate to tier 3 instead of returning Empty.
#[serde(default = "default_true")]
pub tier3_fallback_on_empty: bool,    // default: true

/// Minimum whitespace-separated tokens for the empty-result
/// escalation to fire. Below this we treat the query as a
/// bare keyword and skip tier 3.
#[serde(default = "default_tier3_phrase_min_tokens")]
pub tier3_phrase_min_tokens: usize,   // default: 2
```

Defaults are intentional: `true` for the flag because the
behavior we want is "tier 3 fires by default when configured";
`2` for the token count because most real queries are multi-
token and the bare-keyword case is the rare exception.

**Tier 3's response shape on this path — `Clarify`
payload.** Tier 3 needs to know that its job on this path is
*to ask, not to invent*. New variant on the existing `LlmReply`
enum (or its `Action` subenum, depending on how this lands):

```rust
pub enum Action {
    Cite(Vec<Citation>),  // existing — narrative answer with citations
    Clarify(ClarifyQuestion),
    Done,
}

pub struct ClarifyQuestion {
    pub question: String,
    /// Optional short list of candidate framings, drawn from the
    /// query text. Lets the user pick fast instead of retyping.
    pub candidates: Vec<String>,
}
```

Example emission for the `"design patterns"` case:

```json
{
  "action": "clarify",
  "question": "what about 'design patterns' do you want — implementations in this repo, notes/explanations of the patterns, or both?",
  "candidates": ["implementations", "concept notes", "both"]
}
```

CLI prints the question and (optionally) the candidates as
numbered choices. REPL stores the candidates so the user can
type `1` / `2` / `3` to pick instead of retyping the question.

**Prompt-side changes (`llm/prompt.rs`).** Two additions to the
system prompt for tier 3:

1. *Evidence-first rule (already there).* "If you have evidence,
   answer with `action: cite`. If you do not have evidence, do
   NOT list files or invent — emit `action: clarify`."
2. *Citation-then-clarify fallback.* "If after searching you
   have evidence but it's weak (matched terms don't cohere,
   top hits are substring-noise like `design` ⊂ `designer`),
   prefer `clarify` over a low-confidence `cite`."
3. *One question, max.* "Emit exactly one `clarify` question.
   Do not chain. Do not pre-emptively list everything you
   could ask about."

These three lines turn a model that wants to "just answer"
into one that surfaces its uncertainty to the user.

**What this does NOT solve (capture so we don't promise it):**

- The "find me the factory pattern in code that doesn't
  mention `factory` anywhere" case. That's a separate
  problem requiring either a pre-built concept index
  (design pattern name ↔ structural code features), multiple
  tier-3 queries with different framings, or the user
  phrasing it as a question first. The clarifying question
  surfaces option 3 to the user; it doesn't fix option 1 or 2.
- Long-tail question types where tier 3 itself has nothing
  useful to ask (e.g., the corpus literally has nothing on
  the topic). In that case tier 3 should still emit a
  `clarify` rather than fabricate — "I couldn't find anything
  in this folder about X; is the folder the right scope, or
  should I look elsewhere?" is a useful clarification, not a
  fabricated answer.
- Multi-folder search. v1 is cwd-scoped. Tier 3 escalation
  honors the same scope rule.

**Tests to add when this is implemented:**

1. *Unit: `looks_like_phrase`.* Trigger for `"design
   patterns"`, don't trigger for `"README"`, edge cases for
   single high-entropy tokens and stopword sequences.
2. *Unit: pipeline escalation.* Stub `llm::run` to return a
   canned `Clarify` payload; assert `pipeline::run` returns
   `TierResult::Tier3` instead of `Empty` when tiers 0-2 are
   empty + `llm.enabled` + flag on + phrase-shaped.
3. *Unit: pipeline no-escalation when llm disabled.* Same
   setup with `llm.enabled = false`; assert `Empty`.
4. *Unit: pipeline no-escalation when flag off.* Same
   setup with `tier3_fallback_on_empty = false`; assert
   `Empty`.
5. *Unit: pipeline no-escalation on keyword.* `"factory"`
   with no matches; even with llm+flag on, assert `Empty`.
6. *Integration: CLI prints clarify.* `sl search "design
   patterns"` against an empty corpus with a stubbed
   LLM; assert stdout matches `"by 'design patterns' do you
   mean"` or similar and does NOT match `"no results"`.

**Implementation order once decisions are made:**

1. Add `tier3_fallback_on_empty` and `tier3_phrase_min_tokens`
   to `SearchConfig` with defaults.
2. Add `looks_like_phrase` helper to `query::query` (alongside
   `Query::parse`).
3. Modify `pipeline::run` to add the escalation block.
4. Add `Action::Clarify(ClarifyQuestion)` to the LLM reply
   shape and a corresponding prompt instruction.
5. Update CLI (`cli/cmd_search.rs`) to detect `TierResult::Tier3`
   and check whether the wrapped string is actually a JSON
   `clarify` payload — render the question instead of the raw
   string.
6. Tests 1-6 above.

**Why capture this now (and not when we build tier 3):**

- The pipeline change crosses two modules (`pipeline::run`
  and `cli::cmd_search`) and the JSON reply schema. Doing
  it after tier 3 lands means re-plumbing the schema and
  re-testing everything; doing it now while tier 3 is still
  stubbed-out means the change is small and the tests are
  cheap.
- The phrase heuristic is a UX decision the user should
  agree to before code lands — heuristics on user input are
  easy to get wrong and hard to walk back.
- The `Clarify` payload shape is a public contract between
  the LLM and the CLI/REPL rendering layer. It needs to be
  locked in before any tier-3 prompt work starts, or we'll
  retrofit twice.

**Next action:** resolve the `looks_like_phrase` edge cases
(pronoun-only, stopword-only, high-entropy single tokens)
with the user, then implement steps 1-6 above.

---
