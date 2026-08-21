# ⚡ Hadron Forge Power Tools & MCP Suite

Hadron Forge (`crates/hadron-forge` and `crates/hadron-forge-mcp`) provides a comprehensive suite of developer environment tools and code intelligence capabilities directly to resident ACP Quarks via MCP.

---

## 🛠️ Power Tools Overview

| Tool Subsystem | MCP Tools | Key Capabilities |
| :--- | :--- | :--- |
| **Pre-Flight Merge Gate** | `hadron_forge_preflight_gate`, `hadron_forge_acceptance_gate` | In-worktree merge gate simulation, entrypoint mtime refreshing, branch rebase sync, test runner and acceptance suite execution. |
| **Peer Worktree Inspector**| `hadron_forge_peer_inspect`, `hadron_forge_peers_detect_conflicts` | Inspect peer worktree commits, uncommitted diffs, export symbols, and detect cross-worktree merge conflicts before gate submission. |
| **Nucleus Memory & Distillation** | `hadron_forge_nucleus_lint`, `hadron_forge_nucleus_distill_lesson` | Configured budget checking (16–128 KB), YAML schema validation, orphan detection, and autonomous 1-line pointer distillation. |
| **Autonomous Spec & DAG Compiler** | `hadron_forge_spec_compile` | Compile high-level natural language prompts into formal Design Specs and validated Gluon DAG plans (`.hadron/docs/plans/*.md`). |
| **Visual & Behavioral E2E Asserter** | `hadron_forge_e2e_assert` | Multi-step headless browser flows, DOM text assertions, and jailed visual screenshot captures. |
| **Packaging & Live Preview Launcher** | `hadron_forge_preview_launch` | Release artifact builds, isolated background server lifecycle, and loopback health endpoint probing. |
| **Autonomous Scaffolder & Dependencies** | `hadron_forge_scaffold` | Multi-stack project scaffolder (Rust, Vite, Python, Next.js) and safe package installer with lockfile verification. |
| **Security & Secret Scanner Gate** | `hadron_forge_security_audit` | AST and dependency scanner detecting hardcoded API secrets, private keys, command/SQL injections, and unjailed paths. |
| **Runtime Service & Crash Watchdog** | `hadron_forge_service_watchdog` | Real-time monitoring of service log buffers for panics, tracebacks, port conflicts, and automated remediation advice. |
| **AST Symbol Hierarchy** | `hadron_forge_symbol_hierarchy`, `hadron_forge_lsp_query` | Line-range scoped symbol tree with nested signatures across 14+ languages and generic in-process LSP queries. |
| **Background Processes** | `process_start`, `process_logs`, `process_list`, `process_send_stdin`, `process_kill` | Process group isolation (`PGID`), ring-buffer streaming logs, dev server supervision. |
| **3-Tier Code Intelligence** | `hadron_forge_symbol_lookup`, `hadron_forge_find_callers`, `hadron_forge_lsp_definition`, `hadron_forge_lsp_references` | Universal symbol indexing, caller resolution (Rule 1), and generic STDIO JSON-RPC 2.0 LSP client. |
| **Headless Browser** | `hadron_forge_browser_navigate`, `hadron_forge_browser_snapshot`, `hadron_forge_browser_screenshot`, `hadron_forge_browser_click`, `hadron_forge_browser_fill`, `hadron_forge_browser_eval` | Local CDP bridge (`localhost`, `127.0.0.1`, `file://`), accessibility tree snapshots, DOM inspection, UI testing. |
| **Jailed Screenshots** | `hadron_forge_screenshot_capture`, `hadron_forge_screenshot_list`, `hadron_forge_screenshot_prune` | Strict `<repo_root>/.hadron/screenshots/` containment (gitignored), window/process capture, zero PII leakage. |
| **Interactive PTY** | `hadron_forge_pty_start`, `hadron_forge_pty_write`, `hadron_forge_pty_read`, `hadron_forge_pty_resize`, `hadron_forge_pty_kill`, `hadron_forge_pty_list` | Unix pseudo-terminals (`libc::openpty`), raw keystrokes, interactive CLIs/TUIs. |
| **In-Process Mock Server** | `hadron_forge_mock_start`, `hadron_forge_mock_route_add`, `hadron_forge_mock_requests_list`, `hadron_forge_mock_assert`, `hadron_forge_mock_stop`, `hadron_forge_mock_list` | Loopback HTTP/WS mock endpoints (`127.0.0.1`), request journaling, assertion testing. |
| **Local SQLite Engine** | `hadron_forge_sqlite_query`, `hadron_forge_sqlite_schema`, `hadron_forge_sqlite_migrate`, `hadron_forge_sqlite_export` | Bundled `rusqlite`, schema introspection, transactional migrations, Markdown/CSV/JSON export. |
| **Polyglot Diagnostics** | `hadron_forge_run_command`, `hadron_forge_parse_diagnostics` | Jailed execution allowlist (`node`, `npm`, `pnpm`, `bun`, `deno`, `python3`, `pytest`, `go`, `cargo`) + compiler error parsers. |
| **Blast Radius Analyzer** | `hadron_forge_blast_radius` | Static crate dependency graph, reverse caller traversal, and affected test suite impact calculator. |
| **Automated Git Bisect** | `hadron_forge_git_bisect` | Autonomous binary regression finder executing test predicates across commit ranges. |
| **Wiretap Protocol Monitor** | `hadron_forge_wiretap` | Real-time NDJSON and HTTP IPC packet monitor, query filter, and frame assertion validator. |
| **AST Structural Rewrite** | `hadron_forge_ast_rewrite` | Tree-sitter structural pattern search and cross-file code transformation without regex fragility. |
| **Secret Vault & Masking** | `hadron_forge_secret_vault` | Ephemeral sandboxed credential vault with automated stdout/stderr secret masking. |
| **Universal Flamegraph Profiler** | `hadron_forge_flamegraph_profiler` | Non-intrusive CPU & allocation profiler generating interactive SVGs and folded stack hotspot reports. |
| **Property-Based Fuzz Harness** | `hadron_forge_fuzz_harness` | Randomized fuzz test runner for IPC codecs, serializers, and boundary condition verification. |
| **Nucleus Knowledge Graph** | `hadron_forge_nucleus_graph` | Graph connectivity, dead link, orphaned note, and invariant coverage analyzer for `.hadron/nucleus/`. |
| **Binary Bloat & Symbol Inspector** | `hadron_forge_binary_bloat` | ELF/DWARF symbol size analyzer, section breakdown, and binary bloat regression detector. |
| **Automated Release Sync** | `hadron_forge_release_sync` | Conventional commit analyzer, SemVer bump calculator, and CHANGELOG generator. |
| **Lattice Time-Travel & Branching** | `hadron_forge_time_travel` | Lattice turn rewind, state snapshot diffing, and session branch exploration. |
| **Mutation Testing Gate** | `hadron_forge_mutation_gate` | Adversarial AST mutation runner and test harness kill-rate validator. |
| **Benchmark Regression Guard** | `hadron_forge_benchmark_guard` | Baseline execution timing and CPU flamegraph regression guard. |
| **Spatial Architecture Topology** | `hadron_forge_topology_graph` | Spatial DAG and crate dependency topology visualizer and exporter. |
| **Visual Task Scheduler** | `hadron_forge_task_scheduler` | Topological task graph optimizer and parallel Quark dispatch scheduler. |
| **Preon Dynamic Evolution** | `hadron_forge_preon_evolution` | Recurring failure pattern analyzer and autonomous preon rule synthesizer. |
| **Swarm Prompt Distiller** | `hadron_forge_prompt_distiller` | Per-model context compressor and token spend optimizer. |
| **Hybrid Swarm Mesh** | `hadron_forge_swarm_mesh` | Remote ACP worker container offload, status inspector, and execution runner. |
| **Interactive PTY Pairing** | `hadron_forge_pty_pairing` | Interactive shared PTY pairing broker with live steer and multi-quark observer sessions. |
| **Tool Execution Breakpoints** | `hadron_forge_tool_breakpoints` | Dynamic pause/resume breakpoints on MCP tool execution and subprocess commands. |
| **Structured Research Documents** | `hadron_forge_research_write`, `hadron_forge_research_list`, `hadron_forge_research_read` | Architectural investigation documents, topic syntheses in `.hadron/docs/research/`, and task synchronisation. |

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

