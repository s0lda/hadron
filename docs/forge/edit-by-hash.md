# 🛠️ Hadron Forge (AST Edit-by-Hash Precision)

Hadron Forge replaces fragile line diffs and full file rewrites with atomic, AST-level item block edits verified by cryptographic hashes.

---

## The Problem with Traditional Code Editing Tools

- **Line Diffs**: Prone to offset errors when files change concurrently or when agents generate imprecise line numbers.
- **Whole-File Rewrites**: Extremely token-heavy and risk clobbering concurrent edits made by human developers or parallel agent workers.

---

## How Edit-by-Hash Works

1. **AST Parsing**:
   - Hadron Forge parses source code into discrete AST item blocks (functions, structs, classes, impl blocks, modules, rules) using Tree-Sitter grammars.
2. **`blake3` Hashing**:
   - Every parsed block is assigned a `blake3` cryptographic hash derived from its content, signature, and structural hierarchy.
3. **Compare-and-Swap (CAS) Edits**:
   - When an agent modifies code, it references the specific block hash it intends to replace.
   - If another process modified that block in the interim, the hash will no longer match. Forge immediately rejects the edit with a stale hash error rather than silently overwriting code.

---

## Supported Languages & File Formats

Hadron Forge provides native Tree-Sitter AST item indexing for 14+ programming and markup languages, with structured line-chunk fallback for configuration and opaque text files:

| Language | Extensions | Parsed AST Item Blocks |
| :--- | :--- | :--- |
| **Rust** | `.rs` | Functions, Structs, Enums, Impl Blocks, Traits, Modules, Macros |
| **Python** | `.py` | Functions, Async Functions, Classes, Methods, Modules |
| **TypeScript / TSX** | `.ts`, `.tsx` | Functions, Arrow Functions, Classes, Interfaces, Type Aliases, Enums, Methods |
| **JavaScript / JSX** | `.js`, `.jsx`, `.mjs`, `.cjs` | Functions, Classes, Methods, Exports, Variables |
| **Go** | `.go` | Functions, Methods, Structs, Interfaces, Type Declarations |
| **C** | `.c`, `.h` | Functions, Structs, Unions, Enums, Typedefs |
| **C++** | `.cpp`, `.hpp`, `.cc`, `.cxx`, `.hh` | Functions, Classes, Structs, Namespaces, Templates, Methods |
| **Java** | `.java` | Classes, Interfaces, Enums, Records, Methods, Constructors |
| **C#** | `.cs` | Classes, Structs, Interfaces, Records, Enums, Methods, Properties |
| **Ruby** | `.rb` | Methods, Classes, Modules, Singleton Methods |
| **PHP** | `.php` | Functions, Classes, Interfaces, Traits, Enums, Methods |
| **HTML** | `.html`, `.htm` | Elements, Scripts, Styles, Document Blocks |
| **CSS / SCSS** | `.css`, `.scss` | Rule Sets, Media Queries, Keyframes, Font-Face At-Rules |
| **SQL** | `.sql` | Queries, Create Statements, Alter Statements, Procedures, Triggers |
| **Opaque / Text** | `.toml`, `.json`, `.yaml`, `.md`, `*` | Chunked line blocks for atomic, hash-verified configuration editing |

---

## Transport Protocols & MCP

- **ACP Seats (Agent Client Protocol)**:
  - Hadron attaches `hadron-forge-mcp` (a stdio MCP server) to every ACP agent session.
  - ACP agents call `edit_block`, `inspect_ast`, `grep_symbol`, and `batch_replace` tools directly via MCP.
- **CLI Seats**:
  - Seats using raw coding CLIs receive no MCP server.
  - The agent uses its own built-in editing commands, and Hadron observes changes via git worktrees. The prompt generator gates available tools based on transport type, ensuring CLI seats are never prompted with tools they cannot invoke.
