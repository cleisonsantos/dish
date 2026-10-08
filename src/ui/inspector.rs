//! The inspector: what the conversation does not need to say twice.
//!
//! Session, context and cost live here instead of being repeated in a header, a
//! footer and a rail. It also carries the two things that only make sense on
//! demand: the files the current turn touched, and the session's timeline.
//!
//! Closed by default on small windows, opened automatically on wide ones, and
//! always overlaid rather than squeezing the prose when there is no room.

use gpui::prelude::*;
use gpui::*;

use crate::state::{AppState, ToolStatus, TurnState};
use crate::theme;
use crate::ui::icons::{Icon, icon};
use crate::ui::meter;

/// Abaixo disto o inspetor abre como sobreposição.
pub const NARROW: f32 = 1120.;
pub const WIDTH: f32 = 316.;

pub fn inspector(state: &AppState, cx: &Context<AppState>) -> Stateful<Div> {
    div()
        .id("inspector")
        .flex_none()
        .w(px(WIDTH))
        .h_full()
        .flex()
        .flex_col()
        .bg(theme::surface())
        .border_l_1()
        .border_color(theme::line_soft())
        .child(header(state, cx))
        .child(
            div()
                .id("inspector-body")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .px(px(theme::S4))
                .pb(px(theme::S6))
                .flex()
                .flex_col()
                .gap(px(theme::S5))
                .pt(px(theme::S4))
                .child(session_section(state))
                .child(files_section(state))
                .child(context_section(state))
                .child(timeline_section(state))
                .child(actions_section(state, cx)),
        )
}

/// Sobreposição para janelas estreitas: a conversa mantém a largura.
pub fn overlay(state: &AppState, cx: &Context<AppState>) -> Stateful<Div> {
    div()
        .id("inspector-overlay")
        .absolute()
        .inset(px(0.))
        .flex()
        .flex_row()
        .justify_end()
        .bg(theme::wash(theme::bg(), 0.72))
        .on_mouse_down(MouseButton::Left, cx.listener(|state, _, _, cx| {
            state.sidebar = false;
            cx.notify();
        }))
        .child(
            div()
                .w(px(WIDTH))
                .h_full()
                .bg(theme::surface())
                .border_l_1()
                .border_color(theme::line())
                .shadow(theme::shadow_overlay())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex()
                .flex_col()
                .child(header(state, cx))
                .child(
                    div()
                        .id("inspector-body-narrow")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .px(px(theme::S4))
                        .pb(px(theme::S6))
                        .flex()
                        .flex_col()
                        .gap(px(theme::S5))
                        .pt(px(theme::S4))
                        .child(session_section(state))
                        .child(files_section(state))
                        .child(context_section(state))
                        .child(timeline_section(state))
                        .child(actions_section(state, cx)),
                ),
        )
}

fn header(_state: &AppState, cx: &Context<AppState>) -> Div {
    div()
        .flex_none()
        .h(px(52.))
        .pl(px(theme::S4))
        .pr(px(theme::S2))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(theme::line_soft())
        .child(
            div()
                .text_size(px(theme::TEXT))
                .font_weight(FontWeight::MEDIUM)
                .child("Detalhes"),
        )
        .child(
            div()
                .id("inspector-close")
                .size(px(28.))
                .rounded(theme::r_control())
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(theme::hover()))
                .on_click(cx.listener(|state, _: &ClickEvent, _window, cx| {
                    state.sidebar = false;
                    cx.notify();
                }))
                .child(icon(Icon::Close, 13., theme::faint())),
        )
}

fn heading(label: &str) -> Div {
    div()
        .text_size(px(theme::TEXT_XS))
        .text_color(theme::faint())
        .child(SharedString::from(label.to_string()))
}

