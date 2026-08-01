//! Stateless UI helpers for the chamber: leaf element builders that take explicit
//! parameters and return GPUI elements (backdrop washes, titlebar/window-control
//! buttons, roster rows, settings fields, permission-mode + effort tags, log rows,
//! stat tiles, progress meters, and the markdown text style). None hold or borrow
//! `Chamber` — they render from their arguments alone, so they live apart from the
//! `impl Chamber` render code.

use super::*;

/// Human-readable token count: `50.6k`, `1.2m`, `2m`. One decimal, with a trailing
/// `.0` trimmed so a round value reads `2m`, not `2.0m`. This is the single formatter
/// for every token figure in the UI (roster + stats) — do not re-inline the k/m logic.
pub(super) fn format_num(n: u64) -> String {
    if n >= 1_000_000_000 {
        trim_unit(n as f64 / 1_000_000_000.0, 'b')
    } else if n >= 1_000_000 {
        trim_unit(n as f64 / 1_000_000.0, 'm')
    } else if n >= 1_000 {
        trim_unit(n as f64 / 1_000.0, 'k')
    } else {
        n.to_string()
    }
}

fn trim_unit(v: f64, suffix: char) -> String {
    let s = format!("{v:.1}");
    let s = s.strip_suffix(".0").unwrap_or(&s);
    format!("{s}{suffix}")
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
#[allow(dead_code)] // retained styling helper, not currently wired
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
#[allow(dead_code)] // retained styling helper, not currently wired
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
/// Every item here does something real. "Open Workspace" opens the chosen folder in a
/// SECOND chamber rather than repointing this one: a daemon binds to one workspace at
/// boot, so an item that swapped the folder under a running swarm would be a lie with a
/// file dialog attached. See `Chamber::open_workspace`.
///
/// "New Session", "Rename" and every row under "Sessions" are wrappers, not new
/// behaviour: they drive the existing `/clear`, `/rename` and `/resume` rows of
/// [`crate::text::COMMANDS`], because that table is the one place a command may be
/// defined.
pub(super) fn menu_button(chamber: &Entity<Chamber>) -> impl IntoElement {
    let view = chamber.clone();
    Button::new("app-menu")
        .ghost()
        .icon(Icon::new(IconName::Menu).small())
        .dropdown_menu(move |menu, window, cx| {
            let open_workspace = view.clone();
            let folder = view.clone();
            let new_session = view.clone();
            let rename = view.clone();
            let settings = view.clone();
            let about = view.clone();
            let check_update = view.clone();
            let update_label = match &view.read(cx).update_state {
                crate::app::UpdateState::Available { version, .. } => format!("Update to v{}…", version),
                crate::app::UpdateState::Installing { version } => format!("Installing v{}…", version),
                crate::app::UpdateState::Installed { version } => format!("v{} installed — restarting…", version),
                crate::app::UpdateState::Checking => "Checking for Updates…".to_string(),
                _ => "Check for Updates…".to_string(),
            };
            menu.item(
                PopupMenuItem::new("Open Workspace").on_click(move |_, _, cx| {
                    open_workspace.update(cx, |this, cx| this.open_workspace(cx));
                }),
            )
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
                PopupMenuItem::new("New Session").on_click(move |_, window, cx| {
                    new_session.update(cx, |this, cx| {
                        this.handle_chat_command("clear", "", window, cx);
                    });
                }),
            )
            .item(
                // `/rename` needs a name, and the chamber has no single-line modal to
                // ask for one. Prefilling the chat input is strictly less code than a
                // new modal and leaves the human in the surface they already type in.
                PopupMenuItem::new("Rename").on_click(move |_, window, cx| {
                    rename.update(cx, |this, cx| {
                        this.prefill_chat_input("/rename ", window, cx);
                    });
                }),
            )
            // The archived sessions, newest first, each row driving `/resume`.
            // `Chamber::sessions` is a cache: listing them here would re-read every
            // archive's whole `field.jsonl` on the frame that paints the menu.
            .submenu("Sessions", window, cx, {
                let view = view.clone();
                move |menu, _, cx| {
                    let chamber = view.read(cx);
                    // `/resume` swaps the live field out from under a running daemon, so
                    // it refuses mid-turn — and it refuses to `eprintln!`, which nobody
                    // sees. Say so here instead of offering rows that do nothing.
                    if crate::model::any_quark_mid_turn(&chamber.view.roster) {
                        return menu.item(
                            PopupMenuItem::new("A quark is mid-turn — finish it first")
                                .disabled(true),
                        );
                    }
                    // `list_sessions` is oldest-first (it folds history in that order);
                    // a menu wants the most recent at the top.
                    let rows: Vec<(String, String)> = chamber
                        .sessions
                        .iter()
                        .rev()
                        .map(|s| (s.id.clone(), s.label()))
                        .collect();
                    if rows.is_empty() {
                        return menu
                            .item(PopupMenuItem::new("No archived sessions").disabled(true));
                    }
                    rows.into_iter().fold(menu, |menu, (id, label)| {
                        let resume = view.clone();
                        menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                            resume.update(cx, |this, cx| {
                                this.handle_chat_command("resume", &id, window, cx);
                            });
                        }))
                    })
                }
            })
            .separator()
            .item(
                PopupMenuItem::new("Settings…").on_click(move |_, window, cx| {
                    settings.update(cx, |this, cx| this.open_settings(window, cx));
                }),
            )
            .item(PopupMenuItem::new(update_label).on_click(move |_, window, cx| {
                check_update.update(cx, |this, cx| {
                    this.trigger_update_flow(window, cx);
                });
            }))
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

