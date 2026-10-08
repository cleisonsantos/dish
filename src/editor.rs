//! A real single-field text editor.
//!
//! Upstream copyright: Copyright 2022 - 2025 Zed Industries, Inc.
//! Licensed under Apache-2.0; see LICENSE and THIRD_PARTY_NOTICES.md.
//!
//! Originally adapted from the `Editor` in the gpui-ce `view_example`
//! (Apache-2.0), this has grown the machinery a prompt box actually needs:
//! a selection with an anchor, grapheme- and word-wise movement, undo/redo,
//! clipboard operations, and mouse selection (click, drag, double, triple).
//!
//! Everything mutates through one primitive — `replace_range` — so the undo
//! stack, the selection and the notify are always consistent.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, Bounds, Context, DispatchPhase, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, InspectorElementId,
    InteractiveElement, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PaintQuad, Pixels, Point, Render, SharedString, Style, Subscription, Task, TextAlign, TextRun,
    UTF16Selection, Window, WrappedLine, actions, fill, point, prelude::*, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme;

actions!(
    dish_editor,
    [
        Left,
        Right,
        Up,
        Down,
        Home,
        End,
        DocStart,
        DocEnd,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectHome,
        SelectEnd,
        SelectDocStart,
        SelectDocEnd,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        Backspace,
        Delete,
        DeleteWordBack,
        DeleteWordForward,
        Newline,
        SendPrompt,
        SelectAllText,
        Undo,
        Redo,
        CopySelection,
        CutSelection,
        Paste,
    ]
);

/// How many snapshots the undo stack keeps.
const UNDO_LIMIT: usize = 200;

// ------------------------------------------------------------------ selection

