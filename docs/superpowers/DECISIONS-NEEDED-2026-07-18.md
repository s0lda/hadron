# Decisions I need from you (2026-07-18)

Autonomous run while you're out. I build the **safe + verifiable + reversible** parts and **stop at anything that's a security/product *policy* call** — those are below. Nothing here is blocking the code that's shipping; these gate the parts I deliberately won't guess.

## Current progress (this autonomous stretch) — ALL MERGED TO `main`, gate green (534)
- ✅ **UI fixes** (`c901c09`): mode tag shows grey **"Default"** only for a per-quark inherited mode; the global mode chip + overrides show the real ASK/WRITE/AUTO/BYPASS; tags back in-row and **xsmall** (the squish fix). Relaunch the chamber to see it.
- ✅ **Personas** (merged): `~/.hadron/agents/` + repo `.hadron/agents/` load; `@persona-name` routes via its `preferred_role` (reuses role routing), hermetic + whole-branch-reviewed. Persona *instructions* injection (a persona's body → the seat's prompt) is deferred — routing is the core; when you pick it up, `adapter/prompt.rs` should also start telling quarks `@persona-name` is a valid address.
- ✅ **Tool-gating (pure core)** (`0d6d25d`): `skills::is_tool_allowed(tool, skill)` — a skill with no `tools:` allows everything (default, unchanged); a skill with a `tools:` list permits only those (case-insensitive). ENFORCEMENT is deliberately NOT wired (SDK registry filtering needs the SDK adapter; ACP approval-time rejection is notional until real per-tool asks exist — same as §2). This is the tested decision enforcement will call.

**That is the end of the safe + verifiable + reversible work I can do unattended.** What remains genuinely needs your decisions (below) or is a firm no-build.

## Effort tag ("still not visible") — plain answer, not a bug
There is **no `effort` set on any quark** in either `team.json`, so the effort tag has nothing to render. It shows correctly **when you set a per-quark reasoning effort** in **Settings → Effort** (low/medium/high). Effort has no global-default concept the way mode does, so I did **not** fabricate a "Default" effort. If you'd rather the tag always show something (e.g. an inherited/model default), tell me what that default should be and I'll wire it.

## DECISIONS THAT GATE THE §2 ACTIVATION WORK (I will NOT guess these)
The §2 No-Human-Mode gate is built + reviewed but **inert** (toggle off). Before it can be *activated*, these are yours:

1. **Which command classes must ALWAYS reach a human, even under global Bypass?** You gave a partial list (rm -rf, git push, network exfil, credential access). I need the definitive set — this is "how dangerous is dangerous," which only you can set. (These become hard-never-auto-adjudicable, above even the orchestrator.)
2. **Where do the allow/deny lists live, and in what form?** You said exact-match **and** prefix/glob (good). But storage is open: `team.json`? a new `~/.hadron/permissions.json` / `.hadron/permissions.json`? field `ModeSet`-style events? Pick one and I'll build the loader + the `decide()` wiring (the matcher already supports exact + `*`-glob).
3. **Grant-authorship validation** — I'll fold the implementation into the activation work (it's NOT inert — it shares the live human-approval path, so it needs the same additive-proof + adversarial review). No decision needed from you; just noting it's part of "make activation safe," not a standalone.

## STILL A FIRM NO-BUILD (needs your sandbox answer)
- **Custom script tools (`.py`/`.rs` run in the tree):** you said "add a sandboxed-or-not toggle." That's the *shape*, but I won't build the execution path unattended — even a test of it runs attacker-shaped code. When you're back: confirm (a) who may author a script tool (repo `.hadron` = anyone with repo write, vs global `~/.hadron` = only you), and (b) the sandbox story for the "sandboxed" setting (restricted subprocess: no network, scratch-dir-only writes, resource limits?). Then I'll build it with the toggle.

## Also open (lower stakes)
- **UI pixel/size:** tags are now `xsmall` and in-row — if still too big/small or the grey "Default" isn't grey enough in your theme, tell me the direction.
- **SDK adapter:** native Rust Gemini client vs. just honestly renaming `acp-agy` (no native Rust SDK exists unless we build it).
- **Branch consolidation / push:** everything's on `main` locally, not pushed. Say if you want `main` pushed.
