# Coding style for `sl`

These conventions are enforced by review, not by automation. Read
once, internalize, move on.

## Module / file layout

- **One module per concept, ~150-300 lines.** If a file grows past
  400 lines, split it.
- **Top-of-file `//!` block in every module** — 3-10 lines of plain English
  explaining what the module does and why it exists. New readers should be
  able to skim the table-of-contents below and form a mental model.
- **Every public item gets a `///` doc comment** — even if it's one line.
  Doc tests (`/// ```rust \n example \n ``` `) are encouraged for small,
  illustrative types.

## Naming

| Construct | Convention | Example |
|---|---|---|
| Functions | snake_case | `crawl`, `read_bounded` |
| Types / Enums | PascalCase | `FileEntry`, `TierResult` |
| Constants | SCREAMING_SNAKE | `MAX_DIR_ENTRIES` |
| Modules | snake_case | `src/index/store.rs` |
| Crate names | one word, lower | `sl` |
| Errors | `Error::Variant { fields }` | `Error::NotInitialized(...)` |

## Error handling

- Library code: returns `crate::error::Result<T>`. The `Error` enum lives
  in `src/error.rs` and is `thiserror`-derived.
- `fn main()` uses `anyhow` if convenient, otherwise our own `Result`.
- **Never `.unwrap()` in library code.** Only in `#[cfg(test)]` blocks.
- **Never `.expect(...)` in a way that hides the message.** Always
  include the variable name + what just happened.
- New `Error` variants: one per *category*, not per call site. Use
  `Error::other(string)` for catch-all.

## Logs

- Use `log::{info, warn, debug, error}`. No `println!` for diagnostics.
- `println!` is fine for CLI user-facing output.
- Default level: `info`. Bump to `debug` via `RUST_LOG=sl=debug`.

## Imports

- One `use` per line, alphabetized within a block.
- Prefer `crate::module::Type` over `mod::Type::Type` for clarity in
  cross-module references.
- This project does not use re-exports to flatten the namespace; keep
  the full path in non-`mod.rs` files.

## Visibility

- Start at `pub(super)` or `pub(crate)`.
- Reach for `pub` only when the item is *intended* for users (CLI) or
  for tests in `tests/`.
- If you find yourself wanting to make something `pub` just to share
  with tests, add a `pub(crate)` helper at the call site instead.

## Git

- **One commit per logical change.** Don't bundle test additions with
  refactors.
- **Conventional messages.** `Fix: ...`, `Index: ...`, `Phase A: ...`,
  `Tests: ...`. This matches the existing log.
- **Never commit `target/`** — `.gitignore` already excludes it.
- **Never commit Cargo.lock changes that bundle in new deps without a
  commit message explanation.** Lintable with `git diff --stat`.

## Things I'd reject in PR review

- Adding `unsafe` without a comment explaining why and an audit checklist.
- Adding new deps just to make code shorter.
- Adding `async`/`tokio` for things that fit comfortably in synchronous
  paths.
- Hiding a `Result` instead of propagating it.
- Adding to `pub` more than is needed for the surface.