/// How much of a streaming reply the draft bubble shows, in **characters**.
///
/// The bubble is pinned above the input, so it cannot be allowed to grow without
/// bound and push the message box off the window. Characters, not bytes — a byte cut
/// lands mid-codepoint and panics the renderer (`Char Boundary Safety`).
pub(super) const DRAFT_TAIL_CHARS: usize = 1200;

/// Every quark whose reply is currently streaming in: `(quark id, text so far)`,
/// from the same live files [`active_quarks`] reads.
///
/// The draft rides `Activity::full`, which only [`hadron_lattice::live::Activity::speaking`]
/// sets — so a thought, a tool call and a plan step all correctly yield nothing here
/// and stay in the Live card, which is the whole point of the split: **messages to the
/// chat, tool use to the live view**.
///
/// Shows the **tail** when a reply outgrows [`DRAFT_TAIL_CHARS`]: text is arriving at
/// the end, so the end is what a human is reading. `live` is injected for the same
/// reason [`active_quarks`] injects it — this stays a pure function, testable without
/// touching disk.
pub(super) fn streaming_drafts(
    roster: &[RosterRow],
    live: impl Fn(&str) -> Option<hadron_lattice::live::Activity>,
) -> Vec<(String, String)> {
    roster
        .iter()
        .filter(|r| r.adopted && r.enabled)
        .filter_map(|r| {
            let act = live(&r.id)?;
            let full = act.full?;
            let trimmed = full.trim();
            if trimmed.is_empty() {
                return None;
            }
            Some((act.quark.as_str().to_string(), draft_tail(trimmed)))
        })
        .collect()
}

/// The last [`DRAFT_TAIL_CHARS`] characters of `text`, marked with a leading ellipsis
/// when anything was dropped. Counts characters, never bytes.
fn draft_tail(text: &str) -> String {
    let count = text.chars().count();
    if count <= DRAFT_TAIL_CHARS {
        return text.to_string();
    }
    let tail: String = text.chars().skip(count - DRAFT_TAIL_CHARS).collect();
    format!("…{tail}")
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

    let tokens_str = format_num(r.tokens as u64);

    let flavor_str = match &r.flavor {
        Some(hadron_lattice::Flavor::Orchestrator) => "Orchestrator",
        Some(hadron_lattice::Flavor::Worker) => "Worker",
        None => "",
    };

    let has_activity = activity.is_some();
    // A streaming reply publishes an EMPTY detail on purpose (the text belongs in the
    // chat bubble, not here), so the label stands alone rather than trailing a colon
    // over nothing: "speaking", not "speaking: ".
    let detail_1: SharedString = if let Some(act) = activity {
        if act.detail.is_empty() {
            act.doing.label().into()
        } else {
            format!("{}: {}", act.doing.label(), act.detail).into()
        }
    } else if needs_activity_placeholder(effective_state, r.adopted, r.enabled, has_activity) {
        "working…".into()
    } else if r.vendor.is_empty() && r.model_label().is_empty() {
        label.into()
    } else if r.model_label().is_empty() {
        format!("{} · {}", transport_label, cap(&r.vendor)).into()
    } else {
        format!("{} · {} · {}", transport_label, cap(&r.vendor), cap(&r.model_label())).into()
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
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .flex_none()
                        .text_sm()
                        .text_color(theme::text())
                        .child(label),
                )
                .child(div().flex_1().max_w(px(460.0)).child(content)),
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

/// What the native picker actually said.
///
/// `Option<PathBuf>` conflated the two answers that must behave differently: the human
/// said **no**, and there was **no dialog** for them to say it in. Both arrived as
/// `None`, so a cancel fell through to the subprocess fallback and popped a SECOND
/// dialog the human had to cancel again — reported live against "Open Workspace".
/// An enum makes the two unmistakable at every call site.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Picked {
    Path(String),
    /// The human cancelled. Stop — do NOT offer another dialog.
    Cancelled,
    /// No picker answered at all (no `xdg-desktop-portal`, the usual case under WSL).
    /// Only this one earns a fallback.
    NoPicker,
}

