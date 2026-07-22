//! Stateless UI helpers for the chamber: leaf element builders that take explicit
//! parameters and return GPUI elements (backdrop washes, titlebar/window-control
//! buttons, roster rows, settings fields, permission-mode + effort tags, log rows,
//! stat tiles, progress meters, and the markdown text style). None hold or borrow
//! `Chamber` — they render from their arguments alone, so they live apart from the
//! `impl Chamber` render code.

use super::*;

pub(super) fn format_num(n: u32) -> String {
    if n >= 10_000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        let s = n.to_string();
        if s.len() > 3 {
            let (head, tail) = s.split_at(s.len() - 3);
            format!("{},{}", head, tail)
        } else {
            s
        }
    }
}

/// Corner radii for the full-height content container, matching the client
/// frame ([`crate::window_frame`]). Rounds at the frame's own radius so the
/// content tucks *inside* the 1px border (a hair rounder never pokes past the
/// arc; the frame's matching sidebar fill hides the sub-pixel sliver). Zero on
/// a tiled edge (maximized/snapped) so those corners stay square.
/// One wash of the ambient quark-state field: a full-bleed linear gradient from `hue` at
/// the origin edge, fading to transparent by ~70%. Several of these, layered at different
/// angles, build the "bubble chamber" backdrop. Static (no animation) and gradient-only
/// (no blur), so it costs only per-repaint — see `theme` for why that is affordable now.
///
/// The corners are rounded to the housing radius: GPUI's `overflow_hidden` masks to the
/// rectangular bounds, not the rounded shape, so a full-bleed child would otherwise paint
/// square corners that poke past the window's rounded edge.
pub(super) fn glow_layer(angle: f32, hue: Rgba, top_r: Pixels, bottom_r: Pixels) -> gpui::Div {
    div()
        .absolute()
        .inset_0()
        .rounded_tl(top_r)
        .rounded_tr(top_r)
        .rounded_bl(bottom_r)
        .rounded_br(bottom_r)
        .bg(linear_gradient(
            angle,
            linear_color_stop(hue, 0.0),
            linear_color_stop(rgba(0x00000000), 0.55),
        ))
}

/// The opaque base wash of the field: a full-bleed two-colour gradient from `from` at the
/// origin edge to `to` at the far edge. Rounded to the housing radius for the same reason
/// as [`glow_layer`] (GPUI's overflow mask is rectangular, so a square child would poke
/// past the rounded corners).
pub(super) fn wash_layer(angle: f32, from: Rgba, to: Rgba, top_r: Pixels, bottom_r: Pixels) -> gpui::Div {
    div()
        .absolute()
        .inset_0()
        .rounded_tl(top_r)
        .rounded_tr(top_r)
        .rounded_bl(bottom_r)
        .rounded_br(bottom_r)
        .bg(linear_gradient(
            angle,
            linear_color_stop(from, 0.0),
            linear_color_stop(to, 1.0),
        ))
}

pub(super) fn frame_corner_radii(window: &Window) -> (Pixels, Pixels) {
    let r = crate::window_frame::FRAME_RADIUS;
    match window.window_decorations() {
        Decorations::Client { tiling } => (
            if tiling.top { px(0.0) } else { r },
            if tiling.bottom { px(0.0) } else { r },
        ),
        Decorations::Server => (px(0.0), px(0.0)),
    }
}

/// The titlebar's app/options menu — a 3-line "hamburger" with circular hover.
/// Placeholder for now: opening a menu of options lands later. Stops propagation
/// so a press here can't start a window move.
/// The app menu behind the 3-line icon.
///
/// Every item here does something real. "Open Folder / Recent Projects" is
/// deliberately absent: the daemon is bound to one workspace at boot, so the chamber
/// alone cannot repoint the swarm at another one — an item that opened a folder the
/// quarks could not see would be a lie with a file dialog attached.
pub(super) fn menu_button(chamber: &Entity<Chamber>) -> impl IntoElement {
    let view = chamber.clone();
    Button::new("app-menu")
        .ghost()
        .icon(Icon::new(IconName::Menu).small())
        .dropdown_menu(move |menu, _, _| {
            let settings = view.clone();
            let folder = view.clone();
            let about = view.clone();
            menu.item(
                PopupMenuItem::new("Settings…").on_click(move |_, window, cx| {
                    settings.update(cx, |this, cx| this.open_settings(window, cx));
                }),
            )
            .separator()
            .item(
                PopupMenuItem::new("Reveal Workspace in File Manager").on_click(
                    move |_, _, cx| {
                        folder.update(cx, |this, cx| {
                            this.handle_context_menu_action(
                                ContextMenuAction::OpenInFolder(String::from(".")),
                                cx,
                            );
                        });
                    },
                ),
            )
            .separator()
            .item(
                PopupMenuItem::new("About Hadron").on_click(move |_, _, cx| {
                    about.update(cx, |this, cx| {
                        this.about_open = true;
                        cx.notify();
                    });
                }),
            )
            .separator()
            .item(PopupMenuItem::new("Quit Hadron").on_click(|_, _, cx| cx.quit()))
        })
}

