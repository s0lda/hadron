# Finding: custom script tools (§3.3) are architecturally blocked as specced — 2026-07-18

**TL;DR:** You said "proceed with it, it's a basic thing in LLM CLIs." I did the engineering, and it turned up a wall: **the §3.3 shape — "the engine executes a `.py`/`.rs` when the model invokes it" — has no achievable invocation path over Hadron's supported transports (ACP / CLI).** The achievable version of the feature *already shipped* in this batch as the §2 per-seat `commands` allow-list. Details + your decision below. I did NOT build a runner, on purpose (a runner nothing can call is the `forge-edit-by-hash` unwired-mechanism mistake, and this one executes attacker-shaped code).

## Why it's blocked (primary-source, not a guess)

§3.3 requires **Hadron to be the executor** of a custom named tool (`run_linter` → runs `linter.py`, feeds output back to the model). For that, the model must be able to *call a tool that Hadron provides*. Over the two transports we actually run:

- **ACP quarks (Claude, Gemini, …):** the external agent owns its own tool loop. Hadron is the ACP *client*; the only tool hook it has is `on_receive_request` (`crates/hadron-gluon/src/adapter/acp.rs:601`), which **approves or rejects the agent's own tool calls** — it cannot register a new callable tool, and it never executes anything itself. I confirmed Hadron advertises **no** custom capability that would change this: it sends bare `InitializeRequest::new(ProtocolVersion::V1)` and `NewSessionRequest::new(cwd)` (acp.rs:340/344, 633/638) — no `clientCapabilities` extensions, no `mcp_servers`. Injecting `run_linter` into Claude's registry is an **MCP-server-config** thing (agent-side) or an **SDK-adapter tool-loop** thing — neither exists, and you declined the SDK adapter (it'd only reach metered API keys).
- **CLI quarks (agy):** one-shot subprocess per turn — there is no mid-turn tool-invocation channel at all.

So a Hadron-side script *runner* would have **zero callers** on either transport. Building it would be machinery for a caller that can't exist yet — exactly the trap Rule 1 ("prove it runs, find the caller") and the repo's `forge-edit-by-hash-is-unwired` history warn against.

## The achievable version already shipped (this batch, §2)

Per-seat `commands: { allowed, not_allowed }` (commits `299b3ed` + `5326168`) IS the script-tool primitive that fits the architecture:

```jsonc
// team.json, on a seat:
"commands": { "allowed": ["python3 .hadron/scripts/linter.py", "cargo test *"] }
```

The quark runs the helper with **its own shell tool**, and Hadron's §2 gate pre-authorizes exactly that command (under No-Human-Mode / Auto). No Hadron-side execution, no unenforceable sandbox — the gate is the real control. Add a one-line prompt hint ("helpers available: `linter.py`, …") if you want the model to discover them; that's a small, honest follow-up.

## Your decision (awake)

Pick the path for §3.3-as-specced (Hadron-executes-the-tool), if you still want it beyond the §2 primitive:

1. **MCP-server config** — advertise `mcp_servers` in `NewSessionRequest` so the agent gains your tools through the standard ACP/MCP channel. This is the *correct* ACP-native way to give a quark custom tools, but it's a new, larger design (an MCP tool server + capability wiring), not a `.py` runner. Recommended if you want real custom tools.
2. **SDK adapter** (sub-project #3) — you declined this; it only reaches metered API keys.
3. **Accept §2 `commands` as the answer** — helper scripts are just pre-authorized shell commands. Zero new code; already live. My recommendation for now.

On the "sandboxed" toggle specifically: over ACP it can't be honestly enforced anyway, because Hadron doesn't spawn the agent's tool subprocess — so the toggle would be a label with nothing behind it. Another reason the specced shape doesn't fit today.

## Status

Everything else in the "Proceed with All" batch is done and reviewed-green (see the branch). Script-tools is **surfaced-as-blocked pending your architecture call**, not silently dropped.
