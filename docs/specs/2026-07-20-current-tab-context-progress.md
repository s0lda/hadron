# Design Spec: Current-Tab Only Context Progress Bar

## 1. Problem Statement
The stats views (individual Quark Info panel and team-wide stats view) render the context usage progress bar on any tab (Session, Week, Month, All-Time, Current) where context info is found. 
Because a session can persist for weeks across many Hadron client restarts, showing context usage based on old historical turns on time-aggregated tabs (like Week/Month) is misleading. 
Context usage is a stateful property of the live resident ACP process, so it only makes sense to display it on the **Current** tab (active run).

## 2. Design Details
- **Info Panel Stats Block**: 
  - Restrict the context progress bar and the numeric "Context" details row in `info_panel_overlay` to ONLY render when `self.stats_window == StatsWindow::Current`.
  - For the data itself, load the most recent context from the entire live session (via `self.view.messages`). This ensures that if the agent hasn't responded yet in the current turn but is running/excited, we still display its active context occupancy, rather than rendering nothing.
- **Team-wide Stats View**:
  - Restrict the quark list context progress bar and context details text block in `stats_view` to ONLY render when `self.stats_window == StatsWindow::Current`.
  - Like the info panel, load the most recent context from the entire live session.

## 3. Implementation Details
In [crates/hadron-chamber/src/app/render/stats.rs](file:///home/Jake/dev/hadron/.hadron/trees/cli-agy/crates/hadron-chamber/src/app/render/stats.rs):

### Quark Info Panel (`info_panel_overlay`)
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
        let frac = (ctx.used_percentage as f32 / 100.0).clamp(0.0, 1.0);
        stats_block = stats_block.child(div().mt_1().child(progress_meter(frac, q_color)));
    }
}
```

### Team-wide Stats (`stats_view`)
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
