//! The window: a title bar, the conversation, the composer, a details rail, and
//! the overlays that float above them.
//!
//! The vocabulary here is deliberately small: hairlines, one accent, micro
//! labels, and monospace for anything a machine produced.

use gpui::prelude::*;
use gpui::*;

use crate::editor::SendPrompt;
use crate::markdown::MarkdownStyle;
use crate::state::AppState;
use crate::theme;
use crate::ui::icons::{Icon, icon};

pub mod composer;
pub mod motion;
pub mod icons;
pub mod inspector;
pub mod metadata_view;
pub mod test_probe;
pub mod overlays;
pub mod transcript;
pub mod selectable;
pub mod copy_button;
pub mod window_frame;
mod git_context_view;

actions!(
    dish_ui,
    [Dismiss, ModalSubmit, ToggleSidebar, NewSession, MenuUp, MenuDown, MenuAccept, PreviousFocus, CycleEffort, ToggleHelp, OpenModels, CloseWindow, ModelUp, ModelDown, ModelAccept, ModalUp, ModalDown]
);

impl Render for AppState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.visible && self.composer_focus_pending && self.modal.is_none() {
            self.composer_focus_pending = false;
            let focus = self.editor.read(cx).focus_handle.clone();
            focus.focus(window, cx);
        }
        if self.visible && self.model_menu && self.model_search_focus_pending {
            self.model_search_focus_pending = false;
            let focus = self.model_search.read(cx).focus_handle.clone();
            focus.focus(window, cx);
        }

        // Numa janela larga o inspetor abre sozinho; numa estreita fica fechado.
        // Depois disso a escolha é do usuário.
        let viewport = f32::from(window.bounds().size.width);
        if !self.inspector_initialized {
            self.inspector_initialized = true;
            self.sidebar = !inspector::uses_overlay(viewport);
        }
        let narrow = inspector::uses_overlay(viewport);

        let (slash, slash_index) = self.slash_state(cx);
        let mut main_column = div()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(conversation_header(self, cx))
            .child(transcript::transcript(self, cx));

        if let Some(banner) = self.banner.clone() {
            main_column = main_column.child(banner_element(self, &banner, cx));
        }
        main_column = main_column
            .child(composer::composer(self, &slash, slash_index, window, cx))
            .child(git_context_view::footer(&self.git_context));

        let mut root = div()
            .id("dish-root")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(theme::r_surface_inner())
            .bg(theme::bg())
            .text_color(theme::text())
            .font_family(theme::FONT_UI)
            .text_size(px(theme::TEXT))
            .line_height(theme::body_line_height());
        root = root
            .on_action(cx.listener(|state, _: &SendPrompt, _window, cx| state.submit(cx)))
            .on_action(cx.listener(|state, _: &ToggleHelp, _, cx| {
                if state.modal.is_some() { return; }
                state.help_open = !state.help_open;
                if state.help_open {
                    state.help_focus_pending = true;
                } else {
                    state.help_search.update(cx, |editor, cx| editor.clear(cx));
                }
                cx.notify();
            }))
            .on_action(cx.listener(|state, _: &OpenModels, window, cx| {
                if state.modal.is_some() { return; }
                if state.model_menu {
                    state.model_menu = false;
                    let focus = state.editor.read(cx).focus_handle.clone();
                    focus.focus(window, cx);
                } else {
                    state.open_model_menu(cx);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|state, _: &CycleEffort, _window, cx| {
                state.cycle_effort(cx);
            }))
            .on_action(cx.listener(|state, _: &NewSession, _window, cx| {
                state.new_session();
                cx.notify();
            }))
            .on_action(cx.listener(|state, _: &ToggleSidebar, _window, cx| {
                state.sidebar = !state.sidebar;
                cx.notify();
            }))
            .on_action(cx.listener(|state, _: &ModalSubmit, _window, cx| {
                overlays::submit_modal(state, cx);
            }))
            // Nos diálogos de seleção as setas andam pelo destaque; nos demais,
            // continuam movendo o caret do editor.
            .on_action(cx.listener(|state, _: &ModalUp, window, cx| {
                if state.modal_select_open() {
                    state.modal_move(-1, cx);
                } else {
                    state
                        .modal_editor
                        .update(cx, |editor, cx| editor.up(&crate::editor::Up, window, cx));
                }
            }))
            .on_action(cx.listener(|state, _: &ModalDown, window, cx| {
                if state.modal_select_open() {
                    state.modal_move(1, cx);
                } else {
                    state
                        .modal_editor
                        .update(cx, |editor, cx| editor.down(&crate::editor::Down, window, cx));
                }
            }))
            .on_action(
                cx.listener(|state, _: &MenuUp, window, cx| state.menu_move(-1, window, cx)),
            )
            .on_action(
                cx.listener(|state, _: &MenuDown, window, cx| state.menu_move(1, window, cx)),
            )
            .on_action(cx.listener(|state, _: &MenuAccept, window, cx| {
                // Com o menu `/` aberto, Tab completa; sem ele, anda com o foco.
                if !state.slash_state(cx).0.is_empty() {
                    state.menu_accept(false, cx);
                } else if state.focus_can_move() {
                    window.focus_next(cx);
                }
            }))
            .on_action(cx.listener(|state, _: &PreviousFocus, window, cx| {
                if state.focus_can_move() {
                    window.focus_prev(cx);
                }
            }))
            // O seletor de modelos responde às mesmas teclas, com estado próprio.
            .on_action(cx.listener(|state, _: &ModelUp, _, cx| state.model_move(-1, cx)))
            .on_action(cx.listener(|state, _: &ModelDown, _, cx| state.model_move(1, cx)))
            .on_action(cx.listener(|state, _: &ModelAccept, window, cx| {
                state.model_accept(window, cx)
            }))
            .on_action(cx.listener(|state, _: &crate::editor::Paste, window, cx| {
                state.paste(window, cx)
            }))
            .on_action(
                cx.listener(|state, _: &crate::editor::CopySelection, window, cx| {
                    state.copy_selection(window, cx)
                }),
            )
            .on_action(
                cx.listener(|state, _: &crate::editor::CutSelection, window, cx| {
                    state.cut_selection(window, cx)
                }),
            )
            .on_action(cx.listener(|state, _: &Dismiss, window, cx| {
                if state.modal.is_some() {
                    state.respond_modal(crate::state::ModalAnswer::Cancelled, cx);
                } else if state.help_open {
                    state.help_open = false;
                    state.help_search.update(cx, |editor, cx| editor.clear(cx));
                    let focus = state.editor.read(cx).focus_handle.clone();
                    focus.focus(window, cx);
                    cx.notify();
                } else if state.effort_menu {
                    state.effort_menu = false;
                    let focus = state.editor.read(cx).focus_handle.clone();
                    focus.focus(window, cx);
                    cx.notify();
                } else if state.model_menu {
                    state.model_menu = false;
                    let focus = state.editor.read(cx).focus_handle.clone();
                    focus.focus(window, cx);
                    cx.notify();
                } else if !state.slash_state(cx).0.is_empty() {
                    state.dismiss_slash(cx);
                } else if state.streaming {
                    state.abort(cx);
                } else if state.banner.is_some() {
                    // Aviso sem ação pendente: Esc dispensa, como o botão ×.
                    state.banner = None;
                    cx.notify();
                }
            }))
            // Clicar fora fecha o seletor de modelo; o painel e o próprio botão
            // seguram o evento para não fechar por engano.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|state, _: &MouseDownEvent, window, cx| {
                    if state.model_menu {
                        state.model_menu = false;
                        let focus = state.editor.read(cx).focus_handle.clone();
                        focus.focus(window, cx);
                        cx.notify();
                    }
                }),
            )
            .child(main_column);

        if self.sidebar {
            root = if narrow {
                root.child(inspector::overlay(self, cx))
            } else {
                root
                    .flex()
                    .flex_row()
                    .child(inspector::inspector(self, cx))
            };
        }

        root.children(overlays::toasts(self, cx))
            .children(overlays::modal(self, window, cx))
    }
}

