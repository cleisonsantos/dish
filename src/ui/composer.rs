//! The composer: a roomy writing surface with the model and reasoning controls
//! where they belong — next to the thing they influence — and two unambiguous
//! actions.
//!
//! The visual grammar: one raised card, recessed tokens for the technical
//! controls, and a single light action for sending. The accent appears only
//! where it means "this is your turn" — the focused edge.

use gpui::prelude::*;
use gpui::*;

use crate::editor::standard_actions;
use crate::state::{AppState, FileAttachment};
use crate::theme;
use crate::ui::icons::{Icon, icon};
use super::test_probe::TestProbe;

pub fn composer(
    state: &AppState,
    matches: &[(String, String)],
    selected: usize,
    window: &mut Window,
    cx: &mut Context<AppState>,
) -> Div {
    let source = state.editor.clone();
    let search = window.use_keyed_state(("composer-search", source.entity_id()), cx, |window, cx| {
        ComposerSearch::new(source, window, cx)
    });
    let expanded = state.composer_expanded;
    let stats = state.editor.read(cx).stats(cx);
    let mut counter = format!("{} {}", stats.chars, if stats.chars == 1 { "caractere" } else { "caracteres" });
    if stats.selected > 0 {
        counter.push_str(&format!(" · {} selecionados", stats.selected));
    }
    counter.push_str(&format!(" · Ln {} Col {}", stats.line, stats.col));
    if expanded {
        counter.push_str(&format!(" · {} {}", stats.lines, if stats.lines == 1 { "linha" } else { "linhas" }));
    }
    let editor = state.editor.clone();
    // O editor só conhece a viewport que o hospeda. Este handle é o mesmo que
    // o container abaixo registra com `track_scroll`; sem ele o caret anda
    // fora da área visível em textos longos.
    editor.read(cx).attach_scroll_handle(state.composer_scroll.clone());
    let editor_for_click = editor.clone();
    // O handle precisa carregar o tab stop: com `track_focus` explícito, o
    // `tab_index` do Div não é aplicado pelo GPUI.
    let focus_handle = editor.read(cx).focus_handle.clone().tab_stop(true).tab_index(0);
    let focused = focus_handle.is_focused(window);
    let empty = (editor.read(cx).is_empty(cx) && state.pasted_images.is_empty() && state.attached_files.is_empty()) || state.paste_loading;
    let streaming = state.streaming;
    let has_queue = !state.steering_queue.is_empty() || !state.follow_up_queue.is_empty();
    let focus_for_click = focus_handle.clone();

    let mut column = div()
        .flex_none()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(theme::S2))
        .pt(px(theme::S2))
        .pb(px(theme::S5));
    if expanded {
        column = column.flex_1().min_h(px(0.));
    }

    column = column
        .on_action({
            let search = search.clone();
            move |_: &super::FindInComposer, window, cx| {
                search.update(cx, |search, cx| search.open(window, cx));
            }
        })
        .capture_action({
            let search = search.clone();
            cx.listener(move |state, _: &super::Dismiss, window, cx| {
                // Esc: modal → ajuda → menus → / → busca → expandido → abort → banner.
                // Os ramos anteriores e posteriores continuam no handler da raiz.
                if state.modal.is_none()
                    && !state.help_open
                    && !state.effort_menu
                    && !state.model_menu
                    && state.slash_state(cx).0.is_empty()
                    && search.read(cx).is_open()
                {
                    search.update(cx, |search, cx| search.close(window, cx));
                    cx.stop_propagation();
                }
            })
        })
        .capture_action({
            let search = search.clone();
            move |_: &crate::editor::SendPrompt, _, cx| {
                if search.read(cx).is_open() {
                    search.update(cx, |search, cx| search.navigate(false, cx));
                    cx.stop_propagation();
                }
            }
        })
        .capture_action({
            let search = search.clone();
            move |_: &crate::editor::Newline, _, cx| {
                if search.read(cx).is_open() {
                    search.update(cx, |search, cx| search.navigate(true, cx));
                    cx.stop_propagation();
                }
            }
        })
        .on_action({
            let search = search.clone();
            move |_: &super::NextComposerMatch, _, cx| {
                search.update(cx, |search, cx| search.navigate(false, cx));
            }
        })
        .on_action({
            let search = search.clone();
            move |_: &super::PreviousComposerMatch, _, cx| {
                search.update(cx, |search, cx| search.navigate(true, cx));
            }
        });

    if !state.ext_widget.is_empty() {
        column = column.child(inset_block(state));
    }
    // O Dish ainda não tem drag interno: todo drag ativo vem do SO.
    let drop_hint = cx.has_active_drag();
    if drop_hint {
        column = column.child(div().mx(px(theme::CHAT_INSET)).flex().flex_row().items_center()
            .gap(px(theme::S2)).px(px(theme::S2 + 1.)).py(px(theme::S1 + 2.))
            .rounded(theme::r_control()).bg(theme::surface()).border_1().border_color(theme::line_soft())
            .child(icon(Icon::Plus, 12., theme::faint()))
            .child(div().text_size(px(theme::TEXT_SM)).text_color(theme::dim()).child("Solte para anexar")));
    }
    if state.paste_loading {
        column = column.child(loading_note(state));
    }
    for (index, (name, _)) in state.pasted_images.iter().enumerate() {
        column = column.child(image_note(index, name, cx));
    }
    for file in &state.attached_files {
        column = column.child(file_note(file, cx));
    }
    if has_queue {
        column = column.child(queue(state, cx));
    }
    if !matches.is_empty() {
        column = column.child(super::motion::reveal(slash_menu(matches, selected, cx), "slash-menu-enter"));
    } else if state.model_menu {
        column = column.child(super::motion::reveal(crate::ui::overlays::model_picker(state, window, cx), "model-menu-enter"));
    } else if state.effort_menu {
        column = column.child(super::motion::reveal(effort_menu(state, cx), "effort-menu-enter"));
    }

    // O acento do esforço vive aqui: borda no foco, brilho suave em repouso de
    // foco e um pulso lento enquanto o Pi realmente trabalha.
    let mut surface = div()
        .mx(px(theme::CHAT_INSET))
        .rounded(theme::r_surface())
        .border_1()
        .border_color(if focused || streaming {
            theme::accent_edge()
        } else {
            theme::line_strong()
        })
        .group_drag_over::<ExternalPaths>("conversation-drop", |style| style.border_color(theme::accent_edge()).bg(theme::wash(theme::accent(), 0.06)))
        .bg(theme::surface_2())
        .shadow(if focused {
            theme::accent_glow(0.0)
        } else {
            theme::shadow_composer()
        })
            .flex()
            .flex_col()
            .overflow_hidden();
    if expanded {
        surface = surface.flex_1().min_h(px(0.));
    }
    if search.read(cx).is_open() {
        surface = surface.child(search_bar(&search, cx));
    }
    let surface = surface
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    // Expandido, a linha do editor precisa ocupar a altura que
                    // o `surface` ganhou; senão o texto fica espremido no topo.
                    .when(expanded, |el| el.flex_1().min_h(px(0.)).items_stretch())
                    .gap(px(theme::S2))
                    .px(px(theme::S4))
                    .pt(px(theme::S4))
                    .pb(px(theme::S4))
                    .child(
                        div()
                            .id("composer-input")
                            .test_probe(|| "composer-input".into(), || serde_json::json!({
                                "focused": focused,
                                "expanded": expanded,
                                "stats": stats,
                                "search": ({
                                    let search = search.read(cx);
                                    let open = search.is_open();
                                    serde_json::json!({
                                        "open": open,
                                        "query_chars": if open { search.query.read(cx).stats(cx).chars } else { 0 },
                                        "matches": if open { search.matches.len() } else { 0 },
                                        "current": if open && !search.matches.is_empty() { search.current + 1 } else { 0 },
                                    })
                                }),
                                "loading": state.paste_loading,
                                "drop_hint": drop_hint,
                                "attachments": state.pasted_images.iter().map(|(name, _)| serde_json::json!({"kind": "image", "name": name}))
                                    .chain(state.attached_files.iter().map(|file| serde_json::json!({"kind": "file", "name": file.name, "size": crate::settings::format_size(file.bytes), "missing": file.missing}))).collect::<Vec<_>>(),
                                "toasts": state.toasts.iter().map(|toast| &toast.text).collect::<Vec<_>>(),
                            }))
                            .flex_1()
                            .min_w(px(0.))
                            .min_h(px(0.))
                            // Expandido não tem teto de altura: o editor ocupa
                            // o que o `surface` ganhou (o número é um teto
                            // prático, não infinito, para o layout não receber
                            // constraint infinita).
                            .max_h(if expanded { px(10_000.) } else { px(220.) })
                            .overflow_y_scroll()
                            .track_scroll(&state.composer_scroll)
                            .key_context("DishInput")
                            .track_focus(&focus_handle)
                            .cursor_text()
                            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                                focus_for_click.focus(window, cx);
                                let position = event.position;
                                let clicks = event.click_count.max(1);
                                let shift = event.modifiers.shift;
                                editor_for_click.update(cx, |editor, cx| {
                                    editor.mouse_press(position, clicks, shift, cx)
                                });
                            })
                            .map(standard_actions(editor.clone()))
                            // O piso de altura pertence ao conteúdo, não ao
                            // container rolável: com piso aqui, o
                            // `line_height * rows` do editor brigava com o
                            // `max_h(220.)` e a rolagem nunca alcançava o fim.
                            .child(
                                div()
                                    .w_full()
                                    .min_h(px(theme::WRITING_HEIGHT))
                                    .child(editor.clone()),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(theme::S4))
                    .text_size(px(theme::TEXT_XS))
                    .text_color(theme::faint())
                    .text_align(TextAlign::Right)
                    .child(counter),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(theme::S2))
                    .px(px(theme::S3))
                    .pt(px(theme::S1))
                    .pb(px(theme::S3))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S1 + 1.))
                            .min_w(px(0.))
                            .flex_1()
                            .flex_wrap()
                            .child(attach_control(state, cx))
                            .child(model_control(state, cx))
                            .child(effort_control(state, cx)),
                    )
                    .child(actions(streaming, empty, cx)),
            );

    let surface = if streaming {
        surface
            .with_animation(
                "composer-working-glow",
                Animation::new(std::time::Duration::from_millis(2400)).repeat(),
                |surface, delta| {
                    let wave = 0.5 - 0.5 * (delta * std::f32::consts::TAU).cos();
                    surface
                        .border_color(theme::wash(theme::accent(), 0.35 + 0.35 * wave))
                        .shadow(theme::accent_glow(wave))
                },
            )
            .into_any_element()
    } else {
        surface.into_any_element()
    };
    column.child(surface)
}

