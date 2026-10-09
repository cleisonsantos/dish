//! The conversation.
//!
//! Each assistant message is presented as *activity* and *answer*: consecutive
//! tool calls collapse into one expandable group with a summary, reasoning folds
//! away to a single row, and the prose that remains is the biggest thing on the
//! screen. The message list stays virtualised, and nothing about the persisted
//! session changes — this is presentation only.

use gpui::prelude::*;
use gpui::*;

use crate::markdown;
use crate::state::{AppState, Block, Message, Role, ToolCard, ToolStatus};
use crate::theme;
use crate::ui::icons::{Icon, icon};
use crate::ui::{content_id, markdown_style};

pub fn transcript(state: &AppState, cx: &Context<AppState>) -> Div {
    let weak = cx.entity().downgrade();
    let list_state = state.list_state.clone();
    let show_jump = !list_state.is_following_tail() && !state.messages.is_empty();

    let body = if state.messages.is_empty() {
        empty_state(&weak)
    } else {
        div()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .flex()
            .flex_col()
            .child(
                list(list_state, move |index, _window, cx| {
                    let Some(entity) = weak.upgrade() else {
                        return div().into_any_element();
                    };
                    let state = entity.read(cx);
                    message_element(state, index, &weak)
                })
                .flex_1()
                .min_h(px(0.))
                .w_full(),
            )
    };

    let mut root = div()
        .tab_group()
        .relative()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .flex()
        .flex_col()
        .child(body);

    if show_jump {
        root = root.child(
            div()
                .id("transcript-jump-end")
                .absolute()
                .bottom(px(theme::S3))
                .right(px(theme::S3))
                .size(px(32.))
                .rounded(px(16.))
                .border_1()
                .border_color(theme::line_strong())
                .bg(theme::surface_2())
                .shadow(theme::shadow_overlay())
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|style| style.bg(theme::hover()))
                .on_click(cx.listener(|state, _: &ClickEvent, _window, cx| {
                    state.list_state.set_follow_mode(FollowMode::Tail);
                    cx.notify();
                }))
                .child(icon(Icon::ChevronDown, 16., theme::dim())),
        );
    }
    root
}

// ------------------------------------------------------------- empty state

fn empty_state(weak: &WeakEntity<AppState>) -> Div {
    let weak = weak.clone();
    let prompts: [&'static str; 3] = [
        "explicar este código",
        "corrigir os testes que falham",
        "resumir o que mudou na semana",
    ];

    div()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(520.))
                .flex()
                .flex_col()
                .gap(px(theme::S4))
                .child(
                    div()
                        .text_size(px(theme::TEXT_XS))
                        .text_color(theme::faint())
                        .child("Começar"),
                )
                .children(
                    prompts
                        .into_iter()
                        .enumerate()
                        .map(|(index, prompt)| prompt_row(index, prompt, &weak)),
                ),
        )
}

fn prompt_row(index: usize, prompt: &'static str, weak: &WeakEntity<AppState>) -> AnyElement {
    let weak = weak.clone();
    let text = prompt.to_string();
    div()
        .id(SharedString::from(format!("suggestion-{index}")))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(theme::S3))
        .px(px(theme::S3))
        .py(px(theme::S2))
        .rounded(theme::r_control())
        .cursor_pointer()
        .hover(|style| style.bg(theme::hover()))
        .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
            weak.update(cx, |state, cx| state.set_composer_text(text.clone(), cx))
                .ok();
        })
        .child(icon(Icon::Plus, 13., theme::faint()))
        .child(
            div()
                .text_size(px(theme::PROSE))
                .text_color(theme::dim())
                .child(prompt),
        )
        .into_any_element()
}

// ---------------------------------------------------------------- messages

fn message_element(state: &AppState, index: usize, weak: &WeakEntity<AppState>) -> AnyElement {
    let Some(message) = state.messages.get(index) else {
        return div().into_any_element();
    };
    match message.role {
        Role::User => request(state, message, index, weak),
        Role::Assistant => assistant_turn(state, message, index, weak),
        Role::Bash => match message.blocks.first() {
            Some(Block::Bash(card)) => command_card(index, 0, card, weak),
            _ => div().into_any_element(),
        },
        Role::System => {
            let mut column = div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(theme::S3))
                .px(px(theme::S5))
                .py(px(theme::S2));
            for block in &message.blocks {
                if let Block::Note { label, text, tone } = block {
                    column = column.child(note(label, text, crate::ui::tone_color(*tone)));
                }
            }
            column.into_any_element()
        }
    }
}

