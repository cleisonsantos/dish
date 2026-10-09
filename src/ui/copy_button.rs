//! Shared clipboard action: compact icon, keyboard activation and feedback.
use super::icons::{icon, Icon};
use crate::theme;
use gpui::prelude::*;
use gpui::*;
use std::time::{Duration, Instant};

pub struct TextTooltip(pub String);
impl Render for TextTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("text-tooltip")
            .max_h(px(420.))
            .overflow_y_scroll()
            .max_w(px(640.))
            .p(px(theme::S2))
            .rounded(theme::r_control())
            .bg(theme::surface_2())
            .border_1()
            .border_color(theme::line_strong())
            .text_size(px(theme::TEXT_XS))
            .text_color(theme::text())
            .child(SharedString::from(self.0.clone()))
    }
}

#[derive(IntoElement)]
pub struct CopyButton {
    pub id: ElementId,
    pub text: String,
    pub label: &'static str,
}

fn copy(text: &str, copied: &Entity<Option<Instant>>, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
    let generation = Instant::now();
    copied.update(cx, |value, cx| {
        *value = Some(generation);
        cx.notify();
    });
    let copied = copied.downgrade();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_secs(2)).await;
        let _ = copied.update(cx, |value, cx| {
            if *value == Some(generation) {
                *value = None;
                cx.notify();
            }
        });
    })
    .detach();
}

impl RenderOnce for CopyButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let copied =
            window.use_keyed_state((self.id.clone(), "copied"), cx, |_, _| None::<Instant>);
        let done = copied.read(cx).is_some();
        let key_text = self.text.clone();
        let key_copied = copied.clone();
        let label = self.label;
        div()
            .id(self.id)
            .size(px(theme::S5))
            .rounded(theme::r_control())
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .focusable()
            .tab_index(0)
            .hover(|s| s.bg(theme::hover()))
            .focus(|s| {
                s.bg(theme::hover())
                    .border_1()
                    .border_color(theme::accent())
            })
            .tooltip(move |_, cx| {
                cx.new(|_| TextTooltip(if done { "Copiado".into() } else { label.into() }))
                    .into()
            })
            .on_click(move |_, _, cx| {
                copy(&self.text, &copied, cx);
                cx.stop_propagation();
            })
            .on_key_down(move |event, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                } else if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    copy(&key_text, &key_copied, cx);
                    cx.stop_propagation();
                }
            })
            .child(icon(
                if done { Icon::Check } else { Icon::Copy },
                14.,
                if done { theme::ok() } else { theme::dim() },
            ))
    }
}