## 8. Pre-Flight Merge Gate & Acceptance Suite

Enables Quarks to simulate the daemon's merge gate before completing turns:
- **Stale Target Invalidation**: Touches crate entrypoints (`src/lib.rs`) to prevent reusing foreign `.rlib` build artifacts across concurrent worktrees.
- **Rebase Verification**: Performs `merge::sync` against the base branch to catch rebase conflicts early.
- **Automatic Test Execution**: Runs workspace or crate test suites with environment variables matched to the Gluon Merge Gate.
- **Acceptance Verification**: Executes multi-modal acceptance test suites (`hadron_forge_acceptance_gate`) with bounded timeouts and failure isolation.

## 9. Cross-Worktree Peer Inspector & Conflict Detector

Enables autonomous coordination and cross-examination across concurrent Quarks:
- **Branch & Commit Inspection**: Queries commit history, ahead/behind status, and branch heads in `.hadron/trees/*`.
- **Diff & Symbol Export**: Inspects uncommitted diffs and exported symbols without modifying sibling worktree states.
- **Continuous Conflict Detection**: Proactively identifies file collisions and overlapping modifications across active worktrees before gate arrival (`hadron_forge_peers_detect_conflicts`).

## 10. Nucleus Memory Linter & Autonomous Distillation

Guards the shared Nucleus against corruption and automatically preserves lessons:
- **Budget Enforcement**: Verifies `.hadron/nucleus/index.md` byte size against configured budget ceilings (16–128 KB).
- **Note Frontmatter Schema**: Enforces `name`, `description`, and `metadata.type` validation on notes.
- **Autonomous Distillation**: Distills verified post-mortems into `notes/<slug>.md` while strictly maintaining 1-line pointer entries in `index.md` (`hadron_forge_nucleus_distill_lesson`).

