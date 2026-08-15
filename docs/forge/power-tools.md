# ⚡ Hadron Forge Power Tools & MCP Suite

Hadron Forge (`crates/hadron-forge` and `crates/hadron-forge-mcp`) provides a comprehensive suite of developer environment tools and code intelligence capabilities directly to resident ACP Quarks via MCP.

---

## 🛠️ Power Tools Overview

| Tool Subsystem | MCP Tools | Key Capabilities |
| :--- | :--- | :--- |
| **Pre-Flight Merge Gate** | `hadron_forge_preflight_gate` | In-worktree merge gate simulation, entrypoint mtime refreshing, branch rebase sync, test runner execution. |
| **Peer Worktree Inspector**| `hadron_forge_peer_inspect` | Inspect peer worktree commits, uncommitted diffs, and export symbols across `.hadron/trees/*`. |
| **Nucleus Budget Linter** | `hadron_forge_nucleus_lint` | Strict 32 KB `.hadron/nucleus/index.md` budget checking, note YAML schema validation, unlinked note detection. |
| **AST Symbol Hierarchy** | `hadron_forge_symbol_hierarchy`, `hadron_forge_lsp_query` | Line-range scoped symbol tree with nested signatures across 14+ languages and generic in-process LSP queries. |
| **Background Processes** | `process_start`, `process_logs`, `process_list`, `process_send_stdin`, `process_kill` | Process group isolation (`PGID`), ring-buffer streaming logs, dev server supervision. |
| **3-Tier Code Intelligence** | `hadron_forge_symbol_lookup`, `hadron_forge_find_callers`, `hadron_forge_lsp_definition`, `hadron_forge_lsp_references` | Universal symbol indexing, caller resolution (Rule 1), and generic STDIO JSON-RPC 2.0 LSP client. |
| **Headless Browser** | `hadron_forge_browser_navigate`, `hadron_forge_browser_snapshot`, `hadron_forge_browser_screenshot`, `hadron_forge_browser_click`, `hadron_forge_browser_fill`, `hadron_forge_browser_eval` | Local CDP bridge (`localhost`, `127.0.0.1`, `file://`), accessibility tree snapshots, DOM inspection, UI testing. |
| **Jailed Screenshots** | `hadron_forge_screenshot_capture`, `hadron_forge_screenshot_list`, `hadron_forge_screenshot_prune` | Strict `<repo_root>/.hadron/screenshots/` containment (gitignored), window/process capture, zero PII leakage. |
| **Interactive PTY** | `hadron_forge_pty_start`, `hadron_forge_pty_write`, `hadron_forge_pty_read`, `hadron_forge_pty_resize`, `hadron_forge_pty_kill`, `hadron_forge_pty_list` | Unix pseudo-terminals (`libc::openpty`), raw keystrokes, interactive CLIs/TUIs. |
| **In-Process Mock Server** | `hadron_forge_mock_start`, `hadron_forge_mock_route_add`, `hadron_forge_mock_requests_list`, `hadron_forge_mock_assert`, `hadron_forge_mock_stop`, `hadron_forge_mock_list` | Loopback HTTP/WS mock endpoints (`127.0.0.1`), request journaling, assertion testing. |
| **Local SQLite Engine** | `hadron_forge_sqlite_query`, `hadron_forge_sqlite_schema`, `hadron_forge_sqlite_migrate`, `hadron_forge_sqlite_export` | Bundled `rusqlite`, schema introspection, transactional migrations, Markdown/CSV/JSON export. |
| **Polyglot Diagnostics** | `hadron_forge_run_command`, `hadron_forge_parse_diagnostics` | Jailed execution allowlist (`node`, `npm`, `pnpm`, `bun`, `deno`, `python3`, `pytest`, `go`, `cargo`) + compiler error parsers. |

---

## 1. Background Process Supervisor

Allows Quarks to boot long-running servers, test harnesses, or watcher processes during a turn without blocking synchronous dispatch:
- **Process-Group Teardown**: Automatically attaches processes to dedicated process groups (`setpgid(0, 0)`), ensuring clean shutdown of parent and child subprocesses upon termination.
- **Ring-Buffer Logging**: Captures stdout/stderr streams into fixed-capacity ring buffers, queryable with pagination or offset markers.

## 2. 3-Tier Polyglot Code Intelligence

