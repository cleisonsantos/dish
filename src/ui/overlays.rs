//! Overlays: toasts, the shortcut sheet, and dialogs requested by extensions.
//! These are the only surfaces in the app that float, so they are the only ones
//! that cast a shadow.

use gpui::prelude::*;
use gpui::*;

use crate::editor::standard_actions;
use crate::state::{AppState, ModalAnswer, ModalKind};
use crate::theme;
use crate::ui::icons::{icon, Icon};
use crate::ui::{dot, micro, tone_color};

pub fn toasts(state: &AppState, _cx: &Context<AppState>) -> Vec<AnyElement> {
    if state.toasts.is_empty() {
        return Vec::new();
    }
    let stack = div()
        .absolute()
        .bottom(px(104.))
        .right(px(24.))
        .flex()
        .flex_col()
        .items_end()
        .gap(px(6.))
        .children(state.toasts.iter().map(|toast| {
            let color = tone_color(toast.tone);
            div()
                .id(SharedString::from(format!("toast-{}", toast.id)))
                .max_w(px(420.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .pl(px(11.))
                .pr(px(13.))
                .py(px(8.))
                .border_l_2()
                .border_color(color)
                .rounded(px(3.))
                .bg(theme::surface_2())
                .text_size(px(theme::TEXT_SM))
                .text_color(theme::dim())
                .child(dot(color, 5.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(SharedString::from(toast.text.clone())),
                )
                .into_any_element()
        }));
    vec![stack.into_any_element()]
}

/// The model picker, as a floating panel rather than a list nested inside the
/// scrolling rail — nested scroll areas fight each other.
/// Seletor de modelo: vive acima do compositor, como o menu de esforço.
pub fn model_picker(state: &AppState, cx: &Context<AppState>) -> Stateful<Div> {
    let weak = cx.entity().downgrade();
    let current = state.model.clone();

    // O modelo em uso vem primeiro; o teclado percorre a mesma ordem.
    let query = state.model_search.read(cx).text(cx);
    let order = state.filtered_models(&query);
    let count = order.len();
    let highlighted = state.model_highlight(&query);

    let rows = order.iter().enumerate().map(|(index, model_index)| {
        let model = &state.models[*model_index];
        let selected = current
            .as_ref()
            .map(|current| current.id == model.id && current.provider == model.provider)
            .unwrap_or(false);
        let weak = weak.clone();
        let click = model.clone();
        div()
            .id(SharedString::from(format!("model-option-{index}")))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(3.))
            .cursor_pointer()
            .when(highlighted == Some(index), |el| el.bg(theme::hover()))
            .when(highlighted != Some(index), |el| {
                el.hover(|style| style.bg(theme::hover()))
            })
            .on_click(move |_event: &ClickEvent, window: &mut Window, cx: &mut App| {
                weak.update(cx, |state, cx| {
                    state.set_model(&click);
                    let focus = state.editor.read(cx).focus_handle.clone();
                    focus.focus(window, cx);
                    cx.notify();
                })
                .ok();
            })
            .child(div().flex_none().w(px(8.)).when(selected, |el| {
                el.child(dot(theme::accent(), 5.0))
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        div()
                            .truncate()
                            .font_family(theme::FONT_MONO)
                            .text_size(px(theme::TEXT_SM))
                            .when(selected, |el| el.text_color(theme::text()))
                            .when(!selected, |el| el.text_color(theme::dim()))
                            .child(SharedString::from(model.id.clone())),
                    )
                    .child(
                        div()
                            .font_family(theme::FONT_MONO)
                            .text_size(px(theme::TEXT_MICRO))
                            .text_color(theme::faint())
                            .child(SharedString::from(model.provider.clone())),
                    ),
            )
            .when(model.context_window > 0, |el| {
                el.child(
                    div()
                        .flex_none()
                        .font_family(theme::FONT_MONO)
                        .text_size(px(theme::TEXT_MICRO))
                        .text_color(theme::faint())
                        .child(SharedString::from(format!(
                            "{} ctx",
                            crate::state::format_tokens(model.context_window as f64)
                        ))),
                )
            })
            .into_any_element()
    });

    let panel = div()
        .id("model-picker")
        // Clicar dentro não fecha; o clique fora chega ao root.
        .on_mouse_down(MouseButton::Left, |_event, _window, cx| cx.stop_propagation())
        .mx(px(theme::S5))
        .flex()
        .flex_col()
        .rounded(theme::r_surface())
        .border_1()
        .border_color(theme::line_strong())
        .bg(theme::surface())
        .shadow(theme::shadow_overlay())
        .overflow_hidden()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(theme::S2))
                .px(px(12.))
                .py(px(9.))
                .child(micro("SELECT MODEL"))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(theme::S2))
                        .child(micro(format!("{count} / {} available", state.models.len())))
                        .child(
                            div()
                                .id("model-picker-close")
                                .size(px(22.))
                                .rounded(theme::r_control())
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .hover(|style| style.bg(theme::hover()))
                                .on_click(cx.listener(|state, _: &ClickEvent, window, cx| {
                                    state.model_menu = false;
                                    let focus = state.editor.read(cx).focus_handle.clone();
                                    focus.focus(window, cx);
                                    cx.notify();
                                }))
                                .child(icon(Icon::Close, 12., theme::faint())),
                        ),
                ),
        )
        .child({
            let editor = state.model_search.clone();
            let focus = editor.read(cx).focus_handle.clone();
            div()
                .id("model-search")
                .key_context("DishModelSearch")
                .track_focus(&focus)
                .mx(px(12.))
                .mb(px(9.))
                .px(px(10.))
                .py(px(8.))
                .h(px(38.))
                .overflow_hidden()
                .rounded(px(4.))
                .border_1()
                .border_color(theme::accent_edge())
                .bg(theme::inset())
                .cursor_text()
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    focus.focus(window, cx);
                    editor.update(cx, |editor, cx| {
                        editor.mouse_press(event.position, event.click_count.max(1), event.modifiers.shift, cx);
                    });
                })
                .map(standard_actions(state.model_search.clone()))
                .child(state.model_search.clone())
        })
        .child(crate::ui::rule())
        .child(
            div()
                .id("model-picker-list")
                .flex()
                .flex_col()
                .max_h(px(300.))
                .overflow_y_scroll()
                .track_scroll(&state.model_scroll)
                .p(px(6.))
                .when(count == 0, |el| {
                    el.child(div().p(px(12.)).child(micro("No models found")))
                })
                .children(rows),
        );

    panel
}

