//! Selection on rendered text, with a shared range across Markdown blocks.
//! No editor/input handler: transcript text never becomes an editable field.
use crate::{
    editor::{CopySelection, SelectAllText},
    theme,
};
use gpui::prelude::*;
use gpui::*;
use std::{cell::RefCell, ops::Range, rc::Rc};
use unicode_segmentation::UnicodeSegmentation;

type Handle = Rc<RefCell<Option<Entity<Selection>>>>;

struct Selection {
    text: String,
    anchor: usize,
    head: usize,
    dragging: bool,
    focus: FocusHandle,
}

fn boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn word(text: &str, offset: usize) -> Range<usize> {
    let offset = boundary(text, offset);
    text.split_word_bound_indices()
        .find(|(start, part)| *start <= offset && offset < start + part.len())
        .map(|(start, part)| start..start + part.len())
        .unwrap_or(offset..offset)
}

impl Selection {
    fn range(&self) -> Range<usize> {
        boundary(&self.text, self.anchor.min(self.head))
            ..boundary(&self.text, self.anchor.max(self.head))
    }
    fn update_text(&mut self, text: String) {
        // Appends during streaming preserve selection. Replaced content must not
        // silently change what an old selection would copy.
        if !text.starts_with(&self.text) {
            self.anchor = 0;
            self.head = 0;
            self.dragging = false;
        }
        self.text = text;
    }
}

pub fn plain(id: ElementId, text: &str) -> AnyElement {
    let mut builder = SelectionBuilder::default();
    let child = builder.text(text.to_string(), None);
    builder.finish(id, vec![child])
}

#[derive(Default)]
pub struct SelectionBuilder {
    text: String,
    handle: Handle,
}

impl SelectionBuilder {
    pub fn text(&mut self, text: String, runs: Option<Vec<TextRun>>) -> AnyElement {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        let offset = self.text.len();
        self.text.push_str(&text);
        SelectedText {
            text: text.into(),
            runs,
            offset,
            handle: self.handle.clone(),
            styled: None,
        }
        .into_any_element()
    }
    pub fn finish(self, id: ElementId, children: Vec<AnyElement>) -> AnyElement {
        SelectionGroup {
            id,
            text: self.text,
            handle: self.handle,
            children,
        }
        .into_any_element()
    }
}

#[derive(IntoElement)]
struct SelectionGroup {
    id: ElementId,
    text: String,
    handle: Handle,
    children: Vec<AnyElement>,
}

impl RenderOnce for SelectionGroup {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let selection =
            window.use_keyed_state((self.id.clone(), "selection"), cx, |_, cx| Selection {
                text: String::new(),
                anchor: 0,
                head: 0,
                dragging: false,
                focus: cx.focus_handle(),
            });
        selection.update(cx, |state, _| state.update_text(self.text));
        *self.handle.borrow_mut() = Some(selection.clone());
        let focus = selection.read(cx).focus.clone();
        let select_all = selection.clone();
        let dismiss = selection.clone();
        div()
            .id(self.id)
            .w_full()
            .flex()
            .flex_col()
            .gap(px(theme::S2))
            .key_context("DishSelection")
            .track_focus(&focus)
            .tab_index(0)
            .focus(|s| s.bg(theme::wash(theme::accent(), 0.04)))
            .on_action(move |_: &CopySelection, _, cx| {
                let state = selection.read(cx);
                let range = state.range();
                if !range.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(state.text[range].to_string()));
                }
            })
            .on_action(move |_: &SelectAllText, _, cx| {
                select_all.update(cx, |state, cx| {
                    state.anchor = 0;
                    state.head = state.text.len();
                    cx.notify();
                });
            })
            .on_action(move |_: &super::Dismiss, _, cx| {
                if dismiss.read(cx).range().is_empty() {
                    cx.propagate();
                } else {
                    dismiss.update(cx, |state, cx| {
                        state.anchor = 0;
                        state.head = 0;
                        state.dragging = false;
                        cx.notify();
                    });
                }
            })
            .on_key_down(|event, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            })
            .children(self.children)
    }
}

struct SelectedText {
    text: SharedString,
    runs: Option<Vec<TextRun>>,
    offset: usize,
    handle: Handle,
    styled: Option<StyledText>,
}

