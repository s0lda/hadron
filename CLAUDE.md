# Claude Code Project Guidelines (CLAUDE.md)

This project has strict rules. Read and obey them. If any rule contradicts the code, raise the contradiction instead of executing a confidently-executed fiction.

## Invariants & Standard Model

### 1. Prove it runs. Don't prove it compiles.
Before reporting that a mechanism works, find its caller. Prove the execution path works by finding the call site or running tests/verifications.

### 2. Reuse before you create.
Search the workspace for existing types, helpers, and functions before creating new ones.

### 3. One definition, one place (SSOT).
Keep exactly one source of truth for values, types, and rules.

### 4. Never remove a check/layer because it looks redundant.

### 5. Know your baseline.
Run `cargo test --workspace --features gui` before modifying code. Run the full gate at the end, not a filtered subset.

### 6. Evidence, not adjectives.
No claim of success without command line output showing it ran successfully. Paste the exact command and trimmed output.

### 7. Name the risk.
If touching auth, permissions, files, commands, network boundaries, or untrusted input, write a short **Security** note.

### 8. Make invalid states unrepresentable.

### 9. Learn and write it down.
Append lessons to `.hadron/memory/index.md` and detailed notes in `.hadron/memory/notes/`.

### 10. Simplicity first.
Touch only what you must. Match existing style. Remove unused imports/variables.

## Use Superpowers
You MUST use superpowers skills. Use `superpowers:subagent-driven-development` when available, or `superpowers:executing-plans` for plan implementation.

## Output Format
You MUST format your final response to `@orchestrator` (the orchestrator quark, who is `@agy`) exactly as follows:

**Done**: [Brief outcome summary, including commit hash]

- **Done**:
  - [Brief list of key completed tasks and files changed]
- **Evidence**: [Copy-paste the exact command and a concise/trimmed summary of the test/check output showing it works — keep it to the summary or last few lines]
- **Risks**: [Rule 7 security risk note or 'no new attack surface' with explanation]
- **What I did not verify / clean up**: [Explicitly specify what you did not check or clean up]

Do NOT add any preambles, greetings, or other pleasantries. Lead directly with the outcome.