/// O pedido: superfície discreta, sem balão.
fn request(state: &AppState, message: &Message, index: usize, _weak: &WeakEntity<AppState>) -> AnyElement {
    let text = message.text();
    let style = markdown_style();
    let copied = text.clone();

    div()
        .w_full()
        .flex()
        .flex_col()
        .px(px(theme::S5))
        .py(px(theme::S3))
        .child(
            div()
                .id(SharedString::from(format!("user-{index}")))
                .group("request")
                .flex()
                .flex_col()
                .gap(px(theme::S1))
                .rounded(theme::r_surface())
                .bg(theme::surface_2())
                .border_1()
                .border_color(theme::line_soft())
                .px(px(theme::S4))
                .py(px(theme::S2))
                .children(markdown::blocks(&text, &style, &state.cwd, SharedString::from(format!("request-text-{index}")).into()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_end()
                        .gap(px(theme::S3))
                        .when_some(
                            crate::metadata::describe(
                                Some((message.timestamp, message.timestamp_origin)),
                                &message.timing,
                            ),
                            |el, metadata| {
                                el.child(super::metadata_view::MetadataView {
                                    id: SharedString::from(format!("request-metadata-{index}"))
                                        .into(),
                                    metadata,
                                })
                            },
                        )
                        .child(super::copy_button::CopyButton {
                            id: SharedString::from(format!("copy-prompt-{index}")).into(),
                            text: copied,
                            label: "Copiar mensagem",
                        }),
                ),
        )
        .into_any_element()
}

fn assistant_turn(
    state: &AppState,
    message: &Message,
    index: usize,
    weak: &WeakEntity<AppState>,
) -> AnyElement {
    let style = markdown_style();
    let streaming = message.streaming;
    let model = message
        .model
        .clone()
        .unwrap_or_else(|| "assistente".to_string());
    let duration = message.timing.elapsed_ms.map(crate::metadata::duration);
    let (response_status, response_error) = crate::metadata::assistant_status(
        streaming,
        message.stop_reason.as_deref(),
        message.error.is_some(),
    );

    let mut column = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(theme::S3))
        .px(px(theme::S5))
        .py(px(theme::S3))
        // Cabeçalho de turno no lugar de uma barra vertical colorida.
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(theme::S2))
                .child(
                    div()
                        .text_size(px(theme::TEXT_XS))
                        .text_color(theme::faint())
                        .child(SharedString::from(model)),
                )
                .child(
                    div()
                        .text_size(px(theme::TEXT_XS))
                        .text_color(if response_error {
                            theme::failure()
                        } else {
                            theme::dim()
                        })
                        .child(response_status),
                )
                .when(streaming, |el| {
                    el.child(crate::ui::pulse("turn-pulse", 6.0, theme::running()))
                })
                .when_some(
                    crate::metadata::describe(
                        Some((message.timestamp, message.timestamp_origin)),
                        &message.timing,
                    ),
                    |el, metadata| {
                        el.child(super::metadata_view::MetadataView {
                            id: SharedString::from(format!("reply-metadata-{index}")).into(),
                            metadata,
                        })
                    },
                )
                .child(div().flex_1())
                .when_some(duration, |el, duration| {
                    el.child(
                        div()
                            .font_family(theme::FONT_MONO)
                            .text_size(px(theme::TEXT_XS))
                            .text_color(theme::faint())
                            .child(SharedString::from(duration)),
                    )
                })
                .when(!streaming && !message.text().is_empty(), |el| {
                    el.child(super::copy_button::CopyButton {
                        id: SharedString::from(format!("copy-reply-{index}")).into(),
                        text: message.text(),
                        label: "Copiar resposta",
                    })
                }),
        );

    // Percorre os blocos preservando a ordem, mas agrupando ferramentas
    // consecutivas num único grupo expansível.
    let mut pending: Vec<(usize, ToolRef<'_>)> = Vec::new();
    let mut blocks: Vec<AnyElement> = Vec::new();

    let flush = |pending: &mut Vec<(usize, ToolRef<'_>)>,
                 blocks: &mut Vec<AnyElement>,
                 state: &AppState| {
        if pending.is_empty() {
            return;
        }
        let group: Vec<(usize, ToolRef<'_>)> = std::mem::take(pending);
        blocks.push(activity_group(state, index, &group, weak));
    };

    for (block_index, block) in message.blocks.iter().enumerate() {
        match block {
            Block::Tool(card) => pending.push((block_index, ToolRef::Tool(card))),
            Block::Bash(card) => pending.push((block_index, ToolRef::Bash(card))),
            Block::Text(text) => {
                flush(&mut pending, &mut blocks, state);
                if text.trim().is_empty() {
                    continue;
                }
                blocks.push(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(theme::S2))
                        .children(markdown::blocks(text, &style, &state.cwd, SharedString::from(format!("reply-text-{index}-{block_index}")).into()))
                        .into_any_element(),
                );
            }
            Block::Thinking {
                text,
                done,
                expanded,
            } => {
                flush(&mut pending, &mut blocks, state);
                blocks.push(reasoning(index, block_index, text, *done, *expanded, weak));
            }
            Block::Note { label, text, tone } => {
                flush(&mut pending, &mut blocks, state);
                blocks.push(note(label, text, crate::ui::tone_color(*tone)));
            }
        }
    }
    flush(&mut pending, &mut blocks, state);

    if blocks.is_empty() && streaming {
        blocks.push(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(theme::S2))
                .text_size(px(theme::TEXT_SM))
                .text_color(theme::faint())
                .child(crate::ui::pulse("turn-idle", 6.0, theme::running()))
                .child("pensando")
                .into_any_element(),
        );
    }

    column = column.children(blocks);

    if let Some(error) = &message.error {
        column = column.child(note(
            if message.stop_reason.as_deref() == Some("aborted") {
                "interrupção"
            } else {
                "erro"
            },
            error,
            crate::ui::tone_color(if message.stop_reason.as_deref() == Some("aborted") {
                crate::state::Tone::Warning
            } else {
                crate::state::Tone::Error
            }),
        ));
    }

    column.into_any_element()
}

