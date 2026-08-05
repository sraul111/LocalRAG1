# Round 2 — Deferred design decisions

Capture here anything that's **interesting but not for v1**.
Each entry must be toggleable via the `round2_implement` flag
at the top of this file. Set the flag to `true` once we have
bandwidth to tackle these as a single batch.

---

**Round 2 implement: false**

When `true`, this whole file is in scope. When `false`, only
the items whose status explicitly says "round 1" are fair
game. Anything that's "discussion required" stays parked
until we re-open it deliberately.

---

## Tier 3 — full metadata envelope (deferred from round 1)

**Status:** Round 2. Discussion captured in `todo.md` §
"Tier-3 metadata envelope (request-shaped properties)".

**Why deferred.** The envelope introduces seven property
buckets (per-call / per-session / per-startup / per-config /
per-binary) and a document-block style with `citations.
enabled`. It's a lot of new surface to design correctly on
day one; the basic envelope with just the system-prompt
block is enough for round 1.

**What lands in round 1** (see `todo.md` § "Tier 3
metadata envelope — system-prompt caching (round 1)"):

- System-prompt block built **once per session**, tagged
  with provider-specific cache markers (Anthropic
  `cache_control`, OpenAI `prompt_cache_key`, Gemini
  `cachedContent`, Ollama n/a).
- Session-scoped cache lifetime: rebuilds on session
  start, dies with session end. Refreshes when system
  prompt content changes (e.g., config flip).

**What lands in round 2:**

- The full `LlmRequestEnvelope` with all 7 buckets.
- Document-block evidence bundle with `citations.enabled`.
- Per-provider adapters for Anthropic / OpenAI-compat /
  Gemini / Ollama envelope translation.
- `Evidence` struct (per the Tier 1 → Tier 3 contract).

---

## Tier 3 — provider-level prompt caching (deferred)

**Status:** Round 2. **Discussion required** before
implementation.

**Why deferred.** Provider-level caching for tier 3 prompt
parts (system prompt + evidence + history) varies by
provider in ways that need separate live testing:

- **Anthropic** — `cache_control: {type: "ephemeral"}` on
  message blocks. ~5-min TTL default.
- **OpenAI** — automatic prefix caching with
  `prompt_cache_key`. TTL ~10 min default.
- **Gemini** — explicit `cachedContent` resource
  referenced by name; you `create` it, you reference it.
- **Ollama** — no native prompt cache; locally the cost
  is zero anyway.

The question of *what* to cache and *when to invalidate*
is genuinely per-provider. Round 1 covers the system
prompt only and uses each provider's cheapest cache
marker — that's enough to validate the approach.

**Open questions for round 2:**

1. Should the evidence bundle be cacheable too, or only
   the system prompt? (Depends on whether providers
   handle mid-message cache markers cleanly.)
2. How do we detect cache marker support across
   providers without hardcoding? Feature detect from a
   `ProviderCapabilities` struct, or just lookup table?
3. Should there be a per-call invalidation trigger
   (e.g., user changed config)? Or rely on TTL?
4. Cost observability — log cached vs uncached token
   counts when the provider reports them. Anthropic
   reports cached tokens in usage; Gemini does too.
   Does OpenAI? (Yes, partially — they report
   cached_tokens in the response.)

**Implementation trigger:** after round 1's system-prompt
caching has been in the field long enough that we
have real-world usage data on hit/miss rates.

---

## Citation caching (explicitly out of scope)

**Status:** Round 2 or never. Per user decision.

Tier 2 stays stateless. If a future user complains
"I cited X last turn and turn N didn't find it again,"
the fix is in the prompt (add `prior_citations` to the
tier-3 envelope) — not in tier 2 re-ranking.

Captured here so we don't accidentally re-discuss it
under pressure to "improve" tier 2.

---

## Tier 3 — provider-aware parameter mapping (full spec)

**Status:** Round 2. Skeleton in `todo.md` § "Tier 3
token-economy knobs — provider-aware parameter mapping";
this round-2 entry is for the *implementation* and the
full provider coverage.

**Round 1 scope:** the existing `LlmConfig` stays as-is.
We add `RequestOptions` only as an internal struct used
by the round-1 system-prompt caching.

**Round 2 scope:**

- Add `max_output_tokens`, `thinking_budget`,
  `temperature` fields to `LlmConfig` with defaults
  (`300`, `0`, `0.0`).
- Per-provider translation for the three knobs across
  Ollama, OpenAI-compat, Gemini REST (and Anthropic if
  the user adds it later).
- Refactor `chat_ollama` / `chat_openai_compat` into
  body-builder functions + a thin `send_and_parse`
  shell.
- Add `chat_gemini` if the Gemini REST adapter is in
  scope.
- Fix the existing `unknown_provider_errors` test that
  uses a struct literal (it won't compile after adding
  fields).
- Add per-provider `body_options_are_passed_through`
  tests.

---

## Tier 3 — session-level engagement routing (full spec)

**Status:** Round 2. Skeleton in `todo.md` § "Tier-3
engagement routing (session-aware: tier 3 stays engaged)".

**Round 1 scope:** tier 3 is gated by explicit
`--tier 3` flag or by `tier3_fallback_on_empty` on
empty results. Both code paths exist in round 1 but
*without* the session state — every one-shot CLI call
starts fresh.

**Round 2 scope:**

- `SessionCtx` struct (engagement flag, prior queries,
  prior citations, inactivity counter) owned by REPL.
- Modify `pipeline::run` to take `&SessionCtx`,
  branch on `tier3_engaged`, latch on successful
  tier-3 reply.
- Implement `llm::run_with_evidence(...)` that takes
  tier-1/2 results and includes them in the envelope.
- Inactivity counter (turn-based), `/reset` handler.
- `tier3_inactivity_disengage_turns` config +
  `TIER3_INACTIVITY_DISENGAGE_MAX = 64` hardcoded cap.
- The 8 tests sketched in the round-1 section.

**Reason for round 2:** requires REPL infrastructure
to exist first. Round 1's one-shot CLI path is enough
to validate the empty-escalation behavior with the
existing CLI shape.

---

## Tier 3 — empty-result escalation (clarify payload)

**Status:** Round 2. Skeleton in `todo.md` § "Tier 3
empty-result escalation policy (empty-result → clarify)".

**Round 1 scope:** the empty-result flag
(`tier3_fallback_on_empty`) and the `looks_like_phrase`
heuristic are *implemented but inert* — meaning, they
parse from config and log their decision but don't
actually call tier 3 in round 1. The `--tier 3` flag
remains the only way to reach tier 3 explicitly.

**Round 2 scope:**

- Wire the escalation block in `pipeline::run` to
  actually call tier 3.
- Add `Action::Clarify(ClarifyQuestion)` to `LlmReply`.
- Update CLI rendering layer to detect clarify payloads
  and print questions instead of raw strings.
- Add the 6 tests sketched in the round-1 section.

**Reason for round 2:** the clarify payload requires a
prompt-side instruction that the round-1 system-prompt
caching work doesn't introduce — easier to ship when
the prompt shape is settled, not when we're still
hashing out envelope design.

---

## Notes on round 2 as a whole

**Why batch these together.** They share infrastructure:
the envelope, `SessionCtx`, the prompt-build pipeline.
Doing them as one PR is cheaper than three sequential ones
that each re-plumb the same plumbing.

**Why a flag instead of git branches.** A flag is cheaper
than maintaining a long-lived branch — round 2 is the
default *destination* of these todos, not a separate
codebase.

**When to flip the flag.** After tier 3 has been used in
practice for a few weeks and we have data on:

- How often the empty-escalation rule fires
- What the system-prompt cache hit rate looks like
- Whether users want session-aware tier-3 engagement

The flag flips to `true` once those data points exist.