Operationalizes Standard Model Rule 1 ("Prove it runs. Find its caller") with compiler-grade precision:
- **Tier 1: Embedded Tree-Sitter AST (Zero-Config Fallback)**: In-process AST symbol and caller extraction across 14+ languages with zero external dependencies.
- **Tier 2: Generic STDIO LSP Client**: JSON-RPC 2.0 client connecting dynamically to installed language servers (`rust-analyzer`, `vtsls`/`tsserver`, `pyright`, `gopls`, `clangd`).
- **Tier 3: Ecosystem Context (Context7)**: Integration with third-party library signatures and public documentation.

## 3. Headless Browser Verification Bridge

Empowers Quarks building web applications or visual interfaces to perform end-to-end verification:
- **Local Origin Enforcement**: Strictly confined to `localhost`, `127.0.0.1`, and local `file://` URIs to prevent external network egress.
- **Accessibility & DOM Snapshots**: Extracts lightweight, semantic DOM and accessibility trees ideal for LLM context consumption.
- **Visual Capture**: Takes high-resolution screenshots for visual regression and UI rendering verification.

## 4. Jailed Desktop & Window Screenshot Engine

Provides secure, PII-safe graphical desktop and window capture:
- **Strict Containment**: Saves captures exclusively under `<repo_root>/.hadron/screenshots/`. Path traversal attempts escaping `.hadron/` are rejected with security errors.
- **Gitignored**: `.hadron/` is untracked by default, ensuring screenshots are never committed to public git history.
- **Window Scoping**: Prioritizes capturing target application windows rather than multi-monitor desktops to protect background privacy.

## 5. Interactive PTY Session Manager

Allocates true pseudo-terminals (`/dev/pts`) for running interactive tools:
- Handles line buffering, raw ANSI control codes, and interactive confirmation prompts.
- Enables interactive debugging with REPLs, database shells, and full-screen TUI utilities.

## 6. In-Process Mock HTTP/WebSocket Server

Spins up lightweight loopback mock servers directly within the Forge process:
- **Route Mocking**: Define static or dynamic responses for REST endpoints and WebSocket channels.
- **Request Journaling**: Records all received requests with headers and payloads.
- **Assertion Testing**: Quarks can assert that specific endpoints were called with expected payloads.

## 7. Local SQLite & Migration Engine

Directly inspects and modifies SQLite databases within the worktree:
- **Schema Introspection**: Generates markdown tables of tables, columns, indexes, and foreign keys.
- **Transactional Migrations**: Executes migration scripts within transactions, automatically rolling back on syntax or constraint errors.
- **Exporting**: Formats query outputs as GitHub-flavored Markdown tables, JSON, or CSV.

## 8. Pre-Flight Merge Gate Runner

Enables Quarks to simulate the daemon's merge gate before completing turns:
- **Stale Target Invalidation**: Touches crate entrypoints (`src/lib.rs`) to prevent reusing foreign `.rlib` build artifacts across concurrent worktrees.
- **Rebase Verification**: Performs `merge::sync` against the base branch to catch rebase conflicts early.
- **Automatic Test Execution**: Runs workspace or crate test suites with environment variables matched to the Gluon Merge Gate.

## 9. Cross-Worktree Peer Inspector

Enables autonomous coordination and cross-examination across concurrent Quarks:
- **Branch & Commit Inspection**: Queries commit history, ahead/behind status, and branch heads in `.hadron/trees/*`.
- **Diff & Symbol Export**: Inspects uncommitted diffs and exported symbols without modifying sibling worktree states.

## 10. Nucleus Memory & Budget Linter

Guards the shared Nucleus against corruption and index budget overflow:
- **Strict Budget Ceiling**: Verifies `.hadron/nucleus/index.md` byte size against the 32 KB threshold.
- **Note Frontmatter Schema**: Enforces `name`, `description`, and `metadata.type` validation on notes.
- **Orphan Note Detection**: Detects notes missing pointer lines in the index or dead pointers to non-existent note files.

## 11. AST & LSP Symbol Hierarchy Intelligence

Provides deep symbol navigation across large polyglot codebases:
- **Hierarchical Outline**: Extracts symbol nesting, parent-child relationships, and exact line-range bounds.
- **Generic LSP Query Bridge**: Interfaces with active language servers for type definitions, references, and signatures.

