# 🔄 The Zero-CPU File Bus & Swarm Event Loop

Agents in Hadron communicate by appending structured NDJSON events to `field.jsonl`. File watchers waken waiting components with zero CPU polling overhead.

---

## The Swarm Event Loop Flow

```text
  Human / Chamber UI
         │  appends one event
         ▼
   field.jsonl ───── notify, zero CPU ─────▶  hadron-gluon (daemon)
   append-only                                      │  routes the turn
   event bus   ◀──── every reply, status            ▼
                     and token report        Orchestrator quark
                     is another event               │  @mentions the work out
                               ┌─────────────────────┴─────────────────────┐
                               ▼                                           ▼
                         Worker quark                                Worker quark
                    own worktree + branch                       own worktree + branch
                               └─────────────────────┬─────────────────────┘
                                                     ▼
                                     Merge Gate — rebase onto main, run the
                                     project's own tests, fast-forward or refuse
```

---

## Mechanics & Properties

1. **Append-Only Bus (`field.jsonl`)**:
   - All turns, user prompts, agent thoughts, status updates, token spending logs, and tool executions are serialized as NDJSON records.
   - The file is strictly append-only, ensuring a complete, immutable audit trail of all agent operations.

2. **Zero-CPU Polling Overhead**:
   - `hadron-gluon` utilizes OS-native kernel events via the `notify` crate (`inotify` on Linux, `kqueue` on macOS).
   - When no events are posted, processes sleep with 0% CPU consumption.

3. **Multi-Agent Orchestration**:
   - An **Orchestrator Quark** inspects user input and @mentions **Worker Quarks**.
   - Workers execute tasks concurrently in separate git worktrees, posting intermediate events back to `field.jsonl`.
   - Results pass through the **Merge Gate** for verification before merging to the main branch.
