# 🕹️ Hadron Chat Commands

Hadron provides an interactive suite of `/slash` commands directly inside the Chamber chat interface. Commands can be invoked at the beginning of any line (and are ignored inside markdown code fences).

---

## 🧭 Swarm Mission & Autonomous Execution

| Command | Syntax | Description | Example |
| :--- | :--- | :--- | :--- |
| `/goal` | `/goal <objective>` | Synthesize an end-to-end swarm mission, author a structured plan in `.hadron/docs/plans/`, and fan out tasks across worker Quarks. | `/goal Build user auth with JWT` |
| `/research` | `/research <topic>` | Conduct in-depth research across the codebase or web, synthesising findings into a structured document in `.hadron/docs/research/`. | `/research memory lifecycle and compaction` |
| `/loop` | `/loop [count] <objective>` | Execute an iterative evaluation loop until completion criteria are met or count expires. | `/loop 5 fix compiler errors` |
| `/absorb` | `/absorb [options]` | Scan foreign assistant folders (`.agents/`, `.claude/`, `CLAUDE.md`, `.cursor/`, `.windsurf/`, `.kimi/`, `.superpowers/`) and distill memories, skills, invariants, and plans into `.hadron/`. | `/absorb` |

---

## 🛠️ Workflow & Skill Invocations

Skill commands post a message carrying canonical triggers so the Gluon engine automatically attaches the appropriate procedural skill to the dispatched Quark turn.

| Command | Syntax | Description | Example |
| :--- | :--- | :--- | :--- |
| `/brainstorm` | `/brainstorm [@Quark] <prompt>` | Explore user intent, requirements, and design before any code. | `/brainstorm @Quark the new menu` |
| `/team-brainstorm` | `/team-brainstorm <prompt>` | Kick off multi-agent brainstorming with the whole team. | `/team-brainstorm new LSP architecture` |
| `/writing-plans` | `/writing-plans [@Quark] <prompt>` | Turn a settled design into a detailed, bite-sized implementation plan. | `/writing-plans @Quark database schema` |
| `/executing-plans` | `/executing-plans [@Quark] <prompt>` | Work through an existing plan task-by-task with review checkpoints. | `/executing-plans @Quark task 2` |
| `/reviewing-work` | `/reviewing-work [@Quark] <prompt>` | Perform code review or plan verification on changes or commit diffs. | `/reviewing-work @Quark recent changes` |
| `/dispatching-parallel-agents` | `/dispatching-parallel-agents [@Quark] <prompt>` | Dispatch 2+ independent tasks across available worker Quarks. | `/dispatching-parallel-agents tasks 1 and 2` |
| `/finishing-a-development-branch` | `/finishing-a-development-branch [@Quark] <prompt>` | Guide branch completion, PRs, merge preparation, or cleanup. | `/finishing-a-development-branch @Quark feature-x` |
| `/receiving-code-review` | `/receiving-code-review [@Quark] <prompt>` | Process and verify code review feedback with technical rigor. | `/receiving-code-review @Quark comments` |
| `/requesting-code-review` | `/requesting-code-review [@Quark] <prompt>` | Request review before completing or merging major work. | `/requesting-code-review @Quark ready for review` |
| `/subagent-driven-development` | `/subagent-driven-development [@Quark] <prompt>` | Execute multi-task implementation plan via subagents. | `/subagent-driven-development @Quark execute plan` |
| `/systematic-debugging` | `/systematic-debugging [@Quark] <prompt>` | Investigate bugs, test failures, and unexpected behaviors before proposing fixes. | `/systematic-debugging @Quark test failure in auth` |
| `/test-driven-development` | `/test-driven-development [@Quark] <prompt>` | Implement feature or bugfix using test-driven development. | `/test-driven-development @Quark parser tests` |
| `/using-git-worktrees` | `/using-git-worktrees [@Quark] <prompt>` | Isolate feature work in a dedicated git worktree branch. | `/using-git-worktrees @Quark feature-branch` |
| `/using-superpowers` | `/using-superpowers [@Quark] <prompt>` | Discover and activate available skills/superpowers. | `/using-superpowers @Quark` |
| `/verification-before-completion` | `/verification-before-completion [@Quark] <prompt>` | Run verification commands and assert evidence before claiming work is complete. | `/verification-before-completion @Quark` |
| `/writing-skills` | `/writing-skills [@Quark] <prompt>` | Create, edit, or verify custom skill procedures. | `/writing-skills @Quark new-lint-skill` |
| `/security-review` (alias `/security`) | `/security-review [@Quark] <prompt>` | Audit authentication, permissions, secrets, and injection vectors (Rule 7). | `/security-review @Quark auth endpoints` |
| `/architecture-audit` (alias `/arch`) | `/architecture-audit [@Quark] <prompt>` | Audit component decoupling, SSOT integrity (Rule 3), and type-system invariants (Rule 8). | `/architecture-audit @Quark state machine` |
| `/code-review` | `/code-review [@Quark] <prompt>` | Review code changes, plans, or commit diffs. | `/code-review @Quark recent changes` |
| `/chaos-testing` (alias `/chaos-test`) | `/chaos-testing [@Quark] <prompt>` | Stress test concurrency races, timeout handling, and edge failure modes. | `/chaos-testing @Quark event channel` |
| `/performance-audit` (aliases `/perf-audit`, `/optimize`) | `/performance-audit [@Quark] <prompt>` | Profile CPU/memory allocations, lock contention, and render-loop lag. | `/performance-audit @Quark list rendering` |
| `/code-simplification` (aliases `/simplify`, `/refactor`) | `/code-simplification [@Quark] <prompt>` | Prune dead abstractions, unused types/imports, and reduce complexity (Rule 10). | `/simplify @Quark router module` |
| `/api-design` (alias `/contract`) | `/api-design [@Quark] <prompt>` | Design type-safe API contracts, wire protocols, and error schemas. | `/api-design @Quark daemon RPC` |
| `/incident-investigation` (aliases `/triage`, `/investigate`) | `/incident-investigation [@Quark] <prompt>` | Systematic failure reproduction, log dissection, and post-mortem analysis. | `/triage @Quark flaky gate test` |
| `/memory-curation` (alias `/curate-memory`) | `/memory-curation [@Quark] <prompt>` | Maintain Nucleus (Rule 9) — distill lessons into notes/, prune index.md, update features.md. | `/curate-memory @Quark` |
| `/release` | `/release` | Execute repository release workflow strictly following `.hadron/nucleus/release.md`. | `/release` |
| `/review` | `/review [@Quark]` | Request peer review on active branch before merge gate. | `/review @Quark` |
| `/add-skill` | `/add-skill <name>` | Add a custom skill from file (`@path/to/file.md`) or paste content. | `/add-skill @skills/lint.md` |

