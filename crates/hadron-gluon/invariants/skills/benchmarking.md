---
name: benchmarking
description: Measure and prove empirical performance, latency, and throughput deltas against a verified baseline
---

# Empirical Performance Benchmarking

Measure, profile, and prove execution speed, latency distributions, and memory allocations against empirical baselines before claiming performance improvements.

## Core Principle (Standard Model Rules 5 & 6)
No optimization claim without baseline numbers and measurement evidence. "Faster" is not a finding; `-42% p99 latency (12.4ms → 7.2ms, n=1000)` is a fact.

## Mandatory Benchmarking Workflow

```text
BASELINE (Measure before touching code) →
ISOLATE (Control external variables and CPU throttling) →
OPTIMIZE (Apply targeted hot-path improvement) →
RE-MEASURE (Run identical benchmark harness) →
COMPARE (Report delta with statistical confidence)
```

### 1. Establish Pre-Optimization Baseline (Rule 5)
- Identify the hot path or critical metric (throughput, latency, memory allocations, binary size).
- Run the benchmark harness before making any code modifications.
- Record mean, median, p95, p99, and sample standard deviation across at least 10 iterations.

### 2. Isolate Confounding Variables
- Run on release or optimized builds (`--release`).
- Ensure software rasterizer / CPU frequency scaling does not bias samples.
- Discard cold-cache warmup runs before collecting recorded samples.

### 3. Apply Targeted Optimization (Rule 10)
- Keep optimizations minimal and localized to the profiled bottleneck.
- Avoid speculative complexity; prefer eliminating allocations, reducing lock contention, or caching invariant computations.

### 4. Verification & Delta Reporting (Rule 6)
- Re-run the identical measurement command under the same environmental conditions.
- Structure the report strictly with before, after, and delta:
  ```text
  Benchmark: [Harness Name]
  - Baseline:  X ms/op (± Y%)
  - Optimized: A ms/op (± B%)
  - Delta:     -Z% improvement (p < 0.01)
  - Memory:    M allocs -> N allocs
  ```
- If the delta is within noise threshold (<3% variance), report "no statistically significant improvement" and discard or rethink the change.