// ------------------------------------------------------------------ atividade

/// O que foi chamado dentro de um grupo, sem copiar o card.
enum ToolRef<'a> {
    Tool(&'a ToolCard),
    Bash(&'a crate::state::BashCard),
}

impl<'a> ToolRef<'a> {
    fn status(&self) -> ToolStatus {
        match self {
            ToolRef::Tool(card) => card.status,
            ToolRef::Bash(card) => card.status,
        }
    }

    fn expanded(&self) -> bool {
        match self {
            ToolRef::Tool(card) => card.expanded,
            ToolRef::Bash(_) => false,
        }
    }

    fn name(&self) -> &str {
        match self {
            ToolRef::Tool(card) => &card.name,
            ToolRef::Bash(_) => "bash",
        }
    }
}

/// Verbos por ferramenta, para o resumo do grupo.
fn verb(name: &str) -> &'static str {
    match name {
        "read" | "view" | "open" => "leitura",
        "write" | "create" => "escrita",
        "edit" | "patch" => "edição",
        "bash" | "shell" | "exec" => "comando",
        "grep" | "search" | "glob" | "find" | "ls" | "list" => "busca",
        _ => "chamada",
    }
}

fn plural(verb: &str, count: usize) -> String {
    let word = if count == 1 {
        verb.to_string()
    } else {
        match verb {
            "leitura" => "leituras".into(),
            "escrita" => "escritas".into(),
            "edição" => "edições".into(),
            "comando" => "comandos".into(),
            "busca" => "buscas".into(),
            "chamada" => "chamadas".into(),
            other => other.to_string(),
        }
    };
    format!("{count} {word}")
}

fn tool_icon(name: &str) -> Icon {
    match name {
        "read" | "view" | "open" | "write" | "create" | "edit" | "patch" => Icon::File,
        "bash" | "shell" | "exec" => Icon::Terminal,
        "grep" | "search" | "glob" | "find" => Icon::Search,
        "ls" | "list" => Icon::Folder,
        _ => Icon::Dots,
    }
}