// Estado local ao composer: não compartilha o texto nem o undo do campo de busca.
struct ComposerSearch {
    source: Entity<crate::editor::Editor>,
    query: Entity<crate::editor::Editor>,
    saved_selection: Option<crate::editor::Selection>,
    matches: Vec<std::ops::Range<usize>>,
    current: usize,
    _subscriptions: Vec<Subscription>,
}

fn composer_matches(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    text.match_indices(query)
        .map(|(start, found)| start..start + found.len())
        .collect()
}

fn composer_match_step(current: usize, total: usize, backwards: bool) -> usize {
    if total == 0 {
        return 0;
    }
    if backwards {
        (current + total - 1) % total
    } else {
        (current + 1) % total
    }
}

fn composer_selection(
    text: &str,
    mut selection: crate::editor::Selection,
) -> crate::editor::Selection {
    for offset in [&mut selection.anchor, &mut selection.head] {
        *offset = (*offset).min(text.len());
        while !text.is_char_boundary(*offset) {
            *offset -= 1;
        }
    }
    selection
}

impl ComposerSearch {
    fn new(
        source: Entity<crate::editor::Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| crate::editor::Editor::new("", "Buscar no composer", window, cx));
        let query_value = query.read(cx).value.clone();
        let source_value = source.read(cx).value.clone();
        let query_sub = cx.observe(&query_value, |search, _, cx| search.refresh(cx));
        let source_sub = cx.observe(&source_value, |search, _, cx| search.refresh(cx));
        let release_sub = cx.on_release(|search, cx| {
            if let Some(selection) = search.saved_selection.take() {
                search.select_source(selection, cx);
            }
        });
        Self {
            source,
            query,
            saved_selection: None,
            matches: Vec::new(),
            current: 0,
            _subscriptions: vec![query_sub, source_sub, release_sub],
        }
    }

    fn is_open(&self) -> bool {
        self.saved_selection.is_some()
    }

    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_open() {
            self.saved_selection = Some(self.source.read(cx).selection);
            self.refresh(cx);
        }
        self.query.update(cx, |query, cx| {
            query.select_all(&crate::editor::SelectAllText, window, cx)
        });
        let focus = self.query.read(cx).focus_handle.clone();
        focus.focus(window, cx);
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        self.matches = composer_matches(
            &self.source.read(cx).text(cx),
            &self.query.read(cx).text(cx),
        );
        self.current = 0;
        self.highlight(cx);
    }

    fn navigate(&mut self, backwards: bool, cx: &mut Context<Self>) {
        if !self.is_open() {
            return;
        }
        self.current = composer_match_step(self.current, self.matches.len(), backwards);
        self.highlight(cx);
    }

    fn highlight(&self, cx: &mut Context<Self>) {
        let selection = self
            .matches
            .get(self.current)
            .map(|range| crate::editor::Selection::new(range.start, range.end))
            .or(self.saved_selection);
        if let Some(selection) = selection {
            self.select_source(selection, cx);
        }
        cx.notify();
    }

    fn select_source(&self, mut selection: crate::editor::Selection, cx: &mut App) {
        // Se o usuário editou o composer enquanto buscava, respeitar os novos limites UTF-8.
        selection = composer_selection(&self.source.read(cx).text(cx), selection);
        self.source.update(cx, |editor, cx| {
            editor.selection = selection;
            cx.notify();
        });
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(selection) = self.saved_selection.take() {
            self.select_source(selection, cx);
            let focus = self.source.read(cx).focus_handle.clone();
            focus.focus(window, cx);
            cx.notify();
        }
    }
}

