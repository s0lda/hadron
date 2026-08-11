use super::*;

/// One entry in the in-app Changelog overlay, newest first.
///
/// `date: None` is the *only* way to say "unreleased" — the version and the badge
/// used to be two independent arguments, and 0.1.4 shipped to users still wearing
/// the "Unreleased" badge because the release bumped one and not the other.
struct Release {
    version: &'static str,
    /// `None` renders the "Unreleased" badge; `Some` renders the release date.
    date: Option<&'static str>,
    added: &'static [&'static str],
    changed: &'static [&'static str],
    fixed: &'static [&'static str],
}

/// A hand-written summary of `docs/CHANGELOG.md` — shorter lines, same releases.
/// `tests::the_top_overlay_release_matches_the_changelog_and_the_shipped_version`
/// fails the gate when the newest entry here drifts from the changelog or from
/// the version this binary was built at.
const RELEASES: &[Release] = &[
    Release {
        version: "0.2.2",
        date: Some("2026-08-11"),
        added: &[
            "Dynamic model selection and live SDK model discovery for ACP and CLI Quarks",
            "Capability-gated advanced model parameters with collapsible Settings accordion",
            "Expanded replace-by-hash support for 11 AST languages and structural fallback chunking",
        ],
        changed: &[],
        fixed: &[
            "ACP bridge model resolution and automatic fallback for invalid model strings",
        ],
    },
    Release {
        version: "0.2.1",
        date: Some("2026-08-09"),
        added: &[],
        changed: &[],
        fixed: &[
            "Antigravity Python bridge auto-provisioning and Windows Python launcher (`py.exe`) detection",
        ],
    },
    Release {
        version: "0.2.0",
        date: Some("2026-08-08"),
        added: &[],
        changed: &[],
        fixed: &[
            "Windows PTY terminal initialization, interactive shell execution, and icon resource linking",
        ],
    },
    Release {
        version: "0.1.9",
        date: Some("2026-08-07"),
        added: &[],
        changed: &[],
        fixed: &[
            "Windows ConPTY terminal initialization and taskbar icon integration",
        ],
    },
    Release {
        version: "0.1.8",
        date: Some("2026-08-02"),
        added: &[
            "Worker prompt response format template enforcing @orchestrator tag",
        ],
        changed: &[],
        fixed: &[
            "Prompt cache prefix stability and output formatting for worker quarks",
        ],
    },
    Release {
        version: "0.1.7",
        date: Some("2026-08-02"),
        added: &[
            "Lexical nucleus lesson recall over notes/*.md using BM25 ranking based on task text",
        ],
        changed: &[],
        fixed: &[
            "Preserves partial text streaming output on graceful ACP session cancellation",
            "Preserves remote branch names like origin/main in git graph ref pills",
        ],
    },
    Release {
        version: "0.1.6",
        date: Some("2026-08-02"),
        added: &[
            "Inter and Cascadia Code ship inside the binary and are registered at startup",
            "HTTP quarks get a bounded, jailed, streaming forge tool loop instead of prose",
            "Per-seat temperature, top_p and max_tokens in Settings, persisted in team.json",
            "Delegation subtab: who asked whom to do what, in each quark's name and colour",
        ],
        changed: &[
            "Tasks tab restyled as glass cards matching the Delegation view",
            "The terminal cursor is a line beam, not a colour-inverted block",
        ],
        fixed: &[
            "An HTTP quark declares only the tools its permission mode permits",
            "A message arriving mid-turn adds to the task it interrupts, it does not replace it",
            "A dirty worktree is snapshotted before the next branch is cut, not refused",
            "Gluon's own notices quote bare paths and refs so they don't render as commands",
            "Ollama gets echoed tool arguments as an object, not a string",
            "An empty content field no longer swallows the whole reasoning stream",
            "An absolute path inside the worktree is a spelling, not a jail escape",
            "The mock quark is gone, and an empty team.json no longer resurrects its ghost",
        ],
    },
    Release {
        version: "0.1.5",
        date: Some("2026-08-02"),
        added: &[],
        changed: &[],
        fixed: &[
            "The in-app Changelog dates the shipped release instead of badging it Unreleased",
        ],
    },
    Release {
        version: "0.1.4",
        date: Some("2026-08-02"),
        added: &[
            "Ollama, LM Studio & Cloud OpenAI-compatible HTTP providers over Transport::Http",
            "Add-Quark wizard rows for HTTP providers with keyring API key support",
            "Searchable model picker with pinned Default row in wizard and Settings",
            "CLI seats stream their step/tool feed, so the Live card moves during a CLI turn",
            "@-file completion offers gitignored paths, with an icon and path on each row",
        ],
        changed: &[],
        fixed: &[
            "Enabled rustls-tls on reqwest so cloud endpoints connect over HTTPS",
            "Fixed LM Studio /v1 endpoint path handling and error response parsing",
            "Restored Make Orchestrator context menu action for Worker quarks",
            "The model dropdown and Settings modal paint on opaque surfaces, not glass",
            "Ollama and OpenRouter seats are wired to live_dir, so they show in the Live card",
            "Auto-scroll chat input on paste of large text blocks",
        ],
    },
    Release {
        version: "0.1.3",
        date: Some("2026-07-27"),
        added: &[
            "Unified Swarm Command Deck UI redesign with floating capsule tab bars",
            "/Command picker chip selection alongside @Quark and @File mentions",
            "Task time scrubbing and live merge-gate heartbeats",
            "Live streamed replies directly in chat while tool activity stays in Live card",
        ],
        changed: &["Obsidian Graphite theme, soft amethyst accents, and metallic pastel git graph"],
        fixed: &[],
    },
    Release {
        version: "0.1.2",
        date: Some("2026-07-26"),
        added: &[
            "Graceful turn cancellation & mid-turn interruption for resident ACP sessions",
            "Automatic Chamber restart after successful self-update",
            "Silence-based turn watchdog (TURN_DEADLINE)",
            "Dedicated orchestrator chat lane preserved across reseats",
        ],
        changed: &[],
        fixed: &["Snapshot worktrees on turn interruption to avoid stranding uncommitted edits"],
    },
    Release {
        version: "0.1.1",
        date: Some("2026-07-25"),
        added: &[],
        changed: &[],
        fixed: &["Self-update workflow installs the exact released tag offered"],
    },
    Release {
        version: "0.1.0",
        date: Some("2026-07-24"),
        added: &[
            "Multi-quark swarm orchestration daemon (hadron-gluon)",
            "Lattice IPC data layer and ACP adapter engine",
            "Native GPUI Chamber desktop application",
        ],
        changed: &[],
        fixed: &[],
    },
];