fn group_status(
    failed: usize,
    running: usize,
    pending: usize,
    interrupted: usize,
) -> (Icon, Hsla, Option<String>) {
    if failed > 0 {
        (
            Icon::Alert,
            theme::failure(),
            Some(format!("{failed} falhou")),
        )
    } else if running > 0 {
        (Icon::Activity, theme::running(), Some("rodando".into()))
    } else if pending > 0 {
        (
            Icon::Clock,
            theme::faint(),
            Some(format!("{pending} não iniciada(s)")),
        )
    } else if interrupted > 0 {
        (
            Icon::Stop,
            theme::warn(),
            Some(format!("{interrupted} interrompida(s)")),
        )
    } else {
        (Icon::Check, theme::ok(), None)
    }
}

/// Grupo de atividade: um resumo que abre para as linhas de ferramenta.
fn activity_group(
    state: &AppState,
    message_index: usize,
    group: &[(usize, ToolRef<'_>)],
    weak: &WeakEntity<AppState>,
) -> AnyElement {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    let mut elapsed: u64 = 0;
    let mut failed = 0usize;
    let mut running = 0usize;
    let mut pending = 0usize;
    let mut interrupted = 0usize;
    for (_, tool) in group {
        let entry = counts
            .iter_mut()
            .find(|(name, _)| *name == verb(tool.name()));
        match entry {
            Some((_, count)) => *count += 1,
            None => counts.push((verb(tool.name()), 1)),
        }
        match tool {
            ToolRef::Tool(card) => {
                elapsed += card.elapsed_ms.unwrap_or(0);
            }
            ToolRef::Bash(_) => {}
        }
        if matches!(tool, ToolRef::Bash(card) if card.cancelled) {
            interrupted += 1;
        } else {
            match tool.status() {
                ToolStatus::Failed => failed += 1,
                ToolStatus::Running => running += 1,
                ToolStatus::Pending => pending += 1,
                ToolStatus::Ok => {}
            }
        }
    }

    let summary = counts
        .iter()
        .map(|(name, count)| plural(name, *count))
        .collect::<Vec<_>>()
        .join(" · ");

    let any_expanded = group.iter().any(|(_, tool)| tool.expanded());
    let default_open = group
        .iter()
        .any(|(_, tool)| tool.status() == ToolStatus::Running);
    let open = state.activity_open(message_index).unwrap_or(default_open);
    let _ = any_expanded;

    let (glyph, color, extra) = group_status(failed, running, pending, interrupted);

    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(theme::S2))
        .child(
            div()
                .id(SharedString::from(format!("activity-{message_index}")))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(theme::S2))
                .cursor_pointer()
                .hover(|style| style.text_color(theme::text()))
                .on_click({
                    let weak = weak.clone();
                    move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                        weak.update(cx, |state, cx| {
                            state.set_activity(message_index, !open, cx)
                        })
                        .ok();
                    }
                })
                .child(icon(
                    if open {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    },
                    13.,
                    theme::faint(),
                ))
                .child(icon(glyph, 13., color))
                .child(
                    div()
                        .text_size(px(theme::TEXT_SM))
                        .text_color(theme::dim())
                        .child(SharedString::from(summary)),
                )
                .when_some(extra, |el, extra| {
                    el.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S1 + 1.))
                            .child(icon(glyph, 11., color))
                            .child(
                                div()
                                    .text_size(px(theme::TEXT_XS))
                                    .text_color(color)
                                    .child(SharedString::from(extra)),
                            ),
                    )
                })
                .when(elapsed > 0, |el| {
                    el.child(
                        div()
                            .font_family(theme::FONT_MONO)
                            .text_size(px(theme::TEXT_XS))
                            .text_color(theme::faint())
                            .child(SharedString::from(format!("{:.1}s", elapsed as f64 / 1000.))),
                    )
                }),
        );

    if open {
        let mut rows = div()
            .flex()
            .flex_col()
            .gap(px(theme::S1))
            .ml(px(theme::S1 + 2.))
            .pl(px(theme::S3))
            .border_l_1()
            .border_color(theme::line());
        for (block_index, tool) in group {
            rows = rows.child(match tool {
                ToolRef::Tool(card) => tool_row(message_index, *block_index, card, weak),
                ToolRef::Bash(card) => command_row(message_index, *block_index, card),
            });
        }
        column = column.child(rows);
    }

    column.into_any_element()
}