fn search_bar(search: &Entity<ComposerSearch>, cx: &App) -> Div {
    let state = search.read(cx);
    let query = state.query.clone();
    let focus = query
        .read(cx)
        .focus_handle
        .clone()
        .tab_stop(true)
        .tab_index(0);
    let query_for_click = query.clone();
    let current = if state.matches.is_empty() {
        0
    } else {
        state.current + 1
    };
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(theme::S2))
        .px(px(theme::S4))
        .py(px(theme::S2))
        .border_b_1()
        .border_color(theme::line_soft())
        .child(
            div()
                .id("composer-search-input")
                .test_probe(
                    || "composer-search-input".into(),
                    || {
                        serde_json::json!({
                            "query_chars": query.read(cx).stats(cx).chars,
                            "current": current,
                            "total": state.matches.len(),
                        })
                    },
                )
                .flex_1()
                .min_w(px(0.))
                .key_context("DishInput")
                .track_focus(&focus)
                .cursor_text()
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    focus.focus(window, cx);
                    query_for_click.update(cx, |query, cx| {
                        query.mouse_press(
                            event.position,
                            event.click_count.max(1),
                            event.modifiers.shift,
                            cx,
                        )
                    });
                    cx.stop_propagation();
                })
                .map(standard_actions(query.clone()))
                .on_action(|_: &super::MenuAccept, window, cx| window.focus_next(cx))
                .on_action({
                    let query = query.clone();
                    move |_: &super::MenuUp, window, cx| {
                        query.update(cx, |query, cx| query.up(&crate::editor::Up, window, cx));
                    }
                })
                .on_action({
                    let query = query.clone();
                    move |_: &super::MenuDown, window, cx| {
                        query.update(cx, |query, cx| query.down(&crate::editor::Down, window, cx));
                    }
                })
                .on_action({
                    let query = query.clone();
                    move |_: &crate::editor::CopySelection, _, cx| {
                        let editor = query.read(cx);
                        let text = editor.selected_text(cx).unwrap_or_else(|| editor.text(cx));
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                })
                .on_action({
                    let query = query.clone();
                    move |_: &crate::editor::CutSelection, _, cx| {
                        if let Some(text) = query.read(cx).selected_text(cx) {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            query.update(cx, |query, cx| query.delete_selection(cx));
                        }
                    }
                })
                .on_action({
                    let query = query.clone();
                    move |_: &crate::editor::Paste, _, cx| {
                        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                            query.update(cx, |query, cx| query.insert_text(&text, cx));
                        }
                    }
                })
                .child(query),
        )
        .child(
            div()
                .text_size(px(theme::TEXT_XS))
                .text_color(theme::dim())
                .child(format!("{current}/{}", state.matches.len())),
        )
        .child(
            div()
                .text_size(px(theme::TEXT_XS))
                .text_color(theme::faint())
                .child("Enter ↓ · Shift+Enter ↑ · Esc"),
        )
}