---

## 👥 Quark Governance & Control

| Command | Syntax | Description | Example |
| :--- | :--- | :--- | :--- |
| `/mode` | `/mode [@Quark] <ask\|write\|auto\|bypass>` | Set execution permission mode for a seat or global default. | `/mode @Quark bypass` |
| `/toggle` | `/toggle @Quark` | Park or unpark a Quark — retains seat configuration while skipping turns. | `/toggle @Quark` |
| `/reboot` | `/reboot [@Quark \| all]` | Force-restart resident ACP / CLI agent subprocesses. | `/reboot @Quark` |
| `/stop` | `/stop @Quark` | Gracefully stop a Quark's in-flight turn. | `/stop @Quark` |
| `/kill` | `/kill @Quark` | Force-kill a Quark's subprocess group (`-pgid`). | `/kill @Quark` |
| `/cancel` | `/cancel [@Quark]` | Cancel pending unhandled dispatch for a seat. | `/cancel @Quark` |
| `/retry` | `/retry [@Quark]` | Re-dispatch the last failed message or turn. | `/retry @Quark` |
| `/approve` | `/approve @Quark [remember]` | Approve a pending permission request. | `/approve @worker remember` |
| `/deny` | `/deny @Quark` | Deny a pending permission request. | `/deny @worker` |
| `/limit` | `/limit @Quark <tokens>` | Set custom energy token limit for a seat. | `/limit @Quark 1000000` |
| `/reset-energy` | `/reset-energy [@Quark \| all]` | Reset used token ledger for a seat or all. | `/reset-energy @Quark` |
| `/team` | `/team` | Display active swarm roster, models, transports, and seats. | `/team` |

---

## 🌿 Version Control & Merge Gate

