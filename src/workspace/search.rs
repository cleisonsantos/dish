//! Search UI and orchestration. Only the background executor reads session files.
use super::*;
use crate::content_search::{Document, Hit, Index, Results};
use crate::state::Block;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
pub(super) struct SearchState {
    editor: Option<Entity<Editor>>,
    project: bool,
    open: bool,
    loading: bool,
    last_query: Option<String>,
    scope: Option<(PathBuf, Option<EntityId>)>,
    fingerprint: Vec<(EntityId, usize, usize)>,
    results: Results,
    selected: Option<usize>,
    scroll: ScrollHandle,
    anchors: Vec<ScrollAnchor>,
    generation: Arc<AtomicU64>,
    index: Arc<Mutex<Index>>,
    pending_jump: Option<(PathBuf, String)>,
}

fn message_text(message: &crate::state::Message) -> String {
    message
        .blocks
        .iter()
        .map(|block| match block {
            Block::Text(text) | Block::Thinking { text, .. } | Block::Note { text, .. } => {
                text.clone()
            }
            Block::Tool(tool) => format!("{}\n{}\n{}", tool.args, tool.args_text, tool.output),
            Block::Bash(bash) => format!("{}\n{}", bash.command, bash.output),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn reveal_match(state: &mut AppState, index: usize, query: &str, cx: &mut Context<AppState>) {
    if index >= state.messages.len() {
        return;
    }
    let query = query.to_lowercase();
    let mut open_activity = false;
    if let Some(message) = state.messages.get_mut(index) {
        for block in &mut message.blocks {
            match block {
                Block::Thinking { text, expanded, .. } if text.to_lowercase().contains(&query) => {
                    *expanded = true
                }
                Block::Tool(tool)
                    if format!("{}\n{}\n{}", tool.args, tool.args_text, tool.output)
                        .to_lowercase()
                        .contains(&query) =>
                {
                    tool.expanded = true;
                    open_activity = true;
                }
                _ => {}
            }
        }
    }
    if open_activity {
        state.set_activity(index, true, cx);
    }
    state.search_message = Some(index);
    state.search_query = Some(query);
    state.list_state.remeasure_items(index..index + 1);
    state.list_state.set_follow_mode(FollowMode::Normal);
    state.list_state.scroll_to(ListOffset {
        item_ix: index,
        offset_in_item: px(0.),
    });
    cx.notify();
}

impl Workspace {
    fn search_allowed(&self, cx: &App) -> bool {
        !self.confirm_close
            && !self.settings.as_ref().is_some_and(|s| s.read(cx).open)
            && self.conversations[self.active]
                .state
                .read(cx)
                .modal
                .is_none()
    }

    pub(super) fn open_content_search(
        &mut self,
        project: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.search_allowed(cx) {
            return;
        }
        if self.content_search.editor.is_none() {
            let editor = cx.new(|cx| Editor::new("", "Buscar no conteúdo…", window, cx));
            cx.observe(&editor, |workspace, _, cx| {
                if workspace.content_search.open {
                    workspace.run_content_search(cx);
                }
            })
            .detach();
            self.content_search.editor = Some(editor);
        }
        self.content_search.last_query = None;
        self.content_search.project = project;
        self.content_search.open = true;
        if let Some(editor) = &self.content_search.editor {
            editor.update(cx, |editor, cx| {
                editor.select_all(&crate::editor::SelectAllText, window, cx)
            });
            editor.read(cx).focus_handle.clone().focus(window, cx);
        }
        self.run_content_search(cx);
    }

    pub(super) fn exit_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_allowed(cx) {
            return;
        }
        self.content_search.open = false;
        for conversation in &self.conversations {
            conversation.state.update(cx, |state, cx| {
                state.search_message = None;
                state.search_query = None;
                cx.notify();
            });
        }
        self.content_search.pending_jump = None;
        self.content_search
            .generation
            .fetch_add(1, Ordering::Relaxed);
        self.conversations[self.active]
            .state
            .read(cx)
            .editor
            .read(cx)
            .focus_handle
            .clone()
            .focus(window, cx);
        // The sidebar query stays intact; Esc only leaves its focus.
        cx.notify();
    }

    pub(super) fn refresh_content_search_scope(&mut self, cx: &mut Context<Self>) {
        if !self.content_search.open {
            return;
        }
        let conversation = &self.conversations[self.active];
        let scope = (
            conversation.state.read(cx).cwd.clone(),
            (!self.content_search.project).then_some(conversation.state.entity_id()),
        );
        let fingerprint: Vec<_> = self
            .conversations
            .iter()
            .enumerate()
            .filter(|(i, conversation)| {
                if self.content_search.project {
                    conversation.state.read(cx).cwd == scope.0
                } else {
                    *i == self.active
                }
            })
            .map(|(_, conversation)| {
                let state = conversation.state.read(cx);
                let tail_bytes = state
                    .messages
                    .last()
                    .map(|message| {
                        message
                            .blocks
                            .iter()
                            .map(|block| match block {
                                Block::Text(text)
                                | Block::Thinking { text, .. }
                                | Block::Note { text, .. } => text.len(),
                                Block::Tool(tool) => tool.args_text.len() + tool.output.len(),
                                Block::Bash(bash) => bash.command.len() + bash.output.len(),
                            })
                            .sum()
                    })
                    .unwrap_or(0);
                (
                    conversation.state.entity_id(),
                    state.messages.len(),
                    tail_bytes,
                )
            })
            .collect();
        if self.content_search.scope.as_ref() != Some(&scope)
            || self.content_search.fingerprint != fingerprint
        {
            self.content_search.fingerprint = fingerprint;
            self.content_search.scope = Some(scope);
            self.content_search.last_query = None;
            self.run_content_search(cx);
        }
    }

    pub(super) fn invalidate_content_search(&mut self, cx: &mut Context<Self>) {
        self.content_search.last_query = None;
        if self.content_search.open {
            self.run_content_search(cx);
        }
    }

    fn run_content_search(&mut self, cx: &mut Context<Self>) {
        let query = self
            .content_search
            .editor
            .as_ref()
            .map(|editor| editor.read(cx).text(cx))
            .unwrap_or_default();
        if self.content_search.last_query.as_ref() == Some(&query) {
            return;
        }
        self.content_search.last_query = Some(query.clone());
        let ticket = self
            .content_search
            .generation
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        self.content_search.results = Results::default();
        self.content_search.selected = None;
        self.content_search.loading = !query.trim().is_empty();
        cx.notify();
        if query.trim().is_empty() {
            for conversation in &self.conversations {
                conversation.state.update(cx, |state, cx| {
                    state.search_query = None;
                    state.search_message = None;
                    cx.notify();
                });
            }
            return;
        }
        let generation = self.content_search.generation.clone();
        let index = self.content_search.index.clone();
        cx.spawn(async move |this, cx| {
            // Debounce without keeping the UI busy, and discard superseded work.
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            if generation.load(Ordering::Relaxed) != ticket {
                return;
            }
            let Ok(documents) = this.update(cx, |workspace, cx| workspace.search_documents(cx))
            else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    match index.lock() {
                        Ok(mut index) => index.search(&documents, &query, &generation, ticket),
                        Err(_) => Results {
                            errors: vec!["Não foi possível acessar o índice de busca.".into()],
                            ..Results::default()
                        },
                    }
                })
                .await;
            let _ = this.update(cx, |workspace, cx| {
                if workspace.content_search.generation.load(Ordering::Relaxed) == ticket {
                    workspace.content_search.anchors = (0..result.hits.len())
                        .map(|_| ScrollAnchor::for_handle(workspace.content_search.scroll.clone()))
                        .collect();
                    workspace.content_search.results = result;
                    workspace.content_search.loading = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Snapshot live content only after debounce; file content is never copied
    /// or parsed on the UI thread.
    fn search_documents(&self, cx: &mut Context<Self>) -> Vec<Document> {
        let cwd = self.conversations[self.active].state.read(cx).cwd.clone();
        let mut documents = Vec::new();
        for (i, conversation) in self.conversations.iter().enumerate() {
            let state = conversation.state.read(cx);
            if (self.content_search.project && state.cwd == cwd)
                || (!self.content_search.project && i == self.active)
            {
                documents.push(Document {
                    path: conversation.path.clone(),
                    live: Some(i),
                    title: self.conversation_title(i, cx),
                    messages: Some(state.messages.iter().map(message_text).collect()),
                });
            }
        }
        if self.content_search.project {
            for info in &self.catalog {
                if info.cwd == cwd
                    && !documents
                        .iter()
                        .any(|doc| doc.path.as_ref() == Some(&info.path))
                {
                    documents.push(Document {
                        path: Some(info.path.clone()),
                        live: None,
                        title: info.title.clone(),
                        messages: None,
                    });
                }
            }
        }
        let query = self
            .content_search
            .last_query
            .as_deref()
            .unwrap_or_default()
            .trim();
        for (i, conversation) in self.conversations.iter().enumerate() {
            let matches_scope = documents.iter().any(|doc| doc.live == Some(i));
            conversation.state.update(cx, |state, cx| {
                state.search_query = (matches_scope && !query.is_empty()).then(|| query.to_owned());
                cx.notify();
            });
        }
        documents
    }

    pub(super) fn step_match(
        &mut self,
        direction: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.search_allowed(cx) {
            return;
        }
        let len = self.content_search.results.hits.len();
        if len == 0 {
            return;
        }
        let next = self
            .content_search
            .selected
            .map(|i| (i as isize + direction).rem_euclid(len as isize) as usize)
            .unwrap_or(if direction < 0 { len - 1 } else { 0 });
        self.content_search.selected = Some(next);
        if let Some(anchor) = self.content_search.anchors.get(next) {
            anchor.scroll_to(window, cx);
        }
        cx.notify();
    }

    pub(super) fn accept_match(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_allowed(cx) || self.content_search.results.hits.is_empty() {
            return;
        }
        let selected = self.content_search.selected.unwrap_or(0);
        self.choose_match(selected, window, cx);
        self.content_search.open = false;
        self.content_search
            .generation
            .fetch_add(1, Ordering::Relaxed);
        self.conversations[self.active]
            .state
            .read(cx)
            .editor
            .read(cx)
            .focus_handle
            .clone()
            .focus(window, cx);
        cx.notify();
    }

    fn choose_match(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.content_search.results.hits.get(index).cloned() else {
            return;
        };
        self.content_search.selected = Some(index);
        if let Some(live) = hit.live {
            if live >= self.conversations.len() {
                return;
            }
            self.select(live, window, cx);
            let state = self.conversations[live].state.clone();
            let query = self
                .content_search
                .last_query
                .as_deref()
                .unwrap_or_default();
            state.update(cx, |state, cx| reveal_match(state, hit.message, query, cx));
        } else if let Some(path) = &hit.path {
            if let Some(info) = self.catalog.iter().find(|info| &info.path == path).cloned() {
                self.content_search.pending_jump =
                    Some((path.clone(), hit.snippet.trim_matches('…').to_owned()));
                self.open(info, window, cx);
            }
        }
        // Selection normally focuses the prompt; keep search keyboard navigation.
        if let Some(editor) = &self.content_search.editor {
            editor.read(cx).focus_handle.clone().focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn resolve_search_jump(&mut self, cx: &mut Context<Self>) {
        let Some((path, snippet)) = &self.content_search.pending_jump else {
            return;
        };
        let Some(conversation) = self
            .conversations
            .iter()
            .find(|conversation| conversation.path.as_ref() == Some(path))
        else {
            return;
        };
        let state = conversation.state.clone();
        if state.read(cx).loading {
            return;
        }
        let Some(index) = state
            .read(cx)
            .messages
            .iter()
            .position(|message| message_text(message).contains(snippet))
        else {
            self.content_search.pending_jump = None;
            self.content_search.results.errors.push("A sessão foi aberta, mas o trecho não está no histórico carregado pelo Pi. Ele pode pertencer a outro ramo ou ter sido alterado.".into());
            return;
        };
        let query = self
            .content_search
            .last_query
            .as_deref()
            .unwrap_or_default();
        state.update(cx, |state, cx| reveal_match(state, index, query, cx));
        self.content_search.pending_jump = None;
    }

    pub(super) fn content_search_panel(&self, cx: &Context<Self>) -> Option<Stateful<Div>> {
        if !self.content_search.open {
            return None;
        }
        let editor = self.content_search.editor.as_ref()?.clone();
        let focus = editor.read(cx).focus_handle.clone();
        let click_focus = focus.clone();
        let count = self.content_search.results.hits.len();
        let occurrences: usize = self
            .content_search
            .results
            .hits
            .iter()
            .map(|hit| hit.occurrences)
            .sum();
        let status = if self.content_search.loading {
            "Buscando…".into()
        } else {
            format!(
                "{count} {} · {occurrences} ocorrências{}",
                if count == 1 {
                    "resultado"
                } else {
                    "resultados"
                },
                if self.content_search.results.limited {
                    " · refine a busca"
                } else {
                    ""
                }
            )
        };
        let mut panel = div()
            .id("content-search-panel")
            // O painel é uma sobreposição: inclusive a roda do mouse deve
            // parar aqui, sem atingir o transcript/inspetor por trás.
            .occlude()
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .absolute()
            .top(px(theme::S6))
            .right(px(theme::S5))
            .w(px(520.))
            .max_h(px(480.))
            .flex()
            .flex_col()
            .gap(px(theme::S2))
            .p(px(theme::S3))
            .bg(theme::surface_2())
            .border_1()
            .border_color(theme::line_strong())
            .shadow(theme::shadow_overlay())
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(if self.content_search.project {
                        "Buscar nas sessões da pasta"
                    } else {
                        "Buscar na sessão atual"
                    })
                    .child(
                        div()
                            .id("content-search-close")
                            .cursor_pointer()
                            .on_click(cx.listener(|workspace, _: &ClickEvent, window, cx| {
                                workspace.exit_search(window, cx)
                            }))
                            .child("Fechar · Esc"),
                    ),
            )
            .child(
                div()
                    .id("content-search-input")
                    .key_context("DishContentSearch")
                    .track_focus(&focus)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        click_focus.focus(window, cx)
                    })
                    .map(crate::editor::standard_actions(editor.clone()))
                    .p(px(theme::S2))
                    .bg(theme::inset())
                    .child(editor),
            )
            .child(
                div()
                    .text_size(px(theme::TEXT_SM))
                    .text_color(theme::faint())
                    .child(SharedString::from(status)),
            )
            .child(
                div()
                    .text_size(px(theme::TEXT_SM))
                    .text_color(theme::faint())
                    .child("Tab / ↓: próxima · Shift+Tab / ↑: anterior · Enter: abrir · Esc: sair"),
            );
        if !self.content_search.results.errors.is_empty() {
            panel = panel.child(div().text_color(theme::running()).child(SharedString::from(
                format!(
                    "{} problemas na busca: {}",
                    self.content_search.results.errors.len(),
                    self.content_search.results.errors[0]
                ),
            )));
        }
        let mut hits = div()
            .id("content-search-results")
            .track_scroll(&self.content_search.scroll)
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(theme::S1));
        for (i, Hit { title, snippet, .. }) in self.content_search.results.hits.iter().enumerate() {
            hits = hits.child(
                div()
                    .id(("content-search-hit", i))
                    .anchor_scroll(self.content_search.anchors.get(i).cloned())
                    .p(px(theme::S2))
                    .cursor_pointer()
                    .when(self.content_search.selected == Some(i), |el| {
                        el.bg(theme::hover())
                    })
                    .hover(|el| el.bg(theme::hover()))
                    .on_click(cx.listener(move |workspace, _: &ClickEvent, window, cx| {
                        workspace.content_search.selected = Some(i);
                        workspace.accept_match(window, cx)
                    }))
                    .child(
                        div()
                            .text_size(px(theme::TEXT_SM))
                            .child(SharedString::from(title.clone())),
                    )
                    .child(
                        div()
                            .font_family(theme::FONT_MONO)
                            .text_size(px(theme::TEXT_SM))
                            .text_color(theme::dim())
                            .child(
                                StyledText::new(SharedString::from(snippet.clone()))
                                    .with_highlights(
                                        crate::content_search::match_ranges(
                                            snippet,
                                            &self
                                                .content_search
                                                .last_query
                                                .as_deref()
                                                .unwrap_or_default()
                                                .trim()
                                                .to_lowercase(),
                                        )
                                        .into_iter()
                                        .map(|range| {
                                            (
                                                range,
                                                HighlightStyle {
                                                    background_color: Some(theme::wash(
                                                        theme::accent(),
                                                        0.22,
                                                    )),
                                                    ..HighlightStyle::default()
                                                },
                                            )
                                        }),
                                    ),
                            ),
                    ),
            );
        }
        Some(panel.child(hits))
    }
}