// ------------------------------------------------------------------ controles

/// Token rebaixado: fundo inset, fio fino. É um controle técnico — o modelo e
/// o esforço são valores, não botões de ação — e por isso assenta no mesmo
/// vocabulário do código e da busca.
fn control_shell(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(theme::S1 + 1.))
        .px(px(theme::S2 + 1.))
        .py(px(theme::S1 + 2.))
        .rounded(theme::r_control())
        .min_w(px(0.))
        .focusable()
        .tab_index(0)
        .cursor_pointer()
        .hover(|style| style.bg(theme::hover()))
        .focus(|style| style.bg(theme::hover()).shadow(theme::focus_ring()))
}

fn attach_control(state: &AppState, cx: &Context<AppState>) -> AnyElement {
    control_shell("attach-button")
        .test_probe(|| "attach-button".into(), || serde_json::json!({}))
        .when(state.paste_loading, |element| element.opacity(0.4))
        .on_key_down(cx.listener(|state, event: &KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                state.pick_attachments(cx);
                cx.stop_propagation();
            } else if event.keystroke.key == "tab" {
                if event.keystroke.modifiers.shift { window.focus_prev(cx); } else { window.focus_next(cx); }
                cx.stop_propagation();
            }
        }))
        .on_click(cx.listener(|state, _: &ClickEvent, _, cx| state.pick_attachments(cx)))
        .child(icon(Icon::Plus, 12., theme::faint()))
        .child(div().text_size(px(theme::TEXT_SM)).text_color(theme::dim()).child("Anexar"))
        .into_any_element()
}

fn model_control(state: &AppState, cx: &Context<AppState>) -> AnyElement {
    let label = state
        .model
        .as_ref()
        .map(|model| model.id.clone())
        .unwrap_or_else(|| "modelo".to_string());
    control_shell("composer-model")
        .tooltip(|_, cx| cx.new(|_| super::copy_button::TextTooltip("Escolher modelo (Ctrl+Shift+M)".into())).into())
        .on_key_down(cx.listener(|state, event: &KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                state.open_model_menu(cx);
                cx.stop_propagation();
            } else if event.keystroke.key == "tab" {
                if event.keystroke.modifiers.shift { window.focus_prev(cx); } else { window.focus_next(cx); }
                cx.stop_propagation();
            }
        }))
        .on_mouse_down(MouseButton::Left, |_event, _window, cx| cx.stop_propagation())
        .on_click(cx.listener(|state, _: &ClickEvent, window, cx| {
            if state.model_menu {
                state.model_menu = false;
                let focus = state.editor.read(cx).focus_handle.clone();
                focus.focus(window, cx);
            } else {
                state.open_model_menu(cx);
            }
            cx.notify();
        }))
        .child(
            div()
                .min_w(px(0.))
                .max_w(px(220.))
                .truncate()
                .font_family(theme::FONT_MONO)
                .text_size(px(theme::TEXT_SM))
                .text_color(theme::dim())
                .child(SharedString::from(label)),
        )
        .child(icon(Icon::ChevronDown, 12., theme::faint()))
        .into_any_element()
}