pub fn modal(state: &AppState, window: &mut Window, cx: &mut Context<AppState>) -> Vec<AnyElement> {
    let Some(modal) = state.modal.as_ref() else {
        return Vec::new();
    };

    if state.visible && modal.wants_input {
        let focus = state.modal_editor.read(cx).focus_handle.clone();
        if !focus.is_focused(window) {
            focus.focus(window, cx);
        }
    }

    let body = match modal.kind {
        ModalKind::Select => select_body(state, cx),
        ModalKind::Confirm => confirm_body(cx),
        ModalKind::Input | ModalKind::Editor => input_body(state, modal.kind, cx),
    };
    let wide = modal.kind == ModalKind::Editor;

    let card = div()
        .id("modal-card")
        .w(px(if wide { 640. } else { 460. }))
        .flex()
        .flex_col()
        .gap(px(14.))
        .rounded(px(6.))
        .border_1()
        .border_color(theme::line_strong())
        .bg(theme::surface())
        .shadow(theme::shadow_overlay())
        .px(px(20.))
        .py(px(18.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(theme::text())
                        .child(SharedString::from(modal.title.clone())),
                )
                .when_some(modal.message.clone(), |el, message| {
                    el.child(
                        div()
                            .text_size(px(theme::TEXT_SM))
                            .line_height(px(19.))
                            .text_color(theme::faint())
                            .child(SharedString::from(message)),
                    )
                }),
        )
        .child(body);

    let scrim = div()
        .id("modal-scrim")
        .absolute()
        .inset(px(0.))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::wash(theme::bg(), 0.78))
        .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {})
        .child(card);

    vec![scrim.into_any_element()]
}

