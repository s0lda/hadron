# Hadron Next-Gen Architecture: Headless Tokio Engine, Actor Bus, CoW Workspaces, and Tauri React UI Shell (RFC & SSOT)

- **Date**: 2026-09-03
- **Author**: `@Agy` (Orchestrator Quark)
- **Status**: Proposed / Architecture RFC (SSOT)
- **Target Subsystems**: `hadron-gluon` (Headless Engine), `hadron-lattice` (Data & Wire Protocol), `hadron-forge` (AST & Workspaces), `hadron-tauri` (Decoupled React Shell)

---

## 1. Executive Summary & Problem Statement

Hadron's current architecture couples the orchestration daemon (`hadron-gluon`) and shared data layer (`hadron-lattice`) with a monolithic GPUI desktop client (`hadron-chamber`) built on custom forks (`s0lda/zed`, `s0lda/gpui-component`). 

While this initial design allowed rapid experimentation, it has accumulated critical architectural bottlenecks:
1. **GPU Software-Rasterization Lag on Linux/WSL**: On Linux and WSLg environments without hardware Vulkan ICDs, GPUI forces Lavapipe CPU rasterization, creating frame lag during heavy text streaming and UI rendering.
2. **GPUI Rendering & Layout Constraints**: GPUI lacks web standard capabilities: single font-family restrictions (no CSS font-fallback stacks), render-on-hover churn triggering full window invalidation, and fragile manual list caching (`ChamberView` triple list cache desyncs upon field swaps).
3. **Serial Polling & Text-Parsed Mentions**: The turn loop relies on serial line polling of Markdown `@mentions` in `field.jsonl`, locking the orchestrator turn during multi-quark execution and freezing interactive human chat.
4. **Worktree Isolation & Target Contamination**: Git worktrees share a common `target/` directory, introducing risk of rlib cache poisoning across concurrent quark turns, while untracked `.hadron/` docs created in worktrees become invisible to peer quarks.
5. **Coupled Monolith vs. Headless Operation**: The chamber cannot currently be run in headless server/CI environments, remote cloud boxes, or accessed via mobile/browser clients without running the GPUI desktop loop.

### The Architectural Shift
This RFC establishes the Single Source of Truth (SSOT) for the next-generation Hadron architecture:
- **Headless Tokio Engine**: `hadron-gluon` becomes a completely headless daemon service hosting an asynchronous Actor-model event bus, resident ACP runners, and centralized state storage.
- **Typed Wire Protocol**: Bidirectional typed JSON-RPC 2.0 over local Unix domain sockets (or Windows named pipes) and WebSockets, with streaming event subscriptions.
- **Copy-on-Write (CoW) Workspace Overlays & Compiler Cache Guard**: High-speed, isolated workspace overlays backed by `sccache` to guarantee compile isolation without disk bloat or cache poisoning.
- **Decoupled Tauri v2 + React 19 UI Shell**: Cross-platform desktop shell utilizing standard Web technologies (React 19, Vite, Tailwind CSS, `xterm.js`, Lucide icons), completely free of custom GPUI forks, with full autoupdate support via `@tauri-apps/plugin-updater`.

---

## 2. System Architecture RFC (SSOT)

### 2.1 Wire Protocol Specification (Typed JSON-RPC 2.0 + Streaming)

The wire protocol between the headless engine (`hadron-gluon`) and any client (Tauri UI, CLI, Web browser, remote agent) is strictly typed JSON-RPC 2.0 over bidirectional streaming transports (Unix Domain Socket / Named Pipe for local desktop, WebSocket for network/remote).

```
+--------------------------------------------------------------------------------+
|                             Client (Tauri / Browser / CLI)                     |
+--------------------------------------------------------------------------------+
                                       ▲ │
                    JSON-RPC Request   │ │  JSON-RPC Response
                    & Subscriptions    │ │  & Streaming Notifications
                                       │ ▼
+--------------------------------------------------------------------------------+
|                         Hadron Wire Protocol Router                            |
|             (Unix Domain Socket / Named Pipe / WebSocket Transport)            |
+--------------------------------------------------------------------------------+
                                       ▲ │
                                       │ ▼
+--------------------------------------------------------------------------------+
|                         Hadron Gluon Headless Daemon                           |
|  +------------------------+  +------------------------+  +------------------+  |
|  |    Tokio Actor Bus     |  | Resident ACP Supervisor|  | Central Nucleus  |  |
|  +------------------------+  +------------------------+  +------------------+  |
|  | CoW Workspace Overlay  |  |   Merge Gate Runner    |  | Telemetry Ledger |  |
|  +------------------------+  +------------------------+  +------------------+  |
+--------------------------------------------------------------------------------+
```

