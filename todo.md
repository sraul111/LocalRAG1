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

## Auto-init on first search (lazy index bootstrap)

**Status:** Not started. Design only.

**Motivation.** Today the user has to run `sl init` then `sl index`
explicitly before any search works. That's two steps of friction
before they get an answer, and most users will skip it. Instead,
the first search in a folder should *just work* — the index gets
built behind it, the user sees progress, and they're never blocked
waiting for a huge corpus.

**Desired flow:**

```
$ sl "design patterns"
initializing…                 ← shown the moment we open / create the DB
indexing…   3 / 1284 files    ← single-line, overwrites in place
ready.                        ← index usable, search begins
...top results...
sl [C:\Repos\LocalRAG1] ctx="design patterns">   ← REPL prompt, ready for follow-up
```

**Tier behavior on first search in a folder with no index:**

- **Single-word query (tier-0 territory):** Run tier 0 against
  the raw folder listing immediately — no index needed. Print
  results. Begin indexing in the background so tier 1+ is ready
  for the next query.
- **Phrase / multi-word query (tier-1 territory):** Detect no
  index exists. Show "initializing…", build the index, then run
  tier 1.
- **Corpus above the size threshold (see guard below):** Don't
  block on a full index. Run tier 0 from a directory walk, then
  fall straight to tier 3 with a partial folder snapshot as
  context. Background indexer makes tier 1 available for the
  *next* query.
- **Index build fails (permission error, exclude list blow-up,
  corrupt folder):** Don't surface the error as a wall. Bundle
  the user's raw query plus whatever folder context we *did*
  collect (cwd, file count, top-level layout, extension
  histogram, recent mtimes) and hand it to the LLM as a
  tier-3 fallback. The user gets a meaningful answer regardless
  of index state.

**Size guard — "don't block the user":**

- Configurable threshold, default ~500 MB / ~10k files. Editable
  per-folder via `.localrag1.toml`.
- Above threshold: tier 0 runs from a walk, weak results escalate
  to tier 3 with a partial folder snapshot (cwd + tree depth ≤ 2
  + extension histogram + recent mtimes). Tier 1 catches up in
  the background for the next query.
- Way-above threshold (configurable ceiling, default ~10 GB /
  ~200k files): skip tier 1 for this session entirely. Route
  everything through tier 0 + tier 3. Prevents surprise 30-minute
  index jobs and OOM.
- Hardcoded upper bound on top of the config value so a
  misconfigured `max=∞` can't OOM the process.

**Progress UI:**

- Single-line overwriting progress (`\r`), not a full TUI.
  `initializing…` → `indexing… 412 / 8124 files (12 MB / 1.3 GB)`.
- Spinner only during "initializing" (DB open + schema check).
- Never block the REPL prompt on indexing — once tier 0 returns
  even partial results, drop into REPL. Indexer keeps running.

**REPL integration:**

- This is the natural entry path to the REPL section above. The
  first successful search sets `last_query` / `last_hits` /
  `last_tier` and the REPL prompt appears.
- Inside an existing REPL session the DB is already open, so
  auto-init is a no-op.

**Implementation notes:**

- Lazily open / create the DB inside the search handler, not in
  CLI startup.
- The folder snapshot used as tier-3 fallback context is the
  same metadata tier 2 already computes — no new pipeline, just
  a new consumer of `Tier2Output`.
- Background indexing = a thread (or `tokio::spawn`) owned by
  the session, killed cleanly on REPL `exit`.

**Note on tier routing:** the word-vs-sentence distinction above
is *not* how tiers actually route (per Architecture.md all four
tiers can run on any query). The distinction that's *actually*
useful: single-word queries can be served by tier 0 alone without
waiting for the index, so auto-init is most aggressive there.
Multi-word queries still get tier 0 first; the difference is only
whether we wait for tier 1's FTS5 index to come up before
declaring the search "ready". Tier routing itself is unchanged.

**Open questions:**

1. `--no-auto-index` flag for users who want to wait for a full
   index? Plan: yes, parity with the current explicit
   `init` / `index` flow.
2. Size guard: config, hardcoded, or both? Plan: both — config
   knob plus a hardcoded ceiling to bound worst case.
3. Tier-3 fallback without an index: gated behind
   `llm.enabled = true` like the rest of tier 3, or always
   available? Plan: gated by `llm.enabled` — if the user opted
   out of LLM and the index isn't ready, return a clear
   "index unavailable, run `sl index` manually" message after
   tier 0 fails.

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
