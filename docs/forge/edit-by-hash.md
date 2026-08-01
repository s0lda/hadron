# 🛠️ Hadron Forge (AST Edit-by-Hash Precision)

Hadron Forge replaces fragile line diffs and full file rewrites with atomic, AST-level item block edits verified by cryptographic hashes.

---

## The Problem with Traditional Code Editing Tools

- **Line Diffs**: Prone to offset errors when files change concurrently or when agents generate imprecise line numbers.
- **Whole-File Rewrites**: Extremely token-heavy and risk clobbering concurrent edits made by human developers or parallel agent workers.

---

## How Edit-by-Hash Works

1. **AST Parsing**:
   - Hadron Forge parses source code files (`Rust`, `Python`, `TypeScript`, `Go`) into discrete AST item blocks (functions, structs, classes, impl blocks, modules) using Tree-Sitter.
2. **`blake3` Hashing**:
   - Every parsed block is assigned a `blake3` cryptographic hash derived from its content and structure.
3. **Compare-and-Swap Edits**:
   - When an agent wants to modify code, it references the specific block hash it wants to change.
   - If another process modified that block in the interim, the hash will no longer match. Forge immediately rejects the edit with a stale hash error rather than silently overwriting code.

---

## Transport Protocols & MCP

- **ACP Seats (Agent Client Protocol)**:
  - Hadron attaches `hadron-forge-mcp` (a stdio MCP server) to every ACP agent session.
  - ACP agents call `edit_block`, `inspect_ast`, and `grep_symbol` tools directly via MCP.
- **CLI Seats**:
  - Seats using raw coding CLIs receive no MCP server.
  - The agent uses its own built-in editing commands, and Hadron observes changes via git worktrees. The prompt generator gates available tools based on transport type, ensuring CLI seats are never prompted with tools they cannot invoke.