#### Envelope Framing
Every frame sent across the transport is a newline-delimited JSON-RPC 2.0 object (`application/json-rpc`):

```typescript
// Request Envelope
interface JsonRpcRequest<T = any> {
  jsonrpc: "2.0";
  id: string; // ULID format (e.g. "01HZX...")
  method: string; // Namespaced: e.g. "swarm/turn/dispatch"
  params: T;
}

// Response Envelope
interface JsonRpcResponse<T = any> {
  jsonrpc: "2.0";
  id: string; // Correlates to request ULID
  result?: T;
  error?: JsonRpcError;
}

// Streaming Notification (Unsolicited or Subscribed Stream)
interface JsonRpcNotification<T = any> {
  jsonrpc: "2.0";
  method: string; // e.g. "stream/field/event", "stream/terminal/data"
  params: {
    subscriptionId: string;
    sequence: number;
    payload: T;
  };
}

interface JsonRpcError {
  code: number;
  message: string;
  data?: any;
}
```

#### Standard Namespaces & Core Methods
| Namespace | Method | Description |
|---|---|---|
| `engine/*` | `engine/status` | Queries daemon version, uptime, memory, active quarks |
| | `engine/shutdown` | Graceful shutdown triggering child process group SIGTERM |
| `swarm/*` | `swarm/roster/list` | Returns active seats, available catalog presets, adoption status |
| | `swarm/roster/update_seat`| Updates model, provider credentials, persona, or permission mode |
| | `swarm/turn/dispatch` | Dispatches task or message to a specific quark or orchestrator |
| | `swarm/turn/cancel` | Gracefully cancels an in-flight turn with partial output retention |
| `session/*`| `session/list` | Lists all historical and current field sessions |
| | `session/switch` | Switches active session or clears to archive |
| `stream/*` | `stream/subscribe` | Subscribes to event topics (`field`, `terminal`, `stats`, `gate`) |
| | `stream/unsubscribe` | Cancels an active streaming subscription |
| `workspace/*`| `workspace/status` | Queries dirty files, worktree branches, and git graph |
| | `workspace/gate_run` | Triggers pre-merge test runner and rebase gate |
| `pty/*` | `pty/spawn` | Spawns a stateful background PTY terminal session |
| | `pty/write` | Sends stdin bytes to the active PTY session |
| | `pty/resize` | Resizes terminal rows and columns |

### 2.2 Process Boundaries & Security Architecture

```
+-----------------------------+               +-------------------------------+
|      Tauri UI Process       |               |    Headless Gluon Daemon      |
|  (Unprivileged WebView &    |  Unix Socket  |    (Privileged Supervisor)    |
|   Frontend Sandboxed JS)    | <-----------> |                               |
|                             |  (Auth Token) |  Spawns:                      |
| - React 19 UI               |               |  - Resident ACP Processes     |
| - xterm.js Terminal Canvas  |               |  - CoW Worktree Clones        |
| - State Management (Zustand)|               |  - Sandboxed Compiler (sccache|
+-----------------------------+               +-------------------------------+
```

1. **Client Isolation**: The Tauri UI runs in an unprivileged webview. It has **no direct filesystem write access** to the repository or worktrees. All state mutations and commands must route through the typed RPC API.
2. **Local Transport Authentication**: Local IPC (Unix domain socket / Windows named pipe) is owned strictly by the current user (`0600` permissions). If WebSocket transport is enabled (for browser / remote monitoring), a cryptographic handshake token generated at daemon boot (`~/.hadron/daemon.auth`) is required in the connection headers (`Sec-Hadron-Token`).
3. **Permission Escalation Ladder**: The 4-tier posture (`Ask`, `Write`, `Auto`, `Bypass`) is evaluated exclusively by `hadron-gluon` and `hadron-gatekeeper`. The client cannot bypass this ladder; tools requesting elevated permissions emit an RPC challenge to the client (`engine/permission_request`) requiring user confirmation.

