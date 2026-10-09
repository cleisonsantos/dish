//! The composer: a roomy writing surface with the model and reasoning controls
//! where they belong — next to the thing they influence — and two unambiguous
//! actions. No permanent shortcut row, no character counter.

use gpui::prelude::*;
use gpui::*;

use crate::editor::standard_actions;
use crate::state::AppState;
use crate::theme;
use crate::ui::icons::{Icon, icon};

pub fn composer(
    state: &AppState,
    matches: &[(String, String)],
    selected: usize,
    window: &Window,
    cx: &Context<AppState>,
) -> Div {
    let editor = state.editor.clone();
    let editor_for_click = editor.clone();
    let focus_handle = editor.read(cx).focus_handle.clone();
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
        column = column.child(div().px(px(theme::S5)).text_color(theme::dim()).child("Preparando colagem…"));
    }
    for (index, (name, _)) in state.pasted_images.iter().enumerate() {
        column = column.child(div().px(px(theme::S5)).flex().gap(px(12.))
            .child(SharedString::from(format!("Imagem: {name}")))
            .child(div().id(("remove-pasted-image", index)).cursor_pointer()
                .text_color(theme::dim()).child("remover")
                .on_click(cx.listener(move |state, _: &ClickEvent, _, cx| {
                    if index < state.pasted_images.len() {
                        state.pasted_images.remove(index);
                        cx.notify();
                    }
                }))));
    }
    if has_queue {
        column = column.child(queue(state, cx));
    }
    if !matches.is_empty() {
        column = column.child(slash_menu(matches, selected, cx));
    } else if state.model_menu {
        column = column.child(crate::ui::overlays::model_picker(state, cx));
    } else if state.effort_menu {
        column = column.child(effort_menu(state, cx));
    }

    column.child(
        div()
            .mx(px(theme::S5))
            .rounded(theme::r_surface())
            .border_1()
            // O accent fica no foco: é onde ele significa "esta é a sua vez".
            .border_color(if focused {
                theme::accent_edge()
            } else {
                theme::line_strong()
            })
            .bg(theme::surface_2())
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
                    .pb(px(theme::S2))
                    .child(
                        div()
                            .id("composer-input")
                            .flex_1()
                            .min_w(px(0.))
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
                    .gap(px(theme::S3))
                    .px(px(theme::S3))
                    .pb(px(theme::S3))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S1))
                            .min_w(px(0.))
                            .child(model_control(state, cx))
                            .child(effort_control(state, cx)),
                    )
                    .child(actions(streaming, empty, cx)),
            ),
    )
}

// ------------------------------------------------------------------ controles

fn control_shell(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(theme::S2))
        .px(px(theme::S2 + 2.))
        .py(px(theme::S1 + 2.))
        .rounded(theme::r_control())
        .cursor_pointer()
        .hover(|style| style.bg(theme::hover()))
}