fn effort_control(state: &AppState, cx: &Context<AppState>) -> AnyElement {
    let level = state.thinking_level.clone().unwrap_or_default();
    // O Pi reporta `["off"]` para modelo sem reasoning; lista vazia também é
    // tratada como sem escada. Diga isso em vez de oferecer um `off` sem sentido.
    if !crate::state::supports_reasoning(&state.thinking_levels) {
        return div()
            .id("composer-effort")
            .flex()
            .flex_row()
            .items_center()
            .gap(px(theme::S1 + 1.))
            .px(px(theme::S2 + 1.))
            .py(px(theme::S1 + 2.))
            .rounded(theme::r_control())
            .child(crate::ui::dot(theme::faint(), 8.))
            .child(
                div()
                    .text_size(px(theme::TEXT_SM))
                    .text_color(theme::faint())
                    .child(if state.model_sync.failed {
                        "esforço indisponível"
                    } else if state.loading || !state.model_sync.ready {
                        "atualizando esforço…"
                    } else {
                        "sem raciocínio"
                    }),
            )
            .into_any_element();
    }
    control_shell("composer-effort")
        .tooltip(|_, cx| cx.new(|_| super::copy_button::TextTooltip("Esforço de raciocínio (↑ e ↓ ajustam; Enter confirma)".into())).into())
        .on_key_down(cx.listener(|state, event: &KeyDownEvent, window, cx| {
            match event.keystroke.key.as_str() {
                "up" | "down" => {
                    let forward = event.keystroke.key == "down";
                    let next = effort_step(&state.thinking_levels, state.thinking_level.as_deref(), forward)
                        .map(str::to_owned);
                    if let Some(next) = next { state.set_thinking_level(&next, cx); }
                    state.effort_menu = true;
                    cx.notify();
                    cx.stop_propagation();
                }
                "enter" | "space" => {
                    state.effort_menu = !state.effort_menu;
                    if !state.effort_menu { focus_composer(state, window, cx); }
                    cx.notify();
                    cx.stop_propagation();
                }
                "tab" => {
                    state.effort_menu = false;
                    if event.keystroke.modifiers.shift { window.focus_prev(cx); } else { window.focus_next(cx); }
                    cx.notify();
                    cx.stop_propagation();
                }
                _ => {}
            }
        }))
        .on_click(cx.listener(|state, _: &ClickEvent, _window, cx| {
            state.effort_menu = !state.effort_menu;
            cx.notify();
        }))
        .child(
            div()
                .text_size(px(theme::TEXT_SM))
                .text_color(theme::effort_color(&level, &state.thinking_levels))
                .child(crate::state::effort_label(&level)),
        )
        .child(icon(Icon::ChevronDown, 12., theme::faint()))
        .into_any_element()
}

fn focus_composer(state: &AppState, window: &mut Window, cx: &mut App) {
    state.editor.read(cx).focus_handle.clone().focus(window, cx);
}

/// Step only within capabilities reported by Pi, stopping at either end.
fn effort_step<'a>(levels: &'a [String], current: Option<&str>, forward: bool) -> Option<&'a str> {
    let Some(index) = levels.iter().position(|level| Some(level.as_str()) == current) else {
        return levels.first().map(String::as_str);
    };
    let next = if forward { index.saturating_add(1) } else { index.saturating_sub(1) };
    levels.get(next.min(levels.len().saturating_sub(1))).map(String::as_str)
}

/// Duas ações, dois significados: enviar (que durante a execução orienta) e
/// interromper. Nunca o mesmo espaço para as duas.
fn actions(streaming: bool, empty: bool, cx: &Context<AppState>) -> AnyElement {
    let weak = cx.entity().downgrade();

    let stop = {
        let weak = weak.clone();
        div()
            .id("stop-button")
            .flex_none()
            .size(px(36.))
            .rounded(theme::r_action())
            .focusable()
            .tab_index(0)
            .focus(|style| style.shadow(theme::focus_ring()))
            .on_key_down(cx.listener(|state, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    state.abort(cx);
                    focus_composer(state, window, cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift { window.focus_prev(cx); } else { window.focus_next(cx); }
                    cx.stop_propagation();
                }
            }))
            .border_1()
            .border_color(theme::line())
            .bg(theme::inset())
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(theme::wash(theme::failure(), 0.14)).border_color(theme::failure()))
            .active(|style| style.opacity(0.8))
            .on_click(move |_event: &ClickEvent, window: &mut Window, cx: &mut App| {
                weak.update(cx, |state, cx| {
                    state.abort(cx);
                    focus_composer(state, window, cx);
                }).ok();
            })
            .child(icon(Icon::Stop, 13., theme::failure()))
    };

    let send = {
        let weak = weak.clone();
        div()
            .id("send-button")
            .flex_none()
            .h(px(36.))
            .px(px(theme::S3))
            .gap(px(theme::S2))
            .rounded(theme::r_action())
            .flex()
            .items_center()
            .justify_center()
            .border_1()
            .when(!empty, |el| el
                .focusable()
                .tab_index(0)
                .focus(|style| style.shadow(theme::focus_ring())))
            .on_key_down(cx.listener(|state, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    state.submit(cx);
                    focus_composer(state, window, cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift { window.focus_prev(cx); } else { window.focus_next(cx); }
                    cx.stop_propagation();
                }
            }))
            .child(div().text_size(px(theme::TEXT_SM)).font_weight(FontWeight::MEDIUM).text_color(if empty {
                theme::faint()
            } else {
                theme::on_accent()
            }).child("Enviar"))
            // Sem texto, o botão fica como contorno legível: não some, e não
            // finge ser uma ação disponível.
            .when(empty, |el| {
                el.border_color(theme::line_strong())
                    .child(icon(Icon::Send, 15., theme::faint()))
            })
            // Com texto, a ação principal é o preenchimento neutro invertido
            // (regra do DESIGN.md: Enviar não é preenchido com o esforço); o
            // acento só aparece como brilho ao passar o mouse.
            .when(!empty, |el| {
                el.bg(theme::text())
                    .border_color(theme::text())
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::dim()).shadow(theme::accent_glow(0.7)))
                    .active(|style| style.opacity(0.85))
                    .child(icon(Icon::Send, 15., theme::on_accent()))
            })
            .on_click(move |_event: &ClickEvent, window: &mut Window, cx: &mut App| {
                weak.update(cx, |state, cx| {
                    state.submit(cx);
                    focus_composer(state, window, cx);
                }).ok();
            })
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(theme::S2))
        .flex_none()
        .when(streaming, |el| el.child(stop))
        .child(send)
        .into_any_element()
}