### 2.3 State Invariants (SSOT)

1. **Centralized Field Sourcing**: `field.jsonl`, `ledger.db`, and active turn contexts live exclusively in the daemon's runtime directory (`~/.hadron/`). Worktrees never own session files.
2. **Nucleus Knowledge Root**: `.hadron/nucleus/` (`index.md`, `notes/`, `invariants/`, `features.md`) is maintained as the single shared knowledge root. Worktree commits modifying memory files are synced atomically through the daemon store before worktree teardown, resolving the invisible worktree note bug forever (`notes/a-hadron-doc-written-from-a-worktree-is-invisible.md`).
3. **Turn Watchdog Invariant**: The turn watchdog measures silence via heartbeat timestamps on the actor bus, never elapsed execution time (`notes/the-turn-watchdog-measures-silence-not-elapsed-time.md`).
4. **Zero Untrusted Command Leaks**: All spawned test runners, git commands, and ACP resident runners must spawn in new process groups (`process_group(0)`) registered with `hadron_gluon::proc`, and are bounded by explicit deadlines (`GATE_TEST_DEADLINE`, `GIT_DEADLINE`).

---

## 3. Vertical Milestone Specifications

### 3.1 Milestone 1 (M1): Headless Engine & Actor Bus

#### Objective
Decouple `hadron-gluon` into a completely standalone headless service with an asynchronous Tokio actor bus, resident ACP runners, and a typed RPC server.

#### Architectural Components
1. **Tokio Actor Event Bus (`hadron_gluon::actor`)**:
   - Replaces serial polling of `field.jsonl` with an in-memory broadcast and mpsc channel architecture.
   - Each seated Quark is an independent Tokio actor task with its own mailbox channel (`mpsc::Sender<QuarkMessage>`).
   - The Orchestrator runs in a dedicated actor lane, allowing continuous conversation and status queries even while worker quarks execute heavy background tools.
2. **Resident ACP Supervisor (`hadron_gluon::adapter::acp`)**:
   - Manages subprocess lifecycle for resident ACP agents (`agy`, `claude`, `codex`, `gemini`, etc.).
   - Implements async cancellation without dropping partial text streams (`notes/a-graceful-cancel-discarded-partial-text.md`).
   - Standardizes telemetry ingestion into `hadron_lattice::telemetry::Usage` and `QuotaBucket` snapshots.
3. **Centralized Daemon Memory & Routing**:
   - Daemon holds an in-memory cache of the Nucleus Index, Features Map, and Invariants.
   - Quarks query the daemon RPC for nucleus lessons rather than opening arbitrary local files.
4. **Typed RPC Server (`hadron_gluon::rpc_server`)**:
   - Provides Unix Domain Socket (`~/.hadron/run/gluon.sock`) and WebSocket (`127.0.0.1:4473`) listeners.
   - Handles connection auth, request dispatching, and fan-out event subscriptions.

---

### 3.2 Milestone 2 (M2): Workspace & Build Isolation

#### Objective
Replace raw git worktrees with copy-on-write (CoW) overlay isolation and eliminate compiler target cache collisions.

#### Architectural Components
1. **CoW Workspace Overlays (`hadron_gluon::worktree::cow`)**:
   - Leverages filesystem reflink / hardlink trees (btrfs/ext4 reflink or git alternate object store with shallow checkout overlays).
   - Instantaneous worktree creation (< 100ms vs seconds for git worktree add).
   - Eliminates dangling worktrees and lock file contention.
2. **Compiler Cache Protection & Isolated Target Dirs**:
   - Configures `RUSTC_WRAPPER=sccache` globally for all build invocations.
   - Employs per-quark sub-target paths (e.g. `target/quarks/<quark_id>/`) combined with shared `sccache` backing.
   - Prevents foreign `.rlib` cache contamination (`notes/the-shared-target-dir-can-serve-a-foreign-rlib.md`) while preserving instant incremental rebuild speeds.
