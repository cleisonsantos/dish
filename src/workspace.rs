use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::*;
use gpui::*;

use crate::editor::Editor;
use crate::rpc::{PiClient, PiConfig};
use crate::sessions::{self, SessionInfo};
use crate::state::AppState;
use crate::theme;
use crate::session_activity::{SessionFilter, SessionSignals, epoch_millis, from_epoch_millis, relative_time};

actions!(dish_workspace, [ToggleSessions, NextConversation, PreviousConversation, CloseConversation, SearchSessions, FocusPrompt, NavUp, NavDown, NavAccept, NavCollapse, NavExpand, CycleSessionFilter]);

struct Conversation {
    state: Entity<AppState>,
    path: Option<PathBuf>,
    _subscription: Subscription,
    seen_completed_runs: u64,
    unread_completion: bool,
}

/// A session sitting in the trash while the undo window is open.
struct DeletedSession {
    info: SessionInfo,
    trashed: PathBuf,
    meta: PathBuf,
    id: u64,
}

/// O que está sob o mouse na faixa recolhida.
enum NavHover {
    Project(PathBuf),
    Conversation(usize),
    /// Índice no catálogo de sessões salvas.
    Saved(usize),
}

/// Uma linha navegável da lista de sessões.
#[derive(Clone, PartialEq)]
enum NavEntry {
    Open(usize),
    Saved(PathBuf),
}

impl SessionSignals {
    fn from_state(state: &AppState, unread: bool) -> Self {
        Self {
            running: state.is_executing(),
            needs_input: state.modal.is_some(),
            unread,
            queued: state.steering_queue.len() + state.follow_up_queue.len(),
            error: state.activity_error || state.disconnected,
            interrupted: state.activity_interrupted,
            saved: false,
        }
    }
}

type SessionRow = (
    String,
    Option<SessionInfo>,
    Option<usize>,
    std::time::SystemTime,
);

fn attention_summary(pending: usize, unread: usize) -> String {
    let mut labels = Vec::new();
    if pending > 0 { labels.push(format!("{pending} pend.")); }
    if unread > 0 { labels.push(format!("{unread} {}", if unread == 1 { "nova" } else { "novas" })); }
    labels.join(" · ")
}

fn sort_session_rows(items: &mut [SessionRow], previous: &[String], interacting: bool) {
    items.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| row_key(a).cmp(&row_key(b))));
    if interacting {
        items.sort_by_key(|item| previous.iter().position(|key| *key == row_key(item)).unwrap_or(usize::MAX));
    }
}

fn entry_key(entry: &NavEntry) -> String {
    match entry {
        NavEntry::Open(index) => format!("live-{index}"),
        NavEntry::Saved(path) => format!("saved-{}", path.display()),
    }
}

fn row_key(row: &SessionRow) -> String {
    row.2.map(|index| format!("live-{index}"))
        .unwrap_or_else(|| format!("saved-{}", row.1.as_ref().map(|info| info.path.to_string_lossy()).unwrap_or_default()))
}

fn select_item<'a, T>(items: &'a mut [T], active: &mut usize, index: usize) -> Option<&'a mut T> {
    let item = items.get_mut(index)?;
    *active = index;
    Some(item)
}
pub struct Workspace {
    conversations: Vec<Conversation>,
    active: usize,
    catalog: Vec<SessionInfo>,
    roots: Vec<PathBuf>,
    flags: Vec<String>,
    collapsed: BTreeSet<PathBuf>,
    visible: bool,
    /// Largura do painel de navegação, ajustável pela divisa.
    nav_width: f32,
    /// (x inicial, largura inicial) enquanto a divisa é arrastada.
    resizing_nav: Option<(f32, f32)>,
    /// Navegação por teclado na lista de sessões.
    nav_entries: Vec<NavEntry>,
    nav_index: usize,
    nav_query: String,
    nav_filter: SessionFilter,
    nav_last_project: Option<PathBuf>,
    nav_scroll: ScrollHandle,
    refreshing: bool,
    opening: Option<PathBuf>,
    error: Option<String>,
    /// Sessão salva aguardando confirmação de exclusão.
    pending_delete: Option<PathBuf>,
    /// Última sessão excluída, enquanto o "Desfazer" está disponível.
    undo_delete: Option<DeletedSession>,
    undo_seq: u64,
    confirm_close: bool,
    close_target: Option<usize>,
    pending: Option<(PathBuf, Option<PathBuf>, PiClient)>,
    confirm_focus: FocusHandle,
    /// Campo de busca da navegação.
    query: Entity<Editor>,
    filter: SessionFilter,
    nav_hovered: bool,
    row_order: Vec<String>,
    nav_anchors: BTreeMap<String, ScrollAnchor>,
    /// Se o usuário já decidiu mostrar/ocultar a navegação nesta janela.
    nav_user_choice: bool,
    /// O que está sob o mouse na faixa recolhida, para o chip de título.
    hovered: Option<NavHover>,
    preferences: crate::preferences::Preferences,
    settings: Option<Entity<crate::settings::Settings>>,
}

impl Workspace {
    pub fn new(
        state: Entity<AppState>,
        flags: Vec<String>,
        query: Entity<Editor>,
        cx: &mut Context<Self>,
    ) -> Self {
        let preferences = crate::preferences::load();
        if let Some(open) = preferences.details_open {
            state.update(cx, |state, _| {
                state.sidebar = open;
                state.inspector_initialized = true;
            });
        }
        if let Some(show) = preferences.show_thinking {
            state.update(cx, |state, _| state.show_thinking = show);
        }
        cx.observe(&query, |_, _, cx| cx.notify()).detach();
        let mut roots = sessions::roots(&state.read(cx).cwd, &flags);
        let cwd = state.read(cx).cwd.clone();
        let initial_path = flags
            .windows(2)
            .find(|pair| pair[0] == "--session")
            .filter(|pair| pair[1].ends_with(".jsonl"))
            .map(|pair| sessions::expand_path(&pair[1], &cwd))
            .or_else(|| {
                flags
                    .iter()
                    .find_map(|flag| flag.strip_prefix("--session="))
                    .filter(|path| path.ends_with(".jsonl"))
                    .map(|path| sessions::expand_path(path, &cwd))
            });
        if let Some(parent) = initial_path.as_ref().and_then(|path| path.parent()) {
            roots.push(parent.to_owned());
        }
        let mut launch_flags = sessions::launch_flags(&flags);
        let path_flags = [
            "--session-dir",
            "--extension",
            "-e",
            "--skill",
            "--prompt-template",
            "--theme",
        ];
        for index in 0..launch_flags.len() {
            if path_flags.contains(&launch_flags[index].as_str()) && index + 1 < launch_flags.len()
            {
                let value = &launch_flags[index + 1];
                if value.starts_with('.') || value.starts_with('~') || cwd.join(value).exists() {
                    launch_flags[index + 1] = sessions::expand_path(value, &cwd)
                        .to_string_lossy()
                        .into_owned();
                }
            } else if let Some(value) = launch_flags[index].strip_prefix("--session-dir=") {
                launch_flags[index] = format!(
                    "--session-dir={}",
                    sessions::expand_path(value, &cwd).display()
                );
            }
        }
        let subscription = cx.observe(&state, |_, _, cx| cx.notify());
        Self {
            conversations: vec![Conversation {
                state,
                path: initial_path.map(|path| path.canonicalize().unwrap_or(path)),
                _subscription: subscription,
                seen_completed_runs: 0,
                unread_completion: false,
            }],
            active: 0,
            catalog: Vec::new(),
            roots,
            flags: launch_flags,
            collapsed: preferences.collapsed_projects.clone(),
            visible: preferences.navigation_open.unwrap_or(true),
            nav_width: preferences.navigation_width.unwrap_or(264.).clamp(200., 420.),
            resizing_nav: None,
            nav_entries: Vec::new(),
            nav_index: 0,
            nav_query: String::new(),
            nav_filter: preferences.session_filter,
            nav_last_project: None,
            nav_scroll: ScrollHandle::new(),
            refreshing: false,
            opening: None,
            error: None,
            pending_delete: None,
            undo_delete: None,
            undo_seq: 0,
            confirm_close: false,
            close_target: None,
            pending: None,
            confirm_focus: cx.focus_handle(),
            query,
            filter: preferences.session_filter,
            nav_hovered: false,
            row_order: Vec::new(),
            nav_anchors: BTreeMap::new(),
            nav_user_choice: preferences.navigation_open.is_some(),
            hovered: None,
            preferences,
            settings: None,
        }
    }