/// A circular window-control button (min / max / close) with circular hover.
pub(super) fn control_button(id: &'static str, icon: IconName, is_close: bool) -> impl IntoElement {
    let hover_bg = if is_close {
        theme::danger()
    } else {
        theme::bg_surface_raised()
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(26.0))
        .rounded_full()
        .text_color(theme::text_secondary())
        .hover(|s| s.bg(hover_bg).text_color(theme::text()))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            match id {
                "min" => window.minimize_window(),
                "max" => window.zoom_window(),
                _ => window.remove_window(),
            }
        })
        .child(Icon::new(icon).small())
}

/// A draggable titlebar region: marks the area for the compositor and starts a
/// window move on press.
pub(super) fn drag_region(id: &'static str) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .h_full()
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
}

pub(super) fn effective_presence_state(
    r_state: QuarkState,
    adopted: bool,
    enabled: bool,
    has_activity: bool,
) -> QuarkState {
    if has_activity && adopted && enabled {
        QuarkState::Excited
    } else {
        r_state
    }
}

/// The dot and the words must agree: when the dot renders blue (a turn is in
/// flight) but the adapter has published no live detail — a CLI quark, or the
/// gap between publishes — the subtitle shows a "working…" placeholder rather
/// than the seat's static vendor · model caption.
pub(super) fn needs_activity_placeholder(
    effective: QuarkState,
    adopted: bool,
    enabled: bool,
    has_activity: bool,
) -> bool {
    !has_activity
        && adopted
        && enabled
        && matches!(effective, QuarkState::Excited | QuarkState::Thinking)
}