/// An anchor and a head, both byte offsets. `head` is where the caret is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(at: usize) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }

    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn reversed(&self) -> bool {
        self.head < self.anchor
    }

    /// Pull both ends onto character boundaries of `text` and inside it.
    fn clamp(&mut self, text: &str) {
        for offset in [&mut self.anchor, &mut self.head] {
            let mut value = (*offset).min(text.len());
            while value > 0 && !text.is_char_boundary(value) {
                value -= 1;
            }
            *offset = value;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Edit {
    Insert,
    Backspace,
    Delete,
    Other,
}

#[derive(Clone)]
struct Snapshot {
    text: String,
    selection: Selection,
}

/// A remembered goal column so repeated Up/Down keeps a straight vertical path.
#[derive(Clone, Copy, Default)]
struct GoalColumn(Option<usize>);

/// Line geometry captured during paint so mouse positions can be mapped back to
/// byte offsets without re-shaping the text.
#[derive(Default)]
pub struct TextGeometry {
    lines: Vec<LogicalLine>,
    origin: Point<Pixels>,
    line_height: Pixels,
}

/// One logical line of the buffer. Soft wrapping lives *inside* it: a single
/// logical line can occupy several visual rows, and the byte offsets stay
/// relative to the line's first character.
#[derive(Clone)]
struct LogicalLine {
    /// Byte offset of the line's first character in the buffer.
    start: usize,
    /// Byte length of the line's text, excluding the trailing newline.
    len: usize,
    /// Index of this line's first visual row, across the whole element.
    row: usize,
    /// How many visual rows the line occupies.
    rows: usize,
    /// `None` for an empty line, which has nothing to shape.
    shape: Option<Rc<WrappedLine>>,
}

impl LogicalLine {
    fn end(&self) -> usize {
        self.start + self.len
    }

    /// The visual row (relative to this line) and x offset of a byte index.
    fn x_for_index(&self, index: usize, _line_height: Pixels) -> (usize, Pixels) {
        let Some(shape) = &self.shape else {
            return (0, px(0.));
        };
        let index = index.min(self.len);
        let x = shape.unwrapped_layout.x_for_index(index);
        let mut row = 0usize;
        let mut row_x = px(0.);
        for boundary in shape.wrap_boundaries() {
            let Some(run) = shape.unwrapped_layout.runs.get(boundary.run_ix) else {
                break;
            };
            let Some(glyph) = run.glyphs.get(boundary.glyph_ix) else {
                break;
            };
            if index >= glyph.index {
                row += 1;
                row_x = glyph.position.x;
            } else {
                break;
            }
        }
        (row, x - row_x)
    }

    /// The width of a visual row, used to fill wrapped rows completely.
    fn row_width(&self) -> Pixels {
        match &self.shape {
            Some(shape) => shape.width(),
            None => px(0.),
        }
    }
}

pub struct Editor {
    pub value: Entity<String>,
    pub focus_handle: FocusHandle,
    pub selection: Selection,
    pub cursor_visible: bool,
    pub placeholder: SharedString,
    pub read_only: bool,
    /// Width handed to the text last frame, used to measure wrapping during the
    /// next layout pass.
    pub text_width: Rc<Cell<f32>>,
    geometry: Rc<RefCell<TextGeometry>>,
    goal_column: GoalColumn,
    /// True while a press that began inside this editor is still held.
    dragging: bool,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    last_edit: Option<Edit>,
    /// UTF-8 range of the active input-method composition.
    marked_range: Option<Range<usize>>,
    _blink_task: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl Editor {
    pub fn new(
        text: impl Into<String>,
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let value = cx.new(|_| text.into());
        Self::over(value, placeholder, window, cx)
    }

    pub fn over(
        value: Entity<String>,
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();

        let focus_sub = cx.on_focus(&focus_handle, window, |this, _window, cx| {
            this.start_blink(cx);
        });
        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.stop_blink(cx);
        });

        // External writes (clearing after send, prefilling a dialog) must keep
        // the selection on character boundaries.
        let value_sub = cx.observe(&value, |this, value, cx| {
            let content = value.read(cx);
            this.selection.clamp(content);
            cx.notify();
        });

        Self {
            value,
            focus_handle,
            selection: Selection::caret(0),
            cursor_visible: false,
            placeholder: placeholder.into(),
            read_only: false,
            text_width: Rc::new(Cell::new(0.0)),
            geometry: Rc::new(RefCell::new(TextGeometry::default())),
            goal_column: GoalColumn::default(),
            dragging: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit: None,
            marked_range: None,
            _blink_task: Task::ready(()),
            _subscriptions: vec![focus_sub, blur_sub, value_sub],
        }
    }

    // ------------------------------------------------------------- accessors

    pub fn text(&self, cx: &App) -> String {
        self.value.read(cx).clone()
    }

    pub fn is_empty(&self, cx: &App) -> bool {
        self.value.read(cx).is_empty()
    }

    pub fn selected_text(&self, cx: &App) -> Option<String> {
        if self.selection.is_empty() {
            return None;
        }
        let content = self.value.read(cx);
        let range = self.selection.range();
        Some(content.get(range)?.to_string())
    }

    pub fn cursor_line(&self, cx: &App) -> usize {
        let content = self.value.read(cx);
        content[..self.selection.head.min(content.len())]
            .matches('\n')
            .count()
    }

    // --------------------------------------------------------------- mutation

    fn current_snapshot(&self, cx: &App) -> Snapshot {
        Snapshot {
            text: self.text(cx),
            selection: self.selection,
        }
    }

    /// Record an undo entry unless this edit continues the previous one, so that
    /// typing a word is a single undo step.
    fn record(&mut self, edit: Edit, cx: &mut Context<Self>) {
        let continues = matches!(
            (self.last_edit, edit),
            (Some(Edit::Insert), Edit::Insert)
                | (Some(Edit::Backspace), Edit::Backspace)
                | (Some(Edit::Delete), Edit::Delete)
        );
        if !continues {
            let snapshot = self.current_snapshot(cx);
            self.undo_stack.push(snapshot);
            if self.undo_stack.len() > UNDO_LIMIT {
                self.undo_stack.remove(0);
            }
        }
        self.redo_stack.clear();
        self.last_edit = Some(edit);
    }

    /// The single mutation primitive.
    fn replace_range(&mut self, range: Range<usize>, text: &str, edit: Edit, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let mut selection = Selection::new(range.start, range.end);
        selection.clamp(self.value.read(cx));
        let range = selection.range();
        self.marked_range = None;
        self.record(edit, cx);
        let caret = range.start + text.len();
        self.value.update(cx, |value, cx| {
            value.replace_range(range, text);
            cx.notify();
        });
        self.selection = Selection::caret(caret);
        self.goal_column = GoalColumn::default();
        self.reset_blink(cx);
        cx.notify();
    }

    fn delete_range(&mut self, range: Range<usize>, edit: Edit, cx: &mut Context<Self>) {
        self.replace_range(range, "", edit, cx);
    }

    pub fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.replace_range(self.selection.range(), text, Edit::Insert, cx);
    }

    pub fn delete_selection(&mut self, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            self.delete_range(self.selection.range(), Edit::Delete, cx);
        }
    }

    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        self.marked_range = None;
        self.record(Edit::Other, cx);
        let caret = text.len();
        self.value.update(cx, |value, cx| {
            *value = text;
            cx.notify();
        });
        self.selection = Selection::caret(caret);
        self.goal_column = GoalColumn::default();
        self.reset_blink(cx);
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.is_empty(cx) {
            return;
        }
        self.set_text(String::new(), cx);
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>) {
        self.placeholder = placeholder.into();
    }

    // ------------------------------------------------------------- undo/redo

    pub fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.undo_stack.pop() else {
            return;
        };
        self.redo_stack.push(self.current_snapshot(cx));
        self.restore(snapshot, cx);
    }

    pub fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.redo_stack.pop() else {
            return;
        };
        self.undo_stack.push(self.current_snapshot(cx));
        self.restore(snapshot, cx);
    }

    fn restore(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        self.marked_range = None;
        self.value.update(cx, |value, cx| {
            *value = snapshot.text;
            cx.notify();
        });
        self.selection = snapshot.selection;
        self.last_edit = None;
        self.goal_column = GoalColumn::default();
        self.reset_blink(cx);
        cx.notify();
    }

    // -------------------------------------------------------------- movement

    /// Move the caret. `extend` keeps the anchor, growing the selection.
    fn place(&mut self, offset: usize, extend: bool, cx: &mut Context<Self>) {
        if extend {
            self.selection.head = offset;
        } else {
            self.selection = Selection::caret(offset);
        }
        self.last_edit = None;
        self.reset_blink(cx);
        cx.notify();
    }

    /// Collapse a selection in the direction of travel, as editors do.
    fn caret_after_horizontal(&self, forward: bool) -> usize {
        if self.selection.is_empty() {
            return self.selection.head;
        }
        if forward {
            self.selection.end()
        } else {
            self.selection.start()
        }
    }

    fn move_grapheme(&mut self, forward: bool, extend: bool, cx: &mut Context<Self>) {
        if !extend {
            let content = self.text(cx);
            let caret = self.caret_after_horizontal(forward);
            self.place(caret, false, cx);
            let _ = content;
            return;
        }
        let content = self.text(cx);
        let head = self.selection.head;
        let target = if forward {
            next_boundary(&content, head)
        } else {
            previous_boundary(&content, head)
        };
        self.place(target, true, cx);
    }

    fn move_word(&mut self, forward: bool, extend: bool, cx: &mut Context<Self>) {
        let content = self.text(cx);
        let from = if extend {
            self.selection.head
        } else {
            self.caret_after_horizontal(forward)
        };
        let target = if forward {
            next_word_boundary(&content, from)
        } else {
            previous_word_boundary(&content, from)
        };
        self.place(target, extend, cx);
    }

    fn line_bounds(&self, cx: &App, line: usize) -> (usize, usize) {
        let content = self.value.read(cx);
        let mut start = 0;
        for _ in 0..line {
            match content[start..].find('\n') {
                Some(offset) => start += offset + 1,
                None => return (content.len(), content.len()),
            }
        }
        let end = content[start..]
            .find('\n')
            .map(|offset| start + offset)
            .unwrap_or(content.len());
        (start, end)
    }

    fn column(&self, cx: &App) -> usize {
        let content = self.value.read(cx);
        let head = self.selection.head.min(content.len());
        let line_start = content[..head].rfind('\n').map(|i| i + 1).unwrap_or(0);
        content[line_start..head].graphemes(true).count()
    }

    fn move_line(&mut self, forward: bool, extend: bool, cx: &mut Context<Self>) {
        let column = self.goal_column.0.unwrap_or_else(|| self.column(cx));
        self.goal_column = GoalColumn(Some(column));

        let content = self.text(cx);
        let head = self.selection.head;
        let line = content[..head].matches('\n').count();
        let total_lines = content.matches('\n').count();
        let target_line = if forward {
            if line >= total_lines {
                head
            } else {
                line + 1
            }
        } else if line == 0 {
            head
        } else {
            line - 1
        };
        let (start, end) = self.line_bounds(cx, target_line);
        let target = index_at_column(&content, start, end, column);
        self.place(target, extend, cx);
    }

    fn move_line_edge(&mut self, forward: bool, extend: bool, cx: &mut Context<Self>) {
        let line = self.cursor_line(cx);
        let (start, end) = self.line_bounds(cx, line);
        self.place(if forward { end } else { start }, extend, cx);
    }

    // --------------------------------------------------------------- actions

    pub fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.move_grapheme(false, false, cx);
    }

    pub fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.move_grapheme(true, false, cx);
    }

    pub fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_grapheme(false, true, cx);
    }

    pub fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_grapheme(true, true, cx);
    }

    pub fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_word(false, false, cx);
    }

    pub fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_word(true, false, cx);
    }

    pub fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_word(false, true, cx);
    }

    pub fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_word(true, true, cx);
    }

    pub fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_line(false, false, cx);
    }

    pub fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_line(true, false, cx);
    }

    pub fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_line(false, true, cx);
    }

    pub fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_line(true, true, cx);
    }

    pub fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_line_edge(false, false, cx);
    }

    pub fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_line_edge(true, false, cx);
    }

    pub fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_line_edge(false, true, cx);
    }

    pub fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_line_edge(true, true, cx);
    }

    pub fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.place(0, false, cx);
    }

    pub fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.text(cx).len();
        self.place(end, false, cx);
    }

    pub fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.place(0, true, cx);
    }

    pub fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.text(cx).len();
        self.place(end, true, cx);
    }

    pub fn select_all(&mut self, _: &SelectAllText, _: &mut Window, cx: &mut Context<Self>) {
        let end = self.text(cx).len();
        self.selection = Selection::new(0, end);
        self.last_edit = None;
        self.reset_blink(cx);
        cx.notify();
    }

    pub fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            self.delete_selection(cx);
            return;
        }
        let content = self.text(cx);
        if self.selection.head == 0 {
            return;
        }
        let previous = previous_boundary(&content, self.selection.head);
        let head = self.selection.head;
        self.delete_range(previous..head, Edit::Backspace, cx);
    }

    pub fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            self.delete_selection(cx);
            return;
        }
        let content = self.text(cx);
        let head = self.selection.head;
        if head >= content.len() {
            return;
        }
        let next = next_boundary(&content, head);
        self.delete_range(head..next, Edit::Delete, cx);
    }

    pub fn delete_word_back(&mut self, _: &DeleteWordBack, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            self.delete_selection(cx);
            return;
        }
        let content = self.text(cx);
        let head = self.selection.head;
        if head == 0 {
            return;
        }
        let previous = previous_word_boundary(&content, head);
        self.delete_range(previous..head, Edit::Backspace, cx);
    }

    pub fn delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.selection.is_empty() {
            self.delete_selection(cx);
            return;
        }
        let content = self.text(cx);
        let head = self.selection.head;
        if head >= content.len() {
            return;
        }
        let next = next_word_boundary(&content, head);
        self.delete_range(head..next, Edit::Delete, cx);
    }

    pub fn insert_newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        self.insert_text("\n", cx);
    }

    // ----------------------------------------------------------------- mouse

    /// Map a window position onto a byte offset in the buffer.
    pub fn index_for_position(&self, position: Point<Pixels>) -> usize {
        let geometry = self.geometry.borrow();
        if geometry.lines.is_empty() {
            return 0;
        }
        let line_height = geometry.line_height.max(px(1.));
        let relative_y = position.y - geometry.origin.y;
        let row = ((relative_y / line_height) as i64).max(0) as usize;
        let line = geometry
            .lines
            .iter()
            .find(|line| row >= line.row && row < line.row + line.rows)
            .or_else(|| geometry.lines.last());
        let Some(line) = line else {
            return 0;
        };
        let Some(shape) = &line.shape else {
            return line.start;
        };
        // Clamp the pointer into the line's own rows, so clicking in the empty
        // space below a short line lands on that line rather than at index 0.
        let line_row = row.clamp(line.row, line.row + line.rows.saturating_sub(1));
        let local = point(
            position.x - geometry.origin.x,
            line_height * (line_row - line.row) as f32 + line_height / 2.0,
        );
        let offset = shape
            .closest_index_for_position(local, line_height)
            .unwrap_or_else(|nearest| nearest)
            .min(line.len);
        line.start + offset
    }

    /// Take a click: place the caret, extend the selection with shift, or select
    /// the word / line on a double / triple click.
    ///
    /// This is the single entry point for both our own mouse listener and the
    /// composer, which sees the event first because it owns the hitbox.
    pub fn mouse_press(
        &mut self,
        position: Point<Pixels>,
        click_count: usize,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        self.dragging = true;
        let offset = self.index_for_position(position);
        let content = self.text(cx);
        let mut caret = Selection::caret(offset);
        caret.clamp(&content);
        let offset = caret.head;
        match click_count {
            2 => {
                // Double click selects the word under the pointer.
                let range = word_range_at(&content, offset);
                self.selection = Selection::new(range.start, range.end);
            }
            3 => {
                let line = content[..offset.min(content.len())].matches('\n').count();
                let (start, end) = self.line_bounds(cx, line);
                self.selection = Selection::new(start, end);
            }
            _ => {
                if shift {
                    self.selection.head = offset;
                } else {
                    self.selection = Selection::caret(offset);
                }
            }
        }
        self.last_edit = None;
        self.goal_column = GoalColumn::default();
        self.reset_blink(cx);
        cx.notify();
    }

    fn mouse_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        let mut caret = Selection::caret(self.index_for_position(position));
        caret.clamp(self.value.read(cx));
        let offset = caret.head;
        if self.selection.head != offset {
            self.selection.head = offset;
            self.last_edit = None;
            self.goal_column = GoalColumn::default();
            self.reset_blink(cx);
            cx.notify();
        }
    }

    // ------------------------------------------------------------------ caret

    fn mouse_up(&mut self, button: MouseButton) {
        if button == MouseButton::Left {
            self.dragging = false;
        }
    }

    fn start_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_visible = true;
        self._blink_task = Self::spawn_blink_task(cx);
        cx.notify();
    }

    fn stop_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_visible = false;
        self._blink_task = Task::ready(());
        cx.notify();
    }

    fn spawn_blink_task(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(530))
                    .await;
                let alive = this.update(cx, |editor, cx| {
                    editor.cursor_visible = !editor.cursor_visible;
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
    }

    fn reset_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_visible = true;
        self._blink_task = Self::spawn_blink_task(cx);
    }
}