| Command | Syntax | Description | Example |
| :--- | :--- | :--- | :--- |
| `/git-init` | `/git-init` | Initialize a git repository with default `.gitignore` and initial commit. | `/git-init` |
| `/git-status` | `/git-status` | Show working tree status and active branch info. | `/git-status` |
| `/git-log` | `/git-log [N]` | Display the last N commits (default 5). | `/git-log 10` |
| `/push` | `/push [args]` | Push local commits to remote origin (`git push`). | `/push origin main` |
| `/pr` | `/pr [args]` | Create a GitHub Pull Request for active branch via `gh`. | `/pr` |
| `/diff` | `/diff [@Quark]` | Summarize branch diff against base branch or working tree. | `/diff @Quark` |
| `/gate-status` | `/gate-status` | Show which branch the Merge Gate is currently testing and time remaining. | `/gate-status` |
| `/gate-cancel` | `/gate-cancel` | Force cancel a hung merge gate run by terminating its process group. | `/gate-cancel` |
| `/abandon` | `/abandon @Quark [confirm]` | Archive-tag then discard a quark's pending unmerged branch. | `/abandon @Quark confirm` |
| `/unabandon` | `/unabandon <tag>` | Restore an archived branch from its archive tag. | `/unabandon archive/my-branch` |
| `/revert` | `/revert` | Revert the last landed commit on `main` via `git revert`. | `/revert` |
| `/prune` | `/prune [dry-run]` | Safely clean up merged and stale quark worktrees and branches. | `/prune` |

---

## 🧠 Nucleus & Knowledge Management

| Command | Syntax | Description | Example |
| :--- | :--- | :--- | :--- |
| `/learn` | `/learn <fact>` | Pin a post-mortem lesson into this repository's Nucleus (`.hadron/nucleus/`). | `/learn always run cargo fmt first` |
| `/learn-global` | `/learn-global <fact>` | Pin a lesson into your global nucleus across all repositories. | `/learn-global use locked dependencies` |
| `/learn-std-model` | `/learn-std-model <law>` | Add a standard law to this repo (`laws.md`). | `/learn-std-model verify caller before reporting` |
| `/learn-std-model-global`| `/learn-std-model-global <law>` | Add a standard law across every repo you run Hadron in. | `/learn-std-model-global push constraints to types` |
| `/nucleus` | `/nucleus` | Show nucleus index size vs resolved budget (16/32/64/128 KiB), lesson count, and notes path. | `/nucleus` |
| `/compact-nucleus` | `/compact-nucleus [budget_kb]` | Audit, rank, and compact nucleus index against target budget. | `/compact-nucleus 32` |
| `/doctor` | `/doctor` | Run automated system diagnostics on daemon, lockfiles, nucleus budget, fonts, and worktrees. | `/doctor` |
| `/vocabulary` | `/vocabulary` | Display the definitive Hadron particle physics mental model and glossary. | `/vocabulary` |
| `/skills` | `/skills` | List all registered procedural skills and their matching trigger phrases. | `/skills` |

---

## 💬 Session & Chamber UI Management

| Command | Syntax | Description | Example |
| :--- | :--- | :--- | :--- |
| `/help` / `/commands` | `/help` | List every available chat command with gloss and usage. | `/help` |
| `/whoami` | `/whoami` | Show active orchestrator, workspace root, and field path. | `/whoami` |
| `/health` | `/health` | Show daemon PID, process state, repo root, and active worktree count. | `/health` |
| `/status` | `/status [@Quark]` | Show status (permission mode, session tokens, quota, branch state). | `/status @Quark` |
| `/stats` | `/stats` | Switch to telemetry and token spend statistics tab. | `/stats` |
| `/spend` | `/spend [@Quark] [window]` | Show spend per seat over window (`today`, `week`, `all`). | `/spend @Quark week` |
| `/sessions` | `/sessions` | List archived sessions with timestamps and message counts. | `/sessions` |
| `/rename` | `/rename <name>` | Name or rename the active session. | `/rename router-refactor` |
| `/resume` | `/resume [id]` | Reopen an archived session as the live active session. | `/resume router-refactor` |
| `/fork-field` | `/fork-field [id]` | Fork a new session starting from a historical event ID. | `/fork-field 01J8...` |
| `/replay` | `/replay [N]` | Step backwards through historical field events. | `/replay 20` |
| `/export` | `/export [id]` | Export the current session transcript as Markdown. | `/export` |
| `/search` | `/search <query>` | Search this session's message stream for matching text. | `/search merge gate` |
| `/clear` | `/clear` | Archive and clear the current chat history. | `/clear` |
| `/clear-history` | `/clear-history` | Delete all archived sessions while preserving token usage ledger. | `/clear-history` |
| `/theme` | `/theme <preset>` | Switch color theme (`oled`, `tokyo`, `obsidian`, `midnight`). | `/theme tokyo` |
| `/toggle-roster` | `/toggle-roster` | Toggle visibility of the left Roster sidebar. | `/toggle-roster` |
| `/toggle-inspector` | `/toggle-inspector` | Toggle visibility of the right Inspector sidebar. | `/toggle-inspector` |
| `/exit` (alias `/quit`) | `/exit` | Exit the Hadron Chamber desktop application. | `/exit` |
