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
use crate::ui::test_probe::TestProbe;
use crate::session_activity::{SessionFilter, SessionSignals, epoch_millis, from_epoch_millis, relative_time};

mod navigation;
use self::navigation::nav_icon_button;
mod search;

actions!(dish_workspace, [FindCurrent, FindProject, ExitSearch, NextMatch, PreviousMatch, AcceptMatch, ToggleSessions, NextConversation, PreviousConversation, CloseConversation, SearchSessions, FocusPrompt, NavUp, NavDown, NavAccept, NavCollapse, NavExpand, CycleSessionFilter]);

#[derive(Clone)]
struct OpeningSession {
    cwd: PathBuf,
    path: Option<PathBuf>,
    title: Option<String>,
}

struct Conversation {
    state: Entity<AppState>,
    path: Option<PathBuf>,
    _subscription: Subscription,
    seen_completed_runs: u64,
    unread_completion: bool,
}

/// Keep a closed view's process fenced until its exact exit is acknowledged.
struct ClosingConversation {
    path: Option<PathBuf>,
    _client: PiClient,
}

/// A session sitting in the trash while the undo window is open.
struct DeletedSession {
    info: SessionInfo,
    trashed: PathBuf,
    id: u64,
    pinned: bool,
}

/// Identity must survive removal/reordering of other live conversations.
#[derive(Clone, PartialEq, Eq)]
enum SessionTarget {
    Live(EntityId),
    Saved(PathBuf),
}

impl SessionTarget {
    fn key(&self) -> String {
        match self {
            Self::Live(id) => format!("live-{id:?}"),
            Self::Saved(path) => format!("saved-{}", path.display()),
        }
    }
}

struct SessionMenu {
    target: SessionTarget,
    anchor: Point<Pixels>,
    trigger_bounds: Bounds<Pixels>,
    selected: usize,
    return_focus: FocusHandle,
}

struct PendingDelete {
    target: SessionTarget,
    info: SessionInfo,
    return_focus: FocusHandle,
    confirm_selected: bool,
}

fn resolve_session_target<'a>(target: &SessionTarget,
    mut conversations: impl Iterator<Item = (EntityId, Option<&'a std::path::Path>)>) -> Option<usize> {
    conversations.position(|(id, path)| match target {
        SessionTarget::Live(target) => id == *target,
        SessionTarget::Saved(target) => path == Some(target.as_path()),
    })
}

fn catalog_snapshot(catalog: Vec<SessionInfo>, started: u64, current: u64,
    deleting: &BTreeSet<PathBuf>) -> Option<Vec<SessionInfo>> {
    (started == current).then(|| catalog.into_iter()
        .filter(|info| !deleting.contains(&info.path)).collect())
}

fn lifecycle_label(live: bool, active: bool) -> &'static str {
    match (live, active) {
        (true, true) => "Atual",
        (true, false) => "Aberta",
        (false, _) => "Salva",
    }
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
            completed: state.completed_content_runs > 0 && !state.is_executing(),
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