// -------------------------------------------------------- cabeçalho da conversa

/// Título, projeto e estado num único ponto. Modelo e esforço ficam no
/// compositor; as métricas ficam no inspetor.
fn conversation_header(state: &AppState, cx: &Context<AppState>) -> Div {
    let (glyph, color, label) = conversation_state(state);
    let loading = state.is_loading_session();
    let busy = state.streaming || state.is_busy();
    let folder = state
        .title_override
        .clone()
        .unwrap_or_else(|| crate::rpc::folder_label(&state.cwd));

    div()
        .w_full()
        .min_w(px(0.))
        .flex_none()
        .min_h(px(theme::HEADER_HEIGHT))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(theme::S3))
        .px(px(theme::CHAT_INSET))
        .py(px(theme::S3))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(theme::S1))
                .min_w(px(0.))
                .flex_1()
                .child(
                    div()
                        .truncate()
                        .text_size(px(theme::TITLE))
                        .line_height(px(24.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(SharedString::from(state.display_title())),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(theme::S2))
                        .child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .font_family(theme::FONT_MONO)
                                .text_size(px(theme::TEXT_XS))
                                .text_color(theme::faint())
                                .child(SharedString::from(folder)),
                        )
                        .when(!state.messages.is_empty(), |el| {
                            el.child(crate::ui::dot(theme::line_strong(), 3.)).child(
                                div()
                                    .text_size(px(theme::TEXT_SM))
                                    .text_color(theme::faint())
                                    .child(SharedString::from(format!(
                                        "{} mensagens",
                                        state.messages.len()
                                    ))),
                            )
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(theme::S2))
                .flex_none()
                .child(
                    // One status, sourced from Pi. Motion only while busy or loading.
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(theme::S1 + 2.))
                        .px(px(theme::S2))
                        .py(px(theme::S1 + 3.))
                        .rounded(theme::r_control())
                        .border_1()
                        .border_color(gpui::transparent_black())
                        .when(loading, |el| {
                            el.bg(theme::wash(color, 0.12))
                                .border_color(theme::wash(color, 0.35))
                                .child(icons::spinner("header-loading", 13., color))
                        })
                        .when(!loading && busy, |el| {
                            el.bg(theme::wash(color, 0.12))
                                .border_color(theme::wash(color, 0.35))
                                .child(pulse("header-live", 6., color))
                        })
                        .when(!loading && !busy, |el| {
                            el.child(icon(glyph, 13., color))
                        })
                        .child(
                            div()
                                .max_w(px(124.))
                                .truncate()
                                .text_size(px(theme::TEXT_SM))
                                .text_color(if state.streaming || loading {
                                    color
                                } else {
                                    theme::dim()
                                })
                                .child(SharedString::from(label)),
                        ),
                )
                .child(
                    div()
                        .id("toggle-details")
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(theme::S1 + 2.))
                        .px(px(theme::S3))
                        .py(px(theme::S1 + 3.))
                        .rounded(theme::r_control())
                        .border_1()
                        .border_color(if state.sidebar {
                            theme::line_strong()
                        } else {
                            theme::line_soft()
                        })
                        .when(state.sidebar, |el| el.bg(theme::surface_2()))
                        .cursor_pointer()
                        .focusable()
                        .tab_index(0)
                        .hover(|style| style.bg(theme::hover()))
                        .active(|style| style.opacity(0.8))
                        .focus(|style| style.shadow(theme::focus_ring()))
                        .on_key_down(cx.listener(|state, event: &KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                state.sidebar = !state.sidebar;
                                cx.notify();
                                cx.stop_propagation();
                            } else if event.keystroke.key == "tab" {
                                if event.keystroke.modifiers.shift { window.focus_prev(cx); } else { window.focus_next(cx); }
                                cx.stop_propagation();
                            }
                        }))
                        .on_click(cx.listener(|state, _: &ClickEvent, _window, cx| {
                            state.sidebar = !state.sidebar;
                            cx.notify();
                        }))
                        .child(icon(Icon::Panel, 13., theme::faint()))
                        .child(
                            div()
                                .text_size(px(theme::TEXT_SM))
                                .text_color(theme::dim())
                                .child("Detalhes"),
                        ),
                ),
        )
}