// ---------------------------------------------------------------- anotações

fn loading_note(_state: &AppState) -> Div {
    div()
        .mx(px(theme::CHAT_INSET))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(theme::S2))
        .px(px(theme::S2))
        .py(px(theme::S1 + 2.))
        .rounded(theme::r_control())
        .bg(theme::surface())
        .border_1()
        .border_color(theme::line_soft())
        .child(crate::ui::pulse("paste-pulse", 6., theme::running()))
        .child(
            div()
                .text_size(px(theme::TEXT_SM))
                .text_color(theme::dim())
                .child("Preparando colagem…"),
        )
}

fn file_note(file: &FileAttachment, cx: &Context<AppState>) -> Div {
    let id = file.id;
    let name = file.name.clone();
    div().mx(px(theme::CHAT_INSET)).flex().flex_row().items_center()
        .gap(px(theme::S2)).px(px(theme::S2 + 1.)).py(px(theme::S1 + 2.))
        .rounded(theme::r_control()).bg(theme::surface()).border_1().border_color(theme::line_soft())
        .child(icon(if file.missing { Icon::Alert } else { Icon::File }, 12., if file.missing { theme::failure() } else { theme::faint() }))
        .child(div().flex_1().min_w(px(0.)).truncate().text_size(px(theme::TEXT_SM)).text_color(theme::dim()).child(SharedString::from(file.name.clone())))
        .child(div().flex_none().font_family(theme::FONT_MONO).text_size(px(theme::TEXT_XS))
            .text_color(if file.missing { theme::failure() } else { theme::faint() })
            .child(SharedString::from(if file.missing { "sumiu do disco".into() } else { crate::settings::format_size(file.bytes) })))
        .child(div().id(("remove-attached-file", id as usize))
            .test_probe(move || format!("remove-attached-file:{name}"), || serde_json::json!({}))
            .flex_none().px(px(theme::S1 + 1.)).py(px(2.)).rounded(theme::r_control()).cursor_pointer()
            .hover(|style| style.text_color(theme::failure()).bg(theme::hover()))
            .child(icon(Icon::Close, 11., theme::faint()))
            .on_click(cx.listener(move |state, _: &ClickEvent, _, cx| { state.remove_file(id); cx.notify(); })))
}

fn image_note(index: usize, name: &str, cx: &Context<AppState>) -> Div {
    div()
        .mx(px(theme::CHAT_INSET))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(theme::S2))
        .px(px(theme::S2 + 1.))
        .py(px(theme::S1 + 2.))
        .rounded(theme::r_control())
        .bg(theme::surface())
        .border_1()
        .border_color(theme::line_soft())
        .child(icon(Icon::File, 12., theme::faint()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(px(theme::TEXT_SM))
                .text_color(theme::dim())
                .child(SharedString::from(format!("Imagem: {name}"))),
        )
        .child(
            div()
                .id(("remove-pasted-image", index))
                .flex_none()
                .px(px(theme::S1 + 1.))
                .py(px(2.))
                .rounded(theme::r_control())
                .cursor_pointer()
                .text_size(px(theme::TEXT_XS))
                .text_color(theme::faint())
                .hover(|style| style.text_color(theme::failure()).bg(theme::hover()))
                .child("remover")
                .on_click(cx.listener(move |state, _: &ClickEvent, _, cx| {
                    if index < state.pasted_images.len() {
                        state.pasted_images.remove(index);
                        cx.notify();
                    }
                })),
        )
}

// ---------------------------------------------------------------------- fila

