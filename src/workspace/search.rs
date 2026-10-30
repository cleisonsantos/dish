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
    results: Results,
    selected: Option<usize>,
    scroll: ScrollHandle,
    anchors: Vec<ScrollAnchor>,
    generation: Arc<AtomicU64>,
    index: Arc<Mutex<Index>>,
    pending_jump: Option<(PathBuf, String)>,
    /// Live text snapshots keyed by conversation entity and revision.
    live_snapshots: std::collections::HashMap<EntityId, (u64, Arc<Vec<String>>)>,
}

impl SearchState {
    /// Results are a snapshot of the query, not a live projection of streaming
    /// messages. Only switching scope, editing or reopening search refreshes it.
    fn set_scope(&mut self, scope: (PathBuf, Option<EntityId>)) -> bool {
        if self.scope.as_ref() == Some(&scope) {
            return false;
        }
        self.scope = Some(scope);
        self.last_query = None;
        self.clear_results();
        true
    }

    fn clear_results(&mut self) {
        self.results = Results::default();
        self.selected = None;
        self.anchors.clear();
    }

    fn begin_query(&mut self, query: &str) -> Option<u64> {
        if self.last_query.as_deref() == Some(query) {
            return None;
        }
        self.last_query = Some(query.to_owned());
        self.loading = !query.trim().is_empty();
        // Keep the last complete list while debouncing/reading the next query.
        // Empty queries and scope changes intentionally clear it.
        if !self.loading {
            self.clear_results();
        }
        Some(self.generation.fetch_add(1, Ordering::Relaxed) + 1)
    }

    fn replace_results(&mut self, result: Results) {
        self.selected = self
            .selected
            .and_then(|index| self.results.hits.get(index))
            .and_then(|selected| {
                result.hits.iter().position(|hit| {
                    hit.path == selected.path
                        && hit.live == selected.live
                        && hit.message == selected.message
                })
            });
        self.results = result;
        self.loading = false;
    }
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

fn cached_snapshot<K: std::hash::Hash + Eq + Copy>(
    cache: &mut std::collections::HashMap<K, (u64, Arc<Vec<String>>)>,
    key: K,
    revision: u64,
    messages: &[crate::state::Message],
) -> Arc<Vec<String>> {
    match cache.get(&key) {
        Some((cached, snapshot)) if *cached == revision => snapshot.clone(),
        _ => {
            let snapshot = Arc::new(messages.iter().map(message_text).collect::<Vec<_>>());
            cache.insert(key, (revision, snapshot.clone()));
            snapshot
        }
    }
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
        let scope = self.current_search_scope(cx);
        self.content_search.set_scope(scope);
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
        let scope = self.current_search_scope(cx);
        if self.content_search.set_scope(scope) {
            self.run_content_search(cx);
        }
    }

    fn current_search_scope(&self, cx: &App) -> (PathBuf, Option<EntityId>) {
        let conversation = &self.conversations[self.active];
        (
            conversation.state.read(cx).cwd.clone(),
            (!self.content_search.project).then_some(conversation.state.entity_id()),
        )
    }

