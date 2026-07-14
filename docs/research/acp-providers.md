# ACP provider catalogue — which *agents* can Hadron seat?

**The question this answers.** Which coding agents speak ACP as a **server**, so that
Hadron can boot one as a quark. This is *not* the question `acp-clients.md` answers —
that one lists the editors and IDEs that **consume** ACP (Zed, JetBrains, Neovim…),
which tells us nothing about who we can seat. The two got confused once and the wrong
one was delivered as the answer; keep them apart.

- **Agent** = the thing at the far end of the pipe that does the work. *We seat these.*
- **Client** = the editor driving it. *Hadron is one of these.*

**Source.** The official ACP agents page, <https://agentclientprotocol.com/get-started/agents.md>,
read 2026-07-14. There is also an ACP Registry (<https://agentclientprotocol.com/get-started/registry.md>)
intended as the install-time discovery surface. The list below is the upstream one,
reproduced, with a Hadron status column added.

## The catalogue

`Seatable` means: Hadron can boot it **today** with no code change — a `team.json` seat
naming `transport: "acp"` and a `command`. That is true of *every* agent here, because
`AcpTarget` takes any program and argv. The distinction that matters is whether the
command is a **built-in preset** (in `ACP_AGENTS`, so the Settings wizard offers it and
the seat needs no `command`), and whether we have ever **proven** it with a live turn.

| Agent | Vendor | ACP command / package | Hadron status |
|---|---|---|---|
| **Claude Agent** | Anthropic | `@agentclientprotocol/claude-agent-acp` | **preset `acp-claude` · proven** (live turn, `82339b5`) |
| **Codex CLI** | OpenAI | `@agentclientprotocol/codex-acp` | **preset `acp-codex` · unproven** — added this turn (#32) |
| **Gemini CLI** | Google | `gemini --experimental-acp` | preset `acp-gemini` · unproven |
| Antigravity | Google (SDK, not ACP upstream) | our own `scripts/agy_acp.py` | preset `acp-agy` · ours, unproven |
| AgentPool | Phil65 | `agentpool` | seatable, no preset |
| Augment Code | Augment | `augmentcode` CLI | seatable, no preset |
| AutoDev | Phodal | `auto-dev` | seatable, no preset |
| Blackbox AI | Blackbox | `blackbox-cli` | seatable, no preset |
| Bub | Bub Build | `bub-acp-server` | seatable, no preset |
| Cline | Cline | `cline` | seatable, no preset |
| Code Assistant | Stippi | `code-assistant` | seatable, no preset |
| Construct | Construct Worlds | `construct` | seatable, no preset |
| crow-cli | Crow AI | `crow-cli` | seatable, no preset |
| Cursor | Anysphere | `cursor` CLI | seatable, no preset |
| Docker cagent | Docker | `cagent` | seatable, no preset |
| Factory Droid | Factory AI | `factory` | seatable, no preset |
| fast-agent | Fast Agent | `fast-agent` | seatable, no preset |
| fount | Steve0208 | `fount` | seatable, no preset |
| **GitHub Copilot** | GitHub | `copilot` CLI | seatable, no preset |
| Goose | Block | `goose` | seatable, no preset |
| Hermes Agent | Nous Research | `hermes-agent` | seatable, no preset |
| Junie | JetBrains | `junie` | seatable, no preset |
| Kimi CLI | Moonshot AI | `kimi-cli` | seatable, no preset |
| Kiro CLI | Kiro | `kiro` CLI | seatable, no preset |
| Minion Code | Femto | `minion-code` | seatable, no preset |
| Mistral Vibe | Mistral AI | `mistral-vibe` | seatable, no preset |
| OpenClaw | OpenClaw | `openclaw` CLI | seatable, no preset |
| OpenCode | SST | `opencode` | seatable, no preset |
| OpenHands | OpenHands | `openhands` | seatable, no preset |
| Pi | BadLogic | `pi-acp` adapter | seatable, no preset |
| Poolside | Poolside AI | `pool` | seatable, no preset |
| Qoder CLI | Qoder | `qoder` CLI | seatable, no preset |
| Qwen Code | Alibaba | `qwen-code` | seatable, no preset |
| siGit Code | siGit | `sigit` | seatable, no preset |
| Stakpak | Stakpak | `agent` | seatable, no preset |
| stdio Bus | StdioBus | `stdiobus` | seatable, no preset |
| VT Code | Vinh Nguyen | `vtcode` | seatable, no preset |

**Why only four presets.** A preset is a promise that the daemon can boot that exact
command line. The upstream page gives each agent's *package*, not always its exact
ACP-mode invocation (Gemini's is `--experimental-acp`; others differ). Writing a guessed
flag into `ACP_AGENTS` would produce a provider list of promises nothing keeps — the
precise failure the catalogue exists to prevent. An agent earns a preset when someone
confirms its invocation from the vendor's own docs. Everything else is one `command`
away in `team.json` and needs no code.

## The GPT seat (#32)

```jsonc
// .hadron/team.json
{
  "id": "gpt",
  "provider": "acp-codex",
  "transport": "acp",
  "model": "gpt-5.1-codex"   // optional; the agent advertises its own list
}
```

No `command` needed — `acp-codex` is a preset now. Facts, from the adapter's README
(<https://github.com/agentclientprotocol/codex-acp>):

- Boot command is `npx -y @agentclientprotocol/codex-acp@latest` — the same publisher
  and the same shape as the Claude adapter we already run.
- It **bundles its own `@openai/codex`**, so no separate Codex install is required
  (`CODEX_PATH` overrides it if you want the system one — this box has
  `@openai/codex@0.142.3` already).
- **Auth**: a ChatGPT login, or `OPENAI_API_KEY` / `CODEX_API_KEY` in the daemon's
  environment. The adapter advertises ACP auth methods at `initialize`.
- The Codex CLI itself has **no** ACP mode — `codex --help` offers `mcp-server` and
  `app-server`, nothing else. The bridge is not optional.

**Not proven.** I did not complete a live handshake against it: the sandbox declined to
execute an npm package that no human had named, and working around that would defeat the
point of the guardrail. So `proven: false`, honestly. What remains is one command with a
human present, and it is the same probe "Connect" already runs (`adapter::acp::probe`).

## Security

Seating an agent is **executing a program of the operator's choosing** with the daemon's
authority, in the shared checkout. That is the existing model for every ACP seat, and this
change does not widen it — but two things are worth stating plainly:

- A preset is a **default argv baked into the binary**. `acp-codex` points at `npx`, which
  resolves and executes a package **from the network at boot**, pinned only to `@latest`.
  That is already true of `acp-claude`; adding a second one doubles the surface without
  changing its shape. Pinning both to exact versions would be a real hardening step.
- Codex auth reads `OPENAI_API_KEY` from the **daemon's** environment, so seating it means
  the key is readable by that subprocess. No new mechanism — same as every other seat.

## Correction to a memory line

`acp-python-is-a-hallucination` says there is no official Python ACP package. **That is
false.** `agent-client-protocol` is on PyPI (v0.11.0, first released 2025-09-06,
<https://agentclientprotocol.github.io/python-sdk/>) and is listed as an official library
by upstream. What was actually hallucinated was the **import name**: `import acp` does not
exist; the module is `agent_client_protocol`. The lesson was right that the code was
fabricated, and wrong about why.