fn queue(state: &AppState, cx: &Context<AppState>) -> Div {
    let total = state.steering_queue.len() + state.follow_up_queue.len();
    let open = state.queue_open;

    let mut column = div()
        .mx(px(theme::CHAT_INSET))
        .flex()
        .flex_col()
        .gap(px(theme::S1))
        .child(
            div()
                .id("queue-header")
                .flex()
                .flex_row()
                .items_center()
                .gap(px(theme::S2))
                .px(px(theme::S2))
                .py(px(theme::S1 + 1.))
                .rounded(theme::r_control())
                .cursor_pointer()
                .hover(|style| style.bg(theme::hover()))
                .on_click(cx.listener(|state, _: &ClickEvent, _window, cx| {
                    state.queue_open = !state.queue_open;
                    cx.notify();
                }))
                .child(icon(
                    if open {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    },
                    13.,
                    theme::faint(),
                ))
                .child(icon(Icon::Clock, 13., theme::running()))
                .child(
                    div()
                        .text_size(px(theme::TEXT_SM))
                        .text_color(theme::dim())
                        .child(SharedString::from(if total == 1 {
                            "Fila · 1 item".to_string()
                        } else {
                            format!("Fila · {total} itens")
                        })),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .id("clear-queue")
                        .text_size(px(theme::TEXT_XS))
                        .text_color(theme::faint())
                        .cursor_pointer()
                        .hover(|style| style.text_color(theme::failure()))
                        .on_click(cx.listener(|state, _: &ClickEvent, _window, cx| {
                            state.clear_queue(cx);
                            cx.notify();
                        }))
                        .child("limpar"),
                ),
        );

    if open {
        let mut items = div().flex().flex_col().gap(px(theme::S1));
        for text in &state.steering_queue {
            items = items.child(queue_item("orientação", text, theme::running()));
        }
        for text in &state.follow_up_queue {
            items = items.child(queue_item("próximo", text, theme::dim()));
        }
        column = column.child(items);
    }
    column
}

fn queue_item(label: &str, text: &str, color: Hsla) -> AnyElement {
    let preview: String = text.chars().take(160).collect();
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .pl(px(theme::S3))
        .border_l_1()
        .border_color(theme::line())
        .child(
            div()
                .text_size(px(theme::TEXT_XS))
                .text_color(color)
                .child(SharedString::from(label.to_string())),
        )
        .child(
            div()
                .text_size(px(theme::TEXT_SM))
                .line_height(px(18.))
                .text_color(theme::dim())
                .child(SharedString::from(preview)),
        )
        .into_any_element()
}

/// Menu do esforço: cada nível na sua cor do Pi, como no rail antigo.
fn effort_menu(state: &AppState, cx: &Context<AppState>) -> Div {
    let current = state.thinking_level.clone().unwrap_or_default();
    // Pi owns capabilities. Never invent a fallback ladder for an empty list.
    let levels = state.thinking_levels.clone();

    let rows = levels.iter().enumerate().map(|(index, level)| {
        let selected = *level == current;
        let color = theme::effort_color(level, &levels);
        let level_for_click = level.clone();
        div()
            .id(SharedString::from(format!("effort-{index}")))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(theme::S3))
            .px(px(theme::S3))
            .py(px(theme::S1 + 2.))
            .rounded(theme::r_small())
            .hover(|style| style.bg(theme::hover()))
            .on_click(cx.listener(move |state, _: &ClickEvent, window, cx| {
                state.set_thinking_level(&level_for_click, cx);
                state.effort_menu = false;
                focus_composer(state, window, cx);
                cx.notify();
            }))
            .child(
                div()
                    .flex_none()
                    .w(px(14.))
                    .child(crate::ui::dot(color, if selected { 8. } else { 6. })),
            )
            .child(
                div()
                    .text_size(px(theme::TEXT))
                    .text_color(if selected { theme::text() } else { theme::dim() })
                    .child(SharedString::from(crate::state::effort_label(level))),
            )
            .child(div().flex_1())
            .when(selected, |el| {
                el.child(
                    div()
                        .text_size(px(theme::TEXT_XS))
                        .text_color(theme::faint())
                        .child("ativo"),
                )
            })
    });

    div()
        .mx(px(theme::CHAT_INSET))
        .max_w(px(theme::EFFORT_MENU_WIDTH))
        .rounded(theme::r_surface())
        .border_1()
        .border_color(theme::line_strong())
        .bg(theme::surface())
        .shadow(theme::shadow_overlay())
        .p(px(theme::S1 + 1.))
        .flex()
        .flex_col()
        .children(rows)
        .child(
            div()
                .px(px(theme::S3))
                .pt(px(theme::S2))
                .pb(px(theme::S1))
                .mt(px(theme::S1))
                .border_t_1()
                .border_color(theme::line_soft())
                .text_size(px(theme::TEXT_XS))
                .text_color(theme::faint())
                .child("↑ e ↓ no controle ajustam o esforço; Enter confirma."),
        )
}

// ---------------------------------------------------------------- slash menu