/// Superfície da conversa exibida imediatamente ao clicar numa sessão salva,
/// enquanto o processo do Pi está sendo iniciado em background.
pub fn opening_session_view(cwd: &std::path::Path, title: Option<&str>) -> Stateful<Div> {
    let header_title = title
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or("Carregando sessão…");
    let folder = crate::rpc::folder_label(cwd);
    let color = theme::accent();

    div()
        .id("dish-root-opening")
        .relative()
        .size_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(theme::r_surface_inner())
        .bg(theme::bg())
        .text_color(theme::text())
        .font_family(theme::FONT_UI)
        .text_size(px(theme::TEXT))
        .line_height(theme::body_line_height())
        .child(
            div()
                .w_full()
                .min_w(px(0.))
                .flex_none()
                .min_h(px(theme::HEADER_HEIGHT))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(theme::S3))
                .px(px(theme::CHAT_INSET))
                .py(px(theme::S3))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(theme::S1))
                        .min_w(px(0.))
                        .flex_1()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(theme::TITLE))
                                .line_height(px(24.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::text())
                                .child(SharedString::from(header_title.to_string())),
                        )
                        .child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .font_family(theme::FONT_MONO)
                                .text_size(px(theme::TEXT_XS))
                                .text_color(theme::faint())
                                .child(SharedString::from(folder)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(theme::S1 + 2.))
                        .px(px(theme::S2))
                        .py(px(theme::S1 + 3.))
                        .rounded(theme::r_control())
                        .border_1()
                        .bg(theme::wash(color, 0.12))
                        .border_color(theme::wash(color, 0.35))
                        .child(icons::spinner("header-opening", 13., color))
                        .child(
                            div()
                                .text_size(px(theme::TEXT_SM))
                                .text_color(color)
                                .child("carregando…"),
                        ),
                ),
        )
        .child(transcript::loading_state(title))
}