    pub fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_window, cx| {
            weak.update(cx, |workspace, cx| {
                if workspace
                    .conversations
                    .iter()
                    .any(|c| c.state.read(cx).is_busy())
                {
                    workspace.confirm_close = true;
                    workspace.close_target = None;
                    cx.notify();
                    false
                } else {
                    true
                }
            })
            .unwrap_or(true)
        });
        self.refresh(cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_secs(15))
                .await;
            if this
                .update(cx, |workspace, cx| workspace.refresh(cx))
                .is_err()
            {
                break;
            }
        })
        .detach();
    }

    fn open_settings(&mut self, section: crate::settings::Section, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm_close || self.conversations[self.active].state.read(cx).modal.is_some() { return; }
        if self.settings.is_none() {
            let settings = cx.new(|cx| crate::settings::Settings::new(window, cx));
            cx.observe(&settings, |_, _, cx| cx.notify()).detach();
            cx.subscribe(&settings, |this, _, event, cx| {
                use crate::settings::SettingsEvent;
                match event {
                    SettingsEvent::Preference(name, value) => {
                        if *name == "navigation" { this.visible = *value; this.nav_user_choice = true; }
                        for conversation in &this.conversations {
                            conversation.state.update(cx, |state, cx| {
                                match *name {
                                    "details" => { state.sidebar = *value; state.inspector_initialized = true; }
                                    "thinking" => state.show_thinking = *value,
                                    _ => {}
                                }
                                cx.notify();
                            });
                        }
                    }
                    SettingsEvent::Closed => {}
                }
                cx.notify();
            }).detach();
            self.settings = Some(settings);
        }
        let state = self.conversations[self.active].state.read(cx);
        let options = [state.sidebar, self.visible, state.show_thinking];
        self.conversations[self.active].state.update(cx, |state, _| {
            state.visible = false;
            state.model_menu = false;
            state.model_search_focus_pending = false;
            state.effort_menu = false;
        });
        self.settings.as_ref().unwrap().update(cx, |settings, cx| settings.show(section, options, window, cx));
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refreshing {
            return;
        }
        self.refreshing = true;
        let roots = self.roots.clone();
        let previous = self.catalog.clone();
        cx.spawn(async move |this, cx| {
            let catalog = cx
                .background_executor()
                .spawn(async move { sessions::discover_cached(&roots, &previous) })
                .await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.catalog = catalog;
                workspace.refreshing = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn select(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(conversation) = select_item(&mut self.conversations, &mut self.active, index) else {
            return;
        };
        conversation.unread_completion = false;
        conversation.seen_completed_runs = conversation.state.read(cx).completed_content_runs;
        if let Some(path) = &conversation.path {
            if self.preferences.unread_sessions.remove(path).is_some() {
                crate::preferences::save(self.preferences.clone());
            }
        }
        let state = conversation.state.clone();
        let focus = {
            let state = state.read(cx);
            if state.modal.as_ref().is_some_and(|modal| modal.wants_input) {
                state.modal_editor.read(cx).focus_handle.clone()
            } else if state.model_menu {
                state.model_search.read(cx).focus_handle.clone()
            } else {
                state.editor.read(cx).focus_handle.clone()
            }
        };
        focus.focus(window, cx);
        cx.notify();
    }

    fn open(&mut self, info: SessionInfo, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self
            .conversations
            .iter()
            .position(|c| c.path.as_ref() == Some(&info.path))
        {
            self.select(index, window, cx);
            return;
        }
        if self.conversations[0].state.read(cx).loading {
            self.error =
                Some("Wait for the initial session to load before opening a saved session.".into());
            cx.notify();
            return;
        }
        if self.opening.is_some() || self.pending.is_some() {
            return;
        }
        if !info.cwd.is_dir() {
            self.error = Some(format!(
                "Project folder no longer exists: {}",
                info.cwd.display()
            ));
            cx.notify();
            return;
        }
        self.spawn_conversation(info.cwd, Some(info.path), cx);
    }

    /// Move o destaque na lista de sessões (abertas e salvas).
    fn nav_move(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav_entries.is_empty() {
            return;
        }
        let count = self.nav_entries.len();
        self.nav_index = (self.nav_index as isize + delta).rem_euclid(count as isize) as usize;
        if let Some(anchor) = self.nav_anchors.get(&entry_key(&self.nav_entries[self.nav_index])) { anchor.scroll_to(window, cx); }
        cx.notify();
    }

    /// Abre a sessão destacada na lista de sessões.
    fn nav_accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.nav_entries.get(self.nav_index).cloned() else {
            return;
        };
        match entry {
            NavEntry::Open(index) => self.select(index, window, cx),
            NavEntry::Saved(path) => {
                let Some(info) = self
                    .catalog
                    .iter()
                    .find(|info| info.path == path)
                    .cloned()
                else {
                    return;
                };
                self.open(info, window, cx);
            }
        }
    }

    /// Recolhe ou expande o projeto da sessão destacada.
    fn nav_toggle_project(&mut self, collapse: bool, cx: &mut Context<Self>) {
        if !collapse {
            if let Some(cwd) = &self.nav_last_project {
                if self.collapsed.remove(cwd) { cx.notify(); return; }
            }
        }
        let cwd = match self.nav_entries.get(self.nav_index) {
            Some(NavEntry::Open(index)) => self
                .conversations
                .get(*index)
                .map(|conversation| conversation.state.read(cx).cwd.clone()),
            Some(NavEntry::Saved(path)) => self
                .catalog
                .iter()
                .find(|info| &info.path == path)
                .map(|info| info.cwd.clone()),
            None => self.nav_last_project.clone(),
        };
        let Some(cwd) = cwd else {
            return;
        };
        self.nav_last_project = Some(cwd.clone());
        if collapse {
            self.collapsed.insert(cwd);
        } else {
            self.collapsed.remove(&cwd);
        }
        cx.notify();
    }

    fn spawn_conversation(&mut self, cwd: PathBuf, path: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.opening.is_some() || self.pending.is_some() {
            return;
        }
        self.opening = Some(path.clone().unwrap_or_else(|| cwd.clone()));
        self.error = None;
        let mut flags = self.flags.clone();
        if let Some(path) = &path {
            flags.extend(["--session".into(), path.to_string_lossy().into_owned()]);
        }
        let config = PiConfig::new(cwd.clone()).with_args(flags);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { PiClient::spawn(&config) })
                .await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.opening = None;
                match result {
                    Ok(client) => {
                        // Entity construction needs the window and is deferred to render.
                        workspace.pending = Some((cwd, path, client));
                    }
                    Err(error) => workspace.error = Some(format!("Cannot open session: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn close_conversation(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.conversations.get(index).is_none() {
            return;
        }
        if self.conversations.len() == 1 {
            self.error = Some(
                "Keep at least one session open. Create another before closing this one.".into(),
            );
            cx.notify();
            return;
        }
        self.conversations.remove(index);
        if self.active > index {
            self.active -= 1;
        }
        self.active = self.active.min(self.conversations.len() - 1);
        self.select(self.active, window, cx);
    }

    /// Move uma sessão salva para a lixeira e oferece "Desfazer" por 10s.
    fn delete_session(&mut self, info: SessionInfo, cx: &mut Context<Self>) {
        match sessions::trash(&info) {
            Ok(trashed) => {
                drop_from_catalog(&mut self.catalog, &info.path);
                self.pending_delete = None;
                self.undo_seq += 1;
                let id = self.undo_seq;
                self.undo_delete = Some(DeletedSession {
                    info,
                    trashed: trashed.trashed,
                    meta: trashed.meta,
                    id,
                });
                cx.notify();
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_secs(10))
                        .await;
                    this.update(cx, |workspace, cx| {
                        if workspace
                            .undo_delete
                            .as_ref()
                            .is_some_and(|undo| undo.id == id)
                        {
                            workspace.undo_delete = None;
                            cx.notify();
                        }
                    })
                    .ok();
                })
                .detach();
            }
            Err(error) => {
                self.pending_delete = None;
                self.error = Some(format!("Cannot delete session: {error}"));
                cx.notify();
            }
        }
    }

    /// Devolve a última sessão excluída para o lugar de onde saiu.
    fn undo_delete(&mut self, cx: &mut Context<Self>) {
        let Some(undo) = self.undo_delete.take() else {
            return;
        };
        match sessions::restore(&undo.trashed, &undo.info.path) {
            Ok(()) => {
                let _ = std::fs::remove_file(&undo.meta);
                add_to_catalog(&mut self.catalog, undo.info);
                self.refresh(cx);
            }
            Err(error) => {
                self.undo_delete = Some(undo);
                self.error = Some(format!("Cannot restore session: {error}"));
            }
        }
        cx.notify();
    }

    /// Barra "Sessão excluída — Desfazer" acima do rodapé da navegação.
    fn undo_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(undo) = &self.undo_delete else {
            return div().into_any_element();
        };
        let title = undo.info.title.clone();
        let weak = cx.entity().downgrade();
        div()
            .id("undo-session-delete")
            .flex_none()
            .mx(px(theme::S3))
            .mb(px(theme::S2))
            .px(px(theme::S2 + theme::S1))
            .py(px(theme::S1 + 2.))
            .rounded(theme::r_control())
            .border_1()
            .border_color(theme::line())
            .bg(theme::surface_2())
            .flex()
            .flex_row()
            .items_center()
            .gap(px(theme::S2))
            .child(crate::ui::icons::icon(
                crate::ui::icons::Icon::Trash,
                12.,
                theme::faint(),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(px(theme::TEXT_XS))
                    .text_color(theme::dim())
                    .child(SharedString::from(format!("“{title}” excluída"))),
            )
            .child(
                div()
                    .id("undo-session-delete-action")
                    .cursor_pointer()
                    .text_size(px(theme::TEXT_XS))
                    .text_color(theme::running())
                    .hover(|style| style.text_color(theme::text()))
                    .on_click(move |_event: &ClickEvent, _, cx: &mut App| {
                        weak.update(cx, |workspace, cx| workspace.undo_delete(cx))
                            .ok();
                    })
                    .child("Desfazer"),
            )
            .into_any_element()
    }

    /// Título de uma conversa aberta: nome da sessão, título salvo, ou o
    /// primeiro pedido do usuário.
    fn conversation_title(&self, index: usize, cx: &App) -> String {
        let Some(conversation) = self.conversations.get(index) else {
            return "Nova conversa".into();
        };
        let state = conversation.state.read(cx);
        state
            .session
            .name
            .clone()
            .or_else(|| {
                conversation.path.as_ref().and_then(|path| {
                    self.catalog
                        .iter()
                        .find(|info| &info.path == path)
                        .map(|info| info.title.clone())
                })
            })
            .or_else(|| {
                state
                    .messages
                    .iter()
                    .find(|message| message.role == crate::state::Role::User)
                    .map(|message| {
                        message
                            .text()
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .chars()
                            .take(80)
                            .collect()
                    })
            })
            .unwrap_or_else(|| "Nova conversa".into())
    }

    /// Navegação: cabeçalho integrado, busca, filtros e a lista de projetos.
    /// A identidade, a busca e "nova sessão" ficam no mesmo lugar — não há mais
    /// uma barra "Sessions" separada gastando uma linha inteira.
    fn navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let query = self.query.read(cx).text(cx).to_lowercase();
        let filter = self.filter;

        let mut groups: BTreeMap<PathBuf, Vec<SessionRow>> = BTreeMap::new();
        for (index, conversation) in self.conversations.iter().enumerate() {
            let state = conversation.state.read(cx);
            let title = self.conversation_title(index, cx);
            // Recência observada nos limites de atividade, nunca nos deltas.
            // Sem atividade ao vivo: prompt, mtime salvo ou criação da conversa.
            let last_run = state
                .last_activity.or(state.last_run)
                .or_else(|| {
                    conversation.path.as_ref().and_then(|path| {
                        self.catalog
                            .iter()
                            .find(|info| &info.path == path)
                            .map(|info| info.modified)
                    })
                })
                .unwrap_or(state.created_at);
            groups.entry(state.cwd.clone()).or_default().push((
                title,
                None,
                Some(index),
                last_run,
            ));
        }
        for info in &self.catalog {
            if self
                .conversations
                .iter()
                .any(|c| c.path.as_ref() == Some(&info.path))
            {
                continue;
            }
            groups.entry(info.cwd.clone()).or_default().push((
                info.title.clone(),
                Some(info.clone()),
                None,
                self.preferences.unread_sessions.get(&info.path).copied().map(from_epoch_millis).unwrap_or(info.modified),
            ));
        }

        // Filtros independentes de atenção, leitura e atividade.
        if !query.is_empty() || filter != SessionFilter::All {
            groups.retain(|cwd, items| {
                let project_matches = cwd
                    .file_name()
                    .map(|name| name.to_string_lossy().to_lowercase().contains(&query))
                    .unwrap_or(false);
                items.retain(|(title, info, live, _)| {
                    let signals = live.map(|index| {
                        let conversation = &self.conversations[index];
                        SessionSignals::from_state(conversation.state.read(cx), conversation.unread_completion)
                    }).unwrap_or(SessionSignals {
                        saved: true,
                        unread: info.as_ref().is_some_and(|info| self.preferences.unread_sessions.contains_key(&info.path)),
                        ..Default::default()
                    });
                    if !signals.matches(filter) {
                        return false;
                    }
                    if project_matches || query.is_empty() {
                        return true;
                    }
                    title.to_lowercase().contains(&query)
                });
                !items.is_empty()
            });
        }

        // Novos resultados de busca/filtro são revelados; recolher manualmente
        // continua funcionando e a seta direita pode reabrir o último projeto.
        if self.nav_query != query || self.nav_filter != filter {
            for cwd in groups.keys() { self.collapsed.remove(cwd); }
            self.nav_filter = filter;
        }
        // A mesma ordem estável alimenta o desenho e a navegação por teclado.
        for items in groups.values_mut() {
            sort_session_rows(items, &self.row_order,
                self.nav_hovered || self.query.read(cx).focus_handle.is_focused(window));
        }
        let mut entries: Vec<NavEntry> = Vec::new();
        for (cwd, items) in &groups {
            if !self.collapsed.contains(cwd) {
                for (_, info, live, _) in items {
                    if let Some(index) = live {
                        entries.push(NavEntry::Open(*index));
                    } else if let Some(info) = info {
                        entries.push(NavEntry::Saved(info.path.clone()));
                    } else {
                        continue;
                    }
                }
            }
        }
        let previous = self.nav_entries.get(self.nav_index);
        if self.nav_query != query {
            self.nav_query = query.clone();
            self.nav_index = 0;
        } else if let Some(index) = previous.and_then(|previous| entries.iter().position(|entry| entry == previous)) {
            self.nav_index = index;
        }
        self.nav_entries = entries;
        self.nav_index = self.nav_index.min(self.nav_entries.len().saturating_sub(1));
        let highlighted = self.nav_entries.get(self.nav_index).cloned();

        let open_count = self.conversations.len();
        let running_count = self
            .conversations
            .iter()
            .filter(|c| c.state.read(cx).is_executing())
            .count();
        let focus = self.query.read(cx).focus_handle.clone();
        let query_editor = self.query.clone();
        let focus_for_click = focus.clone();

        let mut rail = div()
            .id("sessions-navigation")
            .on_hover(cx.listener(|workspace, hovered: &bool, _, cx| { workspace.nav_hovered = *hovered; cx.notify(); }))
            .w(px(self.nav_width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme::surface())
            .border_r_1()
            .border_color(theme::line_soft())
            // cabeçalho integrado
            .child(
                div()
                    .flex_none()
                    .h(px(52.))
                    .pl(px(theme::S4))
                    .pr(px(theme::S2))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("nav-collapse")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S2))
                            .px(px(theme::S2))
                            .py(px(theme::S1))
                            .rounded(theme::r_control())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::hover()))
                            .on_click(cx.listener(|workspace, _: &ClickEvent, _, cx| {
                                workspace.visible = false;
                                workspace.nav_user_choice = true;
                                cx.notify();
                            }))
                            .child(img("dish-icon.png").flex_none().w(px(22.)).h(px(22.)))
                            .child(
                                div()
                                    .text_size(px(theme::TEXT))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Dish"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S1))
                            .child(nav_icon_button(
                                "nav-new-session",
                                crate::ui::icons::Icon::Plus,
                                cx,
                                |workspace, window, cx| {
                                    let cwd = workspace.conversations[workspace.active]
                                        .state
                                        .read(cx)
                                        .cwd
                                        .clone();
                                    workspace.spawn_conversation(cwd, None, cx);
                                    let _ = window;
                                },
                            ))
                            .child(nav_icon_button(
                                "nav-help",
                                crate::ui::icons::Icon::Dots,
                                cx,
                                |workspace, window, cx| {
                                    workspace.open_settings(crate::settings::Section::App, window, cx);
                                },
                            )),
                    ),
            )
            // busca + filtros
            .child(
                div()
                    .flex_none()
                    .px(px(theme::S3))
                    .pb(px(theme::S3))
                    .flex()
                    .flex_col()
                    .gap(px(theme::S2))
                    .child(
                        div()
                            .id("nav-search")
                            .key_context("DishNavSearch")
                            .track_focus(&focus)
                            .cursor_text()
                            .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                                focus_for_click.focus(window, cx);
                            })
                            .map(crate::editor::standard_actions(query_editor.clone()))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S2))
                            .px(px(theme::S2 + theme::S1))
                            .py(px(theme::S1 + 2.))
                            .rounded(theme::r_control())
                            .bg(theme::inset())
                            .border_1()
                            .border_color(theme::line_soft())
                            .child(crate::ui::icons::icon(
                                crate::ui::icons::Icon::Search,
                                14.,
                                theme::faint(),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(theme::TEXT_SM))
                                    .child(query_editor.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap(px(theme::S1))
                            .child(nav_filter_chip(
                                "nav-filter-all",
                                "Todas",
                                self.filter == SessionFilter::All,
                                cx,
                                |workspace, cx| {
                                    workspace.filter = SessionFilter::All;
                                    cx.notify();
                                },
                            ))
                            .child(nav_filter_chip("nav-filter-input", "Precisa de você", self.filter == SessionFilter::NeedsInput, cx, |workspace, cx| {
                                workspace.filter = SessionFilter::NeedsInput; cx.notify();
                            }))
                            .child(nav_filter_chip("nav-filter-unread", "Não lidas", self.filter == SessionFilter::Unread, cx, |workspace, cx| {
                                workspace.filter = SessionFilter::Unread; cx.notify();
                            }))
                            .child(nav_filter_chip(
                                "nav-filter-running",
                                "Executando",
                                self.filter == SessionFilter::Running,
                                cx,
                                |workspace, cx| {
                                    workspace.filter = SessionFilter::Running;
                                    cx.notify();
                                },
                            )),
                    ),
            );

        if let Some(error) = self.error.clone() {
            let _ = window;
            rail = rail.child(
                div()
                    .px(px(theme::S3))
                    .pb(px(theme::S2))
                    .text_size(px(theme::TEXT_XS))
                    .text_color(theme::failure())
                    .child(SharedString::from(error)),
            );
        }

        let mut list = div()
            .id("nav-list")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .track_scroll(&self.nav_scroll)
            .px(px(theme::S2))
            .pb(px(theme::S4))
            .flex()
            .flex_col()
            .gap(px(theme::S1));

        if groups.is_empty() {
            let label = if !query.is_empty() {
                format!("Nada corresponde a “{query}”")
            } else {
                "Nenhuma sessão neste filtro".to_string()
            };
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme::S2))
                    .px(px(theme::S2))
                    .py(px(theme::S4))
                    .child(
                        div()
                            .text_size(px(theme::TEXT_SM))
                            .text_color(theme::dim())
                            .child(SharedString::from(label)),
                    )
                    .child(
                        div()
                            .id("nav-clear-filter")
                            .text_size(px(theme::TEXT_SM))
                            .text_color(theme::running())
                            .cursor_pointer()
                            .hover(|style| style.text_color(theme::text()))
                            .on_click(cx.listener(|workspace, _: &ClickEvent, _, cx| {
                                workspace.filter = SessionFilter::All;
                                workspace.query.update(cx, |editor, cx| editor.clear(cx));
                                cx.notify();
                            }))
                            .child("Limpar filtro"),
                    ),
            );
        }

        for (cwd, items) in groups {
            let collapsed = self.collapsed.contains(&cwd);
            let click_cwd = cwd.clone();
            let running = items
                .iter()
                .filter(|(_, _, live, _)| match live {
                    Some(index) => self.conversations[*index].state.read(cx).is_executing(),
                    None => false,
                })
                .count();
            let needs_input = items.iter().filter(|(_, _, live, _)| live.is_some_and(|i| self.conversations[i].state.read(cx).modal.is_some())).count();
            let unread_count = items.iter().filter(|(_, info, live, _)| live.map(|i| self.conversations[i].unread_completion).unwrap_or_else(|| info.as_ref().is_some_and(|info| self.preferences.unread_sessions.contains_key(&info.path)))).count();
            if !collapsed {
                for item in &items {
                    self.nav_anchors.entry(row_key(item)).or_insert_with(|| ScrollAnchor::for_handle(self.nav_scroll.clone()));
                }
            }

            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme::S1))
                    .mb(px(theme::S2))
                    .child(
                        div()
                            .id(SharedString::from(format!("project-{cwd:?}")))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S2))
                            .px(px(theme::S2))
                            .py(px(theme::S1 + 2.))
                            .rounded(theme::r_control())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::hover()))
                            .on_click(cx.listener(move |workspace, _: &ClickEvent, _, cx| {
                                if !workspace.collapsed.remove(&click_cwd) {
                                    workspace.collapsed.insert(click_cwd.clone());
                                }
                                cx.notify();
                            }))
                            .child(crate::ui::icons::icon(
                                if collapsed {
                                    crate::ui::icons::Icon::ChevronRight
                                } else {
                                    crate::ui::icons::Icon::ChevronDown
                                },
                                13.,
                                theme::faint(),
                            ))
                            .child(crate::ui::icons::icon(
                                crate::ui::icons::Icon::Folder,
                                14.,
                                theme::faint(),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(px(theme::TEXT_SM))
                                    .text_color(theme::dim())
                                    .child(SharedString::from(folder_of(&cwd))),
                            )
                            .when(needs_input > 0 || unread_count > 0, |el| {
                                el.child(div().text_size(px(theme::TEXT_XS)).text_color(theme::running())
                                    .child(SharedString::from(attention_summary(needs_input, unread_count))))
                            })
                            .when(running > 0, |el| {
                                el.child(crate::ui::icons::icon(
                                    crate::ui::icons::Icon::Activity,
                                    12.,
                                    theme::running(),
                                ))
                                .child(
                                    div()
                                        .text_size(px(theme::TEXT_XS))
                                        .text_color(theme::running())
                                        .child(SharedString::from(running.to_string())),
                                )
                            })
                            .child(
                                div()
                                    .text_size(px(theme::TEXT_XS))
                                    .text_color(theme::faint())
                                    .child(SharedString::from(items.len().to_string())),
                            ),
                    )
                    .when(!collapsed, |el| {
                        el.children(items.into_iter().map(
                            |(title, info, live, activity_time)| {
                                let key = row_key(&(title.clone(), info.clone(), live, activity_time));
                                let highlighted_here = match &highlighted {
                                    Some(NavEntry::Open(index)) => live == Some(*index),
                                    Some(NavEntry::Saved(path)) => info.as_ref().is_some_and(|info| &info.path == path),
                                    None => false,
                                };
                                let keyboard_selected = highlighted_here && self.query.read(cx).focus_handle.is_focused(window);
                                let signals = live.map(|i| {
                                    let conversation = &self.conversations[i];
                                    SessionSignals::from_state(conversation.state.read(cx), conversation.unread_completion)
                                }).unwrap_or(SessionSignals { saved: true,
                                    unread: info.as_ref().is_some_and(|info| self.preferences.unread_sessions.contains_key(&info.path)),
                                    ..Default::default() });
                                let running = signals.running;
                                let active = live == Some(self.active);
                                let unread = signals.unread;
                                let weak = cx.entity().downgrade();
                                let info = info.clone();
                                let delete_info = info.clone();
                                let live_index = live;
                                let pending = delete_info
                                    .as_ref()
                                    .is_some_and(|info| self.pending_delete.as_ref() == Some(&info.path));
                                let anchor = self.nav_anchors.get(&key).cloned();
                                div()
                                    .id(SharedString::from(key))
                                    .anchor_scroll(anchor)
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(theme::S2))
                                    .pl(px(theme::S3))
                                    .pr(px(theme::S2))
                                    .py(px(theme::S1 + 2.))
                                    .rounded(theme::r_control())
                                    .when(active, |el| {
                                        el.bg(theme::surface_2())
                                            .border_1()
                                            .border_color(theme::line())
                                    })
                                    .when(!active && unread, |el| el.bg(theme::wash(theme::ok(), 0.10)))
                                    .when(keyboard_selected, |el| el.bg(theme::hover()).border_1().border_color(theme::running()))
                                    .when(!active, |el| el.hover(|s| s.bg(theme::hover())))
                                    .cursor_pointer()
                                    .on_click(move |_event: &ClickEvent, window, cx: &mut App| {
                                        weak.update(cx, |workspace, cx| {
                                            match live_index {
                                                Some(index) => {
                                                    workspace.select(index, window, cx)
                                                }
                                                None => {
                                                    if let Some(info) = info.clone() {
                                                        workspace.open(info, window, cx)
                                                    }
                                                }
                                            }
                                        })
                                        .ok();
                                    })
                                    // Aberta (viva) ou apenas salva: forma, não só texto.
                                    .child(if live.is_some() {
                                        crate::ui::dot(if unread { theme::ok() } else { theme::dim() }, 6.)
                                    } else {
                                        crate::ui::ring(theme::faint(), 6.)
                                    })
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
                                                    .text_color(if active {
                                                        theme::text()
                                                    } else {
                                                        theme::dim()
                                                    })
                                                    .child(SharedString::from(title)),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .flex_wrap()
                                                    .items_center()
                                                    .gap(px(theme::S1 + 1.))
                                                    .when(running, |el| {
                                                        el.child(crate::ui::icons::icon(
                                                            crate::ui::icons::Icon::Activity,
                                                            11.,
                                                            theme::running(),
                                                        ))
                                                        .child(
                                                            div()
                                                                .text_size(px(theme::TEXT_XS))
                                                                .text_color(theme::running())
                                                                .child("executando"),
                                                        )
                                                    })
                                                    .when(unread, |el| {
                                                        el.child(crate::ui::icons::icon(
                                                            crate::ui::icons::Icon::File, 11., theme::ok(),
                                                        )).child(div().text_size(px(theme::TEXT_XS))
                                                            .text_color(theme::ok()).child("resposta nova"))
                                                    })
                                                    .when(signals.needs_input, |el| {
                                                        el.child(crate::ui::icons::icon(crate::ui::icons::Icon::Alert, 11., theme::running())).child(div().text_size(px(theme::TEXT_XS))
                                                            .text_color(theme::running()).child("precisa de você"))
                                                    })
                                                    .when(signals.queued > 0, |el| {
                                                        el.child(div().text_size(px(theme::TEXT_XS))
                                                            .text_color(theme::dim()).child(SharedString::from(format!("{} em fila", signals.queued))))
                                                    })
                                                    .when(signals.error, |el| {
                                                        el.child(crate::ui::icons::icon(crate::ui::icons::Icon::Alert, 11., theme::failure())).child(div().text_size(px(theme::TEXT_XS))
                                                            .text_color(theme::failure()).child("erro"))
                                                    })
                                                    .when(signals.interrupted, |el| {
                                                        el.child(crate::ui::icons::icon(crate::ui::icons::Icon::Stop, 11., theme::dim())).child(div().text_size(px(theme::TEXT_XS)).text_color(theme::dim()).child("interrompida"))
                                                    })
                                                    .when(!running && !signals.needs_input && !signals.error, |el| {
                                                        el.child(div().text_size(px(theme::TEXT_XS))
                                                            .text_color(theme::faint()).child(if live.is_some() { "inativa" } else { "salva" }))
                                                    })
                                                    .child(div().text_size(px(theme::TEXT_XS)).text_color(theme::faint())
                                                        .child(SharedString::from(relative_time(activity_time, std::time::SystemTime::now())))),
                                            )
                                            .when(running, |el| {
                                                el.child(div().truncate().text_size(px(theme::TEXT_XS)).text_color(theme::dim())
                                                    .child(SharedString::from(live.and_then(|i| self.conversations[i].state.read(cx).activity.clone()).unwrap_or_default().replace("Thinking…", "Pensando…").replace("Compacting…", "Compactando…"))))
                                            }),
                                    )
                                    .when_some(live_index, |el, index| {
                                        let weak = cx.entity().downgrade();
                                        el.child(
                                            div()
                                                .id(SharedString::from(format!("close-{index}")))
                                                .size(px(22.))
                                                .rounded(theme::r_control())
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .opacity(0.0)
                                                .hover(|s| s.opacity(1.0))
                                                .cursor_pointer()
                                                .on_click(move |_event: &ClickEvent, window, cx: &mut App| {
                                                    // Removing this row invalidates the index captured
                                                    // by its parent selection handler.
                                                    cx.stop_propagation();
                                                    weak.update(cx, |workspace, cx| {
                                                        workspace.close_conversation(index, window, cx)
                                                    })
                                                    .ok();
                                                })
                                                .child(crate::ui::icons::icon(
                                                    crate::ui::icons::Icon::Close,
                                                    12.,
                                                    theme::faint(),
                                                )),
                                        )
                                    })
                                    .when(live_index.is_none(), |el| match delete_info.clone() {
                                        Some(target) if pending => {
                                            el.child(delete_confirm_controls(target, cx))
                                        }
                                        Some(target) => el.child(delete_session_button(target, cx)),
                                        None => el,
                                    })
                            },
                        ))
                    }),
            );
        }

        self.row_order = self.nav_entries.iter().map(entry_key).collect();
        self.nav_anchors.retain(|key, _| self.row_order.contains(key));
        rail = rail.child(list);
        if self.undo_delete.is_some() {
            rail = rail.child(self.undo_bar(cx));
        }
        rail.child(
            div()
                .flex_none()
                .px(px(theme::S3))
                .py(px(theme::S2))
                .border_t_1()
                .border_color(theme::line_soft())
                .flex()
                .flex_row()
                .items_center()
                .gap(px(theme::S2))
                .child(crate::ui::icons::icon(
                    crate::ui::icons::Icon::Activity,
                    13.,
                    theme::running(),
                ))
                .child(
                    div()
                        .text_size(px(theme::TEXT_XS))
                        .text_color(theme::faint())
                        .child(SharedString::from(format!(
                            "{open_count} abertas · {running_count} executando"
                        ))),
                ),
        )
    }
}