fn select_body(state: &AppState, cx: &Context<AppState>) -> AnyElement {
    let weak = cx.entity().downgrade();
    let options = state
        .modal
        .as_ref()
        .map(|modal| modal.options.clone())
        .unwrap_or_default();

    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .children(options.into_iter().enumerate().map(|(index, option)| {
            let weak = weak.clone();
            let value = option.clone();
            div()
                .id(SharedString::from(format!("option-{index}")))
                .px(px(11.))
                .py(px(8.))
                .rounded(px(3.))
                .border_1()
                .border_color(theme::line_soft())
                .text_size(px(theme::TEXT))
                .text_color(theme::dim())
                .cursor_pointer()
                .when(index == state.modal_highlight(), |el| {
                    el.bg(theme::hover())
                        .border_color(theme::accent_edge())
                        .text_color(theme::text())
                })
                .hover(|style| {
                    style
                        .bg(theme::hover())
                        .border_color(theme::accent_edge())
                        .text_color(theme::text())
                })
                .child(SharedString::from(option))
                .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                    weak.update(cx, |state, cx| {
                        state.respond_modal(ModalAnswer::Value(serde_json::json!(value)), cx)
                    })
                    .ok();
                })
                .into_any_element()
        }))
        .into_any_element()
}

fn confirm_body(cx: &Context<AppState>) -> AnyElement {
    let weak = cx.entity().downgrade();
    let yes = {
        let weak = weak.clone();
        div()
            .id("confirm-yes")
            .px(px(14.))
            .py(px(7.))
            .rounded(px(4.))
            .bg(theme::accent())
            .text_size(px(theme::TEXT_SM))
            .text_color(theme::on_accent())
            .cursor_pointer()
            .hover(|style| style.bg(theme::accent_hover()))
            .child("confirm")
            .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                weak.update(cx, |state, cx| {
                    state.respond_modal(ModalAnswer::Confirmed(true), cx)
                })
                .ok();
            })
    };
    let no = {
        let weak = weak.clone();
        div()
            .id("confirm-no")
            .px(px(14.))
            .py(px(7.))
            .rounded(px(4.))
            .border_1()
            .border_color(theme::line_strong())
            .text_size(px(theme::TEXT_SM))
            .text_color(theme::dim())
            .cursor_pointer()
            .hover(|style| style.bg(theme::hover()).text_color(theme::text()))
            .child("cancel")
            .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                weak.update(cx, |state, cx| {
                    state.respond_modal(ModalAnswer::Confirmed(false), cx)
                })
                .ok();
            })
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_end()
        .gap(px(8.))
        .child(no)
        .child(yes)
        .into_any_element()
}

fn input_body(state: &AppState, kind: ModalKind, cx: &Context<AppState>) -> AnyElement {
    let weak = cx.entity().downgrade();
    let editor = state.modal_editor.clone();
    let focus_handle = editor.read(cx).focus_handle.clone();
    let rows = if kind == ModalKind::Editor { 8 } else { 1 };
    let height = px(19.0 * rows as f32 + 20.0);

    let editor_for_click = editor.clone();
    let focus_for_click = focus_handle.clone();
    let field = div()
        .id("modal-input")
        .key_context("DishModal")
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
        .w_full()
        .h(height)
        .max_h(px(280.))
        .overflow_y_scroll()
        .rounded(px(4.))
        .border_1()
        .border_color(theme::accent_edge())
        .bg(theme::inset())
        .px(px(12.))
        .py(px(9.))
        .map(standard_actions(editor.clone()))
        .child(editor.clone());

    let cancel = {
        let weak = weak.clone();
        div()
            .id("modal-cancel")
            .px(px(12.))
            .py(px(6.))
            .rounded(px(4.))
            .border_1()
            .border_color(theme::line_strong())
            .text_size(px(theme::TEXT_SM))
            .text_color(theme::faint())
            .cursor_pointer()
            .hover(|style| style.bg(theme::hover()).text_color(theme::text()))
            .child("cancel")
            .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                weak.update(cx, |state, cx| {
                    state.respond_modal(ModalAnswer::Cancelled, cx)
                })
                .ok();
            })
    };
    let submit = {
        let weak = weak.clone();
        div()
            .id("modal-submit")
            .px(px(14.))
            .py(px(6.))
            .rounded(px(4.))
            .bg(theme::accent())
            .text_size(px(theme::TEXT_SM))
            .text_color(theme::on_accent())
            .cursor_pointer()
            .hover(|style| style.bg(theme::accent_hover()))
            .child("submit")
            .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                weak.update(cx, submit_modal).ok();
            })
    };

    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(field)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .font_family(theme::FONT_MONO)
                        .text_size(px(theme::TEXT_MICRO))
                        .text_color(theme::faint())
                        .child("⏎ submit   ⇧⏎ newline   esc cancel"),
                )
                .child(div().flex().flex_row().gap(px(8.)).child(cancel).child(submit)),
        )
        .into_any_element()
}