/// Barra de título separada do conteúdo, apenas onde não há moldura nativa.
pub(crate) fn client_titlebar(window: &Window) -> Stateful<Div> {
    div()
        .id("window-titlebar")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(theme::S2))
        .px(px(theme::S3))
        .py(px(theme::S1))
        .border_b_1()
        .border_color(theme::line_soft())
        .child(
            div()
                .id("window-titlebar-drag")
                .flex_1()
                .h(px(theme::S5))
                .cursor(CursorStyle::Arrow)
                // O Windows trata esta área como HTCAPTION: arrastar,
                // duplo-clique para maximizar/restaurar e menu do sistema vêm
                // do SO. No Linux o compositor precisa do pedido explícito.
                .window_control_area(WindowControlArea::Drag)
                .when(!cfg!(target_os = "windows"), |el| {
                    el.on_mouse_down(MouseButton::Left, |_event, window, _cx| {
                        window.start_window_move();
                    })
                }),
        )
        .child(window_controls(window))
}

/// Minimizar, maximizar/restaurar e fechar, desenhados pelo próprio app quando
/// não há moldura nativa. O fechamento passa pela mesma ação do `ctrl-q`, para
/// respeitar a confirmação de conversa em execução.
fn window_controls(window: &Window) -> Div {
    let button = |id: &'static str, glyph: Icon, area: WindowControlArea| {
        div()
            .id(id)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .size(px(24.))
            .rounded(theme::r_control())
            .cursor_pointer()
            .window_control_area(area)
            .hover(|style| style.bg(theme::hover()))
            .child(icon(glyph, 13., theme::dim()))
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(theme::S1))
        .flex_none()
        .child(
            button("window-minimize", Icon::Minimize, WindowControlArea::Min).on_click(
                |_: &ClickEvent, window: &mut Window, _cx: &mut App| window.minimize_window(),
            ),
        )
        .child(
            button(
                "window-maximize",
                if window.is_maximized() {
                    Icon::Restore
                } else {
                    Icon::Maximize
                },
                WindowControlArea::Max,
            )
            .on_click(|_: &ClickEvent, window: &mut Window, _cx: &mut App| {
                window.zoom_window()
            }),
        )
        .child(
            button("window-close", Icon::Close, WindowControlArea::Close).on_click(
                |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    window.dispatch_action(Box::new(CloseWindow), cx)
                },
            ),
        )
}