/// Botão de ícone do cabeçalho da navegação.
fn nav_icon_button(
    id: &'static str,
    glyph: crate::ui::icons::Icon,
    cx: &mut Context<Workspace>,
    run: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .size(px(28.))
        .rounded(theme::r_control())
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(theme::hover()))
        .on_click(cx.listener(move |workspace, _: &ClickEvent, window, cx| {
            run(workspace, window, cx);
        }))
        .child(crate::ui::icons::icon(glyph, 15., theme::faint()))
        .into_any_element()
}

/// Filtro segmentado da navegação.
fn nav_filter_chip(
    id: &'static str,
    label: &'static str,
    active: bool,
    cx: &mut Context<Workspace>,
    run: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px(px(theme::S2 + theme::S1))
        .py(px(3.))
        .rounded(theme::r_control())
        .when(active, |el| {
            el.bg(theme::surface_2()).border_1().border_color(theme::line())
        })
        .when(!active, |el| {
            el.cursor_pointer().hover(|s| s.bg(theme::hover()))
        })
        .text_size(px(theme::TEXT_XS))
        .text_color(if active { theme::dim() } else { theme::faint() })
        .on_click(cx.listener(move |workspace, _: &ClickEvent, _window, cx| {
            run(workspace, cx);
        }))
        .child(SharedString::from(label))
        .into_any_element()
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.conversations[self.active].state.clone();
        if state.read(cx).help_open {
            state.update(cx, |state, _| {
                state.help_open = false;
                state.help_focus_pending = false;
            });
            self.open_settings(crate::settings::Section::Keys, window, cx);
        }
        if self.confirm_close || state.read(cx).modal.is_some() {
            if let Some(settings) = &self.settings {
                if settings.read(cx).open { settings.update(cx, |settings, cx| settings.suspend(cx)); }
            }
        }
        let active_viewed = window.is_window_active() && !self.confirm_close && !self.settings.as_ref().is_some_and(|settings| settings.read(cx).open);
        for (index, conversation) in self.conversations.iter_mut().enumerate() {
            let completed = conversation.state.read(cx).completed_content_runs;
            let newly_completed = completed != conversation.seen_completed_runs;
            if newly_completed {
                conversation.unread_completion = index != self.active || !active_viewed;
                conversation.seen_completed_runs = completed;
            }
            if index == self.active && active_viewed { conversation.unread_completion = false; }
            if let Some(path) = &conversation.path {
                if index == self.active && active_viewed {
                    if self.preferences.unread_sessions.remove(path).is_some() { crate::preferences::save(self.preferences.clone()); }
                } else if conversation.unread_completion {
                    let time = epoch_millis(conversation.state.read(cx).last_activity.unwrap_or_else(std::time::SystemTime::now));
                    if newly_completed || !self.preferences.unread_sessions.contains_key(path) {
                        self.preferences.unread_sessions.insert(path.clone(), time);
                        crate::preferences::save(self.preferences.clone());
                    }
                } else {
                    conversation.unread_completion = self.preferences.unread_sessions.contains_key(path);
                }
            }
        }
        if let Some((cwd, path, client)) = self.pending.take() {
            let editor = cx.new(|cx| Editor::new("", "Ask anything", window, cx));
            let modal_editor = cx.new(|cx| Editor::new("", "Type here…", window, cx));
            let search = cx.new(|cx| Editor::new("", "Search models or providers…", window, cx));
            let help = cx.new(|cx| Editor::new("", "Buscar atalhos…", window, cx));
            let state = cx.new(|cx| {
                AppState::new(cwd, client, crate::state::AppEditors {
                    composer: editor,
                    modal: modal_editor,
                    model_search: search,
                    help_search: help,
                }, None, cx)
            });
            state.update(cx, |state, cx| {
                if let Some(open) = self.preferences.details_open {
                    state.sidebar = open;
                    state.inspector_initialized = true;
                }
                if let Some(show) = self.preferences.show_thinking {
                    state.show_thinking = show;
                }
                state.start(cx);
            });
            let subscription = cx.observe(&state, |_, _, cx| cx.notify());
            self.conversations.push(Conversation {
                state,
                path,
                _subscription: subscription,
                seen_completed_runs: 0,
                unread_completion: false,
            });
            self.select(self.conversations.len() - 1, window, cx);
        }
        for conversation in &mut self.conversations {
            let session_file = conversation.state.read(cx).session.file.clone();
            if let Some(path) = session_file {
                let path = PathBuf::from(path);
                if conversation.path.as_ref() != Some(&path) {
                    conversation.path = Some(path.canonicalize().unwrap_or(path));
                }
                if let Some(info) = self
                    .catalog
                    .iter()
                    .find(|info| Some(&info.path) == conversation.path.as_ref())
                {
                    if conversation.state.read(cx).cwd != info.cwd {
                        conversation
                            .state
                            .update(cx, |state, _| state.cwd = info.cwd.clone());
                    }
                }
                if let Some(parent) = conversation.path.as_ref().and_then(|p| p.parent()) {
                    if !self.roots.iter().any(|root| root == parent) {
                        self.roots.push(parent.to_owned());
                    }
                }
            }
        }
        let state = self.conversations[self.active].state.clone();
        if state.read(cx).new_session_requested {
            let cwd = state.read(cx).cwd.clone();
            state.update(cx, |state, _| state.new_session_requested = false);
            self.spawn_conversation(cwd, None, cx);
        }
        let settings_open = self.settings.as_ref().is_some_and(|settings| settings.read(cx).open);
        for (index, conversation) in self.conversations.iter().enumerate() {
            conversation.state.update(cx, |state, _| {
                state.visible = index == self.active && !self.confirm_close && !settings_open
            });
        }
        let active = state.read(cx);
        theme::set_effort(active.thinking_level.as_deref(), &active.thinking_levels);
        let mut root = div()
            .id("workspace")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg())
            .text_color(theme::text())
            .font_family(theme::FONT_UI)
            // Arrasto da divisa do painel de sessões.
            .on_mouse_move(cx.listener(|workspace, event: &MouseMoveEvent, _, cx| {
                let Some((start_x, start_width)) = workspace.resizing_nav else {
                    return;
                };
                if event.pressed_button != Some(MouseButton::Left) {
                    workspace.resizing_nav = None;
                } else {
                    let width = (start_width + (f32::from(event.position.x) - start_x))
                        .clamp(200., 420.);
                    if (width - workspace.nav_width).abs() > 0.5 {
                        workspace.nav_width = width;
                    }
                }
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|workspace, _: &MouseUpEvent, _, cx| {
                    if workspace.resizing_nav.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(|workspace, _: &crate::ui::ToggleHelp, window, cx| {
                workspace.open_settings(crate::settings::Section::Keys, window, cx);
            }))
            .on_action(cx.listener(|workspace, _: &NavUp, window, cx| workspace.nav_move(-1, window, cx)))
            .on_action(cx.listener(|workspace, _: &NavDown, window, cx| workspace.nav_move(1, window, cx)))
            .on_action(cx.listener(|workspace, _: &NavAccept, window, cx| {
                workspace.nav_accept(window, cx)
            }))
            .on_action(cx.listener(|workspace, _: &NavCollapse, _, cx| {
                workspace.nav_toggle_project(true, cx)
            }))
            .on_action(cx.listener(|workspace, _: &NavExpand, _, cx| {
                workspace.nav_toggle_project(false, cx)
            }))
            .on_action(cx.listener(|workspace, _: &NextConversation, window, cx| {
                if !workspace.confirm_close && !workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) {
                    workspace.select((workspace.active + 1) % workspace.conversations.len(), window, cx);
                }
            }))
            .on_action(cx.listener(|workspace, _: &PreviousConversation, window, cx| {
                if !workspace.confirm_close && !workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) {
                    let len = workspace.conversations.len();
                    workspace.select((workspace.active + len - 1) % len, window, cx);
                }
            }))
            .on_action(cx.listener(|workspace, _: &FocusPrompt, window, cx| {
                if !workspace.confirm_close && !workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) {
                    workspace.select(workspace.active, window, cx);
                }
            }))
            .on_action(cx.listener(|workspace, _: &SearchSessions, window, cx| {
                if workspace.confirm_close || workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) { return; }
                workspace.visible = true;
                workspace.nav_user_choice = true;
                workspace.query.read(cx).focus_handle.clone().focus(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|workspace, _: &CloseConversation, window, cx| {
                if workspace.confirm_close || workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) { return; }
                let index = workspace.active;
                if workspace.conversations[index].state.read(cx).streaming {
                    workspace.confirm_close = true;
                    workspace.close_target = Some(index);
                    cx.notify();
                } else {
                    workspace.close_conversation(index, window, cx);
                }
            }))
            .on_action(cx.listener(|workspace, _: &ToggleSessions, _, cx| {
                if workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) { return; }
                workspace.visible = !workspace.visible;
                workspace.nav_user_choice = true;
                cx.notify();
            }));
        // Em janela estreita a conversa tem prioridade: a navegação recolhe
        // sozinha até 1120px, e volta a abrir quando houver espaço — a menos
        // que o usuário já tenha escolhido.
        if !self.nav_user_choice {
            self.visible = f32::from(window.bounds().size.width) >= 1120.;
        }
        root = root
            .on_action(cx.listener(|workspace, _: &CycleSessionFilter, window, cx| {
                if workspace.confirm_close || workspace.settings.as_ref().is_some_and(|settings| settings.read(cx).open) { return; }
                workspace.filter = match workspace.filter {
                    SessionFilter::All => SessionFilter::NeedsInput,
                    SessionFilter::NeedsInput => SessionFilter::Unread,
                    SessionFilter::Unread => SessionFilter::Running,
                    SessionFilter::Running => SessionFilter::All,
                };
                workspace.visible = true;
                workspace.nav_user_choice = true;
                let focus = workspace.query.read(cx).focus_handle.clone();
                focus.focus(window, cx);
                cx.notify();
            }));
        let mut body = div().relative().flex_1().min_h(px(0.)).flex().flex_row();
        if self.visible {
            body = body.child(self.navigation(window, cx));
        } else {
            // Recolhida: o pratinho reabre e cada conversa vira um ponto de
            // status — dá para trocar de sessão sem reabrir o painel.
            let rail = div()
                .id("nav-collapsed")
                .w(px(44.))
                .h_full()
                .flex_none()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(theme::S2))
                .py(px(theme::S3))
                .bg(theme::surface())
                .border_r_1()
                .border_color(theme::line_soft())
                .child(
                    div()
                        .id("nav-expand")
                        .size(px(28.))
                        .rounded(theme::r_control())
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|style| style.bg(theme::hover()))
                        .on_click(cx.listener(|workspace, _: &ClickEvent, _, cx| {
                            workspace.visible = true;
                            workspace.nav_user_choice = true;
                            cx.notify();
                        }))
                        .child(img("dish-icon.png").flex_none().w(px(22.)).h(px(22.))),
                )
                .child(div().w(px(20.)).h(px(1.)).bg(theme::line_soft()))
                .child(nav_icon_button("nav-settings-collapsed", crate::ui::icons::Icon::Dots, cx,
                    |workspace, window, cx| workspace.open_settings(crate::settings::Section::App, window, cx)));

            let mut groups: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
            for (index, conversation) in self.conversations.iter().enumerate() {
                groups
                    .entry(conversation.state.read(cx).cwd.clone())
                    .or_default()
                    .push(index);
            }
            // Sessões salvas que não estão abertas, para reabrir sem o painel.
            let mut saved: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
            for (index, info) in self.catalog.iter().enumerate() {
                if self
                    .conversations
                    .iter()
                    .any(|conversation| conversation.path.as_ref() == Some(&info.path))
                {
                    continue;
                }
                saved.entry(info.cwd.clone()).or_default().push(index);
            }
            let mut project_dirs: Vec<PathBuf> = groups.keys().cloned().collect();
            for cwd in saved.keys() {
                if !groups.contains_key(cwd) {
                    project_dirs.push(cwd.clone());
                }
            }
            project_dirs.sort();
            let mut dots = div()
                .id("nav-collapsed-dots")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(theme::S2));
            for cwd in project_dirs {
                let open_indices = groups.get(&cwd).cloned().unwrap_or_default();
                let saved_indices = saved.get(&cwd).cloned().unwrap_or_default();
                let folder_cwd = cwd.clone();
                let pending_count = open_indices.iter().filter(|i| self.conversations[**i].state.read(cx).modal.is_some()).count();
                let new_count = open_indices.iter().filter(|i| self.conversations[**i].unread_completion).count()
                    + saved_indices.iter().filter(|i| self.preferences.unread_sessions.contains_key(&self.catalog[**i].path)).count();
                let mut group = div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(theme::S1))
                    .child(
                        div()
                            .id(SharedString::from(format!("nav-folder-{cwd:?}")))
                            .size(px(28.))
                            .rounded(theme::r_control())
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::hover()))
                            .on_hover(cx.listener(move |workspace, hovered: &bool, _, cx| {
                                workspace.hovered =
                                    hovered.then(|| NavHover::Project(folder_cwd.clone()));
                                cx.notify();
                            }))
                            .on_click(cx.listener(|workspace, _: &ClickEvent, _, cx| {
                                workspace.visible = true;
                                workspace.nav_user_choice = true;
                                cx.notify();
                            }))
                            .child(crate::ui::icons::icon(
                                crate::ui::icons::Icon::Folder,
                                14.,
                                theme::faint(),
                            )),
                    );
                if pending_count > 0 || new_count > 0 {
                    group = group.child(div().text_size(px(theme::TEXT_XS)).text_color(theme::running())
                        .child(SharedString::from(format!("{pending_count}P {new_count}N"))));
                }
                for index in open_indices {
                    let signals = SessionSignals::from_state(self.conversations[index].state.read(cx), self.conversations[index].unread_completion);
                    let busy = signals.running;
                    let unread = self.conversations[index].unread_completion;
                    let color = if index == self.active {
                        theme::text()
                    } else if busy {
                        theme::running()
                    } else if unread {
                        theme::ok()
                    } else {
                        theme::faint()
                    };
                    group = group.child(
                        div()
                            .id(SharedString::from(format!("nav-dot-{index}")))
                            .size(px(28.))
                            .rounded(theme::r_control())
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::hover()))
                            .on_hover(cx.listener(move |workspace, hovered: &bool, _, cx| {
                                workspace.hovered =
                                    hovered.then_some(NavHover::Conversation(index));
                                cx.notify();
                            }))
                            .on_click(cx.listener(move |workspace, _: &ClickEvent, window, cx| {
                                workspace.select(index, window, cx);
                            }))
                            .child(if signals.needs_input {
                                crate::ui::icons::icon(crate::ui::icons::Icon::Alert, 12., theme::running()).into_any_element()
                            } else if signals.error {
                                crate::ui::icons::icon(crate::ui::icons::Icon::Alert, 12., theme::failure()).into_any_element()
                            } else if unread {
                                crate::ui::icons::icon(crate::ui::icons::Icon::File, 12., theme::ok()).into_any_element()
                            } else { crate::ui::dot(color, 8.).into_any_element() }),
                    );
                }
                for info_index in saved_indices {
                    group = group.child(
                        div()
                            .id(SharedString::from(format!("nav-saved-{info_index}")))
                            .size(px(28.))
                            .rounded(theme::r_control())
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::hover()))
                            .on_hover(cx.listener(move |workspace, hovered: &bool, _, cx| {
                                workspace.hovered =
                                    hovered.then_some(NavHover::Saved(info_index));
                                cx.notify();
                            }))
                            .on_click(cx.listener(move |workspace, _: &ClickEvent, window, cx| {
                                if let Some(info) = workspace.catalog.get(info_index).cloned() {
                                    workspace.open(info, window, cx);
                                }
                            }))
                            .child(crate::ui::ring(theme::faint(), 7.)),
                    );
                }
                dots = dots.child(group);
            }
            body = body.child(rail.child(dots));
        }
        let active = self.conversations[self.active].state.read(cx);
        let mut preferences = self.preferences.clone();
        if active.inspector_initialized {
            preferences.details_open = Some(active.sidebar);
        }
        preferences.session_filter = self.filter;
        preferences.navigation_open = Some(self.visible);
        preferences.navigation_width = Some(self.nav_width);
        preferences.show_thinking = Some(active.show_thinking);
        preferences.collapsed_projects = self.collapsed.clone();
        preferences.last_project = Some(active.cwd.clone());
        preferences.last_session = active.session.file.as_ref().map(PathBuf::from);
        let bounds = window.bounds();
        preferences.window_size = Some([f32::from(bounds.size.width), f32::from(bounds.size.height)]);
        if preferences != self.preferences {
            crate::preferences::save(preferences.clone());
            self.preferences = preferences;
        }
        root = root.child(body.child(div().flex_1().min_w(px(0.)).h_full().child(state)));
        // Divisória do painel de sessões: arrastar redimensiona.
        if self.visible {
            root = root.child(
                div()
                    .id("nav-resize-handle")
                    .absolute()
                    .left(px(self.nav_width))
                    .top(px(0.))
                    .bottom(px(0.))
                    .w(px(6.))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|workspace, event: &MouseDownEvent, _, cx| {
                            workspace.resizing_nav =
                                Some((f32::from(event.position.x), workspace.nav_width));
                            cx.notify();
                        }),
                    ),
            );
        }
        // Chip de identificação do que está sob o mouse na faixa recolhida.
        if !self.visible {
            if let Some(hovered) = &self.hovered {
                let (title, status) = match hovered {
                    NavHover::Project(cwd) => {
                        let open = self
                            .conversations
                            .iter()
                            .filter(|conversation| conversation.state.read(cx).cwd == *cwd)
                            .count();
                        let saved = self
                            .catalog
                            .iter()
                            .filter(|info| info.cwd == *cwd)
                            .count();
                        let status = match (open, saved) {
                            (0, saved) => format!("{saved} salvas"),
                            (open, 0) => format!("{open} abertas"),
                            (open, saved) => format!("{open} abertas · {saved} salvas"),
                        };
                        (folder_of(cwd), status)
                    }
                    NavHover::Conversation(index) => {
                        let index = *index;
                        let signals = self.conversations.get(index).map(|conversation| {
                            SessionSignals::from_state(conversation.state.read(cx), conversation.unread_completion)
                        }).unwrap_or_default();
                        (self.conversation_title(index, cx), signals.labels())
                    }
                    NavHover::Saved(index) => {
                        let info = self.catalog.get(*index);
                        (
                            info.map(|info| info.title.clone())
                                .unwrap_or_else(|| "sessão salva".into()),
                            "salva".to_string(),
                        )
                    }
                };
                root = root.child(
                    div()
                        .absolute()
                        .left(px(52.))
                        .top(px(12.))
                        .max_w(px(280.))
                        .px(px(theme::S2 + theme::S1))
                        .py(px(theme::S1 + 2.))
                        .rounded(theme::r_control())
                        .border_1()
                        .border_color(theme::line())
                        .bg(theme::surface_2())
                        .shadow(theme::shadow_overlay())
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(theme::TEXT_SM))
                                .text_color(theme::text())
                                .child(SharedString::from(title)),
                        )
                        .child(
                            div()
                                .text_size(px(theme::TEXT_XS))
                                .text_color(theme::faint())
                                .child(SharedString::from(status)),
                        ),
                );
            }
        }
        if let Some(settings) = &self.settings {
            if settings.read(cx).open { root = root.child(settings.clone()); }
        }
        if self.confirm_close {
            if !self.confirm_focus.is_focused(window) {
                self.confirm_focus.focus(window, cx);
            }
            let target = self.close_target;
            root = root.child(
                div()
                    .absolute()
                    .inset(px(0.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .track_focus(&self.confirm_focus)
                    .on_action(
                        cx.listener(|workspace, _: &crate::ui::Dismiss, window, cx| {
                            workspace.confirm_close = false;
                            workspace.select(workspace.active, window, cx);
                        }),
                    )
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .bg(theme::wash(theme::bg(), 0.85))
                    .child(
                        div()
                            .p(px(24.))
                            .bg(theme::surface())
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child("Tasks are still running. Closing will stop their Pi processes.")
                            .child(
                                div()
                                    .flex()
                                    .gap(px(20.))
                                    .child(
                                        div()
                                            .id("cancel-workspace-close")
                                            .cursor_pointer()
                                            .child("Keep running")
                                            .on_click(cx.listener(
                                                |workspace, _: &ClickEvent, window, cx| {
                                                    workspace.confirm_close = false;
                                                    workspace.select(workspace.active, window, cx);
                                                },
                                            )),
                                    )
                                    .child(
                                        div()
                                            .id("confirm-workspace-close")
                                            .cursor_pointer()
                                            .text_color(theme::danger())
                                            .child("Stop and close")
                                            .on_click(cx.listener(
                                                move |workspace, _: &ClickEvent, window, cx| {
                                                    workspace.confirm_close = false;
                                                    if let Some(index) = target {
                                                        workspace
                                                            .close_conversation(index, window, cx);
                                                    } else {
                                                        window.remove_window();
                                                    }
                                                },
                                            )),
                                    ),
                            ),
                    ),
            );
        }
        root
    }
}

