use super::*;
use hadron_lattice::secrets::SecretStore;

impl super::Chamber {
    /// Write the masked value to the keychain and declare `var` in the current
    /// quark's `secret_env` if it is not already there. A no-op if the var name or
    /// value is blank. The var NAME is a property of the quark, not a per-repo
    /// knob — like `display_name`, it is written to the global catalogue seat when
    /// one exists, falling back to a local legacy seat (mirrors the `display_name`
    /// precedence above; unlike Model/Effort/Mode it cannot go through
    /// `seat_override_delta`, since `SeatOverride` has no `secret_env` field — the
    /// var declares the same quark everywhere, not a per-repo override). Clears the
    /// masked input afterward: the value is written, never re-shown.
    ///
    /// **Security**: `value` is a secret. It is passed straight to `SecretStore::set`
    /// (the OS credential store) and never logged, cloned into `team.json`, or held
    /// anywhere else on `self`.
    pub(super) fn set_settings_secret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let SettingsTarget::Quark(key) = self.settings_target.clone() else {
            return;
        };
        let qid = QuarkId::new(key);
        let var = self.settings_secret_var.read(cx).value().trim().to_string();
        let value = self.settings_secret_value.read(cx).value().to_string();
        if var.is_empty() || value.is_empty() {
            return;
        }
        if let Err(e) = self.secret_store.set(&qid, &var, &value) {
            // Surface the failure in the UI — an `eprintln` alone is invisible in the
            // GUI, so a Set that failed (e.g. no credential service on WSL2) looked like
            // a silent no-op. Fails closed: nothing plaintext is written. (The error `e`
            // may name the service/account but never the secret value.)
            eprintln!("chamber: failed to write secret to the OS credential store: {e}");
            self.settings_secret_status = SecretStatus::Unavailable;
            cx.notify();
            return;
        }
        if let Some(g) = self.global.quarks.iter_mut().find(|s| s.id == qid) {
            if declare_secret_var(g, &var) {
                self.save_global_team(cx);
            }
        } else if let Some(existing) = self.team.quarks.iter_mut().find(|s| s.id == qid) {
            if declare_secret_var(existing, &var) {
                self.save_repo_team(cx);
            }
        }
        // else: an event-only quark with no seatable definition — the value is still
        // written to the keychain (harmless), but there is no `secret_env` to declare
        // it in, matching the model/effort commit's same carve-out above.
        self.settings_secret_value
            .update(cx, |s, cx| s.set_value(String::new(), window, cx));
        self.settings_secret_status = secret_status(self.secret_store.as_ref(), &qid, &var);
        cx.notify();
    }

    /// Delete the keychain value for the current quark's declared var and drop it
    /// from `secret_env` (same global-catalogue-first / legacy-seat-fallback
    /// precedence as `set_settings_secret`). Never displays the value.
    pub(super) fn clear_settings_secret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let SettingsTarget::Quark(key) = self.settings_target.clone() else {
            return;
        };
        let qid = QuarkId::new(key);
        let var = self.settings_secret_var.read(cx).value().trim().to_string();
        if var.is_empty() {
            return;
        }
        if let Err(e) = self.secret_store.delete(&qid, &var) {
            eprintln!("chamber: failed to clear secret from the OS credential store: {e}");
            self.settings_secret_status = SecretStatus::Unavailable;
            cx.notify();
            return;
        }
        if let Some(g) = self.global.quarks.iter_mut().find(|s| s.id == qid) {
            if undeclare_secret_var(g, &var) {
                self.save_global_team(cx);
            }
        } else if let Some(existing) = self.team.quarks.iter_mut().find(|s| s.id == qid) {
            if undeclare_secret_var(existing, &var) {
                self.save_repo_team(cx);
            }
        }
        self.settings_secret_value
            .update(cx, |s, cx| s.set_value(String::new(), window, cx));
        self.settings_secret_status = secret_status(self.secret_store.as_ref(), &qid, &var);
        cx.notify();
    }

    /// The API-key panel: env-var name + masked value inputs, Set/Clear actions, and
    /// a read-only "key set" / "not set" status. The value input is write-only — it
    /// is never populated from the store (see `load_settings_inputs`,
    /// `set_settings_secret`, `clear_settings_secret`), so nothing here can render a
    /// stored secret back to the screen.
    pub(super) fn secret_field(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(px(200.0))
                            .child(Input::new(&self.settings_secret_var)),
                    )
                    .child(div().flex_1().child(Input::new(&self.settings_secret_value))),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(text_button("settings-secret-set", "Set").on_click(cx.listener(
                        |this, _, window, cx| this.set_settings_secret(window, cx),
                    )))
                    .child(text_button("settings-secret-clear", "Clear").on_click(cx.listener(
                        |this, _, window, cx| this.clear_settings_secret(window, cx),
                    )))
                    .child(
                        div()
                            .text_xs()
                            .text_color(match self.settings_secret_status {
                                SecretStatus::Unavailable => theme::danger(),
                                _ => theme::text_muted(),
                            })
                            .child(match self.settings_secret_status {
                                SecretStatus::Set => "key set",
                                SecretStatus::NotSet => "not set",
                                SecretStatus::Unavailable => {
                                    "keychain unavailable — no OS credential service \
                                     (on WSL2, start gnome-keyring + a D-Bus session)"
                                }
                            }),
                    ),
            )
            .into_any_element()
    }
}