## 11. AST & LSP Symbol Hierarchy Intelligence

Provides deep symbol navigation across large polyglot codebases:
- **Hierarchical Outline**: Extracts symbol nesting, parent-child relationships, and exact line-range bounds.
- **Generic LSP Query Bridge**: Interfaces with active language servers for type definitions, references, and signatures.

## 12. Autonomous Spec & DAG Plan Compiler

Translates raw natural language user requests into production architecture invariants and plans:
- **Spec Generation**: Author comprehensive design specifications with invariants, data models, and error modes.
- **DAG Plan Compilation**: Compiles bite-sized, verifiable task graphs into `.hadron/docs/plans/*.md` formatted for Gluon DAG auto-dispatch (`hadron_forge_spec_compile`).

## 13. Visual & Behavioral E2E Asserter

Drives headless browser flows and visual verification:
- **Multi-Step Flow Execution**: Automates navigation, form input, button clicks, and element waits.
- **DOM & Text Assertions**: Asserts presence and content of page elements with detailed pass/fail diagnostics.
- **Visual Proof**: Captures screenshots jailed in `.hadron/screenshots/` proving visual correctness (`hadron_forge_e2e_assert`).

## 14. Packaging & Live Preview Launcher

Delivers verified, runnable applications to users:
- **Stack-Aware Packaging**: Detects project build targets (`cargo build --release`, `npm run build`, `vite build`) and produces production artifacts.
- **Isolated Service Launch**: Supervises background preview servers with process-group teardown.
- **Health Endpoint Probing**: Verifies HTTP response codes and health checks prior to declaring readiness (`hadron_forge_preview_launch`).

## 15. Autonomous Scaffolder & Dependency Resolver

Standardizes project bootstrapping and dependency management:
- **Multi-Stack Scaffolding**: Initializes Rust binaries/libraries, Vite frontends (React, Vue, Svelte, Vanilla TS), Python packages, and Next.js applications (`hadron_forge_scaffold`).
- **Safe Dependency Management**: Adds packages to manifests (`Cargo.toml`, `package.json`, `pyproject.toml`) and runs lockfile verification.

## 16. Security & Secret Scanner Gate

Protects codebases against accidental vulnerability or secret introduction:
- **Secret Detection**: Detects API tokens, AWS keys, private keys, and credential patterns.
- **Vulnerability Analysis**: Scans for shell injection, SQL injection, and path traversal escaping security boundaries (`hadron_forge_security_audit`).

## 17. Runtime Service & Crash Watchdog

Monitors background services for unhandled failures:
- **Log Ring-Buffer Analysis**: Detects panics, stack traces, unhandled promise rejections, and port binding conflicts.
- **Self-Healing Diagnostics**: Extracts root causes and provides structured remediation instructions (`hadron_forge_service_watchdog`).

## 18. Blast Radius Impact Analyzer

Calculates the ripple effect of modified code across crate dependencies and callers:
- **Workspace Dependency Graph**: Computes topological crate dependencies and identifies affected downstream packages (`hadron_forge_blast_radius`).
- **Reverse Caller Traversal**: Traces changed symbols and traits to list callers that require verification.
- **Targeted Test Selection**: Emits the minimum set of targeted `cargo test -p <crate>` commands to run before full gate submission.