/// Estado em um único ponto: ícone + texto, nunca só cor.
fn conversation_state(state: &AppState) -> (Icon, Hsla, String) {
    if state.is_loading_session() {
        return (Icon::Activity, theme::accent(), "carregando…".to_string());
    }
    if state.streaming || state.is_busy() {
        return (
            Icon::Activity,
            theme::running(),
            state
                .activity
                .clone()
                .unwrap_or_else(|| "executando".to_string()),
        );
    }
    if let Some(banner) = &state.banner {
        if banner.tone == crate::state::Tone::Error {
            return (Icon::Alert, theme::failure(), "falhou".to_string());
        }
    }
    (Icon::Check, theme::ok(), "pronta".to_string())
}


// ----------------------------------------------------------------- the rail

fn banner_element(
    state: &AppState,
    banner: &crate::state::Banner,
    cx: &mut Context<AppState>,
) -> Div {
    let color = tone_color(banner.tone);
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .mx(px(theme::CHAT_INSET))
        .mb(px(10.))
        .rounded(theme::r_control())
        .pl(px(12.))
        .pr(px(6.))
        .py(px(8.))
        .border_l_2()
        .border_color(color)
        .bg(theme::wash(color, 0.14))
        .text_size(px(theme::TEXT_SM))
        .text_color(theme::dim())
        .child(div().text_color(color).child("●"))
        .child(div().flex_1().min_w(px(0.)).child(SharedString::from(banner.text.clone())));
    if matches!(banner.action, Some(crate::state::BannerAction::ReopenConversation)) {
        let reason = if state.reopening {
            Some("Reabrindo a conversa…")
        } else {
            state.reopen_block_reason()
        };
        let enabled = state.can_reopen_conversation();
        if let Some(reason) = reason {
            row = row.child(
                div()
                    .flex_none()
                    .max_w(px(220.))
                    .text_size(px(theme::TEXT_XS))
                    .text_color(theme::faint())
                    .child(SharedString::from(reason)),
            );
        }
        row = row.child(
            div()
                .id("banner-reopen")
                .flex_none()
                .px(px(theme::S2))
                .py(px(theme::S1 + 1.))
                .rounded(theme::r_control())
                .bg(theme::wash(color, if enabled { 0.22 } else { 0.10 }))
                .text_size(px(theme::TEXT_SM))
                .text_color(if enabled { color } else { theme::faint() })
                .focusable()
                .tab_index(0)
                .when(enabled, |el| {
                    el.cursor_pointer()
                        .hover(|style| style.bg(theme::wash(color, 0.30)))
                        .focus(|style| style.shadow(theme::focus_ring()))
                })
                .on_click(cx.listener(|state, _: &ClickEvent, _, cx| {
                    if state.can_reopen_conversation() {
                        state.request_reopen_conversation(cx);
                    }
                }))
                .on_key_down(cx.listener(|state, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "tab" {
                        if event.keystroke.modifiers.shift {
                            window.focus_prev(cx);
                        } else {
                            window.focus_next(cx);
                        }
                        cx.stop_propagation();
                    } else if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        if state.can_reopen_conversation() {
                            state.request_reopen_conversation(cx);
                        }
                        cx.stop_propagation();
                    }
                }))
                .child("Reabrir esta conversa"),
        );
    }
    row.child(
        div()
            .id("banner-dismiss")
            .size(px(22.))
            .rounded(theme::r_control())
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .focusable()
            .tab_index(0)
            .hover(|style| style.bg(theme::hover()))
            .focus(|style| style.bg(theme::hover()))
            .tooltip(|_, cx| cx.new(|_| copy_button::TextTooltip("Dispensar aviso (Esc)".into())).into())
            .on_click(cx.listener(|state, _: &ClickEvent, _, cx| {
                state.banner = None;
                cx.notify();
            }))
            .on_key_down(cx.listener(|state, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                } else if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    state.banner = None;
                    cx.notify();
                }
            }))
            .child(icon(Icon::Close, 12., color)),
    )
}

// ------------------------------------------------------------- vocabulary

pub fn tone_color(tone: crate::state::Tone) -> Hsla {
    use crate::state::Tone;
    match tone {
        Tone::Info => theme::accent(),
        Tone::Success => theme::ok(),
        Tone::Warning => theme::warn(),
        Tone::Error => theme::danger(),
    }
}




/// A square marker. Reads as a valve light rather than a bullet.
pub fn dot(color: Hsla, size: f32) -> Div {
    div()
        .flex_none()
        .size(px(size))
        .rounded(px((size / 4.0).max(1.0)))
        .bg(color)
}