/// Copy the actual command, or complete arguments; never the truncated summary.
fn tool_copy_text(args: &serde_json::Value, partial: &str) -> String {
    if let Some(command) = args.get("command").and_then(serde_json::Value::as_str) {
        command.to_owned()
    } else if args.is_null() {
        partial.to_owned()
    } else {
        serde_json::to_string_pretty(args).unwrap_or_else(|_| partial.to_owned())
    }
}

fn tool_row(
    message_index: usize,
    block_index: usize,
    card: &ToolCard,
    weak: &WeakEntity<AppState>,
) -> AnyElement {
    let (glyph, color, label) = crate::ui::status_word(card.status);
    let summary = card.summary();
    let expanded = card.expanded;
    let keyboard_weak = weak.clone();
    let weak = weak.clone();
    let command = card.args.get("command").and_then(serde_json::Value::as_str).map(str::to_owned);
    let complete = tool_copy_text(&card.args, &card.args_text);
    let tooltip = complete.clone();
    let prefix = format!("tool-section-{message_index}-{block_index}");
    let mut sections: Vec<AnyElement> = Vec::new();
    if let Some(command) = command.as_ref() {
        sections.push(section(&prefix, "comando", command));
    }
    if !card.args.is_null() {
        let args =
            serde_json::to_string_pretty(&card.args).unwrap_or_else(|_| card.args_text.clone());
        if !args.is_empty() && args != "{}" {
            sections.push(section(&prefix, "argumentos", &args));
        }
    }
    if !card.output.is_empty() {
        sections.push(section(&prefix, "saída", &card.output));
    }
    if let Some(details) = card
        .details
        .as_ref()
        .map(|details| serde_json::to_string_pretty(details).unwrap_or_default())
        .filter(|text| !text.is_empty() && text != "{}")
    {
        sections.push(section(&prefix, "detalhes", &details));
    }

    let row = div()
        .id(SharedString::from(format!(
            "tool-{message_index}-{block_index}"
        )))
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(theme::S2))
        .py(px(theme::S1))
        .pr(px(theme::S2))
        .rounded(theme::r_control())
        .cursor_pointer()
        .focusable().tab_index(0)
        .focus(|s| s.border_1().border_color(theme::accent()))
        .hoverable_tooltip(move |_, cx| {
            cx.new(|_| super::copy_button::TextTooltip(tooltip.clone())).into()
        })
        .on_key_down(move |event, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                let _ = keyboard_weak.update(cx, |state, cx| {
                    state.toggle_tool(message_index, block_index, cx)
                });
                cx.stop_propagation();
            }
        })
        .hover(|style| style.bg(theme::hover()))
        .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
            weak.update(cx, |state, cx| {
                state.toggle_tool(message_index, block_index, cx)
            })
            .ok();
        })
        .child(icon(tool_icon(&card.name), 13., theme::faint()))
        .child(
            div()
                .flex_none()
                .font_family(theme::FONT_MONO)
                .text_size(px(theme::MONO))
                .text_color(theme::dim())
                .child(SharedString::from(card.name.clone())),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .font_family(theme::FONT_MONO)
                .text_size(px(theme::MONO))
                .text_color(theme::faint())
                .child(SharedString::from(summary)),
        )
        .child(super::copy_button::CopyButton {
            id: SharedString::from(format!("copy-tool-{message_index}-{block_index}")).into(),
            text: complete,
            label: if command.is_some() { "Copiar comando" } else { "Copiar argumentos" },
        })
        .when_some(
            crate::metadata::describe(None, &card.timing),
            |el, metadata| {
                el.child(super::metadata_view::MetadataView {
                    id: SharedString::from(format!("tool-metadata-{message_index}-{block_index}"))
                        .into(),
                    metadata,
                })
            },
        )
        .when_some(card.elapsed_ms, |el, elapsed| {
            el.child(
                div()
                    .text_size(px(theme::TEXT_XS))
                    .text_color(theme::faint())
                    .child(crate::metadata::duration(elapsed)),
            )
        })
        .child(icon(glyph, 12., color))
        .child(
            div()
                .text_size(px(theme::TEXT_XS))
                .text_color(color)
                .child(label),
        );

    div()
        .w_full()
        .flex()
        .flex_col()
        .child(row)
        .when(expanded, |el| {
            el.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme::S3))
                    .pb(px(theme::S2))
                    .children(sections),
            )
        })
        .into_any_element()
}