fn drop_from_catalog(catalog: &mut Vec<SessionInfo>, path: &std::path::Path) {
    catalog.retain(|info| info.path.as_path() != path);
}

fn add_to_catalog(catalog: &mut Vec<SessionInfo>, info: SessionInfo) {
    catalog.retain(|entry| entry.path != info.path);
    catalog.push(info);
    catalog.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
}

/// Lixeira de uma sessão salva; aparece no hover, como o botão de fechar.
fn delete_session_button(target: SessionInfo, cx: &mut Context<Workspace>) -> AnyElement {
    let weak = cx.entity().downgrade();
    div()
        .id(SharedString::from(format!(
            "delete-{}",
            target.path.display()
        )))
        .size(px(22.))
        .rounded(theme::r_control())
        .flex()
        .items_center()
        .justify_center()
        .opacity(0.0)
        .hover(|style| style.opacity(1.0))
        .cursor_pointer()
        .on_click(move |_event: &ClickEvent, _, cx: &mut App| {
            cx.stop_propagation();
            weak.update(cx, |workspace, cx| {
                workspace.pending_delete = Some(target.path.clone());
                cx.notify();
            })
            .ok();
        })
        .child(crate::ui::icons::icon(
            crate::ui::icons::Icon::Trash,
            12.,
            theme::faint(),
        ))
        .into_any_element()
}

