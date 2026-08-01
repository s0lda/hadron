//! A searchable, scrollable model list — a filter Input, a "Default"/blank row
//! pinned above the list at all times, a value the endpoint no longer offers still
//! shown and pinned selected, a bounded scroll region, and a count line. Shared by
//! the Add-Quark wizard's Connect step (`providers.rs`) and the per-quark Settings
//! Model field for a `Transport::Http` seat (`http_probe.rs`) — one definition, not
//! two.

use std::rc::Rc;

use super::*;

const LIST_MAX_H: f32 = 220.0;

impl super::Chamber {
    /// `models` is the full offered list (unfiltered); `selected` is the wire value
    /// currently stored (empty = the Default row). `on_pick` fires with the row's
    /// wire value on click — callers decide what "picking" means (park a wizard
    /// field, or write + commit a Settings Input).
    pub(super) fn model_picker_list(
        &self,
        models: &[String],
        selected: &str,
        filter: &Entity<InputState>,
        id_prefix: &'static str,
        default_label: &'static str,
        on_pick: Rc<dyn Fn(&mut Self, String, &mut Window, &mut Context<Self>)>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let query = filter.read(cx).value().trim().to_lowercase();
        let filtered: Vec<&String> =
            models.iter().filter(|m| query.is_empty() || m.to_lowercase().contains(&query)).collect();

        let mut rows = v_flex()
            .id(SharedString::from(format!("{id_prefix}-rows")))
            .flex_1()
            .min_h_0()
            .max_h(px(LIST_MAX_H))
            .overflow_y_scroll()
            .gap_1();

        rows = rows.child(self.model_picker_row(
            default_label.to_string(),
            String::new(),
            selected.is_empty(),
            format!("{id_prefix}-default"),
            on_pick.clone(),
            cx,
        ));

        // A pinned value the endpoint no longer offers must still show and stay
        // selected — always, regardless of the filter query, same as the Default
        // row above (mirrors `selector_chips`'s fallback chip).
        if !selected.is_empty() && !models.iter().any(|m| m == selected) {
            rows = rows.child(self.model_picker_row(
                selected.to_string(),
                selected.to_string(),
                true,
                format!("{id_prefix}-pinned"),
                on_pick.clone(),
                cx,
            ));
        }

        for (ix, id) in filtered.iter().enumerate() {
            let is_selected = id.as_str() == selected;
            rows = rows.child(self.model_picker_row(
                (*id).clone(),
                (*id).clone(),
                is_selected,
                format!("{id_prefix}-{ix}"),
                on_pick.clone(),
                cx,
            ));
        }

        let count = format!("{} of {} model(s)", filtered.len(), models.len());

        v_flex()
            .gap_2()
            .child(Input::new(filter).w_full())
            .child(
                div()
                    .max_h(px(LIST_MAX_H))
                    .overflow_hidden()
                    .child(rows),
            )
            .child(div().text_xs().text_color(theme::text_muted()).child(count))
            .into_any_element()
    }

    fn model_picker_row(
        &self,
        label: String,
        value: String,
        selected: bool,
        elem_id: String,
        on_pick: Rc<dyn Fn(&mut Self, String, &mut Window, &mut Context<Self>)>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id(SharedString::from(elem_id))
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .when(selected, |d| {
                d.bg(theme::glass_card())
                    .border_1()
                    .border_color(theme::accent())
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme::accent())
            })
            .when(!selected, |d| {
                d.text_color(theme::text_secondary()).hover(|s| s.bg(theme::bg_surface_raised()))
            })
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| {
                (*on_pick)(this, value.clone(), window, cx);
            }))
            .into_any_element()
    }
}