fn command_row(
    message_index: usize,
    block_index: usize,
    card: &crate::state::BashCard,
) -> AnyElement {
    let (glyph, color, label) = if card.cancelled {
        (Icon::Stop, theme::warn(), "interrompido")
    } else if card.exit_code.is_none() && card.status == ToolStatus::Ok {
        (Icon::Clock, theme::faint(), "sem código de saída")
    } else {
        crate::ui::status_word(card.status)
    };
    let status_text = match card.exit_code {
        Some(code) if code != 0 => format!("{label} · saída {code}"),
        Some(_) => label.to_string(),
        None => label.to_string(),
    };

    let tooltip = card.command.clone();
    let prefix = format!("bash-section-{message_index}-{block_index}");
    let mut column = div()
        .flex()
        .flex_col()
        .child(
            div()
                .id(SharedString::from(format!("bash-command-{message_index}-{block_index}")))
                .hoverable_tooltip(move |_, cx| {
                    cx.new(|_| super::copy_button::TextTooltip(tooltip.clone())).into()
                })
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(theme::S2))
                .py(px(theme::S1))
                .pr(px(theme::S2))
                .child(icon(Icon::Terminal, 13., theme::faint()))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .font_family(theme::FONT_MONO)
                        .text_size(px(theme::MONO))
                        .text_color(theme::text())
                        .child(super::selectable::plain(SharedString::from(format!("bash-command-text-{message_index}-{block_index}")).into(), &card.command)),
                )
                .child(super::copy_button::CopyButton {
                    id: SharedString::from(format!("copy-bash-command-{message_index}-{block_index}")).into(),
                    text: card.command.clone(),
                    label: "Copiar comando",
                })
                .when_some(
                    crate::metadata::describe(None, &card.timing),
                    |el, metadata| {
                        el.child(super::metadata_view::MetadataView {
                            id: SharedString::from(format!(
                                "bash-metadata-{message_index}-{block_index}"
                            ))
                            .into(),
                            metadata,
                        })
                    },
                )
                .when_some(card.timing.elapsed_ms, |el, elapsed| {
                    el.child(
                        div()
                            .text_size(px(theme::TEXT_XS))
                            .text_color(theme::faint())
                            .child(crate::metadata::duration(elapsed)),
                    )
                })
                .child(icon(glyph, 12., color))
                .child(
                    div()
                        .text_size(px(theme::TEXT_XS))
                        .text_color(color)
                        .child(SharedString::from(status_text)),
                ),
        );

    if !card.output.is_empty() {
        column = column.child(section(&prefix, "saída", &card.output));
    }
    column.into_any_element()
}

/// Cartão de comando solto (o `!` do usuário), fora de um turno do assistente.
fn command_card(
    message_index: usize,
    block_index: usize,
    card: &crate::state::BashCard,
    _weak: &WeakEntity<AppState>,
) -> AnyElement {
    div()
        .w_full()
        .px(px(theme::S5))
        .py(px(theme::S3))
        .child(command_row(message_index, block_index, card))
        .into_any_element()
}

// -------------------------------------------------------------------- blocos

fn reasoning(
    message_index: usize,
    block_index: usize,
    text: &str,
    done: bool,
    expanded: bool,
    weak: &WeakEntity<AppState>,
) -> AnyElement {
    let weak = weak.clone();
    let label = if done { "Raciocínio" } else { "Raciocinando" };
    let preview = text
        .lines()
        .last()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(80)
        .collect::<String>();

    let mut column = div().flex().flex_col().gap(px(theme::S2)).w_full().child(
        div()
            .id(SharedString::from(format!(
                "reasoning-{message_index}-{block_index}"
            )))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(theme::S2))
            .cursor_pointer()
            .hover(|style| style.text_color(theme::text()))
            .on_click(move |_event: &ClickEvent, _window: &mut Window, cx: &mut App| {
                weak.update(cx, |state, cx| {
                    state.toggle_thinking(message_index, block_index, cx)
                })
                .ok();
            })
            .child(icon(
                if expanded {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                },
                13.,
                theme::faint(),
            ))
            .child(
                div()
                    .text_size(px(theme::TEXT_XS))
                    .text_color(theme::faint())
                    .child(SharedString::from(format!(
                        "{label} · {} caracteres",
                        text.chars().count()
                    ))),
            )
            .child(div().flex_1().min_w(px(0.)))
            .when(!expanded && !preview.is_empty(), |el| {
                el.child(
                    div()
                        .max_w(px(340.))
                        .truncate()
                        .text_size(px(theme::TEXT_XS))
                        .text_color(theme::faint())
                        .child(SharedString::from(preview)),
                )
            }),
    );

    if expanded {
        column = column.child(
            div()
                .id(SharedString::from(format!(
                    "reasoning-body-{message_index}-{block_index}"
                )))
                .max_h(px(260.))
                .overflow_y_scroll()
                .pl(px(theme::S3))
                .border_l_1()
                .border_color(theme::line())
                .text_size(px(theme::TEXT_SM))
                .line_height(px(19.))
                .text_color(theme::faint())
                .child(SharedString::from(text.to_string())),
        );
    }
    column.into_any_element()
}

