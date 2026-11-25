# 🏛️ Decoupled 2-Tier Architecture

Hadron separates execution from presentation to ensure maximum performance, stability, and zero-overhead background orchestration.

---

## Overview

Hadron is split into two primary operational tiers:

- **`hadron-gluon` (Headless Daemon)**: 
  - Watches the NDJSON file bus (`field.jsonl`) using low-level filesystem notifications (`notify`) and serves a loopback-enforced (`127.0.0.1`) JSON-RPC 2.0 endpoint (`.hadron/gluon.rpc`) with bounded NDJSON frame decoding (`16 MiB` frame / `4 KiB` metadata caps, strict `deny_unknown_fields`, and response-ID verification) for `<1ms` wakeups (`daemon/wake`), incremental `field/poll`, live `daemon/subscribe` push events (including `GateOutput` test streaming), and status/turn dispatch.
  - Exposes a headless stdio MCP bridge (`hadron-gluon --mcp`) over the loopback RPC endpoint so external editors and CI tools can inspect swarm status/roster, poll field events, wake the daemon, and dispatch turns.
  - Dispatches turns to seated Quarks (agents), with native in-distro Linux execution on WSL UNC workspaces when installed.
  - Manages isolated git worktree environments (`.hadron/trees/<id>`).
  - Enforces gatekeeper security policies alongside non-blocking Rule 1 liveness and evidence-claim advisories.
  - Executes merge gates to safely rebase and verify completed code turns with transitive affected-crate slicing and live test output streaming.

- **`hadron-chamber` (Native Visualizer)**: 
  - Powered by Zed's **GPUI** framework — a GPU-accelerated desktop GUI.
  - Provides real-time chat lanes, embedded PTY terminals, live token/energy telemetry charts, interactive git inspection (including GitHub Issue/PR dispatch intake), and the `Mission > Gate` live Test Explorer.
  - Serves as the primary user interface while delegating heavy orchestration to `hadron-gluon`.

---

## Communication Layer

The visualizer and daemon communicate asynchronously over the zero-CPU filesystem event bus. This decoupled boundary means:
1. The daemon can run headless in CI or remote servers without needing a visual display.
2. UI rendering hiccups or GPU state changes in the visualizer never interrupt background agent execution or test suite verification.
3. Multiple seats and visualization tools can inspect the event stream independently.