/// Add `var` to the seat's `secret_env` if it is not already declared. Returns
/// whether the seat changed (so the caller knows team.json needs saving) — a
/// blank var or one already present is a no-op. Pure and gpui-free, so it is
/// unit-tested directly instead of through the Settings overlay.
pub(super) fn declare_secret_var(seat: &mut Seat, var: &str) -> bool {
    if var.is_empty() || seat.secret_env.iter().any(|v| v == var) {
        false
    } else {
        seat.secret_env.push(var.to_string());
        true
    }
}

/// Remove `var` from the seat's `secret_env` if present. Returns whether the seat
/// changed. Mirrors [`declare_secret_var`] for the Clear action.
pub(super) fn undeclare_secret_var(seat: &mut Seat, var: &str) -> bool {
    let before = seat.secret_env.len();
    seat.secret_env.retain(|v| v != var);
    seat.secret_env.len() != before
}

/// Whether `var`'s value is currently set in `store` for `seat` — the "key set" /
/// "not set" status the masked field shows. Only ever reports presence, never the
/// value: a store error (e.g. no credential service available, as on a headless
/// WSL2 session) reads the same as "not set" rather than surfacing the error here.
/// Whether a seat's secret is set, unset, or the credential store is unreachable.
/// The store being DOWN (no Secret Service — e.g. a bare WSL2 with no D-Bus/keyring
/// daemon) is a DISTINCT state from a key simply not being set: without the
/// distinction, a `Set` that failed because the keychain is unavailable looks
/// identical to a no-op, and the user has no idea their key was not stored.
// `pub(crate)`, not `pub(super)`: re-exported by `settings::mod` for `app::mod`
// (the parent of `settings`), the same reach this type had when it lived directly
// in settings.rs (`pub(super)` there = visible to `app`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SecretStatus {
    Set,
    NotSet,
    Unavailable,
}

pub(super) fn secret_status(store: &dyn SecretStore, seat: &QuarkId, var: &str) -> SecretStatus {
    match store.get(seat, var) {
        Ok(Some(_)) => SecretStatus::Set,
        Ok(None) => {
            if std::env::var(var).is_ok() {
                SecretStatus::Set
            } else {
                SecretStatus::NotSet
            }
        }
        Err(_) => {
            if std::env::var(var).is_ok() {
                SecretStatus::Set
            } else {
                SecretStatus::Unavailable
            }
        }
    }
}