fn slash_menu(matches: &[(String, String)], selected: usize, cx: &Context<AppState>) -> Div {
    let weak = cx.entity().downgrade();
    let rows = matches
        .iter()
        .enumerate()
        .map(|(index, (name, description))| {
            let weak = weak.clone();
            let is_selected = index == selected;
            div()
                .id(SharedString::from(format!("slash-{index}")))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(theme::S3))
                .px(px(theme::S3))
                .py(px(theme::S1 + 2.))
                .rounded(theme::r_small())
                .when(is_selected, |el| el.bg(theme::hover()))
                .when(!is_selected, |el| {
                    el.hover(|style| style.bg(theme::hover()))
                })
                .child(
                    div()
                        .flex_none()
                        .w(px(10.))
                        .when(is_selected, |el| {
                            el.child(crate::ui::dot(theme::accent(), 5.0))
                        }),
                )
                .child(
                    div()
                        .w(px(160.))
                        .flex_none()
                        .font_family(theme::FONT_MONO)
                        .text_size(px(theme::TEXT_SM))
                        .text_color(theme::accent())
                        .child(SharedString::from(name.clone())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(px(theme::TEXT_XS))
                        .text_color(theme::faint())
                        .child(SharedString::from(description.clone())),
                )
                .on_click(
                    move |_event: &ClickEvent, window: &mut Window, cx: &mut App| {
                        weak.update(cx, |state, cx| {
                            state.select_slash(index, cx);
                            let handle = state.editor.read(cx).focus_handle.clone();
                            handle.focus(window, cx);
                        })
                        .ok();
                    },
                )
        });

    div()
        .mx(px(theme::CHAT_INSET))
        .max_w(px(theme::COMPOSER_PICKER_WIDTH))
        .rounded(theme::r_surface())
        .border_1()
        .border_color(theme::line_strong())
        .bg(theme::surface())
        .shadow(theme::shadow_overlay())
        .p(px(theme::S1 + 1.))
        .flex()
        .flex_col()
        .children(rows)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(theme::S3))
                .px(px(theme::S3))
                .pt(px(theme::S2))
                .pb(px(theme::S1))
                .mt(px(theme::S1))
                .border_t_1()
                .border_color(theme::line_soft())
                .font_family(theme::FONT_MONO)
                .text_size(px(theme::TEXT_XS))
                .text_color(theme::faint())
                .child("↑↓ escolher")
                .child("⏎ executar")
                .child("⇥ completar")
                .child("esc fechar"),
        )
}

fn inset_block(state: &AppState) -> Div {
    // Extensão falando dentro do compositor: um poço técnico com uma faixa
    // à esquerda, para ler como saída de máquina e não como campo de digitação.
    div()
        .mx(px(theme::CHAT_INSET))
        .flex()
        .flex_row()
        .items_stretch()
        .gap(px(theme::S2))
        .child(
            div()
                .flex_none()
                .w(px(2.))
                .rounded(px(1.))
                .bg(theme::line_strong()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .px(px(theme::S2))
                .py(px(theme::S2))
                .rounded(theme::r_control())
                .bg(theme::surface())
                .border_1()
                .border_color(theme::line_soft())
                .font_family(theme::FONT_MONO)
                .text_size(px(theme::TEXT_XS))
                .text_color(theme::dim())
                .children(
                    state
                        .ext_widget
                        .iter()
                        .map(|line| SharedString::from(line.clone())),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::{composer_match_step, composer_matches, composer_selection, effort_step};
    use crate::editor::Selection;

    #[test]
    fn composer_search_finds_literal_utf8_ranges_and_handles_empty_queries() {
        let text = "ação🙂 ação\nação";
        let matches = composer_matches(text, "ação");
        assert_eq!(matches, vec![0..6, 11..17, 18..24]);
        for range in matches {
            assert_eq!(&text[range], "ação");
        }
        assert!(composer_matches(text, "").is_empty());
        assert!(composer_matches(text, "ausente").is_empty());
        assert_eq!(composer_matches("aaaa", "aa"), vec![0..2, 2..4]);
    }

    #[test]
    fn composer_search_navigation_wraps_and_handles_no_results() {
        assert_eq!(composer_match_step(0, 3, false), 1);
        assert_eq!(composer_match_step(2, 3, false), 0);
        assert_eq!(composer_match_step(0, 3, true), 2);
        assert_eq!(composer_match_step(2, 3, true), 1);
        assert_eq!(composer_match_step(0, 1, false), 0);
        assert_eq!(composer_match_step(0, 1, true), 0);
        assert_eq!(composer_match_step(0, 0, false), 0);
        assert_eq!(composer_match_step(0, 0, true), 0);
    }

    #[test]
    fn composer_search_restores_caret_and_selection_direction() {
        let text = "ação🙂";
        for selection in [
            Selection::caret(6),
            Selection::new(10, 0),
            Selection::new(0, 6),
        ] {
            assert_eq!(composer_selection(text, selection), selection);
        }
        assert_eq!(
            composer_selection("é", Selection::new(1, 11)),
            Selection::new(0, 2)
        );
        assert_eq!(
            composer_selection("", Selection::caret(11)),
            Selection::caret(0)
        );
    }

    #[test]
    fn keyboard_effort_uses_pi_capabilities_and_stops_at_edges() {
        let levels: Vec<String> = ["off", "low", "high", "max"].into_iter().map(str::to_owned).collect();
        assert_eq!(effort_step(&levels, Some("low"), true), Some("high"));
        assert_eq!(effort_step(&levels, Some("high"), false), Some("low"));
        assert_eq!(effort_step(&levels, Some("off"), false), Some("off"));
        assert_eq!(effort_step(&levels, Some("max"), true), Some("max"));
        assert_eq!(effort_step(&levels, None, true), Some("off"));
        assert_eq!(effort_step(&levels, Some("unknown"), true), Some("off"));
        assert_eq!(effort_step(&[], None, true), None);
        assert_eq!(effort_step(&["high".into()], Some("high"), false), Some("high"));
    }
}