/// Classify what `cx.prompt_for_paths` handed back, after both its error layers have
/// been flattened to `Option`: the outer `None` is "the picker never answered", an
/// inner `None` (or an empty list) is "the human cancelled".
///
/// Pure, so the distinction that caused the double dialog is unit-testable without a
/// GPUI window.
pub(super) fn classify_pick(result: Option<Option<Vec<std::path::PathBuf>>>) -> Picked {
    match result {
        None => Picked::NoPicker,
        Some(None) => Picked::Cancelled,
        Some(Some(paths)) => match paths.into_iter().next() {
            Some(p) => Picked::Path(p.to_string_lossy().into_owned()),
            None => Picked::Cancelled,
        },
    }
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
    wsl_to_linux(win)
}

/// A directory picker with the same two fallbacks as [`fallback_pick_image`], for the
/// titlebar's "Open Workspace". Kept separate rather than parameterised because the two
/// dialogs differ in more than a flag — a Windows folder browser is a different class
/// (`FolderBrowserDialog`) than a file dialog, not the same one with another filter.
///
/// **Blocking** — must be called on a background thread.
pub(super) fn fallback_pick_directory() -> Option<String> {
    use std::process::Command;

    match Command::new("zenity")
        .args(["--file-selection", "--directory", "--title=Open workspace"])
        .output()
    {
        Ok(out) => {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return (out.status.success() && !path.is_empty()).then_some(path);
        }
        Err(_) => { /* zenity not installed — try the Windows dialog below */ }
    }

    let script = "Add-Type -AssemblyName System.Windows.Forms; \
         $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
         if ($d.ShowDialog() -eq 'OK') { $d.SelectedPath }";
    let win = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())?;
    wsl_to_linux(win)
}

/// `C:\…` → `/mnt/c/…`. If the translation fails, hand back the raw path rather than
/// lose it — a path we cannot translate is still better than silently nothing.
fn wsl_to_linux(win: String) -> Option<String> {
    let translated = std::process::Command::new("wslpath")
        .arg("-u")
        .arg(&win)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    translated.or(Some(win))
}

/// The next mode for a **click on a PER-QUARK chip**, cycling Ask → Write → Auto → Ask.
///
/// `Bypass` is deliberately not in this cycle: it is full unattended tool access for
/// one worker, and the chip is a word-sized target with no confirm and no undo behind
/// it. Escalating a worker into it is an explicit act — the Settings picker
/// (`set_quark_mode`) or `/mode bypass @quark`. Clicking a quark that is already in
/// `Bypass` still drops it to `Ask`, since de-escalation by accident costs nothing.
///
/// The GLOBAL chip uses [`next_global_mode`], which DOES reach `Bypass` — see there
/// for why the two ladders differ.
pub(super) fn next_mode(mode: Mode) -> Mode {
    match mode {
        Mode::Ask => Mode::Write,
        Mode::Write => Mode::Auto,
        Mode::Auto | Mode::Bypass => Mode::Ask,
    }
}