/// Every mid-turn quark, not just the first: an adopted, enabled seat with fresh
/// live detail (an ACP seat) gets its raw stream text — the thought, tool title, or
/// plan step, with no "working"/"thinking" label glued in front, since that text
/// already says what is happening. A seat whose field state says a turn is in
/// flight but which has published no detail (a CLI seat — it does not stream to
/// us — or the gap between two ACP publishes) gets the `"working…"` placeholder
/// instead, since there is no stream to show. `live` is injected so this stays a
/// pure function testable without touching disk.
pub(super) fn active_quarks(
    roster: &[RosterRow],
    live: impl Fn(&str) -> Option<hadron_lattice::live::Activity>,
) -> Vec<(String, String)> {
    roster
        .iter()
        .filter(|r| r.adopted && r.enabled)
        .filter_map(|r| {
            if let Some(act) = live(&r.id) {
                let text = if act.detail.is_empty() {
                    act.doing.label().to_string()
                } else {
                    act.detail
                };
                Some((act.quark.as_str().to_string(), text))
            } else if matches!(r.state, QuarkState::Excited | QuarkState::Thinking) {
                Some((r.id.clone(), "working…".to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// One roster entry, styled as a presence list-item: the resolved avatar with a
/// status [`Badge`] dot, a display name, and a one-word presence subtitle, with a
/// tooltip on hover.
pub(super) fn roster_row(
    id: &ResolvedIdentity,
    r: &RosterRow,
    activity: Option<hadron_lattice::live::Activity>,
    controls: gpui::AnyElement,
) -> impl IntoElement {
    let name = id.name.clone();
    let effective_state = effective_presence_state(r.state, r.adopted, r.enabled, activity.is_some());
    // Not adopted here → "available" (in the catalogue, off in this repo); adopted but
    // switched off → "disabled"; otherwise the live presence word.
    let label = if !r.adopted {
        "available"
    } else if r.enabled {
        theme::presence_label(effective_state)
    } else {
        "disabled"
    };
    let tip: SharedString = format!("{name} — {label}").into();

    // Legibility line: "transport · vendor · model" when the seat is in team.json, else
    // the presence label alone.
    let cap = |s: &str| {
        let mut c = s.chars();
        match c.next() {
            None => String::new(),
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        }
    };
    let transport_label = r.transport.code();

    let tokens = r.tokens;
    let tokens_str = if tokens >= 1_000_000 {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{}k", tokens / 1_000)
    } else {
        format!("{}", tokens)
    };

    let flavor_str = match &r.flavor {
        Some(hadron_lattice::Flavor::Orchestrator) => "Orchestrator",
        Some(hadron_lattice::Flavor::Worker) => "Worker",
        None => "",
    };

    let has_activity = activity.is_some();
    let detail_1: SharedString = if let Some(act) = activity {
        format!("{}: {}", act.doing.label(), act.detail).into()
    } else if needs_activity_placeholder(effective_state, r.adopted, r.enabled, has_activity) {
        "working…".into()
    } else if r.vendor.is_empty() && r.model.is_empty() {
        label.into()
    } else if r.model.is_empty() {
        format!("{} · {}", transport_label, cap(&r.vendor)).into()
    } else {
        format!("{} · {} · {}", transport_label, cap(&r.vendor), cap(&r.model)).into()
    };

    let unknown_str = if r.unknown_turns > 0 {
        format!(" (+{} turns of unknown spend)", r.unknown_turns)
    } else {
        "".to_string()
    };

    let detail_2: gpui::AnyElement = if flavor_str.is_empty() {
        div().font_family("Cascadia Code").child(format!("{} tokens{}", tokens_str, unknown_str)).into_any_element()
    } else {
        h_flex().gap_1()
            .child(flavor_str)
            .child("·")
            .child(div().font_family("Cascadia Code").child(format!("{} tokens{}", tokens_str, unknown_str)))
            .into_any_element()
    };

    // Grey dot for both a disabled seat and an available-but-not-adopted quark — the
    // same "there to use, but off" signal the user asked for.
    let dot_color = if r.adopted && r.enabled {
        theme::presence(effective_state)
    } else {
        theme::presence_disabled()
    };

    // A single static presence dot: the colour alone carries the state (blue = working,
    // green = available, amber = waiting, red = unavailable), so every quark's dot reads
    // the same way. No per-state ring and no animation — the chamber software-renders
    // (WSL/llvmpipe, no GPU), and any live animation forces a full-window repaint every
    // frame, historically the app's worst CPU sink.
    let dot = div()
        .absolute()
        .bottom_0()
        .right_0()
        .size(px(10.0))
        .rounded_full()
        .bg(dot_color)
        .border_2()
        .border_color(theme::bg_elevated());

    h_flex()
        .id(SharedString::from(format!("quark-{}", r.id)))
        .items_start()
        .gap_2p5()
        .px_2()
        .py_1p5()
        .rounded_md()
        .hover(|s| s.bg(theme::bg_surface()))
        .child(
            div()
                .relative()
                .mt_1()
                .child(
                    div()
                        .child(identity_avatar(id, 28.0))
                        .map(|el| if r.enabled { el } else { el.opacity(0.6) })
                )
                .child(dot),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .text_color(if r.enabled { theme::text() } else { theme::text_muted() })
                        .truncate()
                        .child(name),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text())
                        .opacity(if r.enabled { 0.8 } else { 0.5 })
                        .truncate()
                        .child(detail_1),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text())
                        .opacity(if r.enabled { 0.7 } else { 0.4 })
                        .truncate()
                        .child(detail_2),
                ),
        )
        // Effort + permission-mode tags, trailing (top-right) as before. Kept `xsmall`
        // so they stay clear of the name/model column instead of squishing it.
        .child(controls)
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
}

/// A labeled row in the Settings card: `Name | control`, with an optional muted
/// caption below explaining what it does. The name sits in a fixed-width column
/// so every row's control lines up, and reads at full text contrast — the old
/// `text_xs` + `text_muted` label was the "very hard to read" complaint.
pub(super) fn settings_field(
    label: &'static str,
    description: Option<&'static str>,
    content: gpui::AnyElement,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_1()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap_4()
                .child(
                    div()
                        .flex_none()
                        .w(px(140.0))
                        .text_sm()
                        .text_color(theme::text())
                        .child(label),
                )
                .child(div().flex_1().min_w_0().child(content)),
        )
        .when_some(description, |v, desc| {
            v.child(div().text_xs().text_color(theme::text_secondary()).child(desc))
        })
}

/// A vertically-stacked labeled field: the label above a full-width control, no
/// fixed-width column. For rows whose label already carries the explanation
/// (the Custom CLI wizard's inline hints) and needs the full card width rather
/// than sharing a line with a short name.
pub(super) fn settings_field_stacked(
    label: impl Into<gpui::SharedString>,
    content: gpui::AnyElement,
) -> impl IntoElement {
    v_flex()
        .gap_1p5()
        .child(div().text_sm().text_color(theme::text()).child(label.into()))
        .child(content)
}

/// A section eyebrow — the small, muted, all-caps label that heads a group of rows in
/// the info/about panels, matching the Settings sidebar's "IDENTITIES"/"SETTINGS" style.
pub(super) fn panel_eyebrow(label: &'static str) -> impl IntoElement {
    div()
        .mt_1()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(label)
}

/// One label→value row: muted label on the left, value right-aligned. The shared row
/// shape for the info and about panels, so both read as the same instrument panel.
pub(super) fn kv_row(label: &'static str, value: impl Into<String>) -> impl IntoElement {
    h_flex()
        .w_full()
        .justify_between()
        .gap_4()
        .text_sm()
        .child(div().flex_none().text_color(theme::text_muted()).child(label))
        .child(
            div()
                .flex_1()
                .text_right()
                .text_color(theme::text())
                .child(value.into()),
        )
}

/// A small, subtle text button for secondary actions (caller attaches on_click).
pub(super) fn text_button(
    id: impl Into<gpui::SharedString>,
    label: impl Into<gpui::SharedString>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px_2()
        .py_1()
        .rounded_md()
        .text_sm()
        .text_color(theme::text_secondary())
        .hover(|s| s.bg(theme::bg_surface_raised()).text_color(theme::text()))
        .child(label.into())
}

/// A best-effort file picker for environments where gpui's native (XDG desktop portal)
/// dialog is unavailable — most notably WSL, which usually has no `xdg-desktop-portal`
/// running, so `prompt_for_paths` resolves to nothing. Tries a GTK dialog (`zenity`,
/// which works under WSLg), then, under WSL, a native Windows dialog driven through
/// PowerShell and translated back to a Linux path with `wslpath`.
///
/// Returns the chosen path, or `None` if the user cancelled or no picker exists.
/// **Blocking** (waits on the dialog) — must be called on a background thread.
pub(super) fn fallback_pick_image() -> Option<String> {
    use std::process::Command;

    // 1) zenity — a GTK file chooser. If it launches at all, its answer is authoritative
    //    (a path, or `None` on cancel); only a *missing* binary (`Err`) falls through, so
    //    a cancel never pops a second dialog.
    match Command::new("zenity")
        .args([
            "--file-selection",
            "--title=Choose avatar image",
            "--file-filter=Images | *.png *.jpg *.jpeg *.gif *.webp *.bmp *.svg",
            "--file-filter=All files | *",
        ])
        .output()
    {
        Ok(out) => {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return (out.status.success() && !path.is_empty()).then_some(path);
        }
        Err(_) => { /* zenity not installed — try the Windows dialog below */ }
    }

    // 2) WSL: drive a native Windows OpenFileDialog through PowerShell, then translate the
    //    returned `C:\…` path to `/mnt/c/…` with `wslpath`.
    let script = "Add-Type -AssemblyName System.Windows.Forms; \
         $d = New-Object System.Windows.Forms.OpenFileDialog; \
         $d.Filter = 'Images|*.png;*.jpg;*.jpeg;*.gif;*.webp;*.bmp|All files|*.*'; \
         if ($d.ShowDialog() -eq 'OK') { $d.FileName }";
    let win = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())?;
    // C:\… → /mnt/c/… ; if the translation fails, hand back the raw path rather than lose it.
    let translated = Command::new("wslpath")
        .arg("-u")
        .arg(&win)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    translated.or(Some(win))
}

/// The next mode in the ladder, cycling Ask → Write → Auto → Bypass → Ask.
pub(super) fn next_mode(mode: Mode) -> Mode {
    match mode {
        Mode::Ask => Mode::Write,
        Mode::Write => Mode::Auto,
        Mode::Auto => Mode::Bypass,
        Mode::Bypass => Mode::Ask,
    }
}

/// The text on a permission-mode badge. A quark with no per-quark override reads
/// **"Default"** (it rides the global/inherited mode); otherwise the actual mode
/// name. Pure so it's unit-testable without a GPUI window. The GLOBAL mode chip
/// passes `is_default = false` so it always shows the live ASK/WRITE/AUTO/BYPASS.
pub(super) fn mode_tag_label(mode: Mode, is_default: bool) -> &'static str {
    if is_default {
        "Default"
    } else {
        mode_label(mode)
    }
}

