//! Compact timestamps with provenance, full tooltip and keyboard-openable details.
use super::{copy_button::TextTooltip, selectable, test_probe::TestProbe};
use crate::{metadata::Metadata, theme};
use gpui::prelude::*;
use gpui::*;

#[derive(IntoElement)]
pub struct MetadataView {
    pub id: ElementId,
    pub metadata: Metadata,
    /// Quando definido, o marcador de horário fica discreto e só ganha
    /// contraste com o mouse sobre o grupo ou com o foco do teclado.
    pub group: Option<&'static str>,
}

impl RenderOnce for MetadataView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let expanded = window.use_keyed_state((self.id.clone(), "expanded"), cx, |_, _| false);
        let open = *expanded.read(cx);
        let keyboard_expanded = expanded.clone();
        let details = self.metadata.details.clone();
        div()
            .id((self.id.clone(), "metadata"))
            .flex()
            .flex_col()
            .gap(px(theme::S1))
            .max_w(px(280.))
            .child(
                div()
                    .id((self.id.clone(), "badge"))
                    .cursor_pointer()
                    .focusable()
                    .tab_index(0)
                    .rounded(theme::r_control())
                    .text_size(px(theme::TEXT_XS))
                    .text_color(theme::faint())
                    .when_some(self.group, |el, group| {
                        el.opacity(0.6).group_hover(group, |s| s.opacity(1.))
                    })
                    .hover(|s| s.text_color(theme::text()))
                    .focus(|s| {
                        s.opacity(1.)
                            .bg(theme::hover())
                            .border_1()
                            .border_color(theme::accent())
                    })
                    .tooltip(move |_, cx| cx.new(|_| TextTooltip(details.clone())).into())
                    .on_click(move |_, _, cx| {
                        expanded.update(cx, |value, cx| {
                            *value = !*value;
                            cx.notify();
                        });
                        cx.stop_propagation();
                    })
                    .on_key_down(
                        move |event, window, cx| match event.keystroke.key.as_str() {
                            "enter" | "space" => {
                                keyboard_expanded.update(cx, |value, cx| {
                                    *value = !*value;
                                    cx.notify();
                                });
                                cx.stop_propagation();
                            }
                            "escape" if *keyboard_expanded.read(cx) => {
                                keyboard_expanded.update(cx, |value, cx| {
                                    *value = false;
                                    cx.notify();
                                });
                                cx.stop_propagation();
                            }
                            "tab" => {
                                if event.keystroke.modifiers.shift {
                                    window.focus_prev(cx);
                                } else {
                                    window.focus_next(cx);
                                }
                                cx.stop_propagation();
                            }
                            _ => {}
                        },
                    )
                    .test_probe(|| format!("{}-badge", self.id), || serde_json::json!({
                        "expanded": open,
                        "label": self.metadata.label,
                        "details": self.metadata.details,
                    }))
                    .child(SharedString::from(self.metadata.label)),
            )
            .when(open, |el| {
                el.child(
                    div()
                        .id((self.id.clone(), "details"))
                        .max_h(px(260.))
                        .overflow_y_scroll()
                        .p(px(theme::S2))
                        .bg(theme::inset())
                        .text_size(px(theme::TEXT_XS))
                        .text_color(theme::dim())
                        .test_probe(|| format!("{}-details", self.id), || serde_json::json!({
                            "text": self.metadata.details,
                        }))
                        .child(selectable::plain(
                            (self.id, "text").into(),
                            &self.metadata.details,
                        )),
                )
            })
    }
}