/// Confirmação embutida na linha: "Cancelar" / "Excluir".
fn delete_confirm_controls(target: SessionInfo, cx: &mut Context<Workspace>) -> AnyElement {
    let weak = cx.entity().downgrade();
    let cancel = weak.clone();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(theme::S1))
        .child(
            div()
                .id(SharedString::from(format!(
                    "delete-cancel-{}",
                    target.path.display()
                )))
                .px(px(theme::S2))
                .py(px(1.))
                .rounded(theme::r_control())
                .cursor_pointer()
                .text_size(px(theme::TEXT_XS))
                .text_color(theme::dim())
                .hover(|style| style.bg(theme::hover()))
                .on_click(move |_event: &ClickEvent, _, cx: &mut App| {
                    cx.stop_propagation();
                    cancel
                        .update(cx, |workspace, cx| {
                            workspace.pending_delete = None;
                            cx.notify();
                        })
                        .ok();
                })
                .child("Cancelar"),
        )
        .child(
            div()
                .id(SharedString::from(format!(
                    "delete-confirm-{}",
                    target.path.display()
                )))
                .px(px(theme::S2))
                .py(px(1.))
                .rounded(theme::r_control())
                .cursor_pointer()
                .text_size(px(theme::TEXT_XS))
                .text_color(theme::danger())
                .hover(|style| style.bg(theme::hover()))
                .on_click(move |_event: &ClickEvent, _, cx: &mut App| {
                    cx.stop_propagation();
                    weak.update(cx, |workspace, cx| {
                        workspace.delete_session(target.clone(), cx)
                    })
                    .ok();
                })
                .child("Excluir"),
        )
        .into_any_element()
}