## 19. Automated Git Bisect Regression Finder

Automates binary search bisection to pinpoint the commit that introduced a defect:
- **Autonomous Predicate Execution**: Tests custom shell commands across git commit histories (`hadron_forge_git_bisect`).
- **Fault Commit Isolation**: Returns exact offending commit hash, author, and commit message.
- **Clean State Teardown**: Guarantees clean working tree restoration to the original branch head upon completion.

## 20. Wiretap IPC & Protocol Monitor

Real-time packet inspection and verification for Quark and daemon communications:
- **NDJSON Stream Recording**: Captures framing events, payloads, and timestamps across ACP and daemon channels (`hadron_forge_wiretap`).
- **Protocol Filtering**: Search traffic by event kind, sender, or content matching.
- **Assertion Validation**: Asserts expected payload structure and message sequencing.

## 21. AST Structural Pattern Rewrite Engine

Tree-sitter powered semantic pattern matching and structural code transformation:
- **Syntax-Aware Rewriting**: Matches AST patterns rather than fragile raw regular expressions (`hadron_forge_ast_rewrite`).
- **Polyglot Grammar Support**: Structural search and transformation across Rust, TypeScript, JavaScript, Python, and Go.
- **Safe Multi-File Refactoring**: Preview diffs and execute unified replacements across crate trees.

## 22. Secret Vault & Ephemeral Masking Proxy

Sandboxed credential management preventing token leakage:
- **Ephemeral Key Isolation**: Injects secrets into isolated subprocess environments without persisting plaintext to disk (`hadron_forge_secret_vault`).
- **Automated Output Masking**: Intercepts stdout and stderr streams to redact sensitive patterns before field persistence.
- **Zero-Disk Leaks**: Ephemeral memory-only keys destroyed on process exit.

## 23. Universal Flamegraph Profiler

Non-intrusive CPU and memory allocation profiling:
- **Universal Sampling**: Gathers execution profiles across native binaries and interpreted scripts (`hadron_forge_flamegraph_profiler`).
- **Interactive SVG Generation**: Renders responsive SVG flamegraphs with zoomable call stacks.
- **Folded Stack Aggregations**: Generates top-10 hotspot lists and folded stack summaries for autonomous agent reasoning.

## 24. Property-Based Fuzz Harness

Automated edge-case generator for codecs, serializers, and protocols:
- **Randomized Mutation**: Generates adversarial payloads, UTF-8 boundary slices, and malformed headers (`hadron_forge_fuzz_harness`).
- **Crash & Hang Detection**: Traps unexpected panics, infinite loops, and memory blowups with timeout enforcement.
- **Minimal Failure Reproduction**: Isolates minimal reproducible inputs for rapid debugging.

## 25. Nucleus Knowledge Graph

Visual and structural graph analysis of the shared swarm memory:
- **Bidirectional Note Graph**: Traces `[[slug]]` links, orphaned notes, and citation clusters (`hadron_forge_nucleus_graph`).
- **Invariant Coverage Audit**: Maps invariants to implementation files and active notes.
- **Cycle & Dead Link Detection**: Detects broken pointer references before budget exhaustion.

## 26. Binary Bloat & ELF Symbol Inspector

Deep analysis of executable footprint and dependency contribution:
- **Section & Symbol Breakdown**: Analyzes `.text`, `.rodata`, and symbol table sizes (`hadron_forge_binary_bloat`).
- **Crate Size Contribution**: Breaks down static binary size by workspace crate and third-party dependency.
- **Bloat Regression Detection**: Compares builds to detect accidental footprint expansion.

## 27. Automated Release Sync & SemVer Engine

Semantic versioning sync, changelog generation, and tag auditing:
- **Conventional Commit Parsing**: Parses commit subjects (`feat:`, `fix:`, `perf:`, `BREAKING CHANGE`) across commit ranges (`hadron_forge_release_sync`).
- **SemVer Recommendation**: Computes recommended Major/Minor/Patch bump based on conventional specifications.
- **Changelog Formatting**: Formats Keep-a-Changelog Markdown snippets ready for release audits.

## 28. Lattice Time-Travel & Session Branching

Turn-level rewind, ledger diffing, and speculative branching:
- **State Rewind**: Rewinds the swarm event ledger to any historical turn ID (`hadron_forge_time_travel`).
- **Snapshot Diffing**: Computes unified state diffs between arbitrary turn checkpoints.
- **Branching Exploration**: Creates alternate execution worktrees from historical turn points.

