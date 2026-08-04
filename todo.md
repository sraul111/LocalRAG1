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