/// A marker that breathes, for live work.
pub fn pulse(id: impl Into<ElementId>, size: f32, color: Hsla) -> impl IntoElement {
    let id = id.into();
    div()
        .flex_none()
        .size(px(size))
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_millis(1400)).repeat(),
            move |_element, delta| {
                let wave = (delta * std::f32::consts::PI).sin().abs();
                div()
                    .size(px(size))
                    .rounded(px(size / 2.0))
                    .bg(color)
                    .opacity(0.65 + 0.35 * wave)
            },
        )
}

pub fn rule() -> Div {
    div().w_full().h(px(1.)).bg(theme::line_soft())
}





/// A progress rail. `ratio` is clamped to 0..1. The fill is a short gradient
/// so the head of the bar reads as "where we are now".
pub fn meter(ratio: f32, color: Hsla) -> Div {
    let ratio = ratio.clamp(0.0, 1.0);
    div()
        .w_full()
        .h(px(6.))
        .rounded(px(3.))
        .bg(theme::inset())
        .border_1()
        .border_color(theme::line_soft())
        .child(
            div()
                .h_full()
                .w(relative(ratio))
                .rounded(px(3.))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(theme::wash(color, 0.45), 0.0),
                    linear_color_stop(color, 1.0),
                )),
        )
}

/// The sliding knob of an on/off switch. `on` picks the travel direction and
/// the animation restarts only when the id (which carries the state) changes,
/// so a switch at rest costs nothing. Reduced motion is honoured by GPUI.
///
/// The track must be `relative()`, 34×18 with a 1px border.
pub fn switch_knob(id: impl Into<ElementId>, on: bool, color: Hsla) -> impl IntoElement {
    div()
        .absolute()
        .top(px(2.))
        .size(px(12.))
        .rounded(px(6.))
        .bg(color)
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_millis(motion::ENTER_MS))
                .with_easing(motion::settle),
            move |knob, progress| {
                let place = if on { progress } else { 1. - progress };
                knob.left(px(2. + 16. * place))
            },
        )
}

/// Resting position of a mixed (indeterminate) knob: centred on the track.
pub fn switch_knob_mixed(color: Hsla) -> Div {
    div().absolute().top(px(2.)).left(px(10.)).size(px(12.)).rounded(px(6.)).bg(color)
}

/// A stable element id derived from content, for scroll containers that have no
/// natural identity (code blocks, tool output).
/// A 11px label, for the few places that still need one.
pub fn micro(label: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::TEXT_XS))
        .text_color(theme::faint())
        .child(label.into())
}

/// A key chip for the help sheet.
pub fn keycap_all(label: &str) -> Div {
    div()
        .flex_none()
        .px(px(7.))
        .py(px(3.))
        .rounded(px(4.))
        .bg(theme::inset())
        .border_1()
        .border_color(theme::line())
        .font_family(theme::FONT_MONO)
        .text_size(px(theme::TEXT_XS))
        .text_color(theme::dim())
        .child(SharedString::from(label.to_string()))
}

pub fn content_id(prefix: &str, body: &str) -> ElementId {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in body.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    ElementId::NamedInteger(SharedString::from(prefix.to_string()), hash)
}

/// The Markdown style used for assistant output.
pub fn markdown_style() -> MarkdownStyle {
    MarkdownStyle {
        font: font(theme::FONT_UI),
        color: theme::text(),
        accent: theme::accent(),
        dim: theme::dim(),
        code_color: theme::text(),
        code_bg: theme::inset(),
    }
}

/// Estado de uma ferramenta: ícone + cor + palavra. Nunca só cor.
pub fn status_word(status: crate::state::ToolStatus) -> (icons::Icon, Hsla, &'static str) {
    use crate::state::ToolStatus;
    use icons::Icon;
    match status {
        ToolStatus::Pending => (Icon::Clock, theme::faint(), "não iniciada"),
        ToolStatus::Running => (Icon::Activity, theme::running(), "rodando"),
        ToolStatus::Ok => (Icon::Check, theme::ok(), "concluído"),
        ToolStatus::Failed => (Icon::Alert, theme::failure(), "falhou"),
    }
}
