---
name: security-review
description: Audit auth/permission boundaries, secret scanning, injection vectors, and untrusted inputs (Rule 7)
---

# Security Review

Execute a rigorous, adversarial security audit of proposed changes or existing modules.

## Core Principle (Standard Model Rule 7)
Any code touching authentication, permissions, file access, process execution, network boundaries, secrets, or untrusted input MUST have its risk named, bounded, and tested.

## Audit Phases

### 1. Attack Surface Mapping
- **Input Boundaries:** Identify every external input (CLI args, environment variables, NDJSON payloads, IPC channels, HTTP requests, file contents).
- **Privilege Boundaries:** Identify privilege shifts (process spawning, sudo, file system modifications, permission escalations, bypass modes).
- **Secret Paths:** Trace API keys, tokens, auth headers, and sensitive environment variables. Verify no secrets leak into logs, field events, git objects, or error messages.

### 2. Vulnerability Checklist
- **Command / Shell Injection:** Ensure commands never interpolate unescaped strings into shell executors (`sh -c`, `bash -c`). Use argument vectors (`std::process::Command::arg`) and sanitize inputs.
- **Path Traversal & Media Jailing:** Verify all file reads/writes are strictly anchored to authorized workspace roots or jailed capture paths (e.g. `<repo>/.hadron/screenshots/`). Prohibit un-sanitized `../` resolution.
- **Resource Exhaustion & Denial-of-Service:** Check unbounded loops, missing stream buffers, unbounded memory allocations on untrusted payloads, and missing process timeouts (`DEADLINE` constants).
- **Deserialization / Protocol Hijacking:** Ensure strict schema validation on NDJSON/JSON inputs. Discard or safely reject unknown/malformed payloads without panicking.

### 3. Verification & Proof of Exploitability
- Formulate concrete threat scenarios: "An attacker supplying X can cause Y."
- Where feasible, write a negative unit test reproducing the exploit vector before verifying the patch.

### 4. Output Contract
Produce a structured security audit report:
- **Scope & Entrypoints:** Analyzed files and trust boundaries.
- **Identified Risks (Ranked by Severity: Critical / High / Medium / Low):** Line-level `file:line` locations, exploit vector description, and impact.
- **Remediation Diffs:** Concrete code patches hardening the attack surface.
- **Security Verdict:** One of `Clean / Hardened (no new attack surface)` or `Blocked (unmitigated high/critical risk)`.