fn pair(label: &str, value: String, mono: bool) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_baseline()
        .gap(px(theme::S3))
        .child(
            div()
                .flex_none()
                .w(px(74.))
                .text_size(px(theme::TEXT_XS))
                .text_color(theme::faint())
                .child(SharedString::from(label.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(px(theme::TEXT_SM))
                .text_color(theme::dim())
                .when(mono, |el| el.font_family(theme::FONT_MONO))
                .child(SharedString::from(value)),
        )
        .into_any_element()
}

// ------------------------------------------------------------------- sessão

fn session_section(state: &AppState) -> Div {
    let name = state.display_title();
    let file = state
        .session
        .file
        .clone()
        .map(|file| file.rsplit('/').next().unwrap_or(&file).to_string())
        .unwrap_or_else(|| "em memória".to_string());
    let id = state
        .session
        .id
        .as_deref()
        .map(|id| id.chars().filter(|c| *c != '-').take(12).collect::<String>())
        .unwrap_or_else(|| "—".to_string());

    div()
        .flex()
        .flex_col()
        .gap(px(theme::S2))
        .child(heading("Sessão"))
        .child(
            div()
                .text_size(px(theme::TEXT))
                .text_color(theme::text())
                .truncate()
                .child(SharedString::from(name)),
        )
        .child(pair(
            "Projeto",
            crate::rpc::folder_label(&state.cwd),
            false,
        ))
        .child(pair(
            "Caminho",
            state.cwd.to_string_lossy().into_owned(),
            true,
        ))
        .child(pair("Arquivo", file, true))
        .child(pair("Id", id, true))
        .child(pair("Mensagens", state.messages.len().to_string(), false))
}

// -------------------------------------------------- arquivos tocados no turno

fn files_section(state: &AppState) -> Div {
    let files = state.current_files();
    let mut section = div()
        .flex()
        .flex_col()
        .gap(px(theme::S2))
        .child(heading("Arquivos neste turno"));

    if files.is_empty() {
        return section.child(
            div()
                .text_size(px(theme::TEXT_SM))
                .text_color(theme::faint())
                .child("nenhum ainda"),
        );
    }

    for file in files {
        let (glyph, color) = match file.status {
            ToolStatus::Failed => (Icon::Alert, theme::failure()),
            ToolStatus::Running => (Icon::Activity, theme::running()),
            _ => (Icon::Check, theme::ok()),
        };
        section = section.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(theme::S2))
                .child(icon(Icon::File, 12., theme::faint()))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .font_family(theme::FONT_MONO)
                        .text_size(px(theme::TEXT_XS))
                        .text_color(if file.edited {
                            theme::dim()
                        } else {
                            theme::faint()
                        })
                        .child(SharedString::from(file.path.clone())),
                )
                .when(file.edited, |el| {
                    el.child(
                        div()
                            .text_size(px(theme::TEXT_XS))
                            .text_color(theme::faint())
                            .child("editado"),
                    )
                })
                .child(icon(glyph, 11., color)),
        );
    }
    section
}

// ------------------------------------------------------------------ contexto

fn context_section(state: &AppState) -> Div {
    let usage = state.context_usage();
    let cost = state.total_cost();
    let tokens = state
        .stats
        .as_ref()
        .and_then(|stats| stats.get("tokens"))
        .cloned();

    let mut section = div()
        .flex()
        .flex_col()
        .gap(px(theme::S2))
        .child(heading("Contexto"));

    match usage {
        Some((used, window, percent)) => {
            let ratio = (percent / 100.0).clamp(0.0, 1.0) as f32;
            let color = if ratio > 0.85 {
                theme::failure()
            } else if ratio > 0.6 {
                theme::warn()
            } else {
                theme::line_strong()
            };
            section = section
                .child(meter(ratio, color))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .font_family(theme::FONT_MONO)
                                .text_size(px(theme::TEXT_XS))
                                .text_color(theme::faint())
                                .child(SharedString::from(format!(
                                    "{} de {}",
                                    crate::state::format_tokens(used),
                                    crate::state::format_tokens(window)
                                ))),
                        )
                        .child(
                            div()
                                .font_family(theme::FONT_MONO)
                                .text_size(px(theme::TEXT_XS))
                                .text_color(if ratio > 0.6 { color } else { theme::dim() })
                                .child(SharedString::from(format!("{percent:.0}%"))),
                        ),
                );
        }
        None => {
            section = section.child(
                div()
                    .text_size(px(theme::TEXT_SM))
                    .text_color(theme::faint())
                    .child("sem medição ainda"),
            );
        }
    }

    if let Some(tokens) = tokens {
        let read = |key: &str| tokens.get(key).and_then(serde_json::Value::as_f64).unwrap_or(0.);
        section = section
            .child(pair(
                "Entrada",
                crate::state::format_tokens(read("input")),
                true,
            ))
            .child(pair(
                "Saída",
                crate::state::format_tokens(read("output")),
                true,
            ))
            .child(pair(
                "Cache",
                crate::state::format_tokens(read("cacheRead")),
                true,
            ));
        if tokens
            .get("cacheWrite")
            .and_then(serde_json::Value::as_f64)
            .is_some_and(|value| value > 0.0)
        {
            section = section.child(pair(
                "Cache W",
                crate::state::format_tokens(read("cacheWrite")),
                true,
            ));
        }
    }
    if let Some(cost) = cost {
        section = section.child(pair("Custo", format!("${cost:.4}"), true));
    }
    section
}