fn model_control(state: &AppState, cx: &Context<AppState>) -> AnyElement {
    let label = state
        .model
        .as_ref()
        .map(|model| model.id.clone())
        .unwrap_or_else(|| "modelo".to_string());
    control_shell("composer-model")
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
            .gap(px(theme::S2))
            .px(px(theme::S2 + 2.))
            .py(px(theme::S1 + 2.))
            .rounded(theme::r_control())
            .child(crate::ui::dot(theme::faint(), 8.))
            .child(
                div()
                    .text_size(px(theme::TEXT_SM))
                    .text_color(theme::faint())
                    .child(if state.loading {
                        "esforço…"
                    } else {
                        "sem raciocínio"
                    }),
            )
            .into_any_element();
    }
    let color = theme::effort_color(&level, &state.thinking_levels);
    control_shell("composer-effort")
        .on_click(cx.listener(|state, _: &ClickEvent, _window, cx| {
            state.effort_menu = !state.effort_menu;
            cx.notify();
        }))
        .child(crate::ui::dot(color, 8.))
        .child(
            div()
                .text_size(px(theme::TEXT_SM))
                .text_color(theme::dim())
                .child(crate::state::effort_label(&level)),
        )
        .child(icon(Icon::ChevronDown, 12., theme::faint()))
        .into_any_element()
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
            .size(px(34.))
            .rounded(px(10.))
            .border_1()
            .border_color(theme::line())
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|style| style.bg(theme::wash(theme::failure(), 0.14)))
            .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                weak.update(cx, |state, cx| state.abort(cx)).ok();
            })
            .child(icon(Icon::Stop, 13., theme::failure()))
    };

    let send = {
        let weak = weak.clone();
        div()
            .id("send-button")
            .flex_none()
            .size(px(34.))
            .rounded(px(10.))
            .flex()
            .items_center()
            .justify_center()
            .when(empty, |el| {
                el.bg(theme::line()).child(icon(Icon::Send, 15., theme::faint()))
            })
            .when(!empty, |el| {
                el.bg(theme::text())
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::dim()))
                    .child(icon(Icon::Send, 15., theme::on_light()))
            })
            .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                weak.update(cx, |state, cx| state.submit(cx)).ok();
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

// ---------------------------------------------------------------------- fila

fn queue(state: &AppState, cx: &Context<AppState>) -> Div {
    let total = state.steering_queue.len() + state.follow_up_queue.len();
    let open = state.queue_open;

    let mut column = div()
        .mx(px(theme::S5))
        .flex()
        .flex_col()
        .gap(px(theme::S2))
        .child(
            div()
                .id("queue-header")
                .flex()
                .flex_row()
                .items_center()
                .gap(px(theme::S2))
                .cursor_pointer()
                .hover(|style| style.text_color(theme::text()))
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
                            state.clear_queue();
                            state.steering_queue.clear();
                            state.follow_up_queue.clear();
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
    let levels: Vec<String> = if state.thinking_levels.is_empty() {
        vec!["off".into(), "low".into(), "medium".into(), "high".into()]
    } else {
        state.thinking_levels.clone()
    };

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
            .rounded(theme::r_control())
            .when(!selected, |el| {
                el.hover(|style| style.bg(theme::hover()))
            })
            .on_click(cx.listener(move |state, _: &ClickEvent, _window, cx| {
                state.set_thinking_level(&level_for_click);
                state.effort_menu = false;
                cx.notify();
            }))
            .child(
                div()
                    .flex_none()
                    .w(px(8.))
                    .when(selected, |el| el.child(crate::ui::dot(color, 7.))),
            )
            .child(
                div()
                    .text_size(px(theme::TEXT_SM))
                    .text_color(if selected { color } else { theme::dim() })
                    .child(SharedString::from(crate::state::effort_label(level))),
            )
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
        .mx(px(theme::S5))
        .rounded(theme::r_surface())
        .border_1()
        .border_color(theme::line_strong())
        .bg(theme::surface())
        .shadow(theme::shadow_overlay())
        .p(px(theme::S1))
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
                .child("o accent da janela acompanha este nível"),
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
                .rounded(theme::r_control())
                .when(is_selected, |el| el.bg(theme::hover()))
                .when(!is_selected, |el| {
                    el.hover(|style| style.bg(theme::hover()))
                })
                .child(
                    div()
                        .flex_none()
                        .w(px(8.))
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
        .mx(px(theme::S5))
        .rounded(theme::r_surface())
        .border_1()
        .border_color(theme::line_strong())
        .bg(theme::surface())
        .shadow(theme::shadow_overlay())
        .p(px(theme::S1))
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
    div()
        .mx(px(theme::S5))
        .pl(px(theme::S3))
        .py(px(theme::S2))
        .border_l_2()
        .border_color(theme::line_strong())
        .font_family(theme::FONT_MONO)
        .text_size(px(theme::TEXT_XS))
        .text_color(theme::dim())
        .children(
            state
                .ext_widget
                .iter()
                .map(|line| SharedString::from(line.clone())),
        )
}
