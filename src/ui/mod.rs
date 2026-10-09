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
pub mod icons;
pub mod inspector;
pub mod metadata_view;
pub mod overlays;
pub mod transcript;
pub mod selectable;
pub mod copy_button;
pub mod window_frame;

actions!(
    dish_ui,
    [Dismiss, ModalSubmit, ToggleSidebar, NewSession, MenuUp, MenuDown, MenuAccept, CycleEffort, ToggleHelp, OpenModels, CloseWindow, ModelUp, ModelDown, ModelAccept, ModalUp, ModalDown]
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
            self.sidebar = viewport >= inspector::NARROW;
        }
        let narrow = viewport < inspector::NARROW;

        let (slash, slash_index) = self.slash_state(cx);
        let mut main_column = div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .child(conversation_header(
                self,
                cx,
                window_frame::client_side(window),
            ))
            .child(transcript::transcript(self, cx));

        if let Some(banner) = self.banner.clone() {
            main_column = main_column.child(banner_element(&banner, cx));
        }
        main_column = main_column.child(composer::composer(
            self,
            &slash,
            slash_index,
            window,
            cx,
        ));

        let mut root = div()
            .id("dish-root")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme::bg())
            .text_color(theme::text())
            .font_family(theme::FONT_UI)
            .text_size(px(theme::TEXT))
            .line_height(theme::body_line_height());
        // Em compositores sem moldura (GNOME/Wayland) esta camada redimensiona
        // a janela; as laterais ficam reservadas para as alças de resize e o
        // arrasto vive no título. Onde há decoração nativa ela não existe.
        if let Some(frame) = window_frame::layer(window) {
            root = root
                .child(frame)
                .pl(px(window_frame::BORDER))
                .pr(px(window_frame::BORDER));
        }
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
            .on_action(cx.listener(|state, _: &MenuAccept, _window, cx| {
                state.menu_accept(false, cx)
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
fn conversation_header(state: &AppState, cx: &Context<AppState>, draggable: bool) -> Div {
    let (glyph, color, label) = conversation_state(state);
    let folder = state
        .title_override
        .clone()
        .unwrap_or_else(|| crate::rpc::folder_label(&state.cwd));

    div()
        .flex_none()
        .flex()
        .flex_row()
        .items_start()
        .justify_between()
        .gap(px(theme::S4))
        .px(px(theme::S5))
        .pt(px(theme::S4))
        .pb(px(theme::S3))
        .border_b_1()
        .border_color(theme::line_soft())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(theme::S1))
                .min_w(px(0.))
                .when(draggable, |el| {
                    el.cursor(CursorStyle::Arrow).on_mouse_down(
                        MouseButton::Left,
                        |_event, window, _cx| window.start_window_move(),
                    )
                })
                .child(
                    div()
                        .truncate()
                        .text_size(px(theme::TITLE))
                        .font_weight(FontWeight::MEDIUM)
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
                                .font_family(theme::FONT_MONO)
                                .text_size(px(theme::TEXT_SM))
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
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(theme::S1 + 2.))
                        .px(px(theme::S3))
                        .py(px(theme::S1 + 3.))
                        .rounded(theme::r_control())
                        .bg(theme::surface_2())
                        .border_1()
                        .border_color(theme::line_soft())
                        .child(icon(glyph, 13., color))
                        .child(
                            div()
                                .text_size(px(theme::TEXT_SM))
                                .text_color(color)
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
                            theme::line()
                        } else {
                            theme::line_soft()
                        })
                        .when(state.sidebar, |el| el.bg(theme::surface_2()))
                        .cursor_pointer()
                        .hover(|style| style.bg(theme::hover()))
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
                )
                // Em compositores que deixam a moldura para o aplicativo
                // (GNOME/Wayland) não existe botão do sistema: estes são os
                // controles da janela.
                .when(draggable, |el| el.child(window_controls())),
        )
}

/// Minimizar e fechar, desenhados pelo próprio app quando o compositor não
/// fornece a moldura. O fechamento passa pela mesma ação do `ctrl-q`, para
/// respeitar a confirmação de conversa em execução.
fn window_controls() -> Div {
    let button = |id: &'static str, glyph: Icon| {
        div()
            .id(id)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .size(px(24.))
            .rounded(theme::r_control())
            .cursor_pointer()
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
            button("window-minimize", Icon::Minimize).on_click(
                |_: &ClickEvent, window: &mut Window, _cx: &mut App| window.minimize_window(),
            ),
        )
        .child(button("window-close", Icon::Close).on_click(
            |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                window.dispatch_action(Box::new(CloseWindow), cx)
            },
        ))
}

/// Estado em um único ponto: ícone + texto, nunca só cor.
fn conversation_state(state: &AppState) -> (Icon, Hsla, String) {
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

fn banner_element(banner: &crate::state::Banner, cx: &mut Context<AppState>) -> Div {
    let color = tone_color(banner.tone);
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .mx(px(24.))
        .mb(px(10.))
        .pl(px(12.))
        .pr(px(6.))
        .py(px(8.))
        .border_l_2()
        .border_color(color)
        .bg(theme::wash(color, 0.07))
        .text_size(px(theme::TEXT_SM))
        .text_color(theme::dim())
        .child(div().text_color(color).child("●"))
        .child(div().flex_1().min_w(px(0.)).child(SharedString::from(banner.text.clone())))
        .child(
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
                    .rounded(px((size / 4.0).max(1.0)))
                    .bg(color)
                    .opacity(0.35 + 0.65 * wave)
            },
        )
}

pub fn rule() -> Div {
    div().w_full().h(px(1.)).bg(theme::line_soft())
}





/// A progress rail. `ratio` is clamped to 0..1.
pub fn meter(ratio: f32, color: Hsla) -> Div {
    let ratio = ratio.clamp(0.0, 1.0);
    div()
        .w_full()
        .h(px(3.))
        .rounded(px(1.))
        .bg(theme::line())
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(ratio))
                .rounded(px(1.))
                .bg(color),
        )
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

/// A hollow marker, for the unselected half of a set.
pub fn ring(color: Hsla, size: f32) -> Div {
    div()
        .flex_none()
        .size(px(size))
        .rounded(px((size / 4.).max(1.)))
        .border_1()
        .border_color(color)
}

/// A key chip for the help sheet.
pub fn keycap_all(label: &str) -> Div {
    div()
        .flex_none()
        .px(px(7.))
        .py(px(3.))
        .rounded(px(3.))
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
