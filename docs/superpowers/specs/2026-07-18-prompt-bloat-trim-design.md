# Prompt-bloat trim — decommission `skills::corpus()` (WS4 §5)

**Status:** design (autonomous; review when awake)
**Date:** 2026-07-18
**Source:** §5 of `docs/superpowers/specs/2026-07-17-permissions-and-extensibility-design.md`
**Branch:** `feat/prompt-bloat-trim` (stacked on `feat/custom-cli-transport`)

## 1. Problem
For **resident (ACP)** quarks the engine appends the entire skill corpus — `skills::corpus()`, all skills rendered verbatim — into `invariants_text` on every excitation (`crates/hadron-gluon/src/engine.rs:836-839`). Intended to prime composition once, it actually re-sends ~70-80k tokens of markdown procedures every turn, burning prompt context and accelerating truncation/compaction. CLI (one-shot) quarks already avoid the corpus and get only the selected skill's body.

## 2. Change (exactly what the spec §5 prescribes)
For **both** CLI and ACP quarks, inject only:
1. `skills::index()` — the brief bulleted index of available skills (unchanged, already always injected at `engine.rs:828`).
2. The full body of the **active starting skill** for the task — `skills::render(&m, target, &handoff, /* include_body = */ true)`.

Concretely, in `engine.rs`'s projection builder:
- **Delete** the resident-only corpus block (`engine.rs:836-839`: `let resident = …; if resident { invariants_text.push_str(&skills::corpus()); }`).
- **Change** the `render(...)` `include_body` argument from `!resident` to `true` (`engine.rs:864`), so a resident quark now receives the active skill's body (it no longer comes from the corpus).
- The `resident` local becomes unused for skill injection — remove it if it has no other use in this function, or leave the `self.resident.contains(target)` lookup only if still needed elsewhere.
- **Remove `skills::corpus()`** (`skills.rs:289`) once it has no callers (this is its only caller), or keep it `#[cfg(test)]`/documented-dead if a test references it — prefer deletion (SSOT: no dead public API).
- Update the now-stale doc comments (`engine.rs:830-835`, `858-859`) that explain the removed resident-corpus logic.

## 3. Why this is safe
- **Composition still works.** The `index()` (always injected) lists every available skill with its summary, so a resident quark still knows the full menu and can invoke another skill as the work crosses phases — it just isn't handed all bodies verbatim up front. This is the same discipline CLI quarks already run under.
- **No dangling cross-references at the injection layer.** The active skill's full body is still injected; other skills are referenced by the index. If a skill's body cross-links another skill, that link resolves the same way it already does for CLI quarks today (which never had the corpus).
- **Behaviour for CLI quarks is unchanged** (they already got `include_body = !resident = true` and never got the corpus).

## 4. Testing
- A projection built for a **resident** target must have `invariants` that: (a) contain `skills::index()`'s content, (b) contain the selected skill's **body** (e.g. a distinctive line from the matched skill), and (c) do NOT contain corpus-only text (a body line from a *different, non-selected* skill that would only appear if the whole corpus were dumped). This is the discriminating test — it proves the trim happened.
- A projection for a **CLI** target is unchanged (index + selected body, no corpus) — pin it so the change doesn't regress the CLI path.
- Full gate `cargo test --workspace --features gui` green. Existing engine/skills tests that asserted corpus presence for resident quarks must be updated to the new contract (they encode the OLD behaviour — updating them is the point, not papering over a regression); call out each such test in the report.

## 5. Security note (Rule 7)
No security surface: this only reduces what text is placed into a quark's own prompt. No new input, no execution, no permission change. It strictly *removes* content from the prompt.

## 6. Judgment calls (autonomous — flag for review)
- **Delete `skills::corpus()` vs keep it dead.** Deleting (SSOT, no dead public API) is preferred; if a test genuinely wants "render everything," keep a `#[cfg(test)]` helper rather than a public `corpus()`. Reversible.
- **Token-savings claim (~90%) is from the spec**, not independently measured here; the mechanism (stop dumping all bodies every turn) is what's verified, not an exact token count.