impl super::Chamber {
    /// The completion card: rows floating just above the message box, spanning the
    /// input's full width. It is a normal render-tree descendant — `.absolute()`
    /// with `.bottom(100%)` inside the input area's `.relative()` wrapper — so it
    /// draws *upward* and stays inside the window, unlike the fork's `deferred()`
    /// menu that painted off the bottom edge (`completion-menu-draws-out-of-bounds`).
    pub(super) fn completion_card_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let card = self.completion.as_ref();
        let mut list = v_flex()
            .id("completion-card-list")
            .flex_1()
            .min_h_0()
            .max_h(px(280.0))
            .overflow_y_scroll()
            .track_scroll(&self.completion_scroll)
            .p_1()
            .gap_1();

        if let Some(card) = card {
            let sel = card.selected.min(card.candidates.len().saturating_sub(1));
            for (i, cand) in card.candidates.iter().enumerate() {
                let selected = i == sel;
                let label = cand.label.clone();
                let detail = cand.detail.clone();
                list = list.child(
                    div()
                        .id(("completion-row", i))
                        .flex()
                        .justify_between()
                        .items_center()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .when(selected, |s| s.bg(theme::glass_highlight()))
                        .hover(|s| s.bg(theme::glass_highlight()))
                        .child(div().text_sm().text_color(theme::text()).child(label))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(detail),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(c) = this.completion.as_mut() {
                                c.selected = i;
                            }
                            this.accept_completion(window, cx);
                        })),
                );
            }
        }

        h_flex()
            .id("completion-card")
            .absolute()
            .bottom(gpui::relative(1.0))
            .left_0()
            .right_0()
            .mb_2()
            .occlude()
            .max_h(px(280.0))
            .bg(theme::glass_card())
            .border_1()
            .border_color(theme::glass_highlight())
            .rounded_xl()
            .shadow_lg()
            .overflow_hidden()
            .child(list)
            .vertical_scrollbar(&self.completion_scroll)
    }

    /// The non-blocking permission toast: when a quark is waiting on the human,
    /// a banner drops in with Approve / Deny. `None` when nothing is pending.
    pub(super) fn permission_toast(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let pending = self.view.pending_permission.as_ref()?;
        let text = format!(
            "⚠️ {} wants to: {} ({:?})",
            pending.quark.as_str(),
            pending.description,
            pending.risk,
        );
        Some(
            v_flex()
                .flex_none()
                .mx_4()
                .mt_2()
                .px_3()
                .py_2()
                .gap_2()
                .rounded_lg()
                .bg(theme::bg_surface_raised())
                .child(
                    div()
                        .text_sm()
                        .text_color(theme::text())
                        .child(text),
                )
                // Buttons on their own row, right-aligned — the question and the
                // choices no longer fight for the same line (Jake's request).
                // Outline variants carry the answer's severity: green to approve,
                // amber for the remembered/always-on choice, red to deny.
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("perm-approve")
                                .outline()
                                .success()
                                .label("Approve")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.answer_permission(true, cx)),
                                ),
                        )
                        // "Always allow" remembers this (quark, op) so Auto mode won't ask again.
                        .child(
                            Button::new("perm-always")
                                .outline()
                                .warning()
                                .label("Always allow")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.answer_permission_remember(cx)
                                })),
                        )
                        .child(
                            Button::new("perm-deny")
                                .outline()
                                .danger()
                                .label("Deny")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.answer_permission(false, cx)),
                                ),
                        ),
                ),
        )
    }

    /// The "gluon stopped" banner: the swarm daemon holds `gluon.lock` exclusively,
    /// so no quark can take a turn while it's down — a critical, easy-to-miss event
    /// (a human staring at an idle chat has no other signal). Set on the
    /// running→stopped edge by the 400ms reload tick (`app::reload::reload_if_changed`),
    /// cleared on stopped→running or manual dismiss. Same non-blocking toast pattern
    /// as [`Self::permission_toast`], just a distinct (red) severity color.
    pub(super) fn gluon_stopped_toast(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.gluon_stopped_notice {
            return None;
        }
        Some(
            v_flex()
                .flex_none()
                .mx_4()
                .mt_2()
                .px_3()
                .py_2()
                .gap_2()
                .rounded_lg()
                .bg(theme::bg_surface_raised())
                .border_1()
                .border_color(rgb(0xef4444))
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text())
                                .child("⚠️ Gluon stopped — no quark can take a turn until it's restarted."),
                        )
                        .child(
                            text_button("gluon-stopped-dismiss", "Dismiss").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.gluon_stopped_notice = false;
                                    cx.notify();
                                }),
                            ),
                        ),
                ),
        )
    }

    /// The About dialog. Every value here is read from the build, not typed in: the
    /// version comes from the crate's own manifest, so it cannot drift from what
    /// shipped.
    pub(super) fn about_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let close = cx.listener(|this, _, _, cx| {
            this.about_open = false;
            cx.notify();
        });

        let adopted = self.view.roster.iter().filter(|r| r.adopted).count();
        let available = self.view.roster.len().saturating_sub(adopted);
        let workspace = crate::vcs::repo_root_of(&self.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| crate::vcs::repo_root_of(&self.path).to_string_lossy().to_string());

        // Signature brand motif: the four quark energies as a small constellation of dots,
        // echoing the field's corner glows.
        let quark_dots = h_flex().gap_1p5().items_center().children(
            [0x60a5fau32, 0xc084fc, 0x34d399, 0xfbbf24]
                .into_iter()
                .map(|c| div().size(px(9.0)).rounded_full().bg(rgb(c)).into_any_element()),
        );

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000099))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.about_open = false;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .occlude()
                    .w(px(420.0))
                    .p_5()
                    .gap_4()
                    .rounded(INNER_RADIUS)
                    .bg(theme::glass_card())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {}) // swallow inner clicks
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .child(quark_dots)
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::text())
                                    .child("Hadron"),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme::text_secondary())
                            .child("A multi-agent operating system. Quarks take turns in one shared workspace, on one shared field."),
                    )
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(panel_eyebrow("BUILD"))
                            .child(kv_row("Version", env!("CARGO_PKG_VERSION")))
                            .child(kv_row("Licence", "Apache-2.0"))
                            .child(kv_row("Workspace", workspace))
                            .child(kv_row(
                                "Quarks",
                                format!("{adopted} adopted · {available} available"),
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child("Built on GPUI (Zed) and gpui-component (Longbridge), and speaks the Agent Client Protocol."),
                    )
                    .child(
                        div()
                            .id("about-close")
                            .self_end()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(theme::bg_surface_raised())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::glass_highlight()))
                            .text_sm()
                            .text_color(theme::text())
                            .child("Close")
                            .on_click(close),
                    ),
            )
    }

    /// The Changelog overlay modal displaying release history back to v0.1.0.
    pub(super) fn changelog_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let render_section = |release: &'static Release| {
            let mut sec = v_flex().gap_2().pb_4().border_b_1().border_color(theme::border());

            let mut header = h_flex().items_center().gap_2();
            header = header.child(
                div()
                    .text_base()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme::text())
                    .child(format!("v{}", release.version)),
            );
            header = header.child(match release.date {
                Some(date) => div()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(date),
                None => div()
                    .px_2()
                    .py_0p5()
                    .rounded_full()
                    .bg(theme::accent_soft())
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme::accent())
                    .child("Unreleased"),
            });
            sec = sec.child(header);

            let render_group = |title: &'static str, items: &[&'static str]| {
                if items.is_empty() {
                    return None;
                }
                let mut grp = v_flex().gap_1();
                grp = grp.child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(theme::text_secondary())
                        .child(title),
                );
                for item in items {
                    grp = grp.child(
                        h_flex()
                            .gap_2()
                            .items_start()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child("•"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text())
                                    .child(*item),
                            ),
                    );
                }
                Some(grp)
            };

            if let Some(grp) = render_group("Added", release.added) {
                sec = sec.child(grp);
            }
            if let Some(grp) = render_group("Changed", release.changed) {
                sec = sec.child(grp);
            }
            if let Some(grp) = render_group("Fixed", release.fixed) {
                sec = sec.child(grp);
            }

            sec
        };

        div()
            .id("changelog-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000099))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.changelog_open = false;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .occlude()
                    .w(px(540.0))
                    .max_h(px(580.0))
                    .p_5()
                    .gap_4()
                    .rounded(INNER_RADIUS)
                    .bg(theme::glass_card())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {})
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2p5()
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(theme::text())
                                            .child("Changelog"),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_full()
                                            .bg(theme::bg_surface_raised())
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(theme::text_muted())
                                            .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                                    ),
                            )
                            .child(
                                div()
                                    .id("changelog-close-icon")
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .size(px(24.0))
                                    .rounded_full()
                                    .text_color(theme::text_secondary())
                                    .hover(|s| s.bg(theme::bg_surface_raised()).text_color(theme::text()))
                                    .child(Icon::new(IconName::WindowClose).small())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.changelog_open = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("changelog-list")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .gap_4()
                            .pr_1()
                            .children(RELEASES.iter().map(render_section)),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .child(
                                div()
                                    .id("changelog-close")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .bg(theme::bg_surface_raised())
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme::glass_highlight()))
                                    .text_sm()
                                    .text_color(theme::text())
                                    .child("Close")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.changelog_open = false;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
    }


    /// Best-effort, read-only probe of whether `hadron-gluon` currently holds
    /// `gluon.lock` — the same flock check `main.rs` runs once at chamber startup
    /// (`gluon_running`), made callable live each time the Process Manager opens.
    /// Any lock this acquires is released immediately; it never blocks the daemon.
    /// `pub(in crate::app)` (not private) so the 400ms reload tick (`app::reload`,
    /// a sibling of `app::render`, not a descendant) can poll it for the "gluon
    /// stopped" banner, not just this module.
    pub(in crate::app) fn gluon_running(&self) -> bool {
        let field_dir = hadron_lattice::hadron_dir_of(&self.path);
        let lock_path = field_dir.join("gluon.lock");
        if !lock_path.exists() {
            return false;
        }
        if let Ok(content) = std::fs::read_to_string(&lock_path) {
            if let Some(first_line) = content.lines().next() {
                if let Ok(pid) = first_line.trim().parse::<u32>() {
                    if hadron_lattice::sys::inspect::is_process_alive(pid, "hadron-gluon") {
                        return true;
                    }
                }
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let Ok(file) = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock_path)
            else {
                return false;
            };
            let fd = file.as_raw_fd();
            let acquired = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0;
            if acquired {
                unsafe { libc::flock(fd, libc::LOCK_UN) };
            }
            !acquired
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// Live rows for the Process Manager overlay: the daemon's real running state
    /// (a flock probe — only the OS knows) plus every adopted roster seat. See
    /// [`crate::model::build_process_rows`] for the pure row-building logic.
    pub(super) fn resolve_running_processes(&self) -> Vec<crate::model::ProcessRow> {
        crate::model::build_process_rows(self.gluon_running(), &self.view.roster)
    }

    /// The Process Manager overlay: a dim backdrop (click to dismiss) behind a card
    /// listing the daemon and every adopted quark seat, each with its live status
    /// and whichever *real* control action applies — force-restart (`Kind::Reboot`,
    /// [`Self::reboot_quark`]) for an enabled resident ACP seat, and the
    /// enable/disable toggle ([`Self::toggle_quark_enabled`]) every adopted seat
    /// already has in Settings. Deliberately no OS-level "Kill": the chamber is a
    /// separate process from the daemon and never sees a quark's child PID, so a
    /// kill switch here would have nothing real to act on.
    pub(super) fn process_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.resolve_running_processes();

        let live_dir = hadron_lattice::live::live_dir(&self.path);
        let now = chrono::Utc::now();

        let mut list = v_flex().gap_1p5();
        for row in rows {
            // Determine dot color & presence label using Hadron SSOT halo dot logic (matching Roster)
            let (dot, status_label) = if row.id == "gluon" {
                if row.status == "Running" {
                    (theme::halo_idle(), "Running".to_string())
                } else {
                    (theme::halo_error(), "Stopped".to_string())
                }
            } else if let Some(roster_row) = self.view.roster.iter().find(|r| r.id == row.id) {
                let activity = hadron_lattice::live::read(
                    &live_dir,
                    &hadron_lattice::QuarkId::new(&row.id),
                    now,
                );
                let effective_state = effective_presence_state(
                    roster_row.state,
                    roster_row.adopted,
                    roster_row.enabled,
                    activity.is_some(),
                );
                if roster_row.enabled {
                    (
                        theme::halo_dot(effective_state),
                        theme::presence_label(effective_state).to_string(),
                    )
                } else if !roster_row.adopted {
                    (gpui::rgb(0x71717a).into(), "available".to_string())
                } else {
                    (gpui::rgb(0x71717a).into(), "disabled".to_string())
                }
            } else {
                (gpui::rgb(0x71717a).into(), row.status.clone())
            };

            let avatar_or_icon = if row.id == "gluon" {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.0))
                    .rounded_full()
                    .bg(theme::bg_surface_raised())
                    .text_color(theme::text_secondary())
                    .child(Icon::new(IconName::Cpu).small())
                    .into_any_element()
            } else {
                let resolved = self.resolve_identity(&row.id);
                identity_avatar(&resolved, 24.0).into_any_element()
            };

            let mut row_actions = h_flex().gap_1p5();
            if row.can_restart {
                let id = row.id.clone();
                row_actions = row_actions.child(
                    text_button(SharedString::from(format!("proc-restart-{}", row.id)), "Restart")
                        .on_click(cx.listener(move |this, _, _, cx| this.reboot_quark(&id, cx))),
                );
            }
            if row.can_toggle {
                let id = row.id.clone();
                let label = if row.enabled { "Disable" } else { "Enable" };
                row_actions = row_actions.child(
                    text_button(SharedString::from(format!("proc-toggle-{}", row.id)), label).on_click(
                        cx.listener(move |this, _, _, cx| this.toggle_quark_enabled(&id, cx)),
                    ),
                );
            }

            list = list.child(
                h_flex()
                    .id(SharedString::from(format!("proc-row-{}", row.id)))
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(theme::bg_surface())
                    .hover(|s| s.bg(theme::bg_surface_raised()))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2p5()
                            .child(avatar_or_icon)
                            .child(div().size(px(7.0)).rounded_full().bg(dot))
                            .child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).text_color(theme::text()).child(row.label)),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(status_label),
                            )
                            .child(row_actions),
                    ),
            );
        }

        let card = v_flex()
            .occlude()
            .w(px(500.0))
            .max_h(px(560.0))
            .p_4()
            .gap_4()
            .rounded(INNER_RADIUS)
            .bg(theme::glass_card())
            .border_1()
            .border_color(theme::glass_highlight())
            .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {}) // swallow inner clicks
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::text())
                                    .child("Processes"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child("Daemon & Quark Seat Control"),
                            ),
                    )
                    .child(
                        div()
                            .id("processes-close")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(24.0))
                            .rounded_full()
                            .text_color(theme::text_secondary())
                            .hover(|s| s.bg(theme::bg_surface_raised()).text_color(theme::text()))
                            .child(Icon::new(IconName::WindowClose).small())
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_process_manager(cx))),
                    ),
            )
            .child(
                div()
                    .id("processes-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            );

        div()
            .id("processes-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000099))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| this.toggle_process_manager(cx)),
            )
            .child(card)
    }

    /// The per-quark permission ladder (Ask / Write / Auto / Bypass) as an explicit
    /// segmented picker for Settings. Unlike the roster's cycle-on-click tag, each rung is
    /// directly selectable, the current resolved mode is highlighted on its risk colour,
    /// and a gloss explains what the choice delegates. The leading **Default** rung clears
    /// any override (`ModeClear`) so the quark follows the global default; the four posture
    /// rungs each pin a per-quark `ModeSet` override. The daemon honours it next tick.
    pub(crate) fn mode_select(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        use gpui_component::select::Select;

        let (current, is_override) = self
            .view
            .roster
            .iter()
            .find(|r| r.id == id)
            .map(|r| (r.mode, r.mode_is_override))
            .unwrap_or((self.view.global_mode, false));

        let current_val = if is_override {
            format!("{:?}", current)
        } else {
            String::new()
        };

        let key = (id.to_string(), current_val.clone());
        if self.mode_select_key.as_ref() != Some(&key) {
            self.mode_select_key = Some(key);
            let modes = vec![
                "Ask".to_string(),
                "Write".to_string(),
                "Auto".to_string(),
                "Bypass".to_string(),
            ];
            let delegate = create_model_delegate("Default", &modes, Some(&current_val));
            self.mode_select_state.update(cx, |s, cx| {
                s.set_items(delegate, window, cx);
                if !current_val.is_empty() {
                    s.set_selected_value(&current_val.into(), window, cx);
                } else {
                    s.set_selected_value(&"".into(), window, cx);
                }
            });
        }

        v_flex()
            .gap_1p5()
            .child(Select::new(&self.mode_select_state).placeholder("Select permission mode..."))
            .child(div().text_xs().text_color(theme::text_muted()).child(if is_override {
                format!("Pinned for this quark ({}) — global setting is overridden.", mode_label(current))
            } else {
                format!("Default — following global setting ({}).", mode_label(current))
            }))
            .into_any_element()
    }

    /// The Settings overlay: a dim backdrop (click to dismiss) behind a card
    /// that edits one identity — an avatar switcher, a live preview, a display
    /// name, a color swatch row, and an image path (image wins over color).
    /// The keyboard-triggered app menu (F10): the same actions as the hamburger
    /// dropdown, but reachable without the mouse. A full-bleed backdrop dismisses on
    /// any outside click (and swallows it); the panel sits under the top-left button.
    pub(super) fn app_menu_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        fn item(
            id: &'static str,
            label: &'static str,
            on_click: impl Fn(&mut Chamber, &mut Window, &mut Context<Chamber>) + 'static,
            cx: &mut Context<Chamber>,
        ) -> gpui::AnyElement {
            div()
                .id(id)
                .w_full()
                .px_2()
                .py_1p5()
                .rounded(px(6.0))
                .cursor_pointer()
                .text_sm()
                .text_color(theme::text())
                .hover(|s| s.bg(theme::bg_surface_raised()))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.app_menu_open = false;
                    on_click(this, window, cx);
                    cx.notify();
                }))
                .child(label)
                .into_any_element()
        }

        let sep = || div().h(px(1.0)).w_full().bg(theme::border());

        div()
            .absolute()
            .inset_0()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.app_menu_open = false;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .occlude()
                    .absolute()
                    .top(px(44.0))
                    .left(px(12.0))
                    .w(px(280.0))
                    .p_2()
                    .gap_0p5()
                    .rounded(INNER_RADIUS)
                    .bg(theme::glass_card())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    // Swallow clicks inside the panel so they don't hit the dismiss backdrop.
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {})
                    .child(item(
                        "menu-settings",
                        "Settings…",
                        |this, window, cx| this.open_settings(window, cx),
                        cx,
                    ))
                    .child(sep())
                    .child(item(
                        "menu-reveal",
                        "Reveal Workspace in File Manager",
                        |this, _w, cx| {
                            this.handle_context_menu_action(
                                ContextMenuAction::OpenInFolder(String::from(".")),
                                cx,
                            );
                        },
                        cx,
                    ))
                    .child(sep())
                    .child(item(
                        "menu-about",
                        "About Hadron",
                        |this, _w, _cx| this.about_open = true,
                        cx,
                    ))
                    .child(sep())
                    .child(item("menu-quit", "Quit Hadron", |_t, _w, cx| cx.quit(), cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::RELEASES;

    /// `RELEASES` is a hand-written mirror of `docs/CHANGELOG.md`, and it drifted: the
    /// 0.1.4 release bumped the changelog and the version but not the overlay, so the
    /// installed build showed its own shipped release badged "Unreleased". Nothing but
    /// this test links the three, so it asserts all three agree on the newest release.
    #[test]
    fn the_top_overlay_release_matches_the_changelog_and_the_shipped_version() {
        const CHANGELOG: &str = include_str!("../../../../../docs/CHANGELOG.md");

        let header = CHANGELOG
            .lines()
            .find(|line| line.starts_with("## ["))
            .expect("docs/CHANGELOG.md has a `## [x.y.z] - DATE` release header");
        let (version, date) = header
            .trim_start_matches("## [")
            .split_once("] - ")
            .expect("the newest changelog entry is dated, not `[Unreleased]`");

        let top = &RELEASES[0];
        assert_eq!(top.version, version, "overlay's newest release vs docs/CHANGELOG.md");
        assert_eq!(top.date, Some(date.trim()), "overlay's release date vs docs/CHANGELOG.md");
        assert_eq!(
            top.version,
            env!("CARGO_PKG_VERSION"),
            "overlay's newest release vs the version this binary was built at",
        );
    }
}