/// Nome curto de um projeto: o nome da pasta, ou o caminho inteiro.
fn folder_of(cwd: &std::path::Path) -> String {
    cwd.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| cwd.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::{add_to_catalog, drop_from_catalog, select_item, sort_session_rows, row_key, attention_summary, SessionSignals, SessionFilter};
    use crate::sessions::SessionInfo;
    use std::path::PathBuf;

    fn info(path: &str, modified: u64) -> SessionInfo {
        SessionInfo {
            path: PathBuf::from(path),
            cwd: PathBuf::from("/project"),
            title: path.into(),
            modified: std::time::UNIX_EPOCH + std::time::Duration::from_secs(modified),
        }
    }

    #[test]
    fn row_order_has_stable_ties_and_freezes_during_interaction() {
        use std::time::{Duration, UNIX_EPOCH};
        let old = UNIX_EPOCH + Duration::from_secs(1);
        let recent = UNIX_EPOCH + Duration::from_secs(2);
        let mut rows = vec![("b".into(), None, Some(1), old), ("a".into(), None, Some(0), old)];
        sort_session_rows(&mut rows, &[], false);
        assert_eq!(rows.iter().map(row_key).collect::<Vec<_>>(), ["live-0", "live-1"]);
        let previous = rows.iter().map(row_key).collect::<Vec<_>>();
        rows[1].3 = recent;
        rows.push(("c".into(), None, Some(2), recent));
        sort_session_rows(&mut rows, &previous, true);
        assert_eq!(rows.iter().map(row_key).collect::<Vec<_>>(), ["live-0", "live-1", "live-2"]);
        sort_session_rows(&mut rows, &previous, false);
        assert_eq!(rows.iter().map(row_key).collect::<Vec<_>>(), ["live-1", "live-2", "live-0"]);
    }

    #[test]
    fn attention_counts_are_compact_and_do_not_hide_real_counts() {
        assert_eq!(attention_summary(0, 1), "1 nova");
        assert_eq!(attention_summary(2, 3), "2 pend. · 3 novas");
        assert_eq!(attention_summary(1, 0), "1 pend.");
    }

    #[test]
    fn running_filter_excludes_open_but_inactive_sessions() {
        assert!(!SessionSignals::default().matches(SessionFilter::Running));
        assert!(SessionSignals { running: true, ..Default::default() }.matches(SessionFilter::Running));
    }

    #[test]
    fn attention_signals_do_not_imply_running() {
        let signals = SessionSignals {
            needs_input: true, unread: true, queued: 2, ..Default::default()
        };
        assert!(!signals.matches(SessionFilter::Running));
        assert!(signals.needs_input && signals.unread);
        assert_eq!(signals.queued, 2);
    }

    #[test]
    fn selecting_removed_last_session_preserves_active_session() {
        let mut conversations = vec!["first", "second", "third"];
        let mut active = 2;
        conversations.remove(2);
        active = active.min(conversations.len() - 1);

        assert!(select_item(&mut conversations, &mut active, 2).is_none());
        assert_eq!(active, 1);
        assert_eq!(conversations[active], "second");
    }

    #[test]
    fn selecting_valid_session_updates_active_session() {
        let mut conversations = vec!["first", "second"];
        let mut active = 0;
        assert_eq!(select_item(&mut conversations, &mut active, 1), Some(&mut "second"));
        assert_eq!(active, 1);
    }

    #[test]
    fn selecting_empty_session_list_is_safe() {
        let mut conversations: Vec<()> = Vec::new();
        let mut active = 0;
        assert!(select_item(&mut conversations, &mut active, 0).is_none());
        assert_eq!(active, 0);
    }

    #[test]
    fn deleting_a_session_removes_it_from_the_catalog() {
        let mut catalog = vec![info("/a.jsonl", 1), info("/b.jsonl", 2)];
        drop_from_catalog(&mut catalog, std::path::Path::new("/a.jsonl"));
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].path, PathBuf::from("/b.jsonl"));
    }

    #[test]
    fn undoing_a_delete_reinserts_the_session_in_order() {
        let mut catalog = vec![info("/a.jsonl", 1), info("/c.jsonl", 3)];
        add_to_catalog(&mut catalog, info("/b.jsonl", 2));
        assert_eq!(
            catalog
                .iter()
                .map(|info| info.path.clone())
                .collect::<Vec<_>>(),
            vec![
                PathBuf::from("/c.jsonl"),
                PathBuf::from("/b.jsonl"),
                PathBuf::from("/a.jsonl")
            ]
        );
    }
}
