# Phase roadmap & scope discipline

## Read FIRST

Before touching anything, read `STATUS.md` (in the project root) and
`Architecture.md` (also in the root). They tell you:

- **What's done.** Don't rewrite it.
- **What's deliberately deferred.** If you find yourself thinking "this
  v1 should also support X", check the table — there's a 70% chance X is
  in the "deliberately deferred" column because we concluded it wasn't
  worth shipping in v1. Add it to v2.

## The phases

| Phase | Status | Visible result |
|---|---|---|
| A | ✅ Done | `sl init && sl index` works; SQLite index materializes under cwd |
| B | ✅ Done | `sl search "..."` returns results from filename + body |
| C | ✅ Wired, default-off | `sl search "..." --tier 3` invokes the LLM agent loop |
| D | ⏳ Out of scope | Tauri UI — explicitly deferred to "if CLI is useful" |
| E | ⏳ Out of scope (v2) | OCR, embeddings, API key mgmt |

## What NOT to add right now

- **No embeddings / vector DB / cross-encoder.** The whole point of v1
  is to *not* have those. Re-add only after measuring recall against a
  real corpus and confirming <90% — per `Architecture.md` § "explicitly
  are NOT doing".
- **No multi-process / daemon.** v1 is one process, one cwd, one index.
  No socket, no IPC, no shared cache.
- **No platform-specific code beyond Windows.** The MSVC build env script
  is Windows because that's where we're developing. Mac/Linux are *future*
  work, not now.
- **No new dependencies without a reason.** Adding `tauri`, `axum`,
  `crux`, `meilisearch` etc. all require a separate justification in
  the commit message.

## What you CAN add

- **Bug fixes.** With a regression test that fails before the fix.
- **More tests.** Always welcome. See `testing.md`.
- **Tier 3 tool variants.** E.g. `regex_search`, `summarize_file` —
  additive, read-only, bounded. Update `skills/traps-and-pitfalls.md`
  if you discover something subtle.
- **Indexing of more file types.** Add to `src/crawler/filetype.rs`
  extensions list. Justify each addition in a comment.

## Stop criteria for "v1 done"

`sl search` for "26AS bank details", "design patterns", "rust heredoc
quirk" returns useful results without the LLM tier in <100 ms on any
folder you point it at. Once that's true, ship and start v2.