fn section(prefix: &str, label: &str, body: &str) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(theme::S1))
        .pt(px(theme::S1))
        .child(
            div().flex().items_center().justify_between()
                .child(div().text_size(px(theme::TEXT_XS)).text_color(theme::faint())
                    .child(SharedString::from(label.to_string())))
                .child(super::copy_button::CopyButton {
                    id: SharedString::from(format!("{prefix}-copy-{label}")).into(),
                    text: body.to_owned(),
                    label: match label {
                        "comando" => "Copiar comando",
                        "argumentos" => "Copiar argumentos",
                        "saída" => "Copiar saída",
                        _ => "Copiar detalhes",
                    },
                }),
        )
        .child(
            div()
                .id(SharedString::from(format!("{prefix}-{label}")))
                .max_h(px(420.))
                .overflow_y_scroll()
                .px(px(theme::S3))
                .py(px(theme::S2))
                .rounded(theme::r_control())
                .bg(theme::inset())
                .border_1()
                .border_color(theme::line_soft())
                .font_family(theme::FONT_MONO)
                .text_size(px(theme::MONO))
                .line_height(px(19.))
                .text_color(theme::dim())
                .child(super::selectable::plain(SharedString::from(format!("{prefix}-text-{label}")).into(), body)),
        )
        .into_any_element()
}

fn note(label: &str, text: &str, color: Hsla) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(theme::S1))
        .pl(px(theme::S3))
        .border_l_2()
        .border_color(color)
        .child(
            div()
                .text_size(px(theme::TEXT_XS))
                .text_color(color)
                .child(SharedString::from(label.to_string())),
        )
        .child(
            div()
                .id(content_id("note", text))
                .max_h(px(300.))
                .overflow_y_scroll()
                .text_size(px(theme::TEXT_SM))
                .line_height(px(19.))
                .text_color(theme::dim())
                .child(SharedString::from(text.to_string())),
        )
        .into_any_element()
}

#[cfg(test)]
mod copy_tests {
    use super::{group_status, tool_copy_text};
    use crate::ui::icons::Icon;
    use serde_json::json;

    #[test]
    fn unstarted_or_cancelled_tools_are_not_reported_as_success() {
        assert_eq!(group_status(0, 0, 1, 0).0, Icon::Clock);
        assert_eq!(group_status(0, 0, 0, 1).0, Icon::Stop);
        assert_eq!(group_status(1, 0, 0, 0).0, Icon::Alert);
        assert_eq!(group_status(0, 1, 0, 0).0, Icon::Activity);
        assert_eq!(group_status(0, 0, 0, 0).0, Icon::Check);
    }

    #[test]
    fn copies_exact_multiline_command_not_summary() {
        let command = format!("printf 'ação\\n'\n{}", "echo café\n".repeat(100));
        assert_eq!(tool_copy_text(&json!({"command": command}), "truncated"), command);
    }

    #[test]
    fn preserves_partial_arguments_and_serializes_complete_arguments() {
        let partial = "{\"path\":\"ação";
        assert_eq!(tool_copy_text(&serde_json::Value::Null, partial), partial);
        let args = json!({"path": "ação.rs", "limit": 5});
        let copied = tool_copy_text(&args, "");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&copied).unwrap(), args);
    }
}