impl IntoElement for SelectedText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// Split only at UTF-8 boundaries, retaining Markdown fonts and backgrounds
/// outside the selected span.
fn highlighted_runs(runs: Vec<TextRun>, range: Range<usize>, color: Hsla) -> Vec<TextRun> {
    let mut result = Vec::new();
    let mut offset = 0;
    for run in runs {
        let end = offset + run.len;
        let start_selected = range.start.max(offset).min(end);
        let end_selected = range.end.max(start_selected).min(end);
        for (from, to, selected) in [
            (offset, start_selected, false),
            (start_selected, end_selected, true),
            (end_selected, end, false),
        ] {
            if from < to {
                let mut part = run.clone();
                part.len = to - from;
                if selected {
                    part.background_color = Some(color);
                }
                result.push(part);
            }
        }
        offset = end;
    }
    result
}

impl Element for SelectedText {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let range = self
            .handle
            .borrow()
            .as_ref()
            .map(|entity| entity.read(cx).range())
            .unwrap_or(0..0);
        let local = range.start.saturating_sub(self.offset).min(self.text.len())
            ..range.end.saturating_sub(self.offset).min(self.text.len());
        let runs = self
            .runs
            .take()
            .unwrap_or_else(|| vec![window.text_style().to_run(self.text.len())]);
        let runs = highlighted_runs(runs, local, theme::wash(theme::accent(), 0.35));
        let mut styled = StyledText::new(self.text.clone()).with_runs(runs);
        let layout = styled.request_layout(id, inspector, window, cx);
        self.styled = Some(styled);
        layout
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        if let Some(styled) = self.styled.as_mut() {
            styled.prepaint(id, inspector, bounds, state, window, cx);
        }
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(styled) = self.styled.as_mut() else {
            return;
        };
        styled.paint(id, inspector, bounds, state, &mut (), window, cx);
        let layout = styled.layout().clone();
        let Some(selection) = self.handle.borrow().clone() else {
            return;
        };
        let offset = self.offset;
        let len = self.text.len();
        window.set_cursor_style(CursorStyle::IBeam, hitbox);
        window.on_mouse_event({
            let selection = selection.clone();
            let layout = layout.clone();
            let hitbox = hitbox.clone();
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !hitbox.is_hovered(window)
                {
                    return;
                }
                let index = offset
                    + layout
                        .index_for_position(event.position)
                        .unwrap_or_else(|index| index)
                        .min(len);
                selection.update(cx, |state, cx| {
                    state.focus.focus(window, cx);
                    if event.click_count >= 2 {
                        let range = word(&state.text, index);
                        state.anchor = range.start;
                        state.head = range.end;
                    } else {
                        if !event.modifiers.shift {
                            state.anchor = index;
                        }
                        state.head = index;
                    }
                    state.dragging = true;
                    cx.notify();
                });
                cx.stop_propagation();
            }
        });
        window.on_mouse_event({
            let selection = selection.clone();
            let hitbox = hitbox.clone();
            move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.pressed_button != Some(MouseButton::Left)
                    || !hitbox.is_hovered(window)
                {
                    return;
                }
                let index = offset
                    + layout
                        .index_for_position(event.position)
                        .unwrap_or_else(|index| index)
                        .min(len);
                selection.update(cx, |state, cx| {
                    if state.dragging {
                        state.head = index;
                        cx.notify();
                    }
                });
            }
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                selection.update(cx, |state, _| state.dragging = false);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{boundary, highlighted_runs, word, SelectionBuilder};
    use crate::theme;
    use gpui::{Font, TextRun};
    #[test]
    fn unicode_word_selection() {
        assert_eq!(word("ação e café", 2), 0..6);
        assert_eq!(word("ação e café", 10), 9..14);
        assert_eq!(boundary("😀á", 3), 0);
    }
    #[test]
    fn selection_highlight_preserves_runs() {
        let font = Font {
            family: "test".into(),
            ..Default::default()
        };
        let runs = vec![TextRun {
            len: 6,
            font,
            color: theme::text(),
            ..Default::default()
        }];
        let result = highlighted_runs(runs, 1..5, theme::accent());
        assert_eq!(
            result.iter().map(|r| r.len).collect::<Vec<_>>(),
            vec![1, 4, 1]
        );
        assert_eq!(result[1].background_color, Some(theme::accent()));
        assert_eq!(result[0].background_color, None);
    }
    #[test]
    fn blocks_share_plain_text_offsets() {
        let mut builder = SelectionBuilder::default();
        builder.text("ação".into(), None);
        builder.text("código\nlinha".into(), None);
        assert_eq!(builder.text, "ação\ncódigo\nlinha");
        assert_eq!(&builder.text[0..6], "ação");
    }
}