3. **Autonomous Merge Gate Pipeline**:
   - Headless test execution pipeline running under `GATE_TEST_DEADLINE` in dedicated process groups.
   - Speculative rebase on `base` before test execution (`notes/the-gate-rebases-before-it-tests`).
   - Atomic merge and fast-forward with zero uncommitted drift.

---

### 3.3 Milestone 3 (M3): Decoupled UI Shell (Tauri v2 + React 19)

#### Objective
Build a high-performance, platform-native desktop GUI using Tauri v2, React 19, Tailwind CSS, and `xterm.js`, completely replacing GPUI.

#### Architectural Components
1. **Tauri v2 Rust Core (`crates/hadron-tauri`)**:
   - Thin supervisor binary bootstrapping the webview and connecting to the local `hadron-gluon` daemon over IPC.
   - Embeds native window controls, tray menu, and desktop notifications (using native OS notification channels, completely avoiding WSLg layer-shell crashes).
   - Implements atomic self-update using `@tauri-apps/plugin-updater`.
2. **React 19 Frontend Shell**:
   - Modern component library built with Tailwind CSS and Radix UI / Shadcn primitives.
   - **Chat & Swarm Field**: Virtualized message list using `@tanstack/react-virtual`, rendering markdown via `streamdown` / Remark without text shaping crashes or emoji missing glyphs.
   - **Live Activity Rails**: Reactive cards for active quarks showing real-time token spend, latency, and current tool execution.
   - **Terminal (`xterm.js`)**: Hardware-accelerated Canvas/WebGL terminal emulator with bidirectional PTY streaming over RPC.
   - **Visual Commit Graph**: High-performance SVG/Canvas interactive git commit graph, free of single-font limitations.
   - **Telemetry & Quota Hub**: Recharts / SVG area charts for multi-window cumulative spend and live quota countdown rings.

---

## 4. Invariants & Compatibility Registry

| Invariant | Legacy GPUI Status | New Headless/Tauri Status |
|---|---|---|
| **Vulkan / Lavapipe Fallback** | Required CPU software rasterization; stuttered under streaming | **Resolved**: Tauri uses native WebKit/Chromium with standard hardware GPU rasterization |
| **One Font Family Limit** | Hard constraint in GPUI `Theme::font_family` | **Resolved**: Full CSS font-family cascade with system emoji fallbacks |
| **Field Swap List Cache Desync** | Triple cache recreation required in GPUI | **Resolved**: React virtualized DOM automatically binds to state streams |
| **WSLg Layer-Shell Crash** | Crashed Linux notify-send on WSL | **Resolved**: Tauri native notifications route cleanly to Windows host toast |
| **NDJSON Message Framing** | Standardized daemon protocol | **Preserved**: Typed JSON-RPC 2.0 frames over newline-delimited stream |
| **Process Group Teardown** | Required SIGTERM to PGID | **Preserved**: Daemon continues strict process-group teardown |
| **Turn Watchdog Silence Rule** | Checked `live/<quark>.json` | **Preserved**: Actor bus tracks silence timestamps, not wall-clock time |
| **Shared Cargo Target Poisoning**| Caused foreign rlib test failures | **Resolved**: Per-quark targets backed by `sccache` |

---

## 5. Verification & Migration Strategy

1. **Phase Gate 1 (M1 Verification)**:
   - Headless daemon boots, passes all RPC integration tests, runs simulated multi-quark turns via test runner.
2. **Phase Gate 2 (M2 Verification)**:
   - CoW workspaces create in <100ms, concurrent cargo builds execute simultaneously with `sccache` without rlib collisions.
3. **Phase Gate 3 (M3 Verification)**:
   - Tauri React shell connects to headless daemon, streams live chat, displays PTY terminal, renders commit graph, and passes E2E acceptance tests.
4. **Deprecation**:
   - Once M3 passes parity tests, `crates/hadron-chamber` is deprecated, removing the `s0lda/zed` and `s0lda/gpui-component` git patches from root `Cargo.toml`.