/// A permission-mode badge. When `is_default` (no per-quark override), a neutral
/// GREY outlined "Default" chip — the quark is on the global mode. Otherwise the
/// actual mode in its risk colour (Ask muted → Bypass danger). All variants are
/// `outline` (Jake's request) so the tags read as light chips, not solid fills.
/// Kept `xsmall` so it doesn't crowd the name/model column of the roster row.
pub(super) fn mode_tag(mode: Mode, is_default: bool) -> gpui::AnyElement {
    let label = mode_tag_label(mode, is_default);
    if is_default {
        // Neutral grey — a "Default" chip must not borrow the risk colour of
        // whatever the global mode happens to be right now.
        return Tag::secondary()
            .xsmall()
            .outline()
            .child(div().text_xs().child(label))
            .into_any_element();
    }
    let tag = match mode {
        Mode::Ask => Tag::secondary(),
        Mode::Write => Tag::info(),
        Mode::Auto => Tag::warning(),
        Mode::Bypass => Tag::danger(),
    };
    tag.xsmall()
        .outline()
        .child(div().text_xs().child(label))
        .into_any_element()
}

/// The reasoning-effort badge, mirroring [`mode_tag`]'s default behaviour: a set
/// per-quark effort (`Some("high")`) renders as an outlined `HIGH` chip; an absent
/// effort renders a neutral grey outlined `Default` chip (the seat is on the
/// model/provider default). Effort has no risk-colour ladder like mode, so both
/// states are outlined `secondary` — only the label differs. Kept `xsmall` so it
/// doesn't crowd the roster row.
pub(super) fn effort_tag(effort: &Option<String>) -> gpui::AnyElement {
    let label = match effort.as_deref() {
        Some(e) if !e.is_empty() => e.to_uppercase(),
        _ => "Default".to_string(),
    };
    Tag::secondary()
        .xsmall()
        .outline()
        .child(div().text_xs().child(label))
        .into_any_element()
}