/// Abrir uma sessão troca sua representação (salva -> processo vivo), não
/// sua posição. Migra a identidade usada pelo congelamento da navegação.
fn promote_session_row(previous: &mut [String], path: &std::path::Path, index: usize) -> (String, String) {
    let saved = entry_key(&NavEntry::Saved(path.to_owned()));
    let live = entry_key(&NavEntry::Open(index));
    for key in previous {
        if *key == saved { *key = live.clone(); }
    }
    (saved, live)
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

/// Posição de uma entrada navegável na lista atual.
fn nav_position(entries: &[NavEntry], target: &NavEntry) -> Option<usize> {
    entries.iter().position(|entry| entry == target)
}

/// `[todas, precisa de você, novas, executando]`.
fn filter_counts(signals: &[SessionSignals]) -> [usize; 4] {
    let mut counts = [signals.len(), 0, 0, 0];
    for signal in signals {
        counts[1] += usize::from(signal.needs_input);
        counts[2] += usize::from(signal.unread);
        counts[3] += usize::from(signal.running);
    }
    counts
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
    /// Reject discovery snapshots started before a trash/restore transaction.
    catalog_generation: u64,
    opening: Option<OpeningSession>,
    error: Option<String>,
    /// Nothing touches Pi or its file until this confirmation is accepted.
    pending_delete: Option<PendingDelete>,
    session_menu: Option<SessionMenu>,
    session_menu_focus: FocusHandle,
    session_action_focus: BTreeMap<String, FocusHandle>,
    /// Measured once per target, not per render. GPUI may reuse a painted
    /// trigger without rerunning a freshly-created canvas callback; losing the
    /// measurement would place the next popup offscreen.
    session_action_bounds: BTreeMap<String, std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>>,
    closing: BTreeMap<EntityId, ClosingConversation>,
    undo_focus: FocusHandle,
    delete_focus_pending: Option<FocusHandle>,
    /// Exclusões em andamento, para não mover o mesmo arquivo duas vezes.
    deleting: BTreeSet<PathBuf>,
    /// Última sessão excluída, enquanto o "Desfazer" está disponível.
    undo_delete: Option<DeletedSession>,
    /// Restauração em andamento; mantém o "Desfazer" visível até o resultado.
    restoring: Option<u64>,
    undo_seq: u64,
    confirm_close: bool,
    close_target: Option<EntityId>,
    pending: Option<(OpeningSession, PiClient)>,
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
    /// Aviso da linha de comando (pasta inexistente), mostrado no rail.
    notice: Option<String>,
    settings: Option<Entity<crate::settings::Settings>>,
    /// Última lista de modelos enviada às configurações abertas.
    settings_models: Vec<(String, String)>,
    content_search: search::SearchState,
}

impl Workspace {
    pub fn new(
        state: Entity<AppState>,
        flags: Vec<String>,
        query: Entity<Editor>,
        notice: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let preferences = crate::preferences::load();
        if let Some(reduce_motion) = preferences.reduce_motion {
            cx.set_reduce_motion(reduce_motion);
        }
        if let Some(open) = preferences.details_open {
            state.update(cx, |state, _| {
                state.sidebar = open;
                state.inspector_initialized = true;
            });
        }
        if let Some(show) = preferences.show_thinking {
            state.update(cx, |state, _| state.show_thinking = show);
        }
        state.update(cx, |state, _| state.generated_titles = preferences.generated_titles);
        state.update(cx, |state, _| state.hidden_models = preferences.hidden_models.clone());
        // Temas do usuário: um arquivo malformado vira aviso, nunca derruba o app.
        let theme_errors = crate::theme::preload();
        if !theme_errors.is_empty() {
            state.update(cx, |state, cx| {
                for error in theme_errors {
                    state.push_toast(error.clone(), crate::state::Tone::Error, cx);
                }
            });
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
        if initial_path.is_some() {
            state.update(cx, |state, _| state.restoring_session = true);
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
            nav_width: preferences.navigation_width.unwrap_or(theme::NAV_WIDTH).clamp(200., 420.),
            resizing_nav: None,
            nav_entries: Vec::new(),
            nav_index: 0,
            nav_query: String::new(),
            nav_filter: preferences.session_filter,
            nav_last_project: None,
            nav_scroll: ScrollHandle::new(),
            refreshing: false,
            catalog_generation: 0,
            opening: None,
            error: None,
            pending_delete: None,
            session_menu: None,
            session_menu_focus: cx.focus_handle(),
            session_action_focus: BTreeMap::new(),
            session_action_bounds: BTreeMap::new(),
            closing: BTreeMap::new(),
            undo_focus: cx.focus_handle().tab_stop(true),
            delete_focus_pending: None,
            deleting: BTreeSet::new(),
            undo_delete: None,
            restoring: None,
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
            notice,
            settings: None,
            settings_models: Vec::new(),
            content_search: search::SearchState::default(),
        }
    }

    pub fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_window, cx| {
            weak.update(cx, |workspace, cx| {
                if workspace.pending_delete.is_some() { return false; }
                workspace.session_menu = None;
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
        if self.blocking_confirmation() || self.conversations[self.active].state.read(cx).modal.is_some() { return; }
        self.session_menu = None;
        if self.settings.is_none() {
            let settings = cx.new(|cx| crate::settings::Settings::new(window, cx));
            cx.observe(&settings, |_, _, cx| cx.notify()).detach();
            cx.subscribe(&settings, |this, _, event, cx| {
                use crate::settings::SettingsEvent;
                match event {
                    SettingsEvent::Preference(name, value) => {
                        if *name == "navigation" { this.visible = *value; this.nav_user_choice = true; }
                        if *name == "reduce_motion" {
                            this.preferences.reduce_motion = Some(*value);
                            cx.set_reduce_motion(*value);
                            crate::preferences::save(this.preferences.clone());
                        }
                        for conversation in &this.conversations {
                            conversation.state.update(cx, |state, cx| {
                                match *name {
                                    "details" => { state.sidebar = *value; state.inspector_initialized = true; }
                                    "thinking" => state.show_thinking = *value,
                                    "generated_titles" => state.set_generated_titles(*value, cx),
                                    _ => {}
                                }
                                cx.notify();
                            });
                        }
                    }
                    SettingsEvent::Appearance(mode) => {
                        // Escolher o modo solta o tema fixado: cada aparência
                        // volta ao seu tema padrão.
                        this.preferences.appearance = *mode;
                        this.preferences.theme = None;
                        theme::set_mode(*mode);
                        theme::set_theme(None);
                        crate::preferences::save(this.preferences.clone());
                    }
                    SettingsEvent::Theme(id) => {
                        this.preferences.theme = Some(id.clone());
                        if let Some(chosen) = theme::theme_by_id(id) {
                            // Fixar um tema também fixa a sua aparência, para o
                            // desktop não trocar por baixo.
                            let mode = if chosen.dark {
                                crate::theme::AppearanceMode::Dark
                            } else {
                                crate::theme::AppearanceMode::Light
                            };
                            this.preferences.appearance = mode;
                            theme::set_mode(mode);
                        }
                        theme::set_theme(Some(id.clone()));
                        crate::preferences::save(this.preferences.clone());
                    }
                    SettingsEvent::OpenThemes => {
                        if let Some(directory) = crate::preferences::themes_dir() {
                            if let Err(error) = std::fs::create_dir_all(&directory) {
                                crate::logging::warn(format_args!(
                                    "Dish: cannot create themes directory: {error}"
                                ));
                            }
                            crate::platform::open_directory(&directory);
                        }
                    }
                    SettingsEvent::ModelVisibility(key, visible) => {
                        if *visible {
                            this.preferences.hidden_models.remove(key);
                        } else {
                            this.preferences.hidden_models.insert(key.clone());
                        }
                        crate::preferences::save(this.preferences.clone());
                        let hidden = this.preferences.hidden_models.clone();
                        for conversation in &this.conversations {
                            conversation.state.update(cx, |state, cx| {
                                state.hidden_models = hidden.clone();
                                cx.notify();
                            });
                        }
                    }
                    SettingsEvent::ModelsVisibility(keys, visible) => {
                        for key in keys {
                            if *visible {
                                this.preferences.hidden_models.remove(key);
                            } else {
                                this.preferences.hidden_models.insert(key.clone());
                            }
                        }
                        crate::preferences::save(this.preferences.clone());
                        let hidden = this.preferences.hidden_models.clone();
                        for conversation in &this.conversations {
                            conversation.state.update(cx, |state, cx| {
                                state.hidden_models = hidden.clone();
                                cx.notify();
                            });
                        }
                    }
                    SettingsEvent::ShowAllModels => {
                        this.preferences.hidden_models.clear();
                        crate::preferences::save(this.preferences.clone());
                        for conversation in &this.conversations {
                            conversation.state.update(cx, |state, cx| {
                                state.hidden_models.clear();
                                cx.notify();
                            });
                        }
                    }
                    SettingsEvent::ProvidersChanged => {
                        // A credencial mudou no Pi. Cada conversa tem o próprio
                        // processo e a própria lista; reler não toca em rascunho
                        // nem em streaming.
                        for conversation in &this.conversations {
                            conversation
                                .state
                                .update(cx, |state, cx| state.refresh_catalog(cx));
                        }
                    }
                    SettingsEvent::ChoosePi => {
                        let picker = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some("Escolher executável do Pi".into()),
                        });
                        cx.spawn(async move |this, cx| {
                            let Ok(Ok(Some(paths))) = picker.await else { return; };
                            let Some(path) = paths.first() else { return; };
                            let program = path.to_string_lossy().into_owned();
                            let candidate = program.clone();
                            let result = cx
                                .background_executor()
                                .spawn(async move { crate::installation::validate(&candidate) })
                                .await;
                            let _ = this.update(cx, |workspace, cx| {
                                let (message, tone) = match result {
                                    Ok(version) => {
                                        crate::installation::remember(&program);
                                        (format!("Pi {version} salvo. Novas conversas usarão esse executável."), crate::state::Tone::Success)
                                    }
                                    Err(error) => (format!("Esse executável não respondeu como o Pi: {error:#}"), crate::state::Tone::Error),
                                };
                                let state = workspace.conversations[workspace.active].state.clone();
                                state.update(cx, |state, cx| state.push_toast(message, tone, cx));
                            });
                        })
                        .detach();
                    }
                    SettingsEvent::Closed => {}
                }
                cx.notify();
            }).detach();
            self.settings = Some(settings);
        }
        let state = self.conversations[self.active].state.read(cx);
        let view = crate::settings::SettingsView {
            options: [state.sidebar, self.visible, state.show_thinking, state.generated_titles],
            appearance: self.preferences.appearance,
            theme: self.preferences.theme.clone(),
            models: state
                .models
                .iter()
                .map(|model| (model.provider.clone(), model.id.clone()))
                .collect(),
            hidden: self.preferences.hidden_models.clone(),
        };
        self.conversations[self.active].state.update(cx, |state, _| {
            state.visible = false;
            state.model_menu = false;
            state.model_search_focus_pending = false;
            state.effort_menu = false;
        });
        self.settings.as_ref().unwrap().update(cx, |settings, cx| settings.show(section, view, window, cx));
        self.settings_models = self
            .settings
            .as_ref()
            .map(|settings| settings.read(cx).visible_models())
            .unwrap_or_default();
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.refreshing {
            return;
        }
        self.refreshing = true;
        let roots = self.roots.clone();
        let previous = self.catalog.clone();
        let generation = self.catalog_generation;
        cx.spawn(async move |this, cx| {
            let catalog = cx
                .background_executor()
                .spawn(async move { sessions::discover_cached(&roots, &previous) })
                .await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.refreshing = false;
                let Some(catalog) = catalog_snapshot(catalog, generation, workspace.catalog_generation, &workspace.deleting) else {
                    workspace.refresh(cx);
                    return;
                };
                workspace.catalog = catalog;
                // Refresh navigation only. The open content search keeps its
                // query snapshot until edited, reopened or moved to a new scope.
                cx.notify();
            });
        })
        .detach();
    }

    fn select(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.session_menu = None;
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
        state.update(cx, |state, cx| state.refresh_git(cx));
        focus.focus(window, cx);
        if let Some(position) = nav_position(&self.nav_entries, &NavEntry::Open(index)) {
            self.nav_index = position;
        }
        cx.notify();
    }

    /// Mantém o destaque de teclado na linha que o mouse acabou de abrir.
    fn focus_nav(&mut self, live: Option<usize>, path: Option<PathBuf>) {
        let entry = match (live, path) {
            (Some(index), _) => NavEntry::Open(index),
            (None, Some(path)) => NavEntry::Saved(path),
            (None, None) => return,
        };
        if let Some(position) = nav_position(&self.nav_entries, &entry) {
            self.nav_index = position;
        }
    }

    /// Contagens por filtro sobre todas as sessões (abertas e salvas), não só
    /// as visíveis no filtro atual.
    fn session_counts(&self, cx: &App) -> [usize; 4] {
        let mut signals = Vec::new();
        for conversation in &self.conversations {
            signals.push(SessionSignals::from_state(
                conversation.state.read(cx),
                conversation.unread_completion,
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
            signals.push(SessionSignals {
                saved: true,
                unread: self.preferences.unread_sessions.contains_key(&info.path),
                ..Default::default()
            });
        }
        filter_counts(&signals)
    }

    fn open(&mut self, info: SessionInfo, window: &mut Window, cx: &mut Context<Self>) {
        // A exclusão em background ainda não removeu o arquivo do catálogo.
        // Não inicie o Pi com uma sessão que está sendo movida para a lixeira.
        if self.deleting.contains(&info.path) { return; }
        if self.path_is_closing(&info.path) {
            self.error = Some("Pi ainda está encerrando. Aguarde antes de reabrir ou excluir o arquivo.".into());
            cx.notify();
            return;
        }
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
                Some("Aguarde a sessão inicial carregar antes de abrir uma sessão salva.".into());
            cx.notify();
            return;
        }
        if self.opening.is_some() || self.pending.is_some() {
            return;
        }
        if !info.cwd.is_dir() {
            self.error = Some(format!(
                "A pasta do projeto não existe mais: {}",
                info.cwd.display()
            ));
            cx.notify();
            return;
        }
        self.spawn_conversation(info.cwd, Some(info.path), Some(info.title), cx);
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

    /// Pede uma pasta ao sistema e abre uma conversa nova nela.
    ///
    /// O diálogo é o do GPUI (`prompt_for_paths`), sem dependência nova e sem
    /// mexer no bundle de licenças. A pasta só é usada depois de validada.
    fn choose_project(&mut self, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Escolher pasta do projeto".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picker.await else { return; };
            let Some(path) = paths.first().cloned() else { return; };
            let _ = this.update(cx, |workspace, cx| workspace.open_project(path, cx));
        })
        .detach();
    }

    /// Abre uma conversa nova numa pasta escolhida, validando antes de gastar
    /// um processo do Pi. Pasta inválida vira toast; nunca cai na anterior em
    /// silêncio.
    fn open_project(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !path.is_dir() {
            self.project_error(&path, "não é uma pasta", cx);
            return;
        }
        if let Err(error) = std::fs::read_dir(&path) {
            self.project_error(&path, &format!("não foi possível ler ({error})"), cx);
            return;
        }
        self.notice = None;
        let cwd = path.canonicalize().unwrap_or(path);
        self.spawn_conversation(cwd, None, None, cx);
    }

    fn project_error(&mut self, path: &std::path::Path, reason: &str, cx: &mut Context<Self>) {
        let message = format!("Não foi possível abrir {}: {reason}.", path.display());
        let state = self.conversations[self.active].state.clone();
        state.update(cx, |state, cx| {
            state.push_toast(message, crate::state::Tone::Error, cx);
        });
    }

    /// Fixa ou desafixa uma sessão no topo do grupo. O pino é só apresentação
    /// do Dish; os arquivos de sessão do Pi não mudam.
    fn toggle_pin(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.preferences.pinned_sessions.remove(&path) {
            self.preferences.pinned_sessions.insert(path);
        }
        crate::preferences::save(self.preferences.clone());
        cx.notify();
    }

    /// Uma conversa ainda sem arquivo salvo não tem identidade para fixar; o
    /// aviso explica quando o pino fica disponível.
    fn explain_unpinned_session(&mut self, cx: &mut Context<Self>) {
        let message = "A sessão é salva pelo Pi no primeiro turno; depois disso dá para fixá-la."
            .to_string();
        let state = self.conversations[self.active].state.clone();
        state.update(cx, |state, cx| {
            state.push_toast(message, crate::state::Tone::Info, cx);
        });
    }

    fn spawn_conversation(
        &mut self,
        cwd: PathBuf,
        path: Option<PathBuf>,
        title: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.opening.is_some() || self.pending.is_some() {
            return;
        }
        let opening = OpeningSession {
            cwd: cwd.clone(),
            path: path.clone(),
            title,
        };
        self.opening = Some(opening.clone());
        self.error = None;
        cx.notify();
        let mut flags = self.flags.clone();
        if let Some(path) = &path {
            flags.extend(["--session".into(), path.to_string_lossy().into_owned()]);
        }
        let config = PiConfig::new(cwd).with_args(flags);
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
                        workspace.pending = Some((opening, client));
                    }
                    Err(error) => workspace.error = Some(format!("Não foi possível abrir a sessão: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn blocking_confirmation(&self) -> bool {
        self.confirm_close || self.pending_delete.is_some()
    }

    fn target_index(&self, target: &SessionTarget) -> Option<usize> {
        resolve_session_target(target, self.conversations.iter()
            .map(|conversation| (conversation.state.entity_id(), conversation.path.as_deref())))
    }

    fn target_path(&self, target: &SessionTarget) -> Option<PathBuf> {
        match target {
            SessionTarget::Live(_) => self.target_index(target)
                .and_then(|index| self.conversations[index].path.clone()),
            SessionTarget::Saved(path) => Some(path.clone()),
        }
    }

    fn target_info(&self, target: &SessionTarget, cx: &App) -> Option<SessionInfo> {
        let path = self.target_path(target)?;
        self.catalog.iter().find(|info| info.path == path).cloned().or_else(|| {
            // Pi may have reported a file before the next catalog refresh.
            let index = self.target_index(target)?;
            let state = self.conversations[index].state.read(cx);
            Some(SessionInfo {
                path, cwd: state.cwd.clone(), title: self.conversation_title(index, cx),
                named: state.session.name.is_some(), modified: std::time::UNIX_EPOCH,
                scan: Default::default(),
            })
        })
    }

    fn path_is_closing(&self, path: &std::path::Path) -> bool {
        // A just-created session can close before Pi reports its file. Fence
        // saved-file actions briefly rather than guess which file it owns.
        self.closing.values().any(|closing| closing.path.as_deref().is_none_or(|p| p == path))
    }

    fn session_action_enabled(&self, target: &SessionTarget, action: usize) -> bool {
        let path = self.target_path(target);
        if path.as_ref().is_some_and(|path| self.deleting.contains(path) || self.path_is_closing(path)) { return false; }
        let live = self.target_index(target);
        if matches!(target, SessionTarget::Live(_)) && live.is_none() { return false; }
        match action {
            0 => true, // Pathless pin explains when Pi will save it.
            1 => live.is_some() && self.conversations.len() > 1,
            2 => path.as_ref().is_some_and(|path| {
                !(live.is_some() && self.conversations.len() == 1)
                    && !self.opening.as_ref().is_some_and(|o| o.path.as_ref() == Some(path))
                    && !self.pending.as_ref().is_some_and(|(o, _)| o.path.as_ref() == Some(path))
            }),
            _ => false,
        }
    }

    fn dismiss_session_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.session_menu.take() {
            menu.return_focus.focus(window, cx);
            cx.notify();
        }
    }

    fn show_session_menu(&mut self, target: SessionTarget, bounds: Bounds<Pixels>, focus: FocusHandle,
        window: &mut Window, cx: &mut Context<Self>) {
        if self.blocking_confirmation() { return; }
        if self.session_menu.as_ref().is_some_and(|menu| menu.target == target) {
            self.dismiss_session_menu(window, cx);
            return;
        }
        self.session_menu = Some(SessionMenu { target, anchor: bounds.bottom_right(), trigger_bounds: bounds,
            selected: 0, return_focus: focus });
        self.session_menu_focus.focus(window, cx);
        cx.notify();
    }

    fn session_actions_button(&mut self, target: SessionTarget, cx: &mut Context<Self>) -> AnyElement {
        let key = target.key();
        let focus = self.session_action_focus.entry(key.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true)).clone();
        let bounds = self.session_action_bounds.entry(key.clone()).or_default().clone();
        let measured = bounds.clone();
        let keyboard_bounds = bounds.clone();
        let key_target = target.clone();
        let click_focus = focus.clone();
        let key_focus = focus.clone();
        div().id(SharedString::from(format!("session-actions-{key}")))
            .test_probe(|| format!("session-actions-{key}"), || serde_json::json!({}))
            .relative().flex_none().w(px(24.)).h(px(theme::S6)).rounded(theme::r_control())
            .track_focus(&focus).flex().items_center().justify_center()
            .cursor_pointer().hover(|style| style.bg(theme::hover()).text_color(theme::text()))
            .focus(|style| style.shadow(theme::focus_ring()))
            .tooltip(|_, cx| cx.new(|_| crate::ui::copy_button::TextTooltip("Ações da sessão".into())).into())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |workspace, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                // Pointer menus follow the click, never a cached layout rect.
                // Lists scroll and reorder while a run streams, so a stale
                // measurement can anchor the popup outside the viewport.
                let anchor = match event {
                    ClickEvent::Mouse(event) => Bounds {
                        origin: event.up.position,
                        size: size(px(0.), px(0.)),
                    },
                    ClickEvent::Touch(event) => Bounds {
                        origin: event.position,
                        size: size(px(0.), px(0.)),
                    },
                    ClickEvent::Keyboard(event) => {
                        let measured = bounds.get();
                        if measured.size.width > px(0.) { measured } else { event.bounds }
                    }
                };
                workspace.show_session_menu(target.clone(), anchor, click_focus.clone(), window, cx);
            }))
            .on_key_down(cx.listener(move |workspace, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space" | "down") {
                    workspace.show_session_menu(key_target.clone(), keyboard_bounds.get(), key_focus.clone(), window, cx);
                    cx.stop_propagation();
                } else if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift { window.focus_prev(cx); } else { window.focus_next(cx); }
                    cx.stop_propagation();
                }
            }))
            .child(canvas(move |bounds, _, _| { measured.set(bounds); }, |_, _, _, _| {})
                .absolute().size_full())
            .child(crate::ui::icons::icon(crate::ui::icons::Icon::Dots, 16., theme::dim()))
            .into_any_element()
    }

    fn move_session_menu(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(menu) = self.session_menu.as_ref() else { return; };
        let target = menu.target.clone();
        let mut next = menu.selected;
        for _ in 0..3 {
            next = (next as isize + delta).rem_euclid(3) as usize;
            if self.session_action_enabled(&target, next) { break; }
        }
        if let Some(menu) = self.session_menu.as_mut() { menu.selected = next; }
        cx.notify();
    }

    fn request_session_close(&mut self, target: &SessionTarget, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.target_index(target) else { return; };
        if self.conversations.len() == 1 {
            self.error = Some("Mantenha uma sessão aberta. Crie outra antes de fechar esta.".into());
            cx.notify();
            return;
        }
        if self.conversations[index].state.read(cx).is_busy() {
            self.confirm_close = true;
            self.close_target = Some(self.conversations[index].state.entity_id());
            cx.notify();
        } else { self.close_conversation(index, window, cx); }
    }

    fn execute_session_action(&mut self, action: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.session_menu.as_ref() else { return; };
        if !self.session_action_enabled(&menu.target, action) { return; }
        let Some(menu) = self.session_menu.take() else { return; };
        menu.return_focus.focus(window, cx);
        match action {
            0 => if let Some(path) = self.target_path(&menu.target) { self.toggle_pin(path, cx); }
                else { self.explain_unpinned_session(cx); },
            1 => self.request_session_close(&menu.target, window, cx),
            2 => if let Some(info) = self.target_info(&menu.target, cx) {
                self.pending_delete = Some(PendingDelete { target: menu.target, info,
                    return_focus: menu.return_focus, confirm_selected: false });
                self.confirm_focus.focus(window, cx);
            } else {
                self.error = Some("A sessão não está mais no catálogo. Arquivo preservado; tente novamente.".into());
            },
            _ => {}
        }
        cx.notify();
    }

    fn cancel_session_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pending) = self.pending_delete.take() {
            pending.return_focus.focus(window, cx);
            cx.notify();
        }
    }

    fn confirm_session_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_delete.take() else { return; };
        pending.return_focus.focus(window, cx);
        if !self.session_action_enabled(&pending.target, 2)
            || self.target_path(&pending.target).as_ref() != Some(&pending.info.path) {
            self.error = Some("A sessão mudou ou ainda está abrindo. Arquivo preservado; tente novamente.".into());
            cx.notify();
            return;
        }
        // Retain the exact client before removing the view: Drop alone is not
        // a process-exit barrier, and retained UI entities may still own clones.
        let client = self.target_index(&pending.target).map(|index| {
            let client = self.conversations[index].state.read(cx).client.clone();
            client.request_shutdown();
            self.remove_conversation(index, window, cx);
            client
        });
        self.delete_session(pending.info, client, pending.return_focus, cx);
    }

    fn session_menu_element(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.session_menu.as_ref()?;
        let target = menu.target.clone();
        let anchor = menu.anchor;
        let trigger_bounds = menu.trigger_bounds;
        let selected = menu.selected;
        let path = self.target_path(&target);
        let live = self.target_index(&target);
        let pinned = path.as_ref().is_some_and(|path| self.preferences.pinned_sessions.contains(path));
        let title = self.target_info(&target, cx).map(|info| info.title)
            .or_else(|| live.map(|index| self.conversation_title(index, cx)))
            .unwrap_or_else(|| "Sessão indisponível".into());
        let mut card = div().id("session-actions-menu").occlude()
            .track_focus(&self.session_menu_focus)
            .w(px(theme::NAV_WIDTH)).max_w(window.viewport_size().width - px(theme::S4))
            .max_h(window.viewport_size().height - px(theme::S4)).overflow_y_scroll()
            .p(px(theme::S1)).rounded(theme::r_surface()).bg(theme::surface_2())
            .border_1().border_color(theme::line_strong()).shadow(theme::shadow_overlay())
            .flex().flex_col().gap(px(theme::S1))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(move |workspace, event: &MouseDownEvent, window, cx| {
                if !trigger_bounds.contains(&event.position) { workspace.dismiss_session_menu(window, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &crate::ui::Dismiss, window, cx| {
                workspace.dismiss_session_menu(window, cx); cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|workspace, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "up" => workspace.move_session_menu(-1, cx),
                    "down" => workspace.move_session_menu(1, cx),
                    "tab" => workspace.move_session_menu(if event.keystroke.modifiers.shift { -1 } else { 1 }, cx),
                    "enter" | "space" => {
                        if let Some(action) = workspace.session_menu.as_ref().map(|menu| menu.selected) {
                            workspace.execute_session_action(action, window, cx);
                        }
                    }
                    "escape" => workspace.dismiss_session_menu(window, cx),
                    _ => {}
                }
                cx.stop_propagation();
            }))
            .child(div().flex_none().px(px(theme::S3)).py(px(theme::S2))
                .flex().flex_col().gap(px(theme::S1))
                .child(div().truncate().text_size(px(theme::TEXT_SM)).text_color(theme::text()).child(SharedString::from(title)))
                .child(div().text_size(px(theme::TEXT_XS)).text_color(theme::faint())
                    .child(lifecycle_label(live.is_some(), live == Some(self.active)))));
        for (action, icon, label, detail) in [
            (0, crate::ui::icons::Icon::Pin, if pinned { "Desafixar sessão" } else { "Fixar sessão" }, "Prioridade na lista"),
            (1, crate::ui::icons::Icon::Close, "Fechar sessão", "Mantém o arquivo salvo"),
            (2, crate::ui::icons::Icon::Trash, "Excluir arquivo…", "Lixeira · Desfazer por 10 s"),
        ] {
            let enabled = self.session_action_enabled(&target, action);
            let color = if !enabled { theme::faint() } else if action == 2 { theme::danger() } else { theme::text() };
            card = card.child(div().id(("session-action", action)).flex_none()
                .px(px(theme::S3)).py(px(theme::S2)).rounded(theme::r_control())
                .flex().items_center().gap(px(theme::S3))
                .when(enabled, |el| el.cursor_pointer().hover(|style| style.bg(theme::hover())))
                .when(enabled && selected == action, |el| el.bg(theme::hover()).shadow(theme::focus_ring()))
                .on_click(cx.listener(move |workspace, _: &ClickEvent, window, cx| {
                    cx.stop_propagation(); workspace.execute_session_action(action, window, cx);
                }))
                .child(crate::ui::icons::icon(icon, 16., color))
                .child(div().flex_1().min_w(px(0.)).flex().flex_col().gap(px(1.))
                    .child(div().text_size(px(theme::TEXT_SM)).text_color(color).child(label))
                    .child(div().text_size(px(theme::TEXT_XS)).text_color(theme::faint()).child(detail))));
        }
        let explanation = if path.as_ref().is_some_and(|path| self.path_is_closing(path)) {
            "Pi está encerrando. Aguarde antes de reabrir ou excluir o arquivo."
        } else if path.as_ref().is_some_and(|path| self.deleting.contains(path)) {
            "Exclusão em andamento. Aguarde."
        } else if path.is_none() {
            "Pi salva no primeiro turno. Depois disso, dá para fixar e excluir o arquivo. Para fechar a última conversa, crie outra antes."
        } else if live.is_some() && self.conversations.len() == 1 {
            "Mantenha uma sessão aberta. Crie outra antes de fechar ou excluir esta."
        } else if !self.session_action_enabled(&target, 2) {
            "Aguarde a sessão terminar de abrir."
        } else {
            "↑↓ escolher · Enter confirmar · Esc fechar"
        };
        card = card.child(div().flex_none().px(px(theme::S3)).py(px(theme::S2))
            .border_t_1().border_color(theme::line_soft()).text_size(px(theme::TEXT_XS))
            .text_color(theme::dim()).child(explanation));
        Some(deferred(anchored().position(anchor).anchor(Anchor::TopRight)
            .snap_to_window_with_margin(px(theme::S2)).child(card)).with_priority(2).into_any_element())
    }

    fn delete_confirmation_element(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.pending_delete.as_ref()?;
        let live = self.target_index(&pending.target);
        let running = live.is_some_and(|index| self.conversations[index].state.read(cx).is_busy());
        let selected = pending.confirm_selected;
        let action = if running { "Interromper e excluir arquivo" } else if live.is_some() {
            "Fechar e excluir arquivo"
        } else { "Excluir arquivo" };
        let body = if live.is_some() {
            "A conversa será fechada e qualquer execução será interrompida antes de mover o arquivo para a lixeira. Desfazer restaura apenas o arquivo salvo, não a execução."
        } else {
            "O arquivo será movido para a lixeira. Desfazer ficará disponível por 10 segundos e restaura apenas o arquivo salvo."
        };
        let mut controls = div().flex().justify_end().flex_wrap().gap(px(theme::S2));
        for (confirm, label) in [(false, "Cancelar"), (true, action)] {
            controls = controls.child(div().id(if confirm { "confirm-session-delete" } else { "cancel-session-delete" })
                .px(px(theme::S3)).py(px(theme::S2)).rounded(theme::r_control())
                .border_1().border_color(theme::line_strong()).text_size(px(theme::TEXT_SM))
                .text_color(if confirm { theme::danger() } else { theme::text() })
                .when(selected == confirm, |el| el.shadow(theme::focus_ring()))
                .cursor_pointer().hover(|style| style.bg(theme::hover()))
                .on_click(cx.listener(move |workspace, _: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    if confirm { workspace.confirm_session_delete(window, cx); }
                    else { workspace.cancel_session_delete(window, cx); }
                })).child(label));
        }
        Some(div().id("session-delete-scrim").occlude().absolute().inset(px(0.))
            .p(px(theme::S4)).flex().items_center().justify_center()
            .bg(theme::scrim()).backdrop_blur(px(theme::SCRIM_BLUR)).track_focus(&self.confirm_focus)
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, cx.listener(|workspace, _, window, cx| {
                workspace.cancel_session_delete(window, cx); cx.stop_propagation();
            }))
            .on_action(cx.listener(|workspace, _: &crate::ui::Dismiss, window, cx| {
                workspace.cancel_session_delete(window, cx); cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|workspace, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => workspace.cancel_session_delete(window, cx),
                    "tab" | "left" | "right" => {
                        if let Some(pending) = workspace.pending_delete.as_mut() { pending.confirm_selected = !pending.confirm_selected; }
                        cx.notify();
                    }
                    "enter" | "space" => {
                        if workspace.pending_delete.as_ref().is_some_and(|pending| pending.confirm_selected) {
                            workspace.confirm_session_delete(window, cx);
                        } else { workspace.cancel_session_delete(window, cx); }
                    }
                    _ => {}
                }
                cx.stop_propagation();
            }))
            .child(div().id("session-delete-card").w(px(theme::NAV_WIDTH * 2.))
                .max_w(window.viewport_size().width - px(theme::S6))
                .max_h(window.viewport_size().height - px(theme::S6)).overflow_y_scroll()
                .p(px(theme::S5)).rounded(theme::r_surface()).bg(theme::surface())
                .border_1().border_color(theme::line_strong()).shadow(theme::shadow_overlay())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .flex().flex_col().gap(px(theme::S4))
                .child(div().text_size(px(theme::TEXT)).font_weight(FontWeight::MEDIUM)
                    .text_color(theme::text()).child("Excluir arquivo da sessão?"))
                .child(div().text_size(px(theme::TEXT_SM)).text_color(theme::text())
                    .child(SharedString::from(pending.info.title.clone())))
                .child(div().font_family(theme::FONT_MONO).text_size(px(theme::TEXT_XS))
                    .text_color(theme::dim()).overflow_hidden()
                    .child(SharedString::from(pending.info.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default())))
                .child(div().text_size(px(theme::TEXT_SM)).line_height(px(19.)).text_color(theme::dim()).child(body))
                .child(controls)).into_any_element())
    }

    fn close_conversation(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.conversations.get(index).is_none() || self.conversations.len() == 1 {
            self.remove_conversation(index, window, cx);
            return;
        }
        let conversation = &self.conversations[index];
        let id = conversation.state.entity_id();
        let state = conversation.state.read(cx);
        let path = conversation.path.clone().or_else(|| state.session.file.as_ref().map(PathBuf::from));
        let client = state.client.clone();
        client.request_shutdown();
        self.closing.insert(id, ClosingConversation { path, _client: client.clone() });
        self.remove_conversation(index, window, cx);
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { client.shutdown_and_wait() }).await;
            let _ = this.update(cx, |workspace, cx| {
                match result {
                    Ok(()) => { workspace.closing.remove(&id); }
                    Err(_) => {
                        // Keep the fence and client: an unconfirmed process exit
                        // must not turn into permission to move its file.
                        workspace.error = Some("Não foi possível confirmar o encerramento do Pi. Arquivo preservado; reinicie o Dish antes de reabrir ou excluir.".into());
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    fn remove_conversation(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.conversations.get(index).is_none() {
            return;
        }
        if self.conversations.len() == 1 {
            self.error = Some(
                "Mantenha pelo menos uma sessão aberta. Crie outra antes de fechar esta.".into(),
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
        self.invalidate_content_search(cx);
    }

    /// Move uma sessão salva para a lixeira e oferece "Desfazer" por 10s.
    ///
    /// O filesystem pode estar lento ou em outro mount, então a transação
    /// inteira roda no executor de background.  A UI só recebe o resultado.
    fn delete_session(&mut self, info: SessionInfo, client: Option<PiClient>, return_focus: FocusHandle, cx: &mut Context<Self>) {
        let path = info.path.clone();
        if client.is_none() && (self.path_is_closing(&path)
            || self.conversations.iter().any(|conversation| conversation.path.as_ref() == Some(&path))) {
            self.error = Some("Pi ainda usa este arquivo. Aguarde o encerramento antes de excluir.".into());
            cx.notify();
            return;
        }
        if !self.deleting.insert(path.clone()) {
            return;
        }
        self.pending_delete = None;
        let closed_live = client.is_some();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let info = info.clone();
                    async move {
                        if let Some(client) = client { client.shutdown_and_wait()?; }
                        sessions::trash(&info)
                    }
                })
                .await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.deleting.remove(&path);
                match result {
                    Ok(trashed) => {
                        workspace.catalog_generation += 1;
                        drop_from_catalog(&mut workspace.catalog, &info.path);
                        workspace.delete_focus_pending = Some(return_focus);
                        workspace.invalidate_content_search(cx);
                        let pinned = workspace.preferences.pinned_sessions.remove(&info.path);
                        crate::preferences::save(workspace.preferences.clone());
                        workspace.undo_seq += 1;
                        let id = workspace.undo_seq;
                        workspace.undo_delete = Some(DeletedSession {
                            info,
                            trashed: trashed.trashed,
                            id,
                            pinned,
                        });
                        cx.notify();
                        Self::schedule_undo_expiry(id, cx);
                    }
                    Err(error) => {
                        workspace.error = Some(if closed_live {
                            format!("Conversa fechada, mas arquivo não excluído: {error}")
                        } else {
                            format!("Não foi possível excluir a sessão: {error}")
                        });
                        workspace.refresh(cx);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Devolve a última sessão excluída para o lugar de onde saiu.  A
    /// restauração é no-clobber: se o caminho original foi recriado, nada é
    /// sobrescrito e a sessão continua recuperável na lixeira.
    fn undo_delete(&mut self, cx: &mut Context<Self>) {
        let Some(undo) = self.undo_delete.as_ref() else {
            return;
        };
        if self.restoring == Some(undo.id) {
            return;
        }
        let id = undo.id;
        let trashed = undo.trashed.clone();
        let original = undo.info.path.clone();
        let info = undo.info.clone();
        let pinned = undo.pinned;
        self.restoring = Some(id);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { sessions::restore(&trashed, &original) })
                .await;
            let _ = this.update(cx, |workspace, cx| {
                if workspace.restoring != Some(id) {
                    return;
                }
                workspace.restoring = None;
                match result {
                    Ok(()) => {
                        if workspace
                            .undo_delete
                            .as_ref()
                            .is_some_and(|undo| undo.id == id)
                        {
                            workspace.undo_delete = None;
                        }
                        workspace.catalog_generation += 1;
                        if pinned {
                            workspace.preferences.pinned_sessions.insert(info.path.clone());
                            crate::preferences::save(workspace.preferences.clone());
                        }
                        add_to_catalog(&mut workspace.catalog, info);
                        workspace.invalidate_content_search(cx);
                        workspace.refresh(cx);
                    }
                    Err(error) => {
                        workspace.error = Some(if error.kind() == std::io::ErrorKind::AlreadyExists
                        {
                            "Não foi possível restaurar: já existe um arquivo no caminho original. A sessão continua na lixeira; mova o arquivo atual e tente novamente.".into()
                        } else {
                            format!("Não foi possível restaurar a sessão: {error}")
                        });
                        // O prazo original pode ter expirado enquanto a
                        // restauração estava em andamento.
                        Self::schedule_undo_expiry(id, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn schedule_undo_expiry(id: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(10))
                .await;
            this.update(cx, |workspace, cx| {
                if workspace
                    .undo_delete
                    .as_ref()
                    .is_some_and(|undo| undo.id == id)
                    && workspace.restoring != Some(id)
                {
                    workspace.undo_delete = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Barra "Sessão excluída — Desfazer" acima do rodapé da navegação.
    fn undo_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(undo) = &self.undo_delete else {
            return div().into_any_element();
        };
        let title = undo.info.title.clone();
        let restoring = self.restoring == Some(undo.id);
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
                    .track_focus(&self.undo_focus)
                    .px(px(theme::S1)).py(px(theme::S1))
                    .rounded(theme::r_control())
                    .focus(|style| style.shadow(theme::focus_ring()))
                    .on_key_down(cx.listener(|workspace, event: &KeyDownEvent, window, cx| {
                        match event.keystroke.key.as_str() {
                            "enter" | "space" => { workspace.undo_delete(cx); cx.stop_propagation(); }
                            "tab" => {
                                if event.keystroke.modifiers.shift { window.focus_prev(cx); }
                                else { window.focus_next(cx); }
                                cx.stop_propagation();
                            }
                            _ => {}
                        }
                    }))
                    .cursor_pointer()
                    .text_size(px(theme::TEXT_XS))
                    .text_color(theme::running())
                    .hover(|style| style.text_color(theme::text()))
                    .when(restoring, |el| el.opacity(0.6))
                    .when(!restoring, |el| {
                        el.on_click(move |_event: &ClickEvent, _, cx: &mut App| {
                            weak.update(cx, |workspace, cx| workspace.undo_delete(cx))
                                .ok();
                        })
                    })
                    .child(if restoring { "Restaurando…" } else { "Desfazer" }),
            )
            .into_any_element()
    }

    /// Título de uma conversa aberta: nome explícito da sessão, nome salvo no
    /// catálogo, título gerado/demanda ao vivo, ou o título salvo enquanto o
    /// histórico ainda não chegou.
    fn conversation_title(&self, index: usize, cx: &App) -> String {
        let Some(conversation) = self.conversations.get(index) else {
            return "Nova conversa".into();
        };
        let state = conversation.state.read(cx);
        let catalog = conversation.path.as_ref().and_then(|path| {
            self.catalog.iter().find(|info| &info.path == path)
        });
        if state.generated_titles {
            return crate::demand::summary_title(
                state.title_or_none(80).as_deref(),
                catalog.map(|info| info.title.as_str()),
                None,
                80,
            ).unwrap_or_else(|| "Nova conversa".into());
        }
        state
            .session
            .name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| {
                catalog
                    .filter(|info| info.named)
                    .map(|info| info.title.clone())
            })
            .or_else(|| state.title_or_none(80))
            .or_else(|| catalog.map(|info| info.title.clone()))
            .unwrap_or_else(|| "Nova conversa".into())
    }

}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.conversations[self.active].state.clone();
        if self.delete_focus_pending.take().is_some_and(|previous| previous.is_focused(window)) {
            self.undo_focus.focus(window, cx);
        }
        // Undo can disappear after restoration or expiry. Never strand focus
        // on an element that just left the dispatch tree or steal newer focus.
        if self.undo_delete.is_none() && self.undo_focus.is_focused(window) {
            let focus = state.read(cx).editor.read(cx).focus_handle.clone();
            focus.focus(window, cx);
        }
        if state.read(cx).help_open {
            state.update(cx, |state, _| {
                state.help_open = false;
                state.help_focus_pending = false;
            });
            self.open_settings(crate::settings::Section::Keys, window, cx);
        }
        if self.blocking_confirmation() || state.read(cx).modal.is_some() {
            if let Some(settings) = &self.settings {
                if settings.read(cx).open { settings.update(cx, |settings, cx| settings.suspend(cx)); }
            }
        }
        let active_viewed = window.is_window_active() && !self.blocking_confirmation() && !self.settings.as_ref().is_some_and(|settings| settings.read(cx).open);
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
        if let Some((opening, client)) = self.pending.take() {
            let OpeningSession { cwd, path, title } = opening;
            let editor = cx.new(|cx| Editor::new("", "Pergunte algo ao agente", window, cx));
            let modal_editor = cx.new(|cx| Editor::new("", "Escreva aqui…", window, cx));
            let search = cx.new(|cx| Editor::new("", "Buscar modelos ou provedores…", window, cx));
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
                state.restoring_session = path.is_some();
                state.initial_title = title;
                if let Some(open) = self.preferences.details_open {
                    state.sidebar = open;
                    state.inspector_initialized = true;
                }
                if let Some(show) = self.preferences.show_thinking {
                    state.show_thinking = show;
                }
                state.generated_titles = self.preferences.generated_titles;
                state.hidden_models = self.preferences.hidden_models.clone();
                state.start(cx);
            });
            let subscription = cx.observe(&state, |_, _, cx| cx.notify());
            if let Some(path) = &path {
                let (saved, live) = promote_session_row(&mut self.row_order, path, self.conversations.len());
                if let Some(anchor) = self.nav_anchors.remove(&saved) {
                    self.nav_anchors.insert(live, anchor);
                }
            }
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
                            .update(cx, |state, cx| {
                                state.cwd = info.cwd.clone();
                                state.git_context = crate::git_context::GitContext::Loading;
                                state.refresh_git(cx);
                            });
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
            self.spawn_conversation(cwd, None, None, cx);
        }
        let settings_open = self.settings.as_ref().is_some_and(|settings| settings.read(cx).open);
        for (index, conversation) in self.conversations.iter().enumerate() {
            conversation.state.update(cx, |state, _| {
                state.visible = index == self.active && !self.blocking_confirmation() && !settings_open
            });
        }
        let active = state.read(cx);
        theme::apply(window.appearance());
        theme::set_effort(active.thinking_level.as_deref(), &active.thinking_levels);
        let mut root = div()
            .id("workspace")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::surface())
            .text_color(theme::text())
            .font_family(theme::FONT_UI)
            .on_scroll_wheel(cx.listener(|workspace, _, window, cx| {
                workspace.dismiss_session_menu(window, cx);
            }))
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
            .on_action(cx.listener(|workspace, _: &NavUp, window, cx| {
                if !workspace.blocking_confirmation() { workspace.nav_move(-1, window, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &NavDown, window, cx| {
                if !workspace.blocking_confirmation() { workspace.nav_move(1, window, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &NavAccept, window, cx| {
                if !workspace.blocking_confirmation() { workspace.nav_accept(window, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &NavCollapse, _, cx| {
                if !workspace.blocking_confirmation() { workspace.nav_toggle_project(true, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &NavExpand, _, cx| {
                if !workspace.blocking_confirmation() { workspace.nav_toggle_project(false, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &NextConversation, window, cx| {
                if !workspace.blocking_confirmation() && !workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) {
                    workspace.select((workspace.active + 1) % workspace.conversations.len(), window, cx);
                }
            }))
            .on_action(cx.listener(|workspace, _: &PreviousConversation, window, cx| {
                if !workspace.blocking_confirmation() && !workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) {
                    let len = workspace.conversations.len();
                    workspace.select((workspace.active + len - 1) % len, window, cx);
                }
            }))
            .on_action(cx.listener(|workspace, _: &FocusPrompt, window, cx| {
                if !workspace.blocking_confirmation() && !workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) {
                    workspace.select(workspace.active, window, cx);
                }
            }))
            .on_action(cx.listener(|workspace, _: &FindCurrent, window, cx| {
                if !workspace.blocking_confirmation() { workspace.session_menu = None; workspace.open_content_search(false, window, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &FindProject, window, cx| {
                if !workspace.blocking_confirmation() { workspace.session_menu = None; workspace.open_content_search(true, window, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &ExitSearch, window, cx| {
                if !workspace.blocking_confirmation() { workspace.session_menu = None; workspace.exit_search(window, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &NextMatch, window, cx| {
                if !workspace.blocking_confirmation() { workspace.session_menu = None; workspace.step_match(1, window, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &PreviousMatch, window, cx| {
                if !workspace.blocking_confirmation() { workspace.session_menu = None; workspace.step_match(-1, window, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &AcceptMatch, window, cx| {
                if !workspace.blocking_confirmation() { workspace.session_menu = None; workspace.accept_match(window, cx); }
            }))
            .on_action(cx.listener(|workspace, _: &SearchSessions, window, cx| {
                if workspace.blocking_confirmation() || workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) { return; }
                workspace.session_menu = None;
                workspace.visible = true;
                workspace.nav_user_choice = true;
                workspace.query.read(cx).focus_handle.clone().focus(window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|workspace, _: &CloseConversation, window, cx| {
                if workspace.blocking_confirmation() || workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) { return; }
                let index = workspace.active;
                let target = SessionTarget::Live(workspace.conversations[index].state.entity_id());
                workspace.request_session_close(&target, window, cx);
            }))
            .on_action(cx.listener(|workspace, _: &ToggleSessions, _, cx| {
                if workspace.blocking_confirmation() || workspace.settings.as_ref().is_some_and(|s| s.read(cx).open) { return; }
                workspace.session_menu = None;
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
                if workspace.blocking_confirmation() || workspace.settings.as_ref().is_some_and(|settings| settings.read(cx).open) { return; }
                workspace.session_menu = None;
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
        // A decoração pertence à janela inteira, nunca ao layout da conversa.
        // Assim o inspetor não altera a posição da barra nem dos controles.
        if let Some(frame) = crate::ui::window_frame::layer(window) {
            root = root
                .child(frame)
                .pl(px(crate::ui::window_frame::BORDER))
                .pr(px(crate::ui::window_frame::BORDER))
                .pt(px(crate::ui::window_frame::BORDER))
                .pb(px(crate::ui::window_frame::BORDER))
                .child(crate::ui::client_titlebar(window));
        }
        let mut body = div().relative().flex_1().min_h(px(0.)).flex().flex_row();
        if self.visible {
            body = body.child(crate::ui::motion::panel(self.navigation(window, cx), "navigation-enter", false));
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
                .child(nav_icon_button("nav-settings-collapsed", crate::ui::icons::Icon::Settings, cx,
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
                    group = group.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(2.))
                            .when(pending_count > 0, |el| {
                                el.child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(3.))
                                        .text_size(px(theme::TEXT))
                                        .text_color(theme::warn())
                                        .child(SharedString::from(pending_count.to_string()))
                                        .child("P"),
                                )
                            })
                            .when(new_count > 0, |el| {
                                el.child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(3.))
                                        .text_size(px(theme::TEXT))
                                        .text_color(theme::attention())
                                        .child(SharedString::from(new_count.to_string()))
                                        .child("N"),
                                )
                            }),
                    );
                }
                // Recolher a navegação muda só sua apresentação: projetos
                // fechados continuam sem sessões visíveis também nesta faixa.
                // Seus indicadores agregados permanecem no ícone da pasta.
                if self.collapsed.contains(&cwd) {
                    dots = dots.child(group);
                    continue;
                }
                let any_opening_saved = self
                    .opening
                    .as_ref()
                    .is_some_and(|opening| opening.path.is_some());
                for index in open_indices {
                    let conv_state = self.conversations[index].state.read(cx);
                    let signals = SessionSignals::from_state(conv_state, self.conversations[index].unread_completion);
                    let loading_session = conv_state.is_loading_session();
                    let active = !any_opening_saved && index == self.active;
                    let pulse_id = format!("nav-pulse-{index}");
                    let spinner_id = format!("nav-dot-spinner-{index}");
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
                            .when(active, |el| {
                                el.bg(theme::surface_2())
                                    .border_1()
                                    .border_color(theme::accent_edge())
                            })
                            .on_hover(cx.listener(move |workspace, hovered: &bool, _, cx| {
                                workspace.hovered =
                                    hovered.then_some(NavHover::Conversation(index));
                                cx.notify();
                            }))
                            .on_click(cx.listener(move |workspace, _: &ClickEvent, window, cx| {
                                workspace.select(index, window, cx);
                            }))
                            .child(if loading_session {
                                crate::ui::icons::spinner(spinner_id, 14., theme::accent()).into_any_element()
                            } else if signals.needs_input {
                                crate::ui::icons::icon(crate::ui::icons::Icon::Alert, 12., theme::warn()).into_any_element()
                            } else if signals.error {
                                crate::ui::icons::icon(crate::ui::icons::Icon::Alert, 12., theme::failure()).into_any_element()
                            } else if signals.running {
                                crate::ui::pulse(pulse_id, 8., theme::running()).into_any_element()
                            } else if signals.unread {
                                crate::ui::icons::icon(crate::ui::icons::Icon::File, 12., theme::attention()).into_any_element()
                            } else {
                                crate::ui::icons::icon(if active { crate::ui::icons::Icon::CurrentSession } else { crate::ui::icons::Icon::OpenSession },
                                    16., if active { theme::accent() } else { theme::dim() }).into_any_element()
                            }),
                    );
                }
                for info_index in saved_indices {
                    let info_ref = self.catalog.get(info_index);
                    let opening_here = info_ref.is_some_and(|info| {
                        self.opening
                            .as_ref()
                            .is_some_and(|opening| opening.path.as_ref() == Some(&info.path))
                    });
                    let unread = info_ref
                        .is_some_and(|info| self.preferences.unread_sessions.contains_key(&info.path));
                    let spinner_id = format!("nav-saved-spinner-{info_index}");
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
                            .when(opening_here, |el| {
                                el.bg(theme::surface_2())
                                    .border_1()
                                    .border_color(theme::accent_edge())
                            })
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
                            .child(if opening_here {
                                crate::ui::icons::spinner(spinner_id, 14., theme::accent()).into_any_element()
                            } else if unread {
                                crate::ui::icons::icon(crate::ui::icons::Icon::File, 12., theme::attention()).into_any_element()
                            } else {
                                crate::ui::icons::icon(crate::ui::icons::Icon::SavedSession, 16., theme::faint()).into_any_element()
                            }),
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
        preferences.generated_titles = active.generated_titles;
        preferences.collapsed_projects = self.collapsed.clone();
        preferences.last_project = Some(active.cwd.clone());
        preferences.last_session = active.session.file.as_ref().map(PathBuf::from);
        preferences.remember_project(&active.cwd);
        let bounds = window.bounds();
        preferences.window_size = Some([f32::from(bounds.size.width), f32::from(bounds.size.height)]);
        if preferences != self.preferences {
            crate::preferences::save(preferences.clone());
            self.preferences = preferences;
        }
        // A lista de modelos chega do Pi de forma assíncrona: se as configurações
        // já estiverem abertas, mantém a cópia delas atualizada.
        if let Some(settings) = self.settings.clone() {
            if settings.read(cx).open {
                let models: Vec<(String, String)> = active
                    .models
                    .iter()
                    .map(|model| (model.provider.clone(), model.id.clone()))
                    .collect();
                if models != self.settings_models {
                    self.settings_models = models.clone();
                    settings.update(cx, |settings, cx| settings.set_models(models, cx));
                }
            }
        }
        let opening_view = self
            .opening
            .as_ref()
            .filter(|opening| opening.path.is_some())
            .map(|opening| {
                crate::ui::opening_session_view(&opening.cwd, opening.title.as_deref())
                    .into_any_element()
            });
        let main_view = opening_view.unwrap_or_else(|| state.into_any_element());
        root = root.child(body.child(div().flex_1().min_w(px(0.)).h_full().p(px(theme::S2)).child(
            crate::ui::motion::reveal(
                div().size_full().rounded(theme::r_surface()).overflow_hidden()
                    .border_1().border_color(theme::line_soft()).child(main_view),
                ElementId::NamedInteger("conversation-enter".into(), self.active as u64),
            ),
        )));
        self.refresh_content_search_scope(cx);
        self.resolve_search_jump(cx);
        if let Some(panel) = self.content_search_panel(cx) {
            root = root.child(panel);
        }
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
                        (self.conversation_title(index, cx), format!("{} · {}", lifecycle_label(true, index == self.active), signals.labels()))
                    }
                    NavHover::Saved(index) => {
                        let info = self.catalog.get(*index);
                        (
                            info.map(|info| info.title.clone())
                                .unwrap_or_else(|| "sessão salva".into()),
                            "Salva".to_string(),
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
        if self.session_menu.is_some() {
            let missing = self.session_menu.as_ref().is_some_and(|menu| {
                matches!(menu.target, SessionTarget::Live(_)) && self.target_index(&menu.target).is_none()
            });
            if missing || !self.visible || self.blocking_confirmation() || settings_open {
                self.dismiss_session_menu(window, cx);
            } else {
                if !self.session_menu_focus.is_focused(window) { self.session_menu_focus.focus(window, cx); }
                if let Some(menu) = self.session_menu_element(window, cx) { root = root.child(menu); }
            }
        }
        if self.pending_delete.is_some() {
            if !self.confirm_focus.is_focused(window) { self.confirm_focus.focus(window, cx); }
            if let Some(confirmation) = self.delete_confirmation_element(window, cx) { root = root.child(confirmation); }
        }
        if self.confirm_close {
            if !self.confirm_focus.is_focused(window) {
                self.confirm_focus.focus(window, cx);
            }
            let target = self.close_target;
            root = root.child(
                div()
                    .occlude()
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
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
                    .bg(theme::scrim()).backdrop_blur(px(theme::SCRIM_BLUR))
                    .child(
                        div()
                            .w(px(420.))
                            .p(px(theme::S5))
                            .rounded(theme::r_surface())
                            .border_1()
                            .border_color(theme::line_strong())
                            .bg(theme::surface())
                            .shadow(theme::shadow_overlay())
                            .flex()
                            .flex_col()
                            .gap(px(theme::S4))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme::text())
                                    .child("Tarefas em execução"),
                            )
                            .child(
                                div()
                                    .text_size(px(theme::TEXT_SM))
                                    .line_height(px(19.))
                                    .text_color(theme::dim())
                                    .child(
                                        "Fechar agora interrompe os processos do Pi destas sessões.",
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(px(theme::S2))
                                    .child(
                                        div()
                                            .id("cancel-workspace-close")
                                            .px(px(theme::S3))
                                            .py(px(theme::S1 + 3.))
                                            .rounded(theme::r_control())
                                            .border_1()
                                            .border_color(theme::line_strong())
                                            .text_size(px(theme::TEXT_SM))
                                            .text_color(theme::dim())
                                            .cursor_pointer()
                                            .hover(|s| {
                                                s.bg(theme::hover()).text_color(theme::text())
                                            })
                                            .child("Manter em execução")
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
                                            .px(px(theme::S3))
                                            .py(px(theme::S1 + 3.))
                                            .rounded(theme::r_control())
                                            .border_1()
                                            .border_color(theme::line_strong())
                                            .text_size(px(theme::TEXT_SM))
                                            .text_color(theme::danger())
                                            .cursor_pointer()
                                            .hover(|s| {
                                                s.bg(theme::wash(theme::failure(), 0.12))
                                                    .border_color(theme::line_strong())
                                            })
                                            .child("Interromper e fechar")
                                            .on_click(cx.listener(
                                                move |workspace, _: &ClickEvent, window, cx| {
                                                    workspace.confirm_close = false;
                                                    if let Some(id) = target {
                                                        if let Some(index) = workspace.target_index(&SessionTarget::Live(id)) {
                                                            workspace.close_conversation(index, window, cx);
                                                        }
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
        // Last painted child delimits a complete observation frame. Nodes not
        // painted in this frame (virtualised/closed) must not survive in tests.
        let active = self.conversations[self.active].state.read(cx);
        root.test_probe(|| "frame".into(), || serde_json::json!({
            "project": active.cwd,
            "draft": active.editor.read(cx).text(cx),
            "nav_query": self.query.read(cx).text(cx),
            "confirming_close": self.confirm_close,
            "session_menu": self.session_menu.is_some(),
        }))
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

/// Nome curto de um projeto: o nome da pasta, ou o caminho inteiro.
fn folder_of(cwd: &std::path::Path) -> String {
    cwd.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| cwd.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::{add_to_catalog, drop_from_catalog, select_item, sort_session_rows, row_key, attention_summary, filter_counts, nav_position, NavEntry, SessionSignals, SessionFilter};
    use crate::sessions::SessionInfo;
    use std::path::PathBuf;

    fn info(path: &str, modified: u64) -> SessionInfo {
        SessionInfo {
            path: PathBuf::from(path),
            cwd: PathBuf::from("/project"),
            title: path.into(),
            named: false,
            modified: std::time::UNIX_EPOCH + std::time::Duration::from_secs(modified),
            scan: Default::default(),
        }
    }

    #[test]
    fn lifecycle_distinguishes_current_open_and_saved_without_activity() {
        assert_eq!(super::lifecycle_label(true, true), "Atual");
        assert_eq!(super::lifecycle_label(true, false), "Aberta");
        assert_eq!(super::lifecycle_label(false, false), "Salva");
    }

    #[test]
    fn delayed_live_action_keeps_identity_when_an_earlier_session_closes() {
        let first = gpui::EntityId::from(0x1_0000_0001_u64);
        let second = gpui::EntityId::from(0x1_0000_0002_u64);
        let target = super::SessionTarget::Live(second);
        assert_eq!(super::resolve_session_target(&target, [(first, None), (second, None)].into_iter()), Some(1));
        assert_eq!(super::resolve_session_target(&target, [(second, None)].into_iter()), Some(0));
        assert_eq!(super::resolve_session_target(&target, [(first, None)].into_iter()), None);
    }

    #[test]
    fn saved_action_resolves_a_file_that_became_live_after_menu_opened() {
        let path = PathBuf::from("/beta.jsonl");
        let target = super::SessionTarget::Saved(path.clone());
        let id = gpui::EntityId::from(0x1_0000_0001_u64);
        assert_eq!(super::resolve_session_target(&target, [(id, Some(path.as_path()))].into_iter()), Some(0));
        assert_eq!(super::resolve_session_target(&target, [(id, None)].into_iter()), None);
    }

    #[test]
    fn stale_catalog_snapshot_cannot_resurrect_a_trashed_session() {
        assert!(super::catalog_snapshot(vec![info("/deleted.jsonl", 1)], 3, 4, &Default::default()).is_none());
    }

    #[test]
    fn catalog_snapshot_fences_paths_while_shutdown_and_trash_are_pending() {
        let deleting = [PathBuf::from("/pending.jsonl")].into_iter().collect();
        let snapshot = super::catalog_snapshot(vec![info("/pending.jsonl", 2), info("/other.jsonl", 1)],
            3, 3, &deleting).unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].path, PathBuf::from("/other.jsonl"));
    }

    #[test]
    fn opening_saved_session_preserves_its_position_during_interaction() {
        let saved = info("/middle.jsonl", 2);
        let mut rows = vec![
            ("first".into(), None, Some(0), std::time::UNIX_EPOCH),
            (saved.title.clone(), Some(saved.clone()), None, saved.modified),
            ("last".into(), None, Some(1), std::time::UNIX_EPOCH),
        ];
        let mut previous = rows.iter().map(row_key).collect::<Vec<_>>();
        let (old, new) = super::promote_session_row(&mut previous, &saved.path, 2);
        assert_eq!(old, "saved-/middle.jsonl");
        assert_eq!(new, "live-2");
        rows[1].1 = None;
        rows[1].2 = Some(2);
        // Freeze applies while the mouse remains over the row just clicked.
        sort_session_rows(&mut rows, &previous, true);
        assert_eq!(rows.iter().map(row_key).collect::<Vec<_>>(), ["live-0", "live-2", "live-1"]);
        // Once interaction ends, normal activity ordering still applies.
        sort_session_rows(&mut rows, &previous, false);
        assert_eq!(row_key(&rows[0]), "live-2");
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
    fn nav_highlight_follows_the_selected_entry() {
        let entries = [
            NavEntry::Open(0),
            NavEntry::Saved(PathBuf::from("/a.jsonl")),
            NavEntry::Open(1),
        ];
        assert_eq!(nav_position(&entries, &NavEntry::Open(1)), Some(2));
        assert_eq!(
            nav_position(&entries, &NavEntry::Saved(PathBuf::from("/a.jsonl"))),
            Some(1)
        );
        assert_eq!(
            nav_position(&entries, &NavEntry::Saved(PathBuf::from("/b.jsonl"))),
            None
        );
    }

    #[test]
    fn filter_counts_include_attention_without_implying_running() {
        let signals = [
            SessionSignals { needs_input: true, ..Default::default() },
            SessionSignals { unread: true, ..Default::default() },
            SessionSignals { running: true, ..Default::default() },
            SessionSignals { saved: true, ..Default::default() },
        ];
        assert_eq!(filter_counts(&signals), [4, 1, 1, 1]);
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