    pub(super) fn invalidate_content_search(&mut self, cx: &mut Context<Self>) {
        self.content_search.last_query = None;
        // Explicit structural changes (closing a conversation) invalidate live
        // indices. Rendering, catalog polling and streaming never do this.
        self.content_search.clear_results();
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
        let Some(ticket) = self.content_search.begin_query(&query) else {
            return;
        };
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
                    workspace.content_search.replace_results(result);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Snapshot live content only after debounce. The per-conversation cache
    /// is keyed by the transcript revision, so typing another query reuses the
    /// previous snapshot instead of rebuilding every message on the UI thread.
    fn search_documents(&mut self, cx: &mut Context<Self>) -> Vec<Document> {
        let cwd = self.conversations[self.active].state.read(cx).cwd.clone();
        let mut documents = Vec::new();
        let mut snapshots = std::mem::take(&mut self.content_search.live_snapshots);
        let live: std::collections::HashSet<_> = self
            .conversations
            .iter()
            .map(|conversation| conversation.state.entity_id())
            .collect();
        snapshots.retain(|entity, _| live.contains(entity));
        for (i, conversation) in self.conversations.iter().enumerate() {
            let entity = conversation.state.entity_id();
            let state = conversation.state.read(cx);
            if (self.content_search.project && state.cwd == cwd)
                || (!self.content_search.project && i == self.active)
            {
                let snapshot = cached_snapshot(
                    &mut snapshots,
                    entity,
                    state.transcript_revision(),
                    &state.messages,
                );
                documents.push(Document {
                    path: conversation.path.clone(),
                    live: Some(i),
                    title: self.conversation_title(i, cx),
                    messages: Some(snapshot),
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
        self.content_search.live_snapshots = snapshots;
        documents
    }

    pub(super) fn step_match(
        &mut self,
        direction: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.search_allowed(cx) || self.content_search.loading {
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
        if !self.search_allowed(cx)
            || self.content_search.loading
            || self.content_search.results.hits.is_empty()
        {
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
        Some(panel.child(hits).test_probe(|| "content-search".into(), || serde_json::json!({
            "query": self.content_search.editor.as_ref().map(|editor| editor.read(cx).text(cx)),
            "loading": self.content_search.loading,
            "hits": self.content_search.results.hits.iter().map(|hit| serde_json::json!({
                "title": hit.title, "snippet": hit.snippet,
            })).collect::<Vec<_>>(),
            "selected": self.content_search.selected,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::{cached_snapshot, Hit, Results, SearchState};
    use crate::state::{Block, Message, Role};
    use std::{
        path::PathBuf,
        sync::{atomic::Ordering, Arc},
    };

    fn hit(message: usize) -> Hit {
        Hit {
            path: None,
            live: Some(0),
            title: "synthetic session".into(),
            snippet: "needle".into(),
            message,
            occurrences: 1,
        }
    }

    fn settled() -> SearchState {
        let mut state = SearchState::default();
        state.set_scope((PathBuf::from("/synthetic/project"), None));
        state.begin_query("needle");
        state.replace_results(Results {
            hits: vec![hit(0), hit(1)],
            ..Results::default()
        });
        state.selected = Some(1);
        state
    }

    #[test]
    fn repeated_renders_and_editor_notifications_keep_snapshot_and_selection() {
        let mut state = settled();
        let ticket = state.generation.load(Ordering::Relaxed);
        for _ in 0..100 {
            assert!(!state.set_scope((PathBuf::from("/synthetic/project"), None)));
            assert_eq!(state.begin_query("needle"), None);
            assert_eq!(state.selected, Some(1));
            assert_eq!(state.results.hits.len(), 2);
            assert!(!state.loading);
        }
        assert_eq!(state.generation.load(Ordering::Relaxed), ticket);
    }

    #[test]
    fn editing_query_keeps_complete_list_until_replacement_is_ready() {
        let mut state = settled();
        let old = state.generation.load(Ordering::Relaxed);
        let ticket = state.begin_query("needle extended").unwrap();
        assert!(ticket > old);
        assert!(state.loading);
        assert_eq!(state.results.hits.len(), 2);
        assert_eq!(state.selected, Some(1));
        state.replace_results(Results {
            hits: vec![hit(1), hit(0)],
            ..Results::default()
        });
        assert_eq!(state.selected, Some(0));
        assert!(!state.loading);
    }

    #[test]
    fn changed_scope_clears_old_results_even_when_query_is_identical() {
        let mut state = settled();
        assert!(state.set_scope((PathBuf::from("/synthetic/other-project"), None)));
        assert!(state.results.hits.is_empty());
        assert_eq!(state.selected, None);
        assert!(state.begin_query("needle").is_some());
    }

    #[test]
    fn live_snapshots_are_reused_until_the_transcript_revision_changes() {
        let mut cache = std::collections::HashMap::new();
        let message = Message {
            role: Role::User,
            blocks: vec![Block::Text("snapshot text".into())],
            streaming: false,
            timestamp: 0,
            timestamp_origin: Default::default(),
            timing: Default::default(),
            finished_at: 0,
            usage: None,
            stop_reason: None,
            error: None,
            local: false,
            model: None,
        };

        let first = cached_snapshot(&mut cache, 7usize, 1, std::slice::from_ref(&message));
        let second = cached_snapshot(&mut cache, 7usize, 1, std::slice::from_ref(&message));
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.as_slice(), ["snapshot text"]);

        let changed = cached_snapshot(&mut cache, 7usize, 2, std::slice::from_ref(&message));
        assert!(!Arc::ptr_eq(&first, &changed));
    }

    #[test]
    fn empty_query_clears_list_and_disappearing_selection_is_not_retargeted() {
        let mut state = settled();
        state.replace_results(Results {
            hits: vec![hit(0)],
            ..Results::default()
        });
        assert_eq!(state.selected, None);
        assert!(state.begin_query(" ").is_some());
        assert!(state.results.hits.is_empty());
        assert!(!state.loading);
    }
}