/// The short badge label for a permission mode, e.g. `Mode::Bypass` → `"BYPASS"`.
/// One source of truth for the ladder's labels, shared by the roster tag and the
/// Settings picker.
pub(super) fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Ask => "ASK",
        Mode::Write => "WRITE",
        Mode::Auto => "AUTO",
        Mode::Bypass => "BYPASS",
    }
}

/// The selected-chip colour for a permission mode — a risk temperature from muted
/// (Ask) through blue/amber to danger red (Bypass), matching the roster tag variants.
pub(super) fn mode_color(mode: Mode) -> gpui::Hsla {
    match mode {
        Mode::Ask => gpui::rgb(0x6b7280).into(),    // gray — pure conversation
        Mode::Write => gpui::rgb(0x3b82f6).into(),   // blue — edits flow
        Mode::Auto => gpui::rgb(0xf59e0b).into(),    // amber — commands remembered
        Mode::Bypass => gpui::rgb(0xef4444).into(),  // red — nothing asks
    }
}

/// A one-line, human-readable gloss of what a permission mode delegates — shown under
/// the Settings picker so the choice is not just a label. Mirrors the [`Mode`] doc.
pub(super) fn mode_hint(mode: Mode) -> &'static str {
    match mode {
        Mode::Ask => "Every edit and command asks you first.",
        Mode::Write => "Edits auto-approve; every command asks you.",
        Mode::Auto => "Edits auto-approve; a command asks once, then is remembered.",
        Mode::Bypass => "The orchestrator owns it — nothing asks you (still audited).",
    }
}

/// A muted placeholder line shown when a tab view has nothing to render.
pub(super) fn empty_hint(text: &'static str) -> impl IntoElement {
    div().text_sm().text_color(theme::text_muted()).child(text)
}