// ------------------------------------------------------------ text utilities

fn previous_boundary(content: &str, offset: usize) -> usize {
    content
        .grapheme_indices(true)
        .rev()
        .find_map(|(index, _)| (index < offset).then_some(index))
        .unwrap_or(0)
}

fn next_boundary(content: &str, offset: usize) -> usize {
    content
        .grapheme_indices(true)
        .find_map(|(index, _)| (index > offset).then_some(index))
        .unwrap_or(content.len())
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Start of the word before `offset`, skipping separators first.
fn previous_word_boundary(content: &str, offset: usize) -> usize {
    let mut index = offset.min(content.len());
    while index > 0 {
        let previous = previous_boundary(content, index);
        let character = content[previous..index].chars().next().unwrap_or(' ');
        if is_word_character(character) {
            break;
        }
        index = previous;
    }
    while index > 0 {
        let previous = previous_boundary(content, index);
        let character = content[previous..index].chars().next().unwrap_or(' ');
        if !is_word_character(character) {
            break;
        }
        index = previous;
    }
    index
}

/// End of the word after `offset`, skipping separators first.
fn next_word_boundary(content: &str, offset: usize) -> usize {
    let mut index = offset.min(content.len());
    while index < content.len() {
        let next = next_boundary(content, index);
        let character = content[index..next].chars().next().unwrap_or(' ');
        if is_word_character(character) {
            break;
        }
        index = next;
    }
    while index < content.len() {
        let next = next_boundary(content, index);
        let character = content[index..next].chars().next().unwrap_or(' ');
        if !is_word_character(character) {
            break;
        }
        index = next;
    }
    index
}

/// The whole word around `offset`, for double-click selection.
fn word_range_at(content: &str, offset: usize) -> Range<usize> {
    let offset = offset.min(content.len());
    let mut start = offset;
    while start > 0 {
        let previous = previous_boundary(content, start);
        let character = content[previous..start].chars().next().unwrap_or(' ');
        if !is_word_character(character) {
            break;
        }
        start = previous;
    }
    let mut end = offset;
    while end < content.len() {
        let next = next_boundary(content, end);
        let character = content[end..next].chars().next().unwrap_or(' ');
        if !is_word_character(character) {
            break;
        }
        end = next;
    }
    if start == end {
        // On a separator: take the run of separators instead.
        return offset..offset;
    }
    start..end
}

/// The offset on `[start, end)` closest to grapheme `column`.
fn index_at_column(content: &str, start: usize, end: usize, column: usize) -> usize {
    let mut index = start;
    for grapheme in content[start..end].graphemes(true).take(column) {
        index += grapheme.len();
    }
    index.min(end)
}

fn offset_from_utf16(content: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for character in content.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += character.len_utf16();
        utf8_offset += character.len_utf8();
    }
    utf8_offset
}