/// Read the modal editor and answer the pending dialog.
pub fn submit_modal(state: &mut AppState, cx: &mut Context<AppState>) {
    // Um diálogo de seleção responde com a linha destacada, não com texto.
    if state.modal_select_open() {
        state.submit_modal_select(cx);
        return;
    }
    let text = state.modal_editor.read(cx).text(cx);
    state.respond_modal(ModalAnswer::Value(serde_json::json!(text)), cx);
}

pub fn keyboard_shortcuts() -> Vec<(&'static str, &'static str)> {
    let app_keys: [(&'static str, &'static str); 20] = [
        ("⏎", "send the prompt"),
        ("⇧⏎", "insert a newline"),
        ("esc", "dismiss a menu, or stop the run"),
        ("↑ ↓", "choose in the slash menu"),
        ("⇥", "complete the highlighted command"),
        ("↑ ↓ ⏎", "pick a model in the model picker"),
        ("↑ ↓ ⏎", "open a session from the session search"),
        ("← →", "collapse or expand that session's project"),
        ("↑ ↓ ⏎", "choose an option in a Pi dialog"),
        ("ctrl-n", "start a new session"),
        ("ctrl-tab", "next open conversation"),
        ("ctrl-⇧tab", "previous open conversation"),
        ("ctrl-w", "close conversation (confirms if running)"),
        ("ctrl-k", "focus session search"),
        ("ctrl-l", "return to prompt or active dialog"),
        ("ctrl-⇧m", "open model picker"),
        ("F1", "open keyboard shortcuts in settings"),
        ("ctrl-⇧e", "cycle the model's effort level"),
        ("ctrl-⇧b", "show or hide project sessions"),
        ("ctrl-q", "close the app"),
    ];
    let editing: [(&'static str, &'static str); 13] = [
        ("← →", "move by character"),
        ("⇧ ← →", "select by character"),
        ("ctrl ← →", "move by word"),
        ("home end", "line start and end"),
        ("ctrl-a", "select everything"),
        ("ctrl-c", "copy"),
        ("ctrl-x", "cut"),
        ("ctrl-v", "paste"),
        ("ctrl-z", "undo"),
        ("ctrl-⇧z", "redo"),
        ("ctrl-Bksp", "delete the previous word"),
        ("click, drag", "place the caret, select"),
        ("2x, 3x", "select a word, a line"),
    ];

    app_keys.into_iter().chain(editing).collect()
}

#[cfg(test)]
mod model_search_tests {
    use crate::state::model_matches;

    #[test]
    fn matches_model_and_provider_case_insensitively() {
        let model = crate::state::ModelInfo {
            id: "gpt-4.1".into(),
            provider: "OpenAI".into(),
            ..Default::default()
        };
        for query in ["", "  ", "GPT", "openai", " OPENAI  4.1 "] {
            assert!(model_matches(&model, query), "{query}");
        }
        for query in ["claude", "openai missing"] {
            assert!(!model_matches(&model, query), "{query}");
        }
    }
}