// ------------------------------------------------------------------ timeline

fn timeline_section(state: &AppState) -> Div {
    let turns = state.timeline(12);
    let mut section = div()
        .flex()
        .flex_col()
        .gap(px(theme::S2))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(heading("Turnos"))
                .child(
                    div()
                        .text_size(px(theme::TEXT_XS))
                        .text_color(theme::faint())
                        .child(SharedString::from(format!("{} recentes", turns.len()))),
                ),
        );

    if turns.is_empty() {
        return section.child(
            div()
                .text_size(px(theme::TEXT_SM))
                .text_color(theme::faint())
                .child("nada enviado ainda"),
        );
    }

    for turn in turns {
        let (glyph, color) = match turn.state {
            TurnState::Running => (Icon::Activity, theme::running()),
            TurnState::Failed => (Icon::Alert, theme::failure()),
            TurnState::Stopped => (Icon::Stop, theme::faint()),
            TurnState::Done => (Icon::Check, theme::ok()),
        };
        let running = turn.state == TurnState::Running;
        section = section.child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(theme::S2))
                .px(px(theme::S2))
                .py(px(theme::S1 + 1.))
                .rounded(theme::r_control())
                .when(running, |el| el.bg(theme::surface_2()))
                .child(
                    div()
                        .flex_none()
                        .w(px(22.))
                        .font_family(theme::FONT_MONO)
                        .text_size(px(theme::TEXT_XS))
                        .text_color(theme::faint())
                        .child(SharedString::from(turn.number.to_string())),
                )
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
                                .text_size(px(theme::TEXT_SM))
                                .text_color(if running { theme::text() } else { theme::dim() })
                                .child(SharedString::from(turn.prompt)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(theme::S1 + 1.))
                                .child(icon(glyph, 11., color))
                                .when_some(turn.duration, |el, duration| {
                                    el.child(
                                        div()
                                            .font_family(theme::FONT_MONO)
                                            .text_size(px(theme::TEXT_XS))
                                            .text_color(theme::faint())
                                            .child(SharedString::from(duration)),
                                    )
                                }),
                        ),
                ),
        );
    }
    section
}

// --------------------------------------------------------------------- ações

fn actions_section(state: &AppState, cx: &Context<AppState>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(theme::S2))
        .child(heading("Ações"))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(theme::S2))
                .child(action(
                    "action-compact",
                    "Compactar",
                    cx,
                    |state, cx| {
                        state.compact();
                        cx.notify();
                    },
                ))
                .child(action("action-export", "Exportar", cx, |state, cx| {
                    state.export_html();
                    cx.notify();
                }))
                .child(action("action-new", "Nova sessão", cx, |state, cx| {
                    // Uma conversa nova e independente: o workspace é quem
                    // decide, para não substituir uma sessão em execução.
                    state.new_session_requested = true;
                    cx.notify();
                }))
                .child(div().w_full().h(px(1.)).bg(theme::line_soft()))
                .child(
                    div()
                        .id("action-auto-compact")
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(theme::S2))
                        .cursor_pointer()
                        .hover(|style| style.text_color(theme::text()))
                        .on_click(cx.listener(|state, _: &ClickEvent, _window, cx| {
                            state.toggle_auto_compaction();
                            cx.notify();
                        }))
                        .child(if state.session.auto_compaction {
                            crate::ui::dot(theme::accent(), 6.)
                        } else {
                            crate::ui::ring(theme::line_strong(), 6.)
                        })
                        .child(
                            div()
                                .text_size(px(theme::TEXT_XS))
                                .text_color(theme::faint())
                                .child("compactar sozinho quando encher"),
                        ),
                ),
        )
}

fn action(
    id: &'static str,
    label: &'static str,
    cx: &Context<AppState>,
    run: impl Fn(&mut AppState, &mut Context<AppState>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px(px(theme::S3))
        .py(px(theme::S1 + 3.))
        .rounded(theme::r_control())
        .border_1()
        .border_color(theme::line())
        .cursor_pointer()
        .hover(|style| style.bg(theme::hover()).border_color(theme::line_strong()))
        .text_size(px(theme::TEXT_SM))
        .text_color(theme::dim())
        .on_click(cx.listener(move |state, _: &ClickEvent, _window, cx| run(state, cx)))
        .child(SharedString::from(label.to_string()))
        .into_any_element()
}
