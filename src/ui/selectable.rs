//! Optional read-only selection view, preserving the normal Markdown presentation.
use gpui::prelude::*;
use gpui::*;
use crate::editor::{Editor, CopySelection, SelectAllText};
use crate::theme;

#[derive(IntoElement)]
pub struct SelectableText(pub String);

impl RenderOnce for SelectableText {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let key = super::content_id("selection", &self.0);
        let expanded = window.use_keyed_state((key.clone(), "expanded"), cx, |_, _| false);
        let editor = window.use_keyed_state((key.clone(), "editor"), cx, |window, cx| {
            let mut editor = Editor::new(self.0, "", window, cx);
            editor.read_only = true;
            editor
        });
        let focus = editor.read(cx).focus_handle.clone();
        let open = *expanded.read(cx);
        div().w_full().flex().flex_col().gap(px(8.))
            .child(div().id(key.clone()).cursor_pointer().text_size(px(theme::TEXT_XS))
                .text_color(theme::faint()).child(if open { "fechar seleção" } else { "selecionar texto" })
                .on_click(move |_, _, cx| expanded.update(cx, |open, cx| { *open = !*open; cx.notify(); })))
            .when(open, |el| {
                let copy = editor.clone();
                let all = editor.clone();
                el.child(div().id((key, "body")).w_full().max_h(px(420.)).overflow_y_scroll()
                    .p(px(12.)).bg(theme::inset()).text_color(theme::text())
                    .key_context("DishSelection").track_focus(&focus)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| window.focus(&focus, cx))
                    .on_action(move |_: &CopySelection, _, cx| {
                        if let Some(text) = copy.read(cx).selected_text(cx) {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }
                    })
                    .on_action(move |a: &SelectAllText, window, cx| all.update(cx, |e, cx| e.select_all(a, window, cx)))
                    .child(editor))
            })
    }
}
