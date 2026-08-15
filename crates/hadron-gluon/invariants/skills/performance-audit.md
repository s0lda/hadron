---
name: performance-audit
description: Profile CPU/memory hot paths, heap allocations, async lock contention, and UI rasterization lag
---

# Performance Audit

Profile resource bottlenecks, redundant allocations, async contention, and rendering loop performance.

## Core Principles
1. **Evidence, Not Adjectives (Rule 6):** Measure before and after with concrete numbers (bytes, milliseconds, frame times, CPU cycles). Never claim an optimization without benchmark evidence.
2. **Zero-Cost Abstractions:** Avoid repeated heap allocations (`.clone()`, `to_string()`, `Vec::clone()`) in hot loops and per-frame rendering passes.

## Investigation Checklist

### 1. UI & Event Loop Rendering Lag
- **Hover & Frame Handlers:** Verify `.hover()` and mouse-move listeners do not trigger heavy recalculations, re-parsing, or synchronous I/O.
- **List & View Caching:** Verify scroll views use cached layout state (`ListState`) rather than recalculating item bounds every frame.
- **Lavapipe / CPU Rasterization:** Ensure clipping bounds and bounding boxes minimize draw quad counts under software rendering.

### 2. Allocations & Data Flow in Hot Paths
- Replace cloned buffers with borrowed slices (`&str`, `&[T]`) or copy-on-write `Cow<str>`.
- Audit tight loops and token parsing streams for intermediate collection allocations (`.collect::<Vec<_>>()`).

### 3. Async Lock Contention & Channel Buffering
- Check for long-held `tokio::sync::Mutex` or `std::sync::RwLock` across async `.await` points.
- Ensure file system I/O in daemons runs on blocking thread pools (`tokio::task::spawn_blocking`) without stalling async reactor threads.

### 4. Output Contract
- **Hot Path Analysis:** Measured bottlenecks with `file:line` locations.
- **Profiling Evidence:** Baseline vs optimized latency / allocation metrics.
- **Optimization Diff:** Minimal, non-breaking performance patches.
