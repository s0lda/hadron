//! The Settings overlay and provider-connection wizard: opening/closing the overlay,
//! editing the target identity's name/colour/avatar, committing those inputs, and
//! rendering the settings panels, nav rows, provider list, and session picker.

use super::*;
use secrets::secret_status;
// Re-exported: `SecretStatus` is stored on `Chamber` (app/mod.rs), a sibling of
// `settings`, so it needs the same reach the type had when it lived directly in
// settings.rs (`pub(super)` there = visible to `app`).
pub(super) use secrets::SecretStatus;

pub(super) use model_select::{create_model_delegate, ModelSelectDelegate};

mod secrets;
mod identity;
mod acp_probe;
mod http_probe;
mod model_select;
mod overlay;
mod providers;
#[cfg(test)]
mod tests;

impl Chamber {
    /// Open the Settings overlay, editing the human's identity first.
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = true;
        self.settings_target = SettingsTarget::General;
        self.load_settings_inputs(window, cx);
        cx.notify();
    }

    /// Commit the name/image inputs, then close the overlay and refocus root.
    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_settings_inputs(cx);
        self.settings_open = false;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// A mutable handle to the identity currently being edited (creating an
    /// empty quark entry on first edit).
    pub(super) fn settings_identity_mut(&mut self) -> Option<&mut Identity> {
        match &self.settings_target {
            SettingsTarget::Human => Some(&mut self.prefs.human),
            SettingsTarget::Quark(id) => Some(self.prefs.quarks.entry(id.clone()).or_default()),
            SettingsTarget::General | SettingsTarget::Providers => None,
        }
    }

    /// The stored color override for the current target, if any (`#rrggbb`).
    pub(super) fn settings_color(&self) -> Option<String> {
        let key = self.settings_target.key();
        let id = if key == "human" {
            Some(&self.prefs.human)
        } else {
            self.prefs.quarks.get(key)
        };
        id.and_then(|i| i.color.clone())
    }

    /// Load the current target's name + image path into the editor inputs.
    pub(super) fn load_settings_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (name, path, model, effort, mode, roles, deny_skills, energy_limit_str, temp_str, top_p_str, max_tokens_str, secret_var, secret_is_set, secret_applies, supports_params) = {
            let key = self.settings_target.key();
            let mut mdl = String::new();
            let mut eff = None;
            let mut mod_cfg = None;
            let mut roles_str = String::new();
            let mut deny_skills_str = String::new();
            let mut energy_limit_str = String::new();
            let mut temp_str = String::new();
            let mut top_p_str = String::new();
            let mut max_tokens_str = String::new();
            // Defaults for a non-quark target (Human/Providers), which never shows the
            // API-key field — overwritten below when `key` resolves to a seat.
            let mut var = String::new();
            let mut is_set = SecretStatus::NotSet;
            // Whether THIS quark's provider needs a secret key at all — the API-key
            // field is shown only then, not under every quark (its var is not a
            // universal default).
            let mut needs_secret = false;
            let mut supports_params = false;
            let id = if key == "human" {
                Some(&self.prefs.human)
            } else if key == "general" || key == "providers" {
                None
            } else {
                // Read model/effort/mode from the RESOLVED seat, not just a legacy one — an
                // adopted quark's definition lives in the catalogue, so a legacy-only
                // lookup would show blank and a later commit could wipe the real value.
                // These show the *effective* value (catalogue default + any repo override).
                let resolved = resolve_team(&self.team, &self.global);
                if let Some(seat) = resolved.get(&QuarkId::new(key)).or_else(|| self.global.get(&QuarkId::new(key))) {
                    mdl = seat.model.clone();
                    eff = seat.effort.clone();
                    mod_cfg = seat.mode_config.clone();
                    roles_str = seat.roles.join(", ");
                    deny_skills_str = seat.deny_skills.join(", ");
                    energy_limit_str = seat.energy_limit.map(|n| n.to_string()).unwrap_or_default();
                    temp_str = seat.model_params.temperature.map(|f| f.to_string()).unwrap_or_default();
                    top_p_str = seat.model_params.top_p.map(|f| f.to_string()).unwrap_or_default();
                    max_tokens_str = seat.model_params.max_tokens.map(|n| n.to_string()).unwrap_or_default();
                    supports_params = seat.supports_model_params();
                    // The provider's required secret vars (catalogue SSOT) plus any the
                    // seat already declares decide whether to show the field and what to
                    // name it — never the value, only ever the NAME (see `secret_status`).
                    let catalogue_vars =
                        hadron_gluon::adapter::registry::QuarkKind::secret_env_for(&seat.vendor, seat.transport);
                    needs_secret = !catalogue_vars.is_empty() || !seat.secret_env.is_empty();
                    var = seat
                        .secret_env
                        .first()
                        .cloned()
                        .or_else(|| catalogue_vars.first().map(|s| s.to_string()))
                        .unwrap_or_default();
                    if needs_secret {
                        is_set = secret_status(self.secret_store.as_ref(), &seat.id, &var);
                    }
                }
                self.prefs.quarks.get(key)
            };
            (
                id.and_then(|i| i.display_name.clone()).unwrap_or_default(),
                id.and_then(|i| i.image_path.clone()).unwrap_or_default(),
                mdl,
                eff.unwrap_or_default(),
                mod_cfg.unwrap_or_default(),
                roles_str,
                deny_skills_str,
                energy_limit_str,
                temp_str,
                top_p_str,
                max_tokens_str,
                var,
                is_set,
                needs_secret,
                supports_params,
            )
        };
        self.settings_name
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.settings_path
            .update(cx, |s, cx| s.set_value(path, window, cx));
        self.settings_model
            .update(cx, |s, cx| s.set_value(model, window, cx));
        self.settings_effort
            .update(cx, |s, cx| s.set_value(effort, window, cx));
        self.settings_mode_config
            .update(cx, |s, cx| s.set_value(mode, window, cx));
        self.settings_roles
            .update(cx, |s, cx| s.set_value(roles, window, cx));
        self.settings_new_role
            .update(cx, |s, cx| s.set_value(String::new(), window, cx));
        self.settings_deny_skills
            .update(cx, |s, cx| s.set_value(deny_skills, window, cx));
        self.settings_energy_limit
            .update(cx, |s, cx| s.set_value(energy_limit_str, window, cx));
        self.settings_temperature
            .update(cx, |s, cx| s.set_value(temp_str.clone(), window, cx));
        self.settings_top_p
            .update(cx, |s, cx| s.set_value(top_p_str.clone(), window, cx));
        self.settings_max_tokens
            .update(cx, |s, cx| s.set_value(max_tokens_str.clone(), window, cx));
        self.settings_secret_var
            .update(cx, |s, cx| s.set_value(secret_var, window, cx));
        // Never populated from the store — write-only, always blank on (re)load.
        self.settings_secret_value
            .update(cx, |s, cx| s.set_value(String::new(), window, cx));
        self.settings_secret_status = secret_is_set;
        self.settings_secret_applies = secret_applies;
        self.settings_model_params_applies = supports_params;
        self.settings_advanced_expanded = !temp_str.is_empty() || !top_p_str.is_empty() || !max_tokens_str.is_empty();
        // Team-wide, not per-identity — loaded unconditionally (not keyed off `key`) so
        // it stays in sync with `self.team.max_exchanges` regardless of which target the
        // overlay happens to be showing when this runs.
        let max_exchanges = self.team.max_exchanges.map(|n| n.to_string()).unwrap_or_default();
        self.settings_max_exchanges
            .update(cx, |s, cx| s.set_value(max_exchanges, window, cx));
    }

    /// Write the editor inputs back into the current target identity and persist.
    pub(super) fn commit_settings_inputs(&mut self, cx: &mut Context<Self>) {
        let name = self.settings_name.read(cx).value().trim().to_string();
        let path = self.settings_path.read(cx).value().trim().to_string();
        let model_val = self.settings_model.read(cx).value().trim().to_string();
        let effort_val = self.settings_effort.read(cx).value().trim().to_string();
        let mode_val = self.settings_mode_config.read(cx).value().trim().to_string();

        let roles_val = self.settings_roles.read(cx).value();
        let new_roles: Vec<String> = roles_val
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let deny_skills_val = self.settings_deny_skills.read(cx).value();
        let new_deny_skills: Vec<String> = deny_skills_val
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let energy_limit_val = self.settings_energy_limit.read(cx).value().trim().to_string();
        let new_energy_limit: Option<u32> = energy_limit_val.parse::<u32>().ok();

        let temp_val = self.settings_temperature.read(cx).value().trim().to_string();
        let top_p_val = self.settings_top_p.read(cx).value().trim().to_string();
        let max_tokens_val = self.settings_max_tokens.read(cx).value().trim().to_string();

        let new_temperature: Option<f32> = temp_val.parse::<f32>().ok();
        let new_top_p: Option<f32> = top_p_val.parse::<f32>().ok();
        let new_max_tokens: Option<u32> = max_tokens_val.parse::<u32>().ok();

        let new_model_params = hadron_lattice::ModelParams {
            temperature: new_temperature,
            top_p: new_top_p,
            max_tokens: new_max_tokens,
        };

        // Team-wide "Max exchanges" (Providers panel) — gated on the Providers target,
        // like the per-quark model/effort/mode fields below are gated on `key`. This
        // must NOT be unconditional: `reload_if_changed` (mod.rs) assigns a freshly
        // polled `self.team` on an external team.json edit WITHOUT touching
        // `settings_max_exchanges`, so if that happened while Settings was open on any
        // OTHER panel, an unconditional read-diff-write here would compare the now-stale
        // input against the just-updated `self.team.max_exchanges` and silently write the
        // stale value back — reverting someone else's (or the daemon's own) edit the next
        // time any control on an unrelated panel commits (closing Settings, an
        // effort/mode/color click, …). Scoping the write to "the human is actually on the
        // Providers panel" makes that impossible: `select_settings_target` commits BEFORE
        // switching away, so an edit made while on Providers is still captured on
        // navigation, and a commit from any other panel never touches this field.
        // `load_settings_inputs` still re-syncs the input from `self.team` unconditionally
        // (every target, including the external-reload path via `reload_if_changed` ->
        // ... -> next `load_settings_inputs` call) — only the *write* is gated.
        if self.settings_target == SettingsTarget::General {
            let max_exchanges_val = self.settings_max_exchanges.read(cx).value().trim().to_string();
            let new_max_exchanges = parse_max_exchanges(&max_exchanges_val);
            if new_max_exchanges != self.team.max_exchanges {
                self.team.max_exchanges = new_max_exchanges;
                self.save_repo_team(cx);
            }
        }

        let key = self.settings_target.key();
        if key != "human" && key != "providers" && key != "general" {
            let qid = QuarkId::new(key);
            // The definition knobs (model/effort/mode/display name) are **per-repo**. How
            // they persist depends on how the quark is seated here:
            //  - a self-contained legacy seat pins the values directly on the seat;
            //  - a catalogue-adopted quark records only the *delta from the catalogue
            //    default* as a per-repo override, so the shared default (and every other
            //    repo) is untouched and a field left at the default inherits it.
            // Only write when a value actually changed, so merely opening Settings for an
            // adopted quark never un-adopts it or rewrites the file.
            let resolved = resolve_team(&self.team, &self.global);
            let def = self.global.get(&qid).cloned();
            if let Some(base) = resolved.get(&qid).cloned() {
                let new_effort = (!effort_val.is_empty()).then_some(effort_val);
                let new_mode = (!mode_val.is_empty()).then_some(mode_val);
                let new_name = (!name.is_empty()).then_some(name.clone());
                // A blank Model means "inherit the catalogue default" (or, for a
                // self-contained seat with no catalogue, keep what it already runs).
                let new_model = if model_val.is_empty() {
                    def.as_ref().map(|d| d.model.clone()).unwrap_or_else(|| base.model.clone())
                } else {
                    model_val.clone()
                };
                // The **display name is global** — a quark is the same quark in every repo,
                // so its name lives in the catalogue, never as a per-repo override. That is
                // also what the router matches `@mentions` against, so a name set once here
                // resolves everywhere. (A purely-local legacy seat has no catalogue entry, so
                // its name lives on the seat itself — the only place it can.)
                if base.display_name != new_name {
                    if let Some(g) = self.global.quarks.iter_mut().find(|s| s.id == qid) {
                        g.display_name = new_name.clone();
                        self.save_global_team(cx);
                    } else if let Some(existing) =
                        self.team.quarks.iter_mut().find(|s| s.id == qid)
                    {
                        existing.display_name = new_name.clone();
                        self.save_repo_team(cx);
                    }
                }

                let desired = hadron_lattice::Seat {
                    model: new_model,
                    effort: new_effort,
                    mode_config: new_mode,
                    roles: new_roles,
                    deny_skills: new_deny_skills,
                    energy_limit: new_energy_limit,
                    model_params: new_model_params,
                    ..base.clone()
                };

                let knobs_changed = base.model != desired.model
                    || base.effort != desired.effort
                    || base.mode_config != desired.mode_config
                    || base.roles != desired.roles
                    || base.deny_skills != desired.deny_skills
                    || base.energy_limit != desired.energy_limit
                    || base.model_params != desired.model_params;
                if knobs_changed {
                    self.update_seat_config(&qid, &desired, cx);
                }
            }
        }

        if let Some(id) = self.settings_identity_mut() {
            id.display_name = (!name.is_empty()).then_some(name);
            id.image_path = (!path.is_empty()).then_some(path);
            let _ = config::save(&self.prefs);
        }
    }

    /// Persist updated configuration knobs (model, effort, mode, roles, deny_skills, energy_limit, model_params)
    /// for a seat, handling both self-contained legacy seats and catalogue-adopted seats.
    pub(super) fn update_seat_config(
        &mut self,
        qid: &QuarkId,
        desired: &Seat,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = self.team.quarks.iter_mut().find(|s| s.id == *qid) {
            // Self-contained legacy seat — pin the values on it directly.
            existing.model = desired.model.clone();
            existing.effort = desired.effort.clone();
            existing.mode_config = desired.mode_config.clone();
            existing.roles = desired.roles.clone();
            existing.deny_skills = desired.deny_skills.clone();
            existing.energy_limit = desired.energy_limit;
            existing.model_params = desired.model_params.clone();
            self.save_repo_team(cx);
        } else if let Some(def) = self.global.get(qid).cloned() {
            // Adopted via the catalogue — write a delta override (only what
            // differs from the shared default), preserving any existing
            // role/participation override. `seat_override_delta` is the tested
            // inverse of resolve_team's def-layering. `display_name` inherits the
            // def, so the name never becomes a per-repo override.
            let prev = self.team.roster.iter().find(|o| o.id == *qid).cloned();
            let ov = hadron_lattice::seat_override_delta(
                qid.clone(),
                &def,
                desired,
                prev.as_ref(),
            );
            self.team.roster.retain(|o| o.id != *qid);
            self.team.roster.push(ov);
            self.save_repo_team(cx);
        }
        // else: an event-only quark with no seatable definition — nothing to
        // persist a per-repo knob against.
    }

    /// Switch which identity the overlay edits (committing the current one).
    pub(super) fn select_settings_target(
        &mut self,
        target: SettingsTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_settings_inputs(cx);
        self.settings_target = target.clone();
        self.load_settings_inputs(window, cx);
        // An ACP quark's Model field is a dropdown of what the agent actually offers, so
        // (re-)probe it on open. Anything else clears the probe and keeps the text field.
        match &target {
            SettingsTarget::Quark(id) => {
                self.start_acp_model_probe(&id.clone(), cx);
                self.start_agy_bridge_provision(&id.clone(), cx);
                self.start_http_model_probe(&id.clone(), cx);
            }
            _ => {
                self.acp_model_probe = None;
                self.agy_bridge_probe = None;
                self.http_model_probe = None;
            }
        }
        cx.notify();
    }

    /// Reset the current target to its code defaults.
    pub(super) fn reset_settings_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.settings_target.clone() {
            SettingsTarget::Human => self.prefs.human = Identity::default(),
            SettingsTarget::Quark(id) => {
                self.prefs.quarks.remove(&id);
            }
            SettingsTarget::General | SettingsTarget::Providers => {}
        }
        self.load_settings_inputs(window, cx);
        let _ = config::save(&self.prefs);
        cx.notify();
    }
}