fn offset_to_utf16(content: &str, offset: usize) -> usize {
    let mut utf16_offset = 0;
    let mut utf8_count = 0;
    for character in content.chars() {
        if utf8_count >= offset {
            break;
        }
        utf8_count += character.len_utf8();
        utf16_offset += character.len_utf16();
    }
    utf16_offset
}

fn range_to_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
    offset_to_utf16(content, range.start)..offset_to_utf16(content, range.end)
}

fn range_from_utf16(content: &str, range_utf16: &Range<usize>) -> Range<usize> {
    offset_from_utf16(content, range_utf16.start)..offset_from_utf16(content, range_utf16.end)
}

// ------------------------------------------------------------ platform input

impl Focusable for Editor {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let content = self.text(cx);
        let range = range_from_utf16(&content, &range_utf16);
        actual_range.replace(range_to_utf16(&content, &range));
        Some(content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let content = self.text(cx);
        let range = range_to_utf16(&content, &self.selection.range());
        Some(UTF16Selection {
            range,
            reversed: self.selection.reversed(),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| range_to_utf16(&self.text(cx), range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = self.text(cx);
        let range = range_utf16
            .as_ref()
            .map(|range| range_from_utf16(&content, range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selection.range());
        self.replace_range(range, new_text, Edit::Insert, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content = self.text(cx);
        let range = range_utf16
            .as_ref()
            .map(|range| range_from_utf16(&content, range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selection.range());
        let start = range.start;
        self.replace_range(range, new_text, Edit::Insert, cx);
        self.marked_range = (!new_text.is_empty()).then_some(start..start + new_text.len());
        if let Some(range) = new_selected_range_utf16 {
            let selected = range_from_utf16(new_text, &range);
            self.selection = Selection::new(start + selected.start, start + selected.end);
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let content = self.text(cx);
        let offset = offset_from_utf16(&content, range_utf16.start);
        let geometry = self.geometry.borrow();
        for line in &geometry.lines {
            if offset >= line.start && offset <= line.end() {
                let (row, x) = line.x_for_index(offset - line.start, geometry.line_height);
                return Some(Bounds::new(
                    point(
                        geometry.origin.x + x,
                        geometry.origin.y + geometry.line_height * (line.row + row) as f32,
                    ),
                    size(px(1.), geometry.line_height),
                ));
            }
        }
        None
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let offset = self.index_for_position(point);
        let content = self.text(cx);
        Some(offset_to_utf16(&content, offset))
    }
}

impl Render for Editor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Editor>) -> impl IntoElement {
        EditorText {
            editor: cx.entity(),
        }
    }
}

// ----------------------------------------------------------------- rendering

struct EditorText {
    editor: Entity<Editor>,
}

struct EditorTextPrepaint {
    lines: Vec<LogicalLine>,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl IntoElement for EditorText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Lay `content` out one logical line at a time. Each line keeps its own wrap
/// boundaries, so a long line can span several rows while its byte offsets stay
/// exactly where they are.
fn shape_lines(
    content: &str,
    font: gpui::Font,
    font_size: Pixels,
    color: gpui::Hsla,
    wrap_width: Option<Pixels>,
    window: &mut Window,
) -> Vec<LogicalLine> {
    let text_system = window.text_system().clone();
    let mut lines = Vec::new();
    let mut offset = 0usize;
    let mut row = 0usize;

    for text in content.split('\n') {
        let run = TextRun {
            len: text.len(),
            font: font.clone(),
            color,
            ..Default::default()
        };
        let shape = text_system
            .shape_text(text.to_string(), font_size, &[run], wrap_width, None)
            .ok()
            .and_then(|mut shaped| shaped.drain(..).next())
            .map(Rc::new);
        let rows = shape
            .as_ref()
            .map(|shape| shape.wrap_boundaries().len() + 1)
            .unwrap_or(1);
        lines.push(LogicalLine {
            start: offset,
            len: text.len(),
            row,
            rows,
            shape,
        });
        offset += text.len() + 1; // step over the newline
        row += rows;
    }

    if lines.is_empty() {
        lines.push(LogicalLine {
            start: 0,
            len: 0,
            row: 0,
            rows: 1,
            shape: None,
        });
    }
    lines
}

/// Where the selection covers part of a line: (row within line, x from, x to).
fn selection_spans(
    line: &LogicalLine,
    selection: Range<usize>,
    line_height: Pixels,
) -> Vec<(usize, Pixels, Pixels)> {
    let mut spans = Vec::new();
    if line.shape.is_none() {
        return spans;
    }
    let overlap_start = line.start.max(selection.start);
    let overlap_end = line.end().min(selection.end);
    let full_width = line.row_width();

    if overlap_start < overlap_end {
        let (start_row, start_x) = line.x_for_index(overlap_start - line.start, line_height);
        let (end_row, end_x) = line.x_for_index(overlap_end - line.start, line_height);
        if start_row == end_row {
            spans.push((start_row, start_x, end_x));
        } else {
            spans.push((start_row, start_x, full_width));
            for row in (start_row + 1)..end_row {
                spans.push((row, px(0.), full_width));
            }
            spans.push((end_row, px(0.), end_x));
        }
    }

    // A selection that runs past the end of the line includes its newline;
    // show a stub so the newline does not vanish.
    if selection.end > line.end() && selection.start <= line.end() {
        let (row, x) = line.x_for_index(line.len, line_height);
        spans.push((row, x, x + px(7.)));
    }
    spans
}

impl Element for EditorText {
    type RequestLayoutState = ();
    type PrepaintState = EditorTextPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let editor = self.editor.read(cx);
        let content = editor.text(cx);
        let width = editor.text_width.get();
        let placeholder = editor.placeholder.clone();

        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

        let measured = if content.is_empty() {
            placeholder.to_string()
        } else {
            content.clone()
        };
        let wrap_width = (width > 1.0).then(|| px(width));
        let lines = shape_lines(
            &measured,
            style.font(),
            font_size,
            style.color,
            wrap_width,
            window,
        );
        let rows: usize = lines.iter().map(|line| line.rows).sum::<usize>().max(1);

        let mut layout_style = Style::default();
        layout_style.size.width = relative(1.).into();
        layout_style.size.height = (line_height * rows as f32).into();
        (window.request_layout(layout_style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (content, selection, cursor_visible, is_focused, placeholder, width_cell, geometry) = {
            let editor = self.editor.read(cx);
            (
                editor.text(cx),
                editor.selection,
                editor.cursor_visible,
                editor.focus_handle.is_focused(window),
                editor.placeholder.clone(),
                editor.text_width.clone(),
                editor.geometry.clone(),
            )
        };

        let style = window.text_style();
        let text_color = style.color;
        let font = style.font();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

        // Feed the real width back so the next layout pass sizes the wrap right.
        let measured_width = f32::from(bounds.size.width);
        if (measured_width - width_cell.get()).abs() > 0.5 {
            width_cell.set(measured_width);
            window.request_animation_frame();
        }

        let is_placeholder = content.is_empty();
        let measured = if is_placeholder {
            placeholder.to_string()
        } else {
            content.clone()
        };
        let lines = shape_lines(
            &measured,
            font,
            font_size,
            if is_placeholder { theme::faint() } else { text_color },
            Some(bounds.size.width),
            window,
        );

        // Hand the geometry to the editor so mouse positions can be resolved.
        {
            let mut target = geometry.borrow_mut();
            target.origin = bounds.origin;
            target.line_height = line_height;
            // Placeholder glyphs are painted, but are not part of the buffer.
            target.lines = if is_placeholder { Vec::new() } else { lines.clone() };
        }

        // Selection rectangles. Wrapped rows are filled to their full width,
        // the way a text editor highlights a soft-wrapped selection.
        let mut selections: Vec<PaintQuad> = Vec::new();
        if !is_placeholder && !selection.is_empty() {
            let selection_color = theme::wash(theme::accent(), 0.30);
            for line in &lines {
                for (row, x0, x1) in selection_spans(line, selection.range(), line_height) {
                    let top = bounds.top() + line_height * (line.row + row) as f32;
                    selections.push(fill(
                        Bounds::new(
                            point(bounds.left() + x0, top),
                            size((x1 - x0).max(px(1.)), line_height),
                        ),
                        selection_color,
                    ));
                }
            }
        }

        // Caret.
        let cursor = if is_focused && cursor_visible {
            let caret = if is_placeholder { 0 } else { selection.head };
            let line = lines
                .iter()
                .rev()
                .find(|line| line.start <= caret)
                .or_else(|| lines.first());
            line.map(|line| {
                let (row, x) = line.x_for_index(caret.saturating_sub(line.start), line_height);
                fill(
                    Bounds::new(
                        point(
                            bounds.left() + x,
                            bounds.top() + line_height * (line.row + row) as f32,
                        ),
                        size(px(1.5), line_height),
                    ),
                    text_color,
                )
            })
        } else {
            None
        };

        EditorTextPrepaint {
            lines,
            cursor,
            selections,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let editor = self.editor.clone();
        let focus_handle = editor.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, editor.clone()),
            cx,
        );

        let line_height = window.line_height();

        for quad in prepaint.selections.drain(..) {
            window.paint_quad(quad);
        }
        for line in &prepaint.lines {
            let Some(shape) = &line.shape else {
                continue;
            };
            let origin = point(bounds.left(), bounds.top() + line_height * line.row as f32);
            shape
                .paint(origin, line_height, TextAlign::Left, None, window, cx)
                .ok();
        }
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }

        // Mouse selection: clicks, drags and multi-clicks, only inside our box.
        window.on_mouse_event({
            let editor = editor.clone();
            move |event: &MouseDownEvent, phase, _window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !bounds.contains(&event.position)
                {
                    return;
                }
                let shift = event.modifiers.shift;
                let clicks = event.click_count.max(1);
                let position = event.position;
                editor.update(cx, |editor, cx| {
                    editor.mouse_press(position, clicks, shift, cx)
                });
            }
        });
        window.on_mouse_event({
            let editor = editor.clone();
            move |event: &MouseUpEvent, phase, _window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let button = event.button;
                editor.update(cx, |editor, _| editor.mouse_up(button));
            }
        });
        window.on_mouse_event({
            let editor = editor.clone();
            move |event: &MouseMoveEvent, phase, _window, cx| {
                if phase != DispatchPhase::Bubble
                    || event.pressed_button != Some(MouseButton::Left)
                {
                    return;
                }
                let position = event.position;
                editor.update(cx, |editor, cx| editor.mouse_drag(position, cx));
            }
        });
    }
}

/// Wire the standard editing actions to an element.
pub fn standard_actions<E: InteractiveElement>(editor: Entity<Editor>) -> impl FnOnce(E) -> E {
    macro_rules! bind {
        ($element:expr, $action:ty, $method:ident) => {
            $element.on_action({
                let editor = editor.clone();
                move |action: &$action, window, cx| {
                    editor.update(cx, |editor, cx| editor.$method(action, window, cx))
                }
            })
        };
    }

    move |element| {
        let element = bind!(element, Left, left);
        let element = bind!(element, Right, right);
        let element = bind!(element, SelectLeft, select_left);
        let element = bind!(element, SelectRight, select_right);
        let element = bind!(element, WordLeft, word_left);
        let element = bind!(element, WordRight, word_right);
        let element = bind!(element, SelectWordLeft, select_word_left);
        let element = bind!(element, SelectWordRight, select_word_right);
        let element = bind!(element, Up, up);
        let element = bind!(element, Down, down);
        let element = bind!(element, SelectUp, select_up);
        let element = bind!(element, SelectDown, select_down);
        let element = bind!(element, Home, home);
        let element = bind!(element, End, end);
        let element = bind!(element, SelectHome, select_home);
        let element = bind!(element, SelectEnd, select_end);
        let element = bind!(element, DocStart, doc_start);
        let element = bind!(element, DocEnd, doc_end);
        let element = bind!(element, SelectDocStart, select_doc_start);
        let element = bind!(element, SelectDocEnd, select_doc_end);
        let element = bind!(element, SelectAllText, select_all);
        let element = bind!(element, Backspace, backspace);
        let element = bind!(element, Delete, delete);
        let element = bind!(element, DeleteWordBack, delete_word_back);
        let element = bind!(element, DeleteWordForward, delete_word_forward);
        let element = bind!(element, Newline, insert_newline);
        let element = bind!(element, Undo, undo);
        bind!(element, Redo, redo)
    }
}

#[cfg(test)]
mod tests {
    use super::Selection;

    #[test]
    fn stale_selection_on_empty_buffer_can_be_replaced() {
        let mut content = String::new();
        let mut selection = Selection::caret(12);
        selection.clamp(&content);
        content.replace_range(selection.range(), "test");
        assert_eq!(selection, Selection::caret(0));
        assert_eq!(content, "test");
    }

    #[test]
    fn selection_clamps_to_utf8_boundaries_and_buffer_length() {
        let mut selection = Selection::new(1, 12);
        selection.clamp("é");
        assert_eq!(selection.range(), 0..2);
    }
}
