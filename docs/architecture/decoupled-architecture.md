# 🏛️ Decoupled 2-Tier Architecture

Hadron separates execution from presentation to ensure maximum performance, stability, and zero-overhead background orchestration.

---

## Overview

Hadron is split into two primary operational tiers:

- **`hadron-gluon` (Headless Daemon)**: 
  - Watches the NDJSON file bus (`field.jsonl`) using low-level filesystem notifications (`notify`) and serves a loopback JSON-RPC 2.0 endpoint (`.hadron/gluon.rpc`) for `<1ms` wakeups (`daemon/wake`), incremental `field/poll`, and status/turn dispatch.
  - Dispatches turns to seated Quarks (agents), with native in-distro Linux execution on WSL UNC workspaces when installed.
  - Manages isolated git worktree environments (`.hadron/trees/<id>`).
  - Enforces gatekeeper security policies.
  - Executes merge gates to safely rebase and verify completed code turns against native project test suites.

- **`hadron-chamber` (Native Visualizer)**: 
  - Powered by Zed's **GPUI** framework — a GPU-accelerated desktop GUI.
  - Provides real-time chat lanes, embedded PTY terminals, live token/energy telemetry charts, and interactive git inspection.
  - Serves as the primary user interface while delegating heavy orchestration to `hadron-gluon`.

---

## Communication Layer

The visualizer and daemon communicate asynchronously over the zero-CPU filesystem event bus. This decoupled boundary means:
1. The daemon can run headless in CI or remote servers without needing a visual display.
2. UI rendering hiccups or GPU state changes in the visualizer never interrupt background agent execution or test suite verification.
3. Multiple seats and visualization tools can inspect the event stream independently.
