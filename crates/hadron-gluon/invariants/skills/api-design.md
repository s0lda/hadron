---
name: api-design
description: Design type-safe API contracts, wire protocols, backwards compatibility, and error schemas
---

# API Design & Contract Verification

Design clean, robust, and backwards-compatible public interfaces, serialization protocols, and error schemas.

## Core Principles
1. **Single Source of Truth (Rule 3):** Wire protocols, serialized schemas, and in-memory models must share a single contract definition.
2. **Make Invalid States Unrepresentable (Rule 8):** Design types such that illegal states fail to construct or compile.
3. **Backwards Compatibility:** Wire formats (NDJSON events, ACP requests, team.json configs) must degrade gracefully when reading older or newer schemas.

## Design Checklist

### 1. Ergonomics & Type Safety
- Design minimal, self-documenting method signatures.
- Prefer explicit newtypes over primitive types for IDs, paths, and domain tokens.
- Return structured `Result<T, E>` with domain-specific error enums; never swallow error details into bare strings.

### 2. Serialization & Wire Protocol
- Ensure JSON/NDJSON serialization round-trips cleanly across version upgrades.
- Annotate non-critical optional fields with `#[serde(default, skip_serializing_if = "...")]` to avoid unnecessary wire bloat.
- Provide explicit backwards-compatibility tests for legacy schema variants.

### 3. Documentation & Caller Verification (Rule 1)
- Document contract invariants, error conditions, and concurrency semantics.
- Verify real callers exist for every new public interface method before finalizing.
