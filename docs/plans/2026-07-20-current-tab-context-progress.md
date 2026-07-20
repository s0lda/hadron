# Current Tab Context Progress Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gate the context used / limit progress bar to the "Current" (Now) tab in stats views, retrieving data from the latest session messages.

**Architecture:** Gating is applied in the Chamber GUI rendering logic in `stats.rs`. We restrict rendering the Context details and progress bar to only when `self.stats_window == StatsWindow::Current`, retrieving the latest active context dynamically from `self.view.messages`.

**Tech Stack:** Rust, GPUI

## Global Constraints
- Target only `crates/hadron-chamber/src/app/render/stats.rs`.
- Retrieve latest live context from `self.view.messages` by finding the last message from the target quark.

---

### Task 1: Gate Context Rendering in Stats Panel

**Files:**
- Modify: `crates/hadron-chamber/src/app/render/stats.rs`

**Interfaces:**
- Consumes: `self.stats_window` (StatsWindow enum), `self.view.messages` (Vec<MessageRow>)
- Produces: Gated UI elements for context rendering.

- [ ] **Step 1: Gate context rendering in `info_panel_overlay`**

Modify `crates/hadron-chamber/src/app/render/stats.rs:255-269`:
```rust
        if self.stats_window == StatsWindow::Current {
            let live_context = self.view.messages
                .iter()
                .rfind(|m| m.from == qid)
                .and_then(|m| m.usage.as_ref())
                .and_then(|u| u.context.as_ref());

            if let Some(ctx) = live_context {
                stats_block = stats_block.child(kv_row(
                    "Context",
                    format!(
                        "{:.1}% ({} / {})",
                        ctx.used_percentage,
                        format_num(ctx.used_tokens),
                        format_num(ctx.context_window_size)
                    ),
                ));
                // Context occupancy is a proportion, not a series — a progress bar reads it
                // better than a two-bar chart. Fill in the quark's colour.
                let frac = (ctx.used_percentage as f32 / 100.0).clamp(0.0, 1.0);
                stats_block = stats_block.child(div().mt_1().child(progress_meter(frac, q_color)));
            }
        }
```

- [ ] **Step 2: Gate context rendering in `stats_view`**

Modify `crates/hadron-chamber/src/app/render/stats.rs:580-592`:
```rust
            if self.stats_window == StatsWindow::Current {
                let live_context = self.view.messages
                    .iter()
                    .rfind(|m| m.from == *q)
                    .and_then(|m| m.usage.as_ref())
                    .and_then(|u| u.context.as_ref());

                if let Some(ctx) = live_context {
                    let frac = (ctx.used_percentage as f32 / 100.0).clamp(0.0, 1.0);
                    block = block
                        .child(
                            div().text_xs().text_color(theme::text_muted()).child(format!(
                                "Context {:.0}% · {} / {}",
                                ctx.used_percentage,
                                format_num(ctx.used_tokens),
                                format_num(ctx.context_window_size),
                            )),
                        )
                        .child(progress_meter(frac, q_color));
                }
            }
```

- [ ] **Step 3: Run compiler checks to verify correctness**

Run: `cargo check -p hadron-chamber --features gui`
Expected: Compile succeeds with no warnings/errors on the modified file.

- [ ] **Step 4: Run unit tests to verify no regression**

Run: `cargo test -p hadron-chamber --features gui`
Expected: Tests pass.

- [ ] **Step 5: Commit changes**

Run:
```bash
git add crates/hadron-chamber/src/app/render/stats.rs
git commit -m "feat(stats): show context progress bar only on current tab"
```
