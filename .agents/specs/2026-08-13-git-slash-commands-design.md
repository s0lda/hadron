# Design Spec: Git Slash Commands

## Goal
Add dedicated Git slash commands (`/git-status`, `/git-log`, `/push`, `/pr`) to the Hadron Chamber chat interface (`crates/hadron-chamber`) to allow developers and quarks to inspect and publish Git status and remote changes without dropping to a manual terminal.

## Requirements & Scope
- **SSOT Registration**: Define all new commands strictly in `hadron_chamber::text::COMMANDS` table (`crates/hadron-chamber/src/text.rs`).
- **Asynchronous Execution**: All remote Git operations must run asynchronously so network/credential delays never block GPUI rendering.
- **Safety & Confirmation**: Remote push actions (`/push`) require confirmation or explicit policy checks to prevent accidental pushes to public remotes.
- **No Conflict with Merge Gate**: Avoid destructive workspace mutation commands (`/pull`, `/commit`, `/rebase`) in chat; let Gluon's local Merge Gate handle branch rebase and test verification.

## Command Surface & Specification

| Command | Arity | Description & Output |
|---|---|---|
| `/git-status` | `Arity::None` | Displays current branch, untracked/modified count, ahead/behind `origin` status, and worktree info in `repo_root`. |
| `/git-log` | `Arity::Line` | Displays the last `N` commits (default 5) formatted as `[hash] message (author, time)`. Named `/git-log` to avoid confusion with internal log files. |
| `/push` | `Arity::Line` | Pushes current branch or `main` to `origin`. Prompts for confirmation when pushing for the first time or if `push_policy` is `confirm`. |
| `/pr` | `Arity::Line` | Triggers `gh pr create` for the active worktree branch when `MergeStrategy::GitHubPr` is enabled or manually requested. |

## Architecture & Data Flow

1. **Parser & Completion Table (`crates/hadron-chamber/src/text.rs`)**:
   - Add `/git-status`, `/git-log`, `/push`, `/pr` entries to `COMMANDS`.
   - Update `Arity` checks and completion tooltips.

2. **Handler Implementation (`crates/hadron-chamber/src/app/input.rs` & `app/actions.rs`)**:
   - Route parsed command tokens through `handle_chat_command`.
   - Execute git commands using `snapshot::git_with_env` (bounded by `GIT_DEADLINE` with closed stdin).
   - Post results back to the Chamber event log as `Actor::Gluon` messages (non-waking notices for output).

3. **Invariants**:
   - Every listed command in `COMMANDS` MUST be handled in `handle_chat_command` (checked by unit tests).
   - No raw `/pull` or `/commit` chat operations that bypass local gate tests.

## Security Considerations
- Bounded execution time via `snapshot::GIT_DEADLINE` (120s max).
- Process group teardown on timeout to prevent orphaned git processes.
- Confirmation gate before pushing commits to external git remotes.

## Testing Strategy
- Add unit test in `app::input::tests` verifying every new command in `COMMANDS` has a corresponding match arm in `handle_chat_command`.
- Test command string parsing for `/git-log` with optional numeric argument (e.g., `/git-log 10`).