/// A single row in the compact activity Log: time · actor · kind · body, tabular and dense
/// so the Log reads like a console rather than a second chat. Body truncates to one line —
/// the Chat tab is where a message is read in full.
pub(super) fn log_row(m: &MessageRow, expanded: bool, author_color: Hsla) -> impl IntoElement {
    let time = m
        .ts
        .with_timezone(&chrono::Local)
        .format("%H:%M:%S")
        .to_string();
    h_flex()
        .w_full()
        .items_start()
        .gap_3()
        .px_2()
        .py_1()
        .rounded_md()
        .hover(|s| s.bg(theme::glass_highlight()))
        .child(
            div()
                .flex_none()
                .w(px(58.0))
                .text_xs()
                .font_family("Cascadia Code")
                .text_color(theme::text_muted())
                .child(time),
        )
        .child(
            div()
                .flex_none()
                .w(px(92.0))
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(author_color)
                .truncate()
                .child(m.from.clone()),
        )
        .child(
            div()
                .flex_none()
                .w(px(80.0))
                .text_xs()
                .text_color(log_kind_color(m.kind_label))
                .child(m.kind_label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(theme::text_secondary())
                // Truncated to one line by default; an expanded row (click) wraps in full.
                .when(!expanded, |d| d.truncate())
                .child(m.body.clone()),
        )
}

/// A quiet accent per event kind, so the Log's kind column reads at a glance.
pub(super) fn log_kind_color(kind: &str) -> gpui::Rgba {
    match kind {
        "status" => theme::accent_secondary(),
        "edit" => rgb(0x22c55e),
        "command" => rgb(0xf59e0b),
        "snapshot" => theme::accent(),
        _ => theme::text_muted(),
    }
}

/// A glass card for the Session panels, matching the chat/roster panels.
pub(super) fn session_card() -> gpui::Div {
    v_flex()
        .p_3()
        .gap_2()
        .rounded(px(12.0))
        .bg(theme::glass_surface())
        .border_1()
        .border_color(theme::glass_highlight())
}

/// A slim horizontal progress meter: a recessed track with a `fill`-coloured bar at
/// `frac` (0..=1) of the width. Pure divs — no chart, no per-frame cost. Shared by the
/// chat stats cards and the info panel's context gauge so they read identically.
pub(super) fn progress_meter(frac: f32, fill: impl Into<gpui::Fill>) -> impl IntoElement {
    div()
        .w_full()
        .h(px(6.0))
        .rounded_full()
        .bg(theme::bg_base())
        .child(
            div()
                .h_full()
                .w(gpui::relative(frac.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(fill),
        )
}

/// A KPI tile: a big value over a small label, for the session totals row.
pub(super) fn stat_tile(label: &str, value: String, accent: gpui::Rgba) -> impl IntoElement {
    v_flex()
        .flex_1()
        .gap_1()
        .p_3()
        .rounded(px(10.0))
        .bg(theme::bg_base())
        .child(
            div()
                .text_lg()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(accent)
                .child(value),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme::text_muted())
                .child(label.to_string()),
        )
}

/// Map an event kind to a timeline step icon.
pub(super) fn kind_icon(kind_label: &str) -> IconName {
    match kind_label {
        "status" => IconName::Info,
        "edit" => IconName::Folder,
        "command" => IconName::SquareTerminal,
        "snapshot" => IconName::CircleCheck,
        _ => IconName::Asterisk,
    }
}

pub(super) fn markdown_style() -> gpui_component::text::TextViewStyle {
    let mut style = gpui_component::text::TextViewStyle::default();
    style.highlight_theme = gpui_component::highlighter::HighlightTheme::default_dark();
    style.table = {
        let mut s = gpui::StyleRefinement::default();
        s.overflow.x = Some(gpui::Overflow::Scroll);
        s
    };
    // Fenced code blocks: a solid dark card (header row with language label +
    // copy button, divider, then the code body) so they read as a distinct
    // block over the flat #101010 field instead of blending into body text.
    // Padding lives inside the header/body rows in the fork's `CodeBlock`
    // render, not here.
    style.code_block = gpui::StyleRefinement::default()
        .bg(theme::input_bg())
        .border_1()
        .border_color(theme::border())
        .rounded_md();
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `mode_tag` used to early-return an empty element for any quark running the
    /// global-default mode (`is_override == false`), so "a mode tag next to each
    /// Quark" wasn't actually true — only overridden quarks showed one. The render
    /// decision isn't directly introspectable on an `AnyElement`, so this asserts
    /// the pure style selector behind it: BOTH branches must select a real style
    /// (neither is "nothing"), and override vs. inherited must land on opposite
    /// styles per the function's own doc comment (override = solid, inherited =
    /// outline).
    #[test]
    fn mode_tag_label_is_default_only_when_inherited() {
        // No per-quark override → the grey "Default" chip.
        assert_eq!(mode_tag_label(Mode::Auto, true), "Default");
        assert_eq!(mode_tag_label(Mode::Bypass, true), "Default");
        // An override (and the global chip, which passes is_default=false) → the real mode.
        assert_eq!(mode_tag_label(Mode::Auto, false), "AUTO");
        assert_eq!(mode_tag_label(Mode::Ask, false), "ASK");
        assert_eq!(mode_tag_label(Mode::Bypass, false), "BYPASS");
    }

    #[test]
    fn presence_state_overrides_to_excited_when_active_live_activity() {
        // Base case: no activity -> keep state
        assert_eq!(
            effective_presence_state(QuarkState::Ground, true, true, false),
            QuarkState::Ground
        );
        // Active activity, adopted, and enabled -> override to Excited
        assert_eq!(
            effective_presence_state(QuarkState::Ground, true, true, true),
            QuarkState::Excited
        );
        // Active activity but not adopted -> keep base state
        assert_eq!(
            effective_presence_state(QuarkState::Ground, false, true, true),
            QuarkState::Ground
        );
        // Active activity but not enabled -> keep base state
        assert_eq!(
            effective_presence_state(QuarkState::Ground, true, false, true),
            QuarkState::Ground
        );
    }

    /// A blue dot must never sit next to a static vendor·model caption: an
    /// excited quark with no published live detail gets the placeholder, and
    /// every other combination does not.
    #[test]
    fn a_blue_dot_without_live_detail_gets_the_placeholder() {
        // Excited (blue dot), no live file — a CLI quark mid-turn → placeholder.
        assert!(needs_activity_placeholder(QuarkState::Excited, true, true, false));
        assert!(needs_activity_placeholder(QuarkState::Thinking, true, true, false));
        // Live detail present → the real activity line renders instead.
        assert!(!needs_activity_placeholder(QuarkState::Excited, true, true, true));
        // Idle (green dot) → the static caption is correct, no placeholder.
        assert!(!needs_activity_placeholder(QuarkState::Ground, true, true, false));
        // Not adopted / disabled seats never fake activity.
        assert!(!needs_activity_placeholder(QuarkState::Excited, false, true, false));
        assert!(!needs_activity_placeholder(QuarkState::Excited, true, false, false));
    }

    fn roster_row_fixture(id: &str, state: QuarkState, adopted: bool, enabled: bool) -> RosterRow {
        RosterRow {
            id: id.to_string(),
            display_name: None,
            state,
            mode: hadron_lattice::Mode::Ask,
            mode_is_override: false,
            vendor: "anthropic".to_string(),
            model: "claude".to_string(),
            flavor: None,
            transport: hadron_lattice::Transport::Acp,
            effort: None,
            enabled,
            adopted,
            tokens: 0,
            unknown_turns: 0,
        }
    }

    /// One row per mid-turn quark, not just the first: an ACP seat with fresh live
    /// detail, a CLI seat with no detail but an excited field state, an idle seat,
    /// and a not-adopted seat all coexist on one roster — only the first two should
    /// surface, each with its own label/detail.
    #[test]
    fn active_quarks_lists_every_quark_mid_turn_not_just_the_first() {
        let roster = vec![
            roster_row_fixture("acp-claude", QuarkState::Ground, true, true),
            roster_row_fixture("cli-agy", QuarkState::Excited, true, true),
            roster_row_fixture("acp-claude-2", QuarkState::Ground, true, true),
            roster_row_fixture("acp-codex", QuarkState::Excited, false, true),
        ];

        let active = active_quarks(&roster, |id| match id {
            "acp-claude" => Some(hadron_lattice::live::Activity::new(
                hadron_lattice::QuarkId::new("acp-claude"),
                hadron_lattice::live::Doing::Working,
                "Terminal",
            )),
            _ => None,
        });

        assert_eq!(
            active,
            vec![
                ("acp-claude".to_string(), "Terminal".to_string()),
                ("cli-agy".to_string(), "working…".to_string()),
            ]
        );
    }
}
