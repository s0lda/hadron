---
name: incident-investigation
description: Systematic failure reproduction, log/NDJSON dissection, bisection, and post-mortem generation
---

# Incident Investigation & Triage

Perform rigorous, evidence-based triage and root-cause analysis on production incidents, daemon wedges, or swarm regressions.

## Core Principles
1. **Measure the Process, Don't Infer It:** Inspect ground truth (`/proc/<pid>`, NDJSON event logs, lockfiles) before forming hypotheses.
2. **Evidence, Not Adjectives (Rule 6):** Every finding must be backed by concrete log timestamps, error codes, and reproducible triggers.

## Triage Workflow

### 1. Forensic Evidence Gathering
- **Event Log Dissection:** Extract historical NDJSON field events leading up to the failure.
- **Process State Inspection:** Check daemon PID, child process trees, process groups, and lockfile holders (`gluon.lock`).
- **Git History Bisection:** Identify the exact commit introducing the regression (`git bisect`).

### 2. Root Cause Classification
- Distinguish between environment quirks (Lavapipe rasterization, tmpfs space, missing fonts) and genuine code-level regressions.
- Trace the chain of causality: Symptom -> Trigger -> Intermediate State -> Root Flaw.

### 3. Post-Mortem & Memory Distillation (Rule 9)
- Distill non-obvious failure modes into a compact nucleus note under `.hadron/nucleus/notes/<slug>.md`.
- Add a pointer line to `.hadron/nucleus/index.md` so the swarm never repeats the mistake.