## 29. Mutation Testing Gate

Adversarial AST mutations to verify test suite fault coverage:
- **AST Operators**: Injects arithmetic, conditional boundary, and boolean inversion mutations (`hadron_forge_mutation_gate`).
- **Kill-Rate Scoring**: Computes mutation kill rates and flags surviving mutant candidates.
- **Test Quality Verification**: Guarantees test suites verify behavior rather than superficial execution.

## 30. Benchmark Regression Guard

Performance baseline timing and CPU regression gate:
- **Timing Benchmarking**: Runs micro-benchmarks with statistical variance detection (`hadron_forge_benchmark_guard`).
- **Flamegraph Profiling Integration**: Compares before/after flamegraph folded stacks.
- **Threshold Gating**: Rejects branches introducing configurable latency regressions (>5%).

## 31. Spatial Architecture Topology Graph

Interactive crate dependency and symbol connection visualizer:
- **Crate & Symbol DAG**: Constructs complete dependency topologies across multi-crate workspaces (`hadron_forge_topology_graph`).
- **Export Formats**: Renders Mermaid DAG diagrams, DOT graphs, and JSON trees.
- **Cycle & Bottleneck Analysis**: Identifies architectural circular dependencies and hot path clusters.

## 32. Visual Task Scheduler & DAG Dispatcher

Topological task planning and parallel Quark scheduling:
- **Dependency Sorting**: Resolves optimal execution ordering for complex task sets (`hadron_forge_task_scheduler`).
- **Parallel Dispatch Waves**: Groups independent tasks into concurrent dispatch waves for available Quarks.
- **Cycle Detection**: Validates plan graphs against accidental deadlocks.

## 33. Preon Dynamic Evolution Engine

Automated failure pattern analysis and preon specialization synthesizer:
- **Failure Cluster Analysis**: Analyzes recurring friction in `.hadron/nucleus/notes/` (`hadron_forge_preon_evolution`).
- **Rule Synthesis**: Generates targeted `.hadron/preons/` markdown specialization rules.
- **Continuous Calibration**: Automatically updates swarm operational heuristics based on real gate outcomes.

## 34. Swarm Prompt Distiller & Context Compressor

Token spend optimization and per-model context distillation:
- **Prompt Compression**: Prunes redundant preamble and dead context tokens (`hadron_forge_prompt_distiller`).
- **Model-Specific Targeting**: Distills instructions to match small, fast models vs heavy reasoning models.
- **Spend Reduction**: Minimizes ongoing per-turn token spend across long-running swarms.

## 35. Hybrid Swarm Mesh & Remote Container Offload

Orchestrated compute offload for mixed-infrastructure swarms:
- **Container Template Generation**: Generates Docker/container definitions for isolated builds (`hadron_forge_swarm_mesh`).
- **Remote Dispatch**: Offloads heavy cargo builds, test matrices, and sandboxed quarks.
- **Mesh Status Telemetry**: Monitors health, CPU/memory, and task status across remote worker nodes.

## 36. Interactive Multi-Quark PTY Pairing Broker

Shared pseudo-terminal pairing and real-time AI steering:
- **Multi-Seat PTY Canvas**: Multiple Quarks and the human co-observe a single live terminal (`hadron_forge_pty_pairing`).
- **Live Steering**: Injects keystrokes, signals, and interactive commands with collision control.
- **Terminal Session Capture**: Streams terminal frames and logs to the swarm ledger.

## 37. Dynamic MCP Tool Execution Breakpoints

Human-in-the-loop and automated breakpoint control on tool execution:
- **Breakpoint Configuration**: Sets execution breakpoints on specific MCP tools or patterns (`hadron_forge_tool_breakpoints`).
- **Turn Pause & Resume**: Pauses Quark execution prior to tool dispatch for inspection or parameter adjustment.
- **Safety Interception**: Prevents accidental destructive modifications mid-turn.

## 38. Structured Research Documents

Standardized architectural investigations and deep codebase research:
- **Research Lifecycle**: Writes, lists, and reads structured research papers in `.hadron/docs/research/` (`hadron_forge_research_write`, `hadron_forge_research_list`, `hadron_forge_research_read`).
- **Standardized Schema**: Frontmatter metadata (`slug`, `title`, `author`, `date`, `target_area`) with structured sections for Executive Summary, Key Findings, Constraints, Trade-Offs, and Recommendations.
- **Task Integration**: Integrates directly with the `/research` slash command and auto-synchronizes plan task titles.




