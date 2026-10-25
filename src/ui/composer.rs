//! The composer: a roomy writing surface with the model and reasoning controls
//! where they belong — next to the thing they influence — and two unambiguous
//! actions. No permanent shortcut row, no character counter.
//!
//! The visual grammar: one raised card, recessed tokens for the technical
//! controls, and a single light action for sending. The accent appears only
//! where it means "this is your turn" — the focused edge.

use gpui::prelude::*;
use gpui::*;

use crate::editor::standard_actions;
use crate::state::AppState;
use crate::theme;
use crate::ui::icons::{Icon, icon};
use super::test_probe::TestProbe;

pub fn composer(
    state: &AppState,
    matches: &[(String, String)],
    selected: usize,
    window: &Window,
    cx: &Context<AppState>,
) -> Div {
    let editor = state.editor.clone();
    let editor_for_click = editor.clone();
    // O handle precisa carregar o tab stop: com `track_focus` explícito, o
    // `tab_index` do Div não é aplicado pelo GPUI.
    let focus_handle = editor.read(cx).focus_handle.clone().tab_stop(true).tab_index(0);
    let focused = focus_handle.is_focused(window);
    let empty = (editor.read(cx).is_empty(cx) && state.pasted_images.is_empty()) || state.paste_loading;
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

    if !state.ext_widget.is_empty() {
        column = column.child(inset_block(state));
    }
    if state.paste_loading {
        column = column.child(loading_note(state));
    }
    for (index, (name, _)) in state.pasted_images.iter().enumerate() {
        column = column.child(image_note(index, name, cx));
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
    let surface = div()
        .mx(px(theme::CHAT_INSET))
        .rounded(theme::r_surface())
        .border_1()
        .border_color(if focused || streaming {
            theme::accent_edge()
        } else {
            theme::line_strong()
        })
        .bg(theme::surface_2())
        .shadow(if focused {
            theme::accent_glow(0.0)
        } else {
            theme::shadow_composer()
        })
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(theme::S2))
                    .px(px(theme::S4))
                    .pt(px(theme::S4))
                    .pb(px(theme::S4))
                    .child(
                        div()
                            .id("composer-input")
                            .test_probe(|| "composer-input".into(), || serde_json::json!({
                                "focused": focused,
                            }))
                            .flex_1()
                            .min_w(px(0.))
                            .min_h(px(theme::WRITING_HEIGHT))
                            .max_h(px(220.))
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
                            .child(editor.clone()),
                    ),
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
    // Modelo sem escada de raciocínio: diga isso em vez de mostrar vazio.
    if state.thinking_levels.is_empty() {
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
    use super::effort_step;

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
