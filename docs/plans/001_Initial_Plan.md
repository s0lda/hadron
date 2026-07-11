# 🗺️ The Execution Phases

## Phase 1: Scaffolding the Lattice (Core Protocol & State)

Goal: Establish the shared language and ensure it runs on Windows, Mac, and Linux.

Shared Structs (hadron-lattice): Define strict Rust types for LedgerEntry (Chat, FileEdit, BashCommand, PermissionRequest).

Global Configs (directories crate):

Automatically resolve OS paths: ~/.config/hadron/ (Linux), AppData\Roaming\hadron\ (Windows), Library/Application Support/hadron/ (Mac).

Scaffold global_usage.sqlite for token/budget tracking.

Workspace Initialization: hadron init scaffolds a hidden .hadron/ folder inside the user's active code project, containing ledger.jsonl (the bus) and session.toml.

## Phase 2: Native Git Integration & Safety (hadron-gluon)

Goal: AI agents hallucinate. Native version control is the instant "Undo" button.

Gitoxide (gix): Integrate pure-Rust Git bindings. No dependency on the host OS's git installation.

Auto-Snapshotting: Before writing an agent's code edit to disk, silently create a hidden stash/commit (auto-save: Pre-Agent-Edit).

Diff-Based Context: Feed the Swarm lightweight git diff outputs via the file bus instead of whole files to drastically reduce token usage.

Auto-Rollback Tool: If an agent executes a command (cargo check or npm run build) that fails, the Orchestrator instantly triggers a rollback via gix, reverting the file and forcing the agent to rethink based on the compiler error.

## Phase 3: Usage Quotas & Intelligent Routing (hadron-gluon)

Goal: Track model budgets locally and intelligently route around exhausted limits.

The Usage Ledger (rusqlite): Track tokens_used, rpm_count, and reset_timestamp in the SQLite DB.

The Pre-Flight Interceptor: Parse HTTP headers on every API call. Before routing a task to an API model (like Claude), check the DB. If Claude is rate-limited, intercept the intent and dynamically reroute the task to an available local model, or pause execution to wait for Quark.

## Phase 4: The 0-CPU File Bus & Edit-by-Hash (hadron-gluon)

Goal: Manage concurrent, multi-agent file editing without corrupting line numbers.

The Watcher (notify): Setup OS-level file watching on .hadron/ledger.jsonl.

AST Hashing (tree-sitter + blake3): Parse target files into logical blocks (Functions, Structs) and hash them. Provide the Swarm with context like: [Hash: 9f86d0] fn main() { ... }.

Optimistic Concurrency: If Agent A submits an edit to [Hash: 9f86d0], but Agent B already modified that block 2 seconds ago, the engine rejects Agent A's edit. Agent A must pull the updated state and try again.

The Swarm Loop: Tokio tasks for active models sleep at 0% CPU. When notify detects an append to the ledger, the targeted model wakes up, executes via its API (reqwest), and appends the result.

## Phase 5: The Chamber (Native GPUI Client)

Goal: A blazing-fast UI that visualizes the Swarm without blocking the backend.

The Layout (Flexbox in Rust):

Omnibar (Bottom): Raycast-style command bar. Hitting Enter simply appends to ledger.jsonl. (The UI never talks to the AI directly).

Center Pane (The Orchestrator): High-quality markdown rendering of the chat. Uses GPUI native tabs for "Chat View" and "Code View".

Right Drawer (The Matrix): A live visual feed of agents working in the background, parsed from the ledger.

Footer: Live token usage, active bypass permissions, and an indicator waiting for the Quark model plugin.

Visualizing "Edit-by-Hash": Because GPUI uses tree-sitter natively, when an agent targets a hashed block, the UI smoothly overlays a pulsing bounding-box over that specific function in the code viewer, letting the user watch the AI work in real-time.

## Phase 6: The Bypass Matrix & Modals (Gatekeeper)

Goal: Safely handle dangerous operations (file deletes, bash scripts).

Non-Blocking Modals: When the Daemon needs permission, it writes {"intent": "permission_req", "command": "cargo publish"} to the ledger and pauses that specific agent's Tokio thread.

UI Response: The UI detects this and drops down a sleek toast: ⚠️ Orchestrator wants to publish. [Approve] [Deny].

God-Mode: Toggle switches in the UI to auto-approve workspace edits (Level 1) or fully bypass all bash execution prompts (Level 2).

# 📦 The Cargo.toml Workspace Stack

```toml
[workspace]
members = ["crates/hadron-gluon", "crates/hadron-chamber", "crates/hadron-lattice"]

# --- hadron-gluon ---
[dependencies]
tokio = { version = "1", features = ["full"] }
directories = "5.0"
rusqlite = { version = "0.31", features = ["bundled"] }
notify = "6.1"
gix = "0.62" # Gitoxide
tree-sitter = "0.22"
blake3 = "1.5"
reqwest = { version = "0.12", features = ["json", "stream"] }
async-openai = "0.23"
hadron-lattice = { path = "../hadron-lattice" }

# --- hadron-chamber ---
[dependencies]
gpui = "0.1"
tokio = { version = "1", features = ["rt-multi-thread"] }
hadron-lattice = { path = "../hadron-lattice" }
```