/// The next mode for a click on the **GLOBAL** mode chip (or `F6`), cycling the full
/// ladder Ask → Write → Auto → Bypass → Ask.
///
/// This is NOT [`next_mode`]. The per-quark clamp exists so a stray click cannot hand
/// one worker unattended access; the global chip is the human's own posture control and
/// `Bypass` is a posture they choose for the whole swarm — the mode Hadron is normally
/// driven in. Sharing one function made the per-quark clamp swallow the global ladder
/// too, so `Bypass` became unreachable from the chip and from `F6`, contradicting the
/// documented shortcut (`README.md`). Keep them separate.
pub(super) fn next_global_mode(mode: Mode) -> Mode {
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

/// The permission-mode ladder in ascending order of delegated authority, for a UI
/// that offers all four at once (the Settings "Default permission mode" picker)
/// rather than cycling. [`next_global_mode`] walks the same order as a cycle and
/// stays separate: one answers "what comes next", this one answers "what are they".
pub(super) const MODE_LADDER: [Mode; 4] = [Mode::Ask, Mode::Write, Mode::Auto, Mode::Bypass];

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

/// A swarm task's state badge — `Working` (info blue, still in flight) or `Done`
/// (success green). Mirrors [`mode_tag`]/[`effort_tag`]'s outlined-chip shape.
pub(super) fn task_state_tag(state: TaskState) -> gpui::AnyElement {
    let (tag, label) = match state {
        TaskState::Working => (Tag::info(), "Working"),
        TaskState::Done => (Tag::success(), "Done"),
    };
    tag.xsmall()
        .outline()
        .child(div().text_xs().child(label))
        .into_any_element()
}

/// One row in the Tasks tab: who it's addressed to, its title, and a state chip —
/// same dense layout as [`log_row`], swapping the kind column for the state tag.
pub(super) fn task_row(t: &SwarmTask) -> impl IntoElement {
    let time = t
        .asked_at
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
                .text_color(theme::text())
                .truncate()
                .child(t.to.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(theme::text_secondary())
                .truncate()
                .child(t.title.clone()),
        )
        .child(div().flex_none().child(task_state_tag(t.state)))
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
#[allow(dead_code)] // only used by the (unwired) timeline_view
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

    #[test]
    fn format_num_is_human_readable_with_trimmed_units() {
        assert_eq!(format_num(0), "0");
        assert_eq!(format_num(999), "999");
        assert_eq!(format_num(50_558), "50.6k");
        assert_eq!(format_num(1_000), "1k"); // trailing .0 trimmed
        assert_eq!(format_num(1_200_000), "1.2m");
        assert_eq!(format_num(2_000_000), "2m"); // not "2.0m"
        assert_eq!(format_num(503_937), "503.9k");
        assert_eq!(format_num(5_400_000_000u64), "5.4b");
    }

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

    /// One click on a small roster chip used to take a quark from `Auto` straight
    /// to `Bypass` — full unattended tool access, no confirm and no undo, from a
    /// stray click on a chip the size of a word. The cycle now stops at `Auto`;
    /// escalating INTO `Bypass` needs a deliberate path (the Settings picker's
    /// `set_quark_mode`, or `/mode bypass @quark`). De-escalating OUT of it by
    /// clicking stays, because that direction is always safe.
    #[test]
    fn the_click_cycle_never_escalates_into_bypass() {
        assert_eq!(next_mode(Mode::Ask), Mode::Write);
        assert_eq!(next_mode(Mode::Write), Mode::Auto);
        assert_eq!(next_mode(Mode::Auto), Mode::Ask, "must NOT reach Bypass");
        assert_eq!(next_mode(Mode::Bypass), Mode::Ask, "but clicking out still works");
    }

    /// The per-quark clamp above was shared with the GLOBAL chip, so `Bypass` fell out
    /// of the global ladder entirely: the chip and `F6` cycled Ask → Write → Auto → Ask
    /// and there was no way back into `Bypass` short of typing `/mode bypass`, while
    /// `README.md` documented `F6` as cycling all four. The global ladder must be
    /// complete and must close the loop.
    #[test]
    fn the_global_cycle_does_reach_bypass() {
        assert_eq!(next_global_mode(Mode::Ask), Mode::Write);
        assert_eq!(next_global_mode(Mode::Write), Mode::Auto);
        assert_eq!(next_global_mode(Mode::Auto), Mode::Bypass, "MUST reach Bypass");
        assert_eq!(next_global_mode(Mode::Bypass), Mode::Ask, "and wrap back round");
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

    /// **The double-dialog bug.** Cancelling gpui's picker used to be indistinguishable
    /// from having no picker at all, so Cancel immediately opened a second folder browser
    /// the human had to cancel again (reported live against "Open Workspace"). Only
    /// `NoPicker` may fall through to the subprocess fallback.
    #[test]
    fn a_cancelled_picker_is_not_a_missing_picker() {
        use std::path::PathBuf;

        assert_eq!(classify_pick(None), Picked::NoPicker, "no answer ⇒ try the fallback");
        assert_eq!(classify_pick(Some(None)), Picked::Cancelled, "the human said no");
        assert_eq!(
            classify_pick(Some(Some(vec![]))),
            Picked::Cancelled,
            "an empty selection is a cancel, not a missing picker"
        );
        assert_eq!(
            classify_pick(Some(Some(vec![PathBuf::from("/home/jake/dev")]))),
            Picked::Path("/home/jake/dev".to_string())
        );
    }
}

#[cfg(test)]
mod draft_tests {
    use super::*;
    use hadron_lattice::live::Activity;
    use hadron_lattice::{Doing, QuarkId};

    fn row(id: &str) -> RosterRow {
        RosterRow {
            id: id.to_string(),
            display_name: None,
            state: QuarkState::Excited,
            mode: hadron_lattice::Mode::Ask,
            mode_is_override: false,
            vendor: "anthropic".to_string(),
            model: "claude-opus-5".to_string(),
            flavor: Some(hadron_lattice::Flavor::Worker),
            transport: hadron_lattice::Transport::Acp,
            effort: None,
            enabled: true,
            adopted: true,
            tokens: 0,
            unknown_turns: 0,
        }
    }

    /// **The split Jake asked for.** A streaming reply becomes a chat draft; a thought,
    /// a tool call and a plan step stay in the Live card and produce no draft at all.
    /// `Activity::full` is what separates them, and only `Activity::speaking` sets it.
    #[test]
    fn only_a_speaking_activity_becomes_a_chat_draft() {
        let roster = vec![row("opus")];
        let speaking = |_: &str| Some(Activity::speaking(QuarkId::new("opus"), "Half a sentence"));
        assert_eq!(
            streaming_drafts(&roster, speaking),
            vec![("opus".to_string(), "Half a sentence".to_string())]
        );

        for doing in [Doing::Thinking, Doing::Working, Doing::Planning] {
            let tool = |_: &str| Some(Activity::new(QuarkId::new("opus"), doing, "Editing engine.rs"));
            assert!(
                streaming_drafts(&roster, tool).is_empty(),
                "{doing:?} belongs in the Live card, not the chat"
            );
        }
    }

    /// The Live card and the chat draft read the SAME file and must not both print the
    /// message: a speaking activity carries an empty `detail`, so the card falls back
    /// to the `speaking` label while the text goes to the bubble.
    #[test]
    fn the_live_card_shows_the_label_not_the_streaming_text() {
        let roster = vec![row("opus")];
        let speaking = |_: &str| Some(Activity::speaking(QuarkId::new("opus"), "Half a sentence"));
        assert_eq!(
            active_quarks(&roster, speaking),
            vec![("opus".to_string(), "speaking".to_string())]
        );
    }

    /// The bubble is pinned above the input, so an unbounded reply would push the
    /// message box off the window. Cut to the TAIL — text arrives at the end — and cut
    /// on a character boundary, never a byte one (`Char Boundary Safety`).
    #[test]
    fn a_long_draft_is_cut_to_its_tail_on_a_character_boundary() {
        let roster = vec![row("opus")];
        let body = "🚀".repeat(DRAFT_TAIL_CHARS * 2);
        let long = move |_: &str| Some(Activity::speaking(QuarkId::new("opus"), &body));
        let (_, text) = streaming_drafts(&roster, long).pop().expect("one draft");
        assert_eq!(text.chars().count(), DRAFT_TAIL_CHARS + 1, "the tail plus its ellipsis");
        assert!(text.starts_with('…'), "the cut is marked: {}", &text[..8]);
        assert!(text.ends_with('🚀'), "the END is what is kept");
    }

    /// An agent that has emitted only whitespace has said nothing yet — an empty
    /// bordered card flashing above the input is worse than no card.
    #[test]
    fn a_blank_draft_renders_nothing() {
        let roster = vec![row("opus")];
        let blank = |_: &str| Some(Activity::speaking(QuarkId::new("opus"), "  \n\n "));
        assert!(streaming_drafts(&roster, blank).is_empty());
    }
}
