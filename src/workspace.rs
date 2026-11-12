use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::*;
use gpui::*;

use crate::editor::Editor;
use crate::preferences::{ProjectOrder, SessionLayout, SessionOrder};
use crate::rpc::{PiClient, PiConfig};
use crate::sessions::{self, SessionInfo};
use crate::state::{AppState, AppStateEvent};
use crate::theme;
use crate::notifications::{Notification, NotificationQueue};

#[derive(serde::Deserialize)]
struct TestNotification {
    id: String,
    title: String,
    detail: String,
}
use crate::ui::test_probe::TestProbe;
use crate::session_activity::{SessionFilter, SessionSignals, epoch_millis, from_epoch_millis, relative_time};

mod navigation;
use self::navigation::nav_icon_button;
mod search;

actions!(dish_workspace, [FindCurrent, FindProject, ExitSearch, NextMatch, PreviousMatch, AcceptMatch, ToggleSessions, NextConversation, PreviousConversation, CloseConversation, SearchSessions, FocusPrompt, NavUp, NavDown, NavAccept, NavCollapse, NavExpand, CycleSessionFilter]);

/// Eventos que precisam subir até a raiz do onboarding.
#[derive(Clone, Copy)]
pub enum WorkspaceEvent {
    ReopenSetup,
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

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
    _events: Subscription,
    seen_completed_runs: u64,
    unread_completion: bool,
}

/// Keep a closed view's process fenced until its exact exit is acknowledged.
struct ClosingConversation {
    path: Option<PathBuf>,
    _client: PiClient,
}

/// One session file sitting in the trash.
#[derive(Clone)]
struct DeletedItem {
    info: SessionInfo,
    trashed: PathBuf,
    pinned: bool,
}

/// The last deleted batch (one session or a whole folder) while the undo
/// window is open.
struct DeletedSession {
    items: Vec<DeletedItem>,
    id: u64,
}

/// Mensagem quando a restauração não devolveu tudo. Uma sessão mantém o texto
/// de sempre; um lote diz quantas voltaram e quais ficaram na lixeira.
fn restore_failure_message(total: usize, failed: &[(DeletedItem, std::io::Error)]) -> String {
    let exists = |error: &std::io::Error| error.kind() == std::io::ErrorKind::AlreadyExists;
    if total == 1 {
        if let Some((_, error)) = failed.first() {
            return if exists(error) {
                "Não foi possível restaurar: já existe um arquivo no caminho original. A sessão continua na lixeira; mova o arquivo atual e tente novamente.".into()
            } else {
                format!("Não foi possível restaurar a sessão: {error}")
            };
        }
    }
    let names: Vec<String> = failed
        .iter()
        .map(|(item, error)| {
            let name = item.info.path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if exists(error) { format!("{name} (já existe um arquivo no caminho original)") } else { format!("{name} ({error})") }
        })
        .collect();
    format!(
        "Restauradas {} de {total} sessões. Não voltaram: {}",
        total - failed.len(),
        names.join(", "),
    )
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

/// Uma conversa aberta: arquivo da sessão (se já existe), pasta e se executa.
type LiveSession = (Option<PathBuf>, PathBuf, bool);

/// A conversa aberta usa esta pasta ou um dos arquivos dela?
fn live_matches(entry: &LiveSession, folder: &std::path::Path, folder_paths: &[PathBuf]) -> bool {
    let (path, cwd, _) = entry;
    cwd.as_path() == folder || path.as_ref().is_some_and(|path| folder_paths.contains(path))
}

/// Agrupa o catálogo por pasta de projeto para a seção Sessões, da atividade
/// mais recente para a mais antiga.
fn session_folders(catalog: &[SessionInfo], live: &[LiveSession]) -> Vec<crate::settings::SessionFolder> {
    let mut groups: BTreeMap<&std::path::Path, Vec<&SessionInfo>> = BTreeMap::new();
    for info in catalog {
        groups.entry(info.cwd.as_path()).or_default().push(info);
    }
    let mut folders: Vec<crate::settings::SessionFolder> = groups
        .into_iter()
        .map(|(cwd, infos)| {
            let paths: Vec<PathBuf> = infos.iter().map(|info| info.path.clone()).collect();
            let matching = || live.iter().filter(|entry| live_matches(entry, cwd, &paths));
            crate::settings::SessionFolder {
                cwd: cwd.to_path_buf(),
                count: infos.len(),
                bytes: infos.iter().map(|info| info.file_len()).sum(),
                last: infos.iter().map(|info| info.modified).max().unwrap_or(std::time::UNIX_EPOCH),
                open: matching().next().is_some(),
                running: matching().any(|(_, _, busy)| *busy),
            }
        })
        .collect();
    folders.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| a.cwd.cmp(&b.cwd)));
    folders
}

/// Por que a pasta não pode ser esvaziada agora, ou `None` se pode.
fn folder_delete_blocker(
    folder: &std::path::Path,
    folder_paths: &[PathBuf],
    live: &[LiveSession],
    closing: &[Option<PathBuf>],
    deleting: &BTreeSet<PathBuf>,
) -> Option<&'static str> {
    if live.iter().any(|entry| live_matches(entry, folder, folder_paths)) {
        return Some("Feche as conversas desta pasta antes de excluir — o Pi pode estar usando os arquivos.");
    }
    // Um encerramento sem arquivo conhecido pode ser de qualquer pasta.
    if closing.iter().any(|path| path.as_ref().is_none_or(|path| folder_paths.contains(path))) {
        return Some("Pi ainda está encerrando. Aguarde antes de excluir.");
    }
    if folder_paths.iter().any(|path| deleting.contains(path)) {
        return Some("Exclusão em andamento.");
    }
    if folder_paths.is_empty() {
        return Some("Nenhuma sessão nesta pasta.");
    }
    None
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
    std::time::SystemTime,
);

fn attention_summary(pending: usize, unread: usize) -> String {
    let mut labels = Vec::new();
    if pending > 0 { labels.push(format!("{pending} pend.")); }
    if unread > 0 { labels.push(format!("{unread} {}", if unread == 1 { "nova" } else { "novas" })); }
    labels.join(" · ")
}

fn sort_session_rows(
    items: &mut [SessionRow],
    previous: &[String],
    interacting: bool,
    order: SessionOrder,
    pinned: impl Fn(&SessionRow) -> bool,
) {
    match order {
        SessionOrder::Activity => {
            items.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| row_key(a).cmp(&row_key(b))));
        }
        SessionOrder::Completed => {
            items.sort_by(|a, b| {
                b.4.cmp(&a.4)
                    .then_with(|| b.3.cmp(&a.3))
                    .then_with(|| row_key(a).cmp(&row_key(b)))
            });
        }
        SessionOrder::Name => {
            items.sort_by(|a, b| {
                a.0.to_lowercase()
                    .cmp(&b.0.to_lowercase())
                    .then_with(|| row_key(a).cmp(&row_key(b)))
            });
        }
    }
    if interacting {
        items.sort_by_key(|item| previous.iter().position(|key| *key == row_key(item)).unwrap_or(usize::MAX));
    }
    // Fixadas primeiro, preservando a ordem escolhida entre elas.
    items.sort_by_key(|item| !pinned(item));
}

/// Estado agregado de um grupo de projeto, para a ordem dos grupos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProjectRank {
    running: bool,
    needs_input: bool,
    latest: std::time::SystemTime,
}

fn compare_projects(
    a: (&std::path::Path, &ProjectRank),
    b: (&std::path::Path, &ProjectRank),
    order: ProjectOrder,
) -> std::cmp::Ordering {
    match order {
        ProjectOrder::Activity => b.1.running.cmp(&a.1.running)
            .then_with(|| b.1.needs_input.cmp(&a.1.needs_input))
            .then_with(|| b.1.latest.cmp(&a.1.latest))
            .then_with(|| a.0.cmp(b.0)),
        ProjectOrder::Name => folder_of(a.0).to_lowercase().cmp(&folder_of(b.0).to_lowercase())
            .then_with(|| a.0.cmp(b.0)),
    }
}

/// Ordena os grupos; durante a interação, a ordem anterior é congelada para
/// que nada se mova sob o ponteiro ou o teclado.
fn sort_project_groups<T>(
    groups: &mut [(PathBuf, ProjectRank, T)],
    previous: &[PathBuf],
    interacting: bool,
    order: ProjectOrder,
) {
    groups.sort_by(|a, b| compare_projects((a.0.as_path(), &a.1), (b.0.as_path(), &b.1), order));
    if interacting {
        groups.sort_by_key(|group| {
            previous.iter().position(|path| *path == group.0).unwrap_or(usize::MAX)
        });
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
    sessions_notice: Option<String>,
    deleting_folder: Option<PathBuf>,
    confirm_close: bool,
    close_target: Option<EntityId>,
    pending: Option<(OpeningSession, PiClient)>,
    /// Conversas cujo processo do Pi está sendo reiniciado por "Reabrir esta
    /// conversa"; impede duas reinicializações para a mesma sessão.
    reopening: BTreeSet<EntityId>,
    confirm_focus: FocusHandle,
    /// Campo de busca da navegação.
    query: Entity<Editor>,
    filter: SessionFilter,
    nav_hovered: bool,
    row_order: Vec<String>,
    /// Última ordem desenhada dos grupos de projeto, para congelar a lista.
    project_order: Vec<PathBuf>,
    nav_anchors: BTreeMap<String, ScrollAnchor>,
    /// Se o usuário já decidiu mostrar/ocultar a navegação nesta janela.
    nav_user_choice: bool,
    /// O que está sob o mouse na faixa recolhida, para o chip de título.
    hovered: Option<NavHover>,
    preferences: crate::preferences::Preferences,
    notifications: NotificationQueue,
    notification_focus: BTreeMap<String, (FocusHandle, FocusHandle)>,
    /// Last version reported by the selected Pi, if it answered `--version`.
    pi_installed: Option<String>,
    /// Last version published in the npm registry, if a check succeeded.
    pi_latest: Option<String>,
    pi_checking: bool,
    /// Manual result shown in Sobre / Pi; automatic failures stay in the log.
    pi_notice: Option<String>,
    /// Invalidates completion monitors from older installer launches.
    pi_install_generation: u64,
    /// Aviso da linha de comando (pasta inexistente), mostrado no rail.
    notice: Option<String>,
    settings: Option<Entity<crate::settings::Settings>>,
    /// Última lista de modelos enviada às configurações abertas.
    settings_models: Vec<(String, String)>,
    content_search: search::SearchState,
}

impl Workspace {
    /// App-wide producer entry point. The id is stable across launches.
    #[allow(dead_code)] // Concrete product producers are deliberately out of scope.
    pub fn enqueue_notification(&mut self, notification: Notification, cx: &mut Context<Self>) {
        if self.preferences.dismissed_notifications.contains(&notification.id) { return; }
        let id = notification.id.clone();
        if self.notifications.push(notification, &self.preferences.dismissed_notifications) {
            self.notification_focus.entry(id).or_insert_with(||
                (cx.focus_handle().tab_stop(true), cx.focus_handle().tab_stop(true)));
            cx.notify();
        }
    }

    fn dismiss_notification(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.notifications.dismiss(id) {
            self.notification_focus.remove(id);
            self.preferences.dismissed_notifications.insert(id.to_string());
            crate::preferences::save(self.preferences.clone());
            cx.notify();
        }
    }

    fn poll_test_notifications(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let Ok(bytes) = std::fs::read(path) else { return; };
        let Ok(items) = serde_json::from_slice::<Vec<TestNotification>>(&bytes) else { return; };
        let ids = items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>().join("\n");
        for item in items {
            self.enqueue_notification(Notification {
                id: item.id, title: item.title, detail: item.detail,
                action_label: None, action: None,
            }, cx);
        }
        let ack = path.with_extension("ack");
        if std::fs::read_to_string(&ack).ok().as_deref() != Some(ids.as_str()) {
            let _ = std::fs::write(ack, ids);
        }
    }

    fn pi_status(&self) -> crate::settings::PiStatus {
        crate::settings::PiStatus {
            installed: self.pi_installed.clone(),
            latest: self.pi_latest.clone(),
            last_check: self.preferences.last_pi_update_check,
            checking: self.pi_checking,
            notice: self.pi_notice.clone(),
            auto_check: self.preferences.pi_auto_check,
            auto_install: self.preferences.pi_auto_install,
        }
    }

    fn sync_pi_settings(&self, cx: &mut Context<Self>) {
        let Some(settings) = &self.settings else { return; };
        let status = self.pi_status();
        settings.update(cx, |settings, cx| settings.set_pi_status(status, cx));
    }

    fn enqueue_pi_available(&mut self, available: crate::pi_update::Available, cx: &mut Context<Self>) {
        self.enqueue_notification(Notification {
            id: available.id,
            title: available.title,
            detail: available.detail,
            action_label: Some("Atualizar o Pi".into()),
            action: Some(std::rc::Rc::new(|workspace, _, cx| workspace.request_pi_update(false, cx))),
        }, cx);
    }

    /// Reserve the daily attempt before going to the network. Failures also
    /// count, so an offline machine does not retry in a loop.
    fn maybe_check_pi_updates(&mut self, cx: &mut Context<Self>) {
        if !crate::pi_update::check_due(
            self.preferences.last_pi_update_check,
            crate::pi_update::now_unix(),
            self.preferences.pi_auto_check,
        ) {
            return;
        }
        self.check_pi_updates(true, cx);
    }

    /// Daily Dish release check: at most one attempt per day, notification
    /// only. Separately gated from the Pi update check above.
    fn maybe_check_dish_update(&mut self, cx: &mut Context<Self>) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_secs() as i64).unwrap_or_default();
        if let Some(check) = crate::update::Check::automatic(&mut self.preferences, now) {
            crate::preferences::save(self.preferences.clone());
            cx.spawn(async move |this, cx| {
                let outcome = cx.background_executor().spawn(async move { check.run() }).await;
                if let Some(outcome) = outcome {
                    let _ = this.update(cx, |workspace, cx| {
                        workspace.preferences.last_update_etag = outcome.etag;
                        crate::preferences::save(workspace.preferences.clone());
                        if let Some(available) = outcome.notification {
                            workspace.enqueue_notification(available.notification(), cx);
                        }
                    });
                }
            }).detach();
        }
    }

    fn check_pi_updates(&mut self, automatic: bool, cx: &mut Context<Self>) {
        if self.pi_checking {
            return;
        }
        self.pi_checking = true;
        self.pi_notice = None;
        self.preferences.last_pi_update_check = Some(crate::pi_update::now_unix());
        crate::preferences::save(self.preferences.clone());
        self.sync_pi_settings(cx);
        let program = crate::installation::program();
        cx.spawn(async move |this, cx| {
            let (installed, latest) = cx
                .background_executor()
                .spawn(async move {
                    let installed = crate::pi_update::installed_version(&program);
                    let latest = crate::pi_update::fetch_latest().ok();
                    (installed, latest)
                })
                .await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.pi_checking = false;
                workspace.pi_installed = installed;
                match latest {
                    Some(latest) => {
                        workspace.pi_latest = Some(latest.clone());
                        if let Some(available) = crate::pi_update::available(
                            workspace.pi_installed.as_deref(),
                            &latest,
                        ) {
                            workspace.pi_notice = Some(format!(
                                "Pi {} disponível. A instalação abre o instalador oficial num terminal.",
                                available.version
                            ));
                            if automatic && workspace.preferences.pi_auto_install {
                                workspace.request_pi_update(true, cx);
                            } else {
                                workspace.enqueue_pi_available(available, cx);
                            }
                        } else {
                            workspace.pi_notice = Some(match workspace.pi_installed.as_deref() {
                                Some(installed) if installed == latest => format!(
                                    "Você está na versão mais recente do Pi ({installed})."
                                ),
                                Some(installed) => format!(
                                    "O Pi instalado ({installed}) não é anterior à versão publicada ({latest})."
                                ),
                                None => format!(
                                    "Última versão publicada do Pi: {latest}. Versão instalada desconhecida."
                                ),
                            });
                        }
                    }
                    None => {
                        crate::pi_update::log_check_failed(automatic);
                        if !automatic {
                            workspace.pi_notice = Some(
                                "Não foi possível verificar agora. Tente novamente.".into(),
                            );
                        }
                    }
                }
                workspace.sync_pi_settings(cx);
            });
        })
        .detach();
    }

    fn request_pi_update(&mut self, automatic: bool, cx: &mut Context<Self>) {
        let busy = self
            .conversations
            .iter()
            .any(|conversation| conversation.state.read(cx).is_executing());
        if busy {
            let message = "Aguarde a resposta terminar para atualizar o Pi.";
            crate::logging::warn(format_args!("Dish: Pi installer deferred while streaming"));
            self.pi_notice = Some(message.into());
            if automatic {
                if let Some(available) = crate::pi_update::available(
                    self.pi_installed.as_deref(),
                    self.pi_latest.as_deref().unwrap_or_default(),
                ) {
                    self.enqueue_pi_available(available, cx);
                }
            } else {
                let state = self.conversations[self.active].state.clone();
                state.update(cx, |state, cx| {
                    state.push_toast(message.into(), crate::state::Tone::Warning, cx);
                });
            }
            self.sync_pi_settings(cx);
            return;
        }
        let before = self.pi_installed.clone();
        crate::pi_update::log_install_start(before.as_deref(), automatic);
        match crate::pi_update::launch_installer() {
            Ok(()) => {
                self.pi_notice = Some(
                    "Instalador oficial do Pi aberto no terminal. O Dish avisa quando a versão mudar."
                        .into(),
                );
                if automatic {
                    if let Some(latest) = self.pi_latest.clone() {
                        self.enqueue_notification(Notification {
                            id: format!("pi-update-started-{latest}"),
                            title: format!("Instalando Pi {latest}"),
                            detail: "O instalador oficial foi aberto no terminal. A versão atual será confirmada ao terminar."
                                .into(),
                            action_label: None,
                            action: None,
                        }, cx);
                    }
                } else {
                    let state = self.conversations[self.active].state.clone();
                    state.update(cx, |state, cx| {
                        state.push_toast(
                            "Instalador do Pi aberto no terminal.".into(),
                            crate::state::Tone::Info,
                            cx,
                        );
                    });
                }
                self.sync_pi_settings(cx);
                self.monitor_pi_install(before, cx);
            }
            Err(error) => {
                let message = format!(
                    "Não foi possível abrir o instalador do Pi: {error:#}. Use o instalador oficial do Pi em um terminal."
                );
                crate::logging::warn(format_args!("Dish: Pi installer unavailable: {error:#}"));
                self.pi_notice = Some(message.clone());
                if automatic {
                    self.enqueue_notification(Notification {
                        id: format!("pi-update-error-{}", self.pi_latest.as_deref().unwrap_or("pi")),
                        title: "Não foi possível atualizar o Pi".into(),
                        detail: message,
                        action_label: None,
                        action: None,
                    }, cx);
                } else {
                    let state = self.conversations[self.active].state.clone();
                    state.update(cx, |state, cx| {
                        state.push_toast(message, crate::state::Tone::Error, cx);
                    });
                }
                self.sync_pi_settings(cx);
            }
        }
    }

    /// The official installer runs in a visible terminal, so its exit is not
    /// tied to an RPC process. Poll the selected executable and report the
    /// version change instead of claiming success when the terminal opens.
    fn monitor_pi_install(&mut self, before: Option<String>, cx: &mut Context<Self>) {
        self.pi_install_generation = self.pi_install_generation.wrapping_add(1);
        let generation = self.pi_install_generation;
        let program = crate::installation::program();
        cx.spawn(async move |this, cx| {
            let mut baseline = before;
            for _ in 0..60 {
                cx.background_executor().timer(Duration::from_secs(10)).await;
                let program = program.clone();
                let found = cx
                    .background_executor()
                    .spawn(async move { crate::pi_update::installed_version(&program) })
                    .await;
                let Some(found) = found else { continue; };
                if baseline.is_none() {
                    // The version may not have been loaded when the button was
                    // pressed. Keep the first answer as the baseline instead of
                    // reporting a false update after a cancelled installer.
                    baseline = Some(found);
                    continue;
                }
                if Some(found.as_str()) == baseline.as_deref() {
                    continue;
                }
                let previous = baseline.clone();
                let detected = found;
                let _ = this.update(cx, move |workspace, cx| {
                    if workspace.pi_install_generation != generation {
                        return;
                    }
                    crate::pi_update::log_install_result(previous.as_deref(), Some(&detected));
                    workspace.pi_installed = Some(detected.clone());
                    let message = format!(
                        "Pi atualizado: {} → {detected}.",
                        crate::pi_update::display_version(previous.as_deref())
                    );
                    workspace.pi_notice = Some(message.clone());
                    workspace.enqueue_notification(Notification {
                        id: format!("pi-updated-{detected}"),
                        title: format!("Pi atualizado para {detected}"),
                        detail: format!(
                            "Versão anterior: {}. Novas conversas usarão o novo executável.",
                            crate::pi_update::display_version(previous.as_deref())
                        ),
                        action_label: None,
                        action: None,
                    }, cx);
                    workspace.sync_pi_settings(cx);
                });
                return;
            }
            let previous = baseline.clone();
            let _ = this.update(cx, move |workspace, _| {
                if workspace.pi_install_generation != generation {
                    return;
                }
                crate::pi_update::log_install_result(
                    previous.as_deref(),
                    workspace.pi_installed.as_deref(),
                );
            });
        })
        .detach();
    }

    fn notification_cards(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let streaming = self.conversations.iter().any(|conversation| conversation.state.read(cx).is_executing());
        let visible: Vec<_> = self.notifications.visible(streaming).collect();
        if visible.is_empty() { return None; }
        let mut stack = div().absolute().top(px(64.)).right(px(24.))
            .w(px(360.)).flex().flex_col().gap(px(8.));
        for notification in visible {
            let id = notification.id.clone();
            let Some((action_focus, dismiss_focus)) = self.notification_focus.get(&id) else { continue; };
            let dismiss_id = id.clone();
            let dismiss_key_id = id.clone();
            let dismiss = div().id(SharedString::from(format!("notification-dismiss-{id}")))
                .test_probe(|| format!("notification-dismiss-{id}"), || serde_json::json!({}))
                .track_focus(dismiss_focus).cursor_pointer().px(px(8.)).py(px(5.))
                .rounded(theme::r_control()).hover(|style| style.bg(theme::hover()))
                .text_size(px(theme::TEXT_SM)).text_color(theme::accent())
                .on_click(cx.listener(move |workspace, _, _, cx| workspace.dismiss_notification(&dismiss_id, cx)))
                .on_key_down(cx.listener(move |workspace, event: &KeyDownEvent, _, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        workspace.dismiss_notification(&dismiss_key_id, cx);
                        cx.stop_propagation();
                    }
                })).child("Dispensar");
            let mut controls = div().flex().flex_row().items_center().gap(px(8.));
            if let Some(label) = notification.action_label.as_ref().filter(|_| notification.action.is_some()) {
                let action_id = id.clone();
                let action_key_id = id.clone();
                controls = controls.child(div().id(SharedString::from(format!("notification-action-{id}")))
                    .test_probe(|| format!("notification-action-{id}"), || serde_json::json!({}))
                    .track_focus(action_focus).cursor_pointer().px(px(8.)).py(px(5.))
                    .rounded(theme::r_control()).bg(theme::accent()).text_color(theme::bg())
                    .text_size(px(theme::TEXT_SM))
                    .on_click(cx.listener(move |workspace, _, window, cx| {
                        if let Some(action) = workspace.notifications.action(&action_id) { action(workspace, window, cx); }
                    }))
                    .on_key_down(cx.listener(move |workspace, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            if let Some(action) = workspace.notifications.action(&action_key_id) { action(workspace, window, cx); }
                            cx.stop_propagation();
                        }
                    })).child(SharedString::from(label.clone())));
            }
            controls = controls.child(dismiss);
            stack = stack.child(div().id(SharedString::from(format!("notification-{id}")))
                .test_probe(|| format!("notification-{id}"), || serde_json::json!({}))
                .p(px(12.)).rounded(theme::r_control()).border_1()
                .border_color(theme::line_strong()).bg(theme::surface_2())
                .shadow(theme::shadow_overlay()).flex().flex_col().gap(px(5.))
                .child(div().truncate().text_size(px(theme::TEXT)).text_color(theme::text())
                    .child(SharedString::from(notification.title.clone())))
                .child(div().truncate().text_size(px(theme::TEXT_SM)).text_color(theme::dim())
                    .child(SharedString::from(notification.detail.clone())))
                .child(controls));
        }
        Some(stack.into_any_element())
    }

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
        let conversation_id = state.entity_id();
        let events = cx.subscribe(&state, move |workspace, _, event, cx| {
            workspace.handle_state_event(conversation_id, event, cx);
        });
        Self {
            conversations: vec![Conversation {
                state,
                path: initial_path.map(|path| path.canonicalize().unwrap_or(path)),
                _subscription: subscription,
                _events: events,
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
            sessions_notice: None,
            deleting_folder: None,
            confirm_close: false,
            close_target: None,
            pending: None,
            confirm_focus: cx.focus_handle(),
            reopening: BTreeSet::new(),
            query,
            filter: preferences.session_filter,
            nav_hovered: false,
            row_order: Vec::new(),
            project_order: Vec::new(),
            nav_anchors: BTreeMap::new(),
            nav_user_choice: preferences.navigation_open.is_some(),
            hovered: None,
            preferences,
            notifications: NotificationQueue::default(),
            notification_focus: BTreeMap::new(),
            pi_installed: None,
            pi_latest: None,
            pi_checking: false,
            pi_notice: None,
            pi_install_generation: 0,
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
        self.maybe_check_pi_updates(cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_secs(60 * 60))
                .await;
            if this
                .update(cx, |workspace, cx| workspace.maybe_check_pi_updates(cx))
                .is_err()
            {
                break;
            }
        })
        .detach();
        self.maybe_check_dish_update(cx);
        // Explicit test hook: a JSON array at DISH_TEST_NOTIFY is polled while
        // the app runs, so a smoke can enqueue during an active Pi stream.
        if let Some(path) = std::env::var_os("DISH_TEST_NOTIFY").map(PathBuf::from) {
            self.poll_test_notifications(&path, cx);
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                if this.update(cx, |workspace, cx| workspace.poll_test_notifications(&path, cx)).is_err() {
                    break;
                }
            }).detach();
        }
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
                        if *name == "pi_auto_check" {
                            this.preferences.pi_auto_check = *value;
                            crate::preferences::save(this.preferences.clone());
                            if *value { this.maybe_check_pi_updates(cx); }
                        }
                        if *name == "pi_auto_install" {
                            this.preferences.pi_auto_install = *value;
                            crate::preferences::save(this.preferences.clone());
                        }
                        if *name == "completion_sound" {
                            this.preferences.completion_sound = *value;
                            crate::preferences::save(this.preferences.clone());
                        }
                        if matches!(*name, "pi_auto_check" | "pi_auto_install") {
                            this.sync_pi_settings(cx);
                        }
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
                    SettingsEvent::CompletionSoundVolume(volume) => {
                        this.preferences.completion_sound_volume = (*volume).min(100);
                        crate::preferences::save(this.preferences.clone());
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
                    SettingsEvent::ProvidersChanged { catalog } => {
                        // A credencial mudou no Pi. Cada conversa tem o próprio
                        // processo e o próprio snapshot; reler não toca em
                        // rascunho nem em streaming. Com o catálogo conhecido,
                        // a conversa compara a lista real com o que o adaptador
                        // reporta e avisa se o snapshot do processo ficou velho.
                        for conversation in &this.conversations {
                            conversation.state.update(cx, |state, cx| match catalog {
                                Some(catalog) => state.refresh_catalog_after_provider_change(
                                    catalog.clone(),
                                    cx,
                                ),
                                None => state.refresh_catalog(cx),
                            });
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
                                        // Se a pessoa escolheu o executável na tela de
                                        // provedores, relê o catálogo com o novo caminho.
                                        if let Some(settings) = workspace.settings.clone() {
                                            if settings.read(cx).open {
                                                settings.update(cx, |settings, cx| settings.refresh_providers(cx));
                                            }
                                        }
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
                    SettingsEvent::OpenPiSetup => {
                        // A tela de configurações não pode ficar por cima do
                        // assistente; o workspace continua vivo por trás dele.
                        if let Some(settings) = &this.settings {
                            settings.update(cx, |settings, cx| settings.suspend(cx));
                        }
                        cx.emit(WorkspaceEvent::ReopenSetup);
                    }
                    SettingsEvent::RequestPiCheck => this.check_pi_updates(false, cx),
                    SettingsEvent::RequestPiUpdate => this.request_pi_update(false, cx),
                    SettingsEvent::DeleteFolderSessions(cwd) => this.delete_folder_sessions(cwd.clone(), cx),
                    SettingsEvent::UndoFolderDelete => this.undo_delete(cx),
                    SettingsEvent::Navigation(choice) => {
                        this.preferences.session_order = choice.session_order;
                        this.preferences.session_layout = choice.session_layout;
                        this.preferences.project_order = choice.project_order;
                        crate::preferences::save(this.preferences.clone());
                        cx.notify();
                    }
                    SettingsEvent::Closed => this.refresh(cx),
                }
                cx.notify();
            }).detach();
            self.settings = Some(settings);
        }
        let state = self.conversations[self.active].state.read(cx);
        let pi = self.pi_status();
        let view = crate::settings::SettingsView {
            options: [state.sidebar, self.visible, state.show_thinking, state.generated_titles, self.preferences.completion_sound],
            completion_sound_volume: self.preferences.completion_sound_volume,
            appearance: self.preferences.appearance,
            navigation_choice: crate::preferences::NavigationChoice {
                session_order: self.preferences.session_order,
                session_layout: self.preferences.session_layout,
                project_order: self.preferences.project_order,
            },
            theme: self.preferences.theme.clone(),
            models: state
                .models
                .iter()
                .map(|model| (model.provider.clone(), model.id.clone()))
                .collect(),
            hidden: self.preferences.hidden_models.clone(),
            pi,
            sessions: Default::default(),
        };
        self.conversations[self.active].state.update(cx, |state, _| {
            state.visible = false;
            state.model_menu = false;
            state.model_search_focus_pending = false;
            state.effort_menu = false;
        });
        self.settings.as_ref().unwrap().update(cx, |settings, cx| settings.show(section, view, window, cx));
        self.sync_settings_sessions(cx);
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
                workspace.sync_settings_sessions(cx);
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
        // Em lista única não há pasta para recolher.
        if self.preferences.session_layout == SessionLayout::Flat { return; }
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

    fn handle_state_event(&mut self, id: EntityId, event: &AppStateEvent, cx: &mut Context<Self>) {
        if matches!(event, AppStateEvent::ReopenConversation) {
            self.reopen_conversation(id, cx);
        }
    }

    /// Reinicia o processo do Pi desta conversa com a mesma sessão.
    ///
    /// O snapshot de modelos é calculado no startup do Pi; quando uma
    /// credencial muda depois disso, `get_available_models` continua velho.
    /// Até o Pi oferecer um comando RPC de reavaliação do catálogo, reabrir é o
    /// único conserto real. O editor (e portanto o rascunho) é reaproveitado:
    /// ele é do Dish, não do processo.
    fn reopen_conversation(&mut self, id: EntityId, cx: &mut Context<Self>) {
        if !self.reopening.insert(id) {
            return;
        }
        let Some(index) = self.conversations.iter()
            .position(|conversation| conversation.state.entity_id() == id) else {
            self.reopening.remove(&id);
            return;
        };
        let state = self.conversations[index].state.clone();
        if let Some(reason) = state.read(cx).reopen_block_reason() {
            self.reopening.remove(&id);
            state.update(cx, |state, cx| {
                state.reopening = false;
                state.push_toast(reason.to_string(), crate::state::Tone::Warning, cx);
                cx.notify();
            });
            return;
        }
        let path = self.conversations[index].path.clone()
            .or_else(|| state.read(cx).session.file.as_ref().map(PathBuf::from));
        let Some(path) = path else {
            self.reopening.remove(&id);
            state.update(cx, |state, cx| {
                state.reopening = false;
                state.push_toast(
                    "Esta conversa ainda não tem uma sessão salva para reabrir.".into(),
                    crate::state::Tone::Warning,
                    cx,
                );
                cx.notify();
            });
            return;
        };
        let (client, cwd, editors, title, title_override, hidden_models, sidebar,
            inspector_initialized, show_thinking, generated_titles) = {
            let state = state.read(cx);
            (state.client.clone(), state.cwd.clone(), crate::state::AppEditors {
                composer: state.editor.clone(),
                modal: state.modal_editor.clone(),
                model_search: state.model_search.clone(),
                help_search: state.help_search.clone(),
            }, Some(state.display_title()), state.title_override.clone(),
                state.hidden_models.clone(), state.sidebar, state.inspector_initialized,
                state.show_thinking, state.generated_titles)
        };
        let mut flags = self.flags.clone();
        flags.extend(["--session".into(), path.to_string_lossy().into_owned()]);
        let config = PiConfig::new(cwd.clone()).with_args(flags);
        client.request_shutdown();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                client.shutdown_and_wait()
                    .map_err(|error| format!("Não foi possível encerrar o Pi antigo: {error}"))?;
                PiClient::spawn(&config)
                    .map_err(|error| format!("Não foi possível reabrir a sessão: {error:#}"))
            }).await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.reopening.remove(&id);
                let Some(index) = workspace.conversations.iter()
                    .position(|conversation| conversation.state.entity_id() == id) else {
                    return;
                };
                match result {
                    Err(error) => {
                        workspace.conversations[index].state.update(cx, |state, cx| {
                            state.reopening = false;
                            state.push_toast(error, crate::state::Tone::Error, cx);
                            cx.notify();
                        });
                    }
                    Ok(client) => {
                        let state = cx.new(|cx| AppState::new(cwd, client, editors, None, cx));
                        state.update(cx, |state, cx| {
                            state.restoring_session = true;
                            state.initial_title = title;
                            state.title_override = title_override;
                            state.hidden_models = hidden_models;
                            state.sidebar = sidebar;
                            state.inspector_initialized = inspector_initialized;
                            state.show_thinking = show_thinking;
                            state.generated_titles = generated_titles;
                            state.reopening = false;
                            state.composer_focus_pending = true;
                            state.start(cx);
                        });
                        let subscription = cx.observe(&state, |_, _, cx| cx.notify());
                        let conversation_id = state.entity_id();
                        let events = cx.subscribe(&state, move |workspace, _, event, cx| {
                            workspace.handle_state_event(conversation_id, event, cx);
                        });
                        let conversation = &mut workspace.conversations[index];
                        conversation.path = conversation.path.clone().or(Some(path));
                        conversation.state = state;
                        conversation._subscription = subscription;
                        conversation._events = events;
                        conversation.seen_completed_runs = 0;
                        conversation.unread_completion = false;
                    }
                }
                cx.notify();
            });
        }).detach();
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
                            items: vec![DeletedItem { info, trashed: trashed.trashed, pinned }],
                            id,
                        });
                        workspace.sync_settings_sessions(cx);
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

    /// Devolve o último lote excluído para o lugar de onde saiu.  A
    /// restauração é no-clobber: se o caminho original foi recriado, nada é
    /// sobrescrito e a sessão continua recuperável na lixeira.  O que não
    /// voltou continua desfazível.
    fn undo_delete(&mut self, cx: &mut Context<Self>) {
        let Some(undo) = self.undo_delete.as_ref() else {
            return;
        };
        if self.restoring == Some(undo.id) {
            return;
        }
        let id = undo.id;
        let items = undo.items.clone();
        let pairs: Vec<(PathBuf, PathBuf)> = items
            .iter()
            .map(|item| (item.trashed.clone(), item.info.path.clone()))
            .collect();
        self.restoring = Some(id);
        self.sync_settings_sessions(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move { sessions::restore_batch(&pairs) })
                .await;
            let _ = this.update(cx, |workspace, cx| {
                if workspace.restoring != Some(id) {
                    return;
                }
                workspace.restoring = None;
                let total = items.len();
                let mut pinned_changed = false;
                let mut failed: Vec<(DeletedItem, std::io::Error)> = Vec::new();
                for (item, (_, result)) in items.into_iter().zip(results) {
                    match result {
                        Ok(()) => {
                            if item.pinned {
                                workspace.preferences.pinned_sessions.insert(item.info.path.clone());
                                pinned_changed = true;
                            }
                            add_to_catalog(&mut workspace.catalog, item.info);
                        }
                        Err(error) => failed.push((item, error)),
                    }
                }
                if pinned_changed {
                    crate::preferences::save(workspace.preferences.clone());
                }
                workspace.sessions_notice = Some(if failed.is_empty() {
                    format!("{total} sessões restauradas; 0 falharam.")
                } else {
                    format!("{} sessões restauradas; {} falharam. {}", total - failed.len(), failed.len(), restore_failure_message(total, &failed))
                });
                let current = workspace.undo_delete.as_ref().is_some_and(|undo| undo.id == id);
                if failed.is_empty() {
                    if current {
                        workspace.undo_delete = None;
                    }
                } else {
                    workspace.error = Some(restore_failure_message(total, &failed));
                    if current {
                        workspace.undo_delete = Some(DeletedSession {
                            items: failed.into_iter().map(|(item, _)| item).collect(),
                            id,
                        });
                        // O prazo original pode ter expirado enquanto a
                        // restauração estava em andamento.
                        Self::schedule_undo_expiry(id, cx);
                    }
                }
                workspace.catalog_generation += 1;
                workspace.invalidate_content_search(cx);
                workspace.refresh(cx);
                workspace.sync_settings_sessions(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Envia à tela de Configurações o estado da seção Sessões.
    fn live_sessions(&self, cx: &Context<Self>) -> Vec<LiveSession> {
        let mut live: Vec<_> = self.conversations.iter().map(|conversation| {
            let state = conversation.state.read(cx);
            (conversation.path.clone().or_else(|| state.session.file.as_ref().map(PathBuf::from)),
                state.cwd.clone(), state.is_busy())
        }).collect();
        for opening in self.opening.iter().chain(self.pending.iter().map(|(opening, _)| opening)) {
            live.push((opening.path.clone(), opening.cwd.clone(), true));
        }
        for closing in self.closing.values() {
            if let Some(info) = self.catalog.iter().find(|info| closing.path.as_ref() == Some(&info.path)) {
                live.push((closing.path.clone(), info.cwd.clone(), true));
            } else {
                // Unknown closing files fence every folder until shutdown is confirmed.
                for folder in self.catalog.iter().map(|info| &info.cwd) {
                    live.push((closing.path.clone(), folder.clone(), true));
                }
            }
        }
        live
    }

    fn sync_settings_sessions(&self, cx: &mut Context<Self>) {
        let Some(settings) = &self.settings else { return; };
        let live = self.live_sessions(cx);
        let view = crate::settings::SessionsView {
            folders: session_folders(&self.catalog, &live),
            deleting: self.deleting_folder.clone().or_else(|| self.catalog.iter()
                .find(|info| self.deleting.contains(&info.path)).map(|info| info.cwd.clone())),
            undo: self.undo_delete.as_ref().map(|undo| (undo.items.len(), self.restoring == Some(undo.id))),
            notice: self.sessions_notice.clone(),
        };
        settings.update(cx, |settings, cx| settings.set_sessions(view, cx));
    }

    fn delete_folder_sessions(&mut self, cwd: PathBuf, cx: &mut Context<Self>) {
        let infos: Vec<_> = self.catalog.iter().filter(|info| info.cwd == cwd).cloned().collect();
        let paths: Vec<_> = infos.iter().map(|info| info.path.clone()).collect();
        let live = self.live_sessions(cx);
        let closing: Vec<_> = self.closing.values().map(|entry| entry.path.clone()).collect();
        let blocker = folder_delete_blocker(&cwd, &paths, &live, &closing, &self.deleting)
            .or_else(|| self.deleting_folder.as_ref().map(|_| "Exclusão em andamento."));
        if let Some(reason) = blocker {
            self.sessions_notice = Some(reason.into());
            self.error = Some(reason.into());
            self.sync_settings_sessions(cx);
            cx.notify();
            return;
        }
        let in_use = live.into_iter().filter_map(|(path, _, _)| path).collect();
        let roots = self.roots.clone();
        self.deleting_folder = Some(cwd);
        self.deleting.extend(paths.iter().cloned());
        self.sessions_notice = None;
        self.sync_settings_sessions(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                let trash = sessions::trash_root().ok_or_else(|| std::io::Error::new(
                    std::io::ErrorKind::NotFound, "Pasta de dados do Dish indisponível."))?;
                sessions::trash_batch_into(&trash, &roots, &infos, &in_use)
            }).await;
            let _ = this.update(cx, |workspace, cx| {
                workspace.deleting_folder = None;
                for path in paths { workspace.deleting.remove(&path); }
                match result {
                    Ok(batch) => {
                        let count = batch.moved.len();
                        if count > 0 {
                            let items = batch.moved.into_iter().map(|(info, trashed)| {
                                drop_from_catalog(&mut workspace.catalog, &info.path);
                                let pinned = workspace.preferences.pinned_sessions.remove(&info.path);
                                DeletedItem { info, trashed: trashed.trashed, pinned }
                            }).collect();
                            workspace.catalog_generation += 1;
                            workspace.undo_seq += 1;
                            let id = workspace.undo_seq;
                            workspace.undo_delete = Some(DeletedSession { items, id });
                            crate::preferences::save(workspace.preferences.clone());
                            workspace.invalidate_content_search(cx);
                            Self::schedule_undo_expiry(id, cx);
                        }
                        workspace.sessions_notice = Some(match batch.failed {
                            Some((path, error)) => format!("{count} sessões movidas para a lixeira. Falha em {}: {error}", path.display()),
                            None => format!("{count} sessões movidas para a lixeira."),
                        });
                    }
                    Err(error) => workspace.sessions_notice = Some(format!("Nenhuma sessão movida: {error}")),
                }
                workspace.sync_settings_sessions(cx);
                workspace.refresh(cx);
                cx.notify();
            });
        }).detach();
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
                    workspace.sync_settings_sessions(cx);
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
        let label = match undo.items.as_slice() {
            [item] => format!("“{}” excluída", item.info.title),
            items => format!("{} sessões excluídas", items.len()),
        };
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
                    .child(SharedString::from(label)),
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
        self.sync_settings_sessions(cx);
        let active_viewed = window.is_window_active() && !self.blocking_confirmation() && !self.settings.as_ref().is_some_and(|settings| settings.read(cx).open);
        for (index, conversation) in self.conversations.iter_mut().enumerate() {
            let completed = conversation.state.read(cx).completed_content_runs;
            let newly_completed = completed != conversation.seen_completed_runs;
            if newly_completed {
                conversation.unread_completion = index != self.active || !active_viewed;
                conversation.seen_completed_runs = completed;
                if crate::sound::should_play(self.preferences.completion_sound, newly_completed, conversation.unread_completion) {
                    crate::sound::play(self.preferences.completion_sound_volume);
                }
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
            let conversation_id = state.entity_id();
            let events = cx.subscribe(&state, move |workspace, _, event, cx| {
                workspace.handle_state_event(conversation_id, event, cx);
            });
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
                _events: events,
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
        if let Some(cards) = self.notification_cards(cx) { root = root.child(cards); }
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
            "executing": active.is_executing(),
            "nav_query": self.query.read(cx).text(cx),
            "confirming_close": self.confirm_close,
            "session_menu": self.session_menu.is_some(),
            "undo_sessions": self.undo_delete.as_ref().map(|undo| undo.items.len()),
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
    use super::{compare_projects, sort_project_groups, ProjectRank, SessionOrder, ProjectOrder};
    use super::{folder_delete_blocker, restore_failure_message, session_folders, DeletedItem};
    use std::collections::BTreeSet;
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
    fn session_folders_group_catalog_by_project() {
        let mut other = info("/s/b.jsonl", 30);
        other.cwd = PathBuf::from("/other");
        let catalog = vec![info("/s/a.jsonl", 10), other, info("/s/c.jsonl", 20)];
        let folders = session_folders(&catalog, &[]);
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].cwd, PathBuf::from("/other"));
        assert_eq!(folders[1].cwd, PathBuf::from("/project"));
        assert_eq!(folders[1].count, 2);
        assert_eq!(folders[1].bytes, 0);
        assert_eq!(folders[1].last, std::time::UNIX_EPOCH + std::time::Duration::from_secs(20));
        assert!(!folders[1].open && !folders[1].running);
    }

    #[test]
    fn session_folders_detect_open_and_running_conversations() {
        let catalog = vec![info("/s/a.jsonl", 10)];
        let by_cwd = session_folders(&catalog, &[(None, PathBuf::from("/project"), false)]);
        assert!(by_cwd[0].open && !by_cwd[0].running);
        // A conversa pode estar em outra pasta; o arquivo ainda é desta.
        let by_path = session_folders(
            &catalog,
            &[(Some(PathBuf::from("/s/a.jsonl")), PathBuf::from("/elsewhere"), true)],
        );
        assert!(by_path[0].open && by_path[0].running);
        let unrelated = session_folders(&catalog, &[(None, PathBuf::from("/elsewhere"), true)]);
        assert!(!unrelated[0].open && !unrelated[0].running);
    }

    #[test]
    fn folder_delete_blocker_refuses_open_closing_busy_or_empty_folders() {
        let folder = PathBuf::from("/project");
        let paths = vec![PathBuf::from("/s/a.jsonl")];
        let none = BTreeSet::new();
        assert_eq!(folder_delete_blocker(&folder, &paths, &[], &[], &none), None);
        assert!(folder_delete_blocker(&folder, &paths, &[(None, folder.clone(), false)], &[], &none)
            .is_some_and(|reason| reason.starts_with("Feche")));
        assert!(folder_delete_blocker(&folder, &paths, &[], &[Some(paths[0].clone())], &none)
            .is_some_and(|reason| reason.starts_with("Pi ainda")));
        assert!(folder_delete_blocker(&folder, &paths, &[], &[None], &none)
            .is_some_and(|reason| reason.starts_with("Pi ainda")));
        let busy: BTreeSet<PathBuf> = paths.iter().cloned().collect();
        assert_eq!(folder_delete_blocker(&folder, &paths, &[], &[], &busy), Some("Exclusão em andamento."));
        assert_eq!(folder_delete_blocker(&folder, &[], &[], &[], &none), Some("Nenhuma sessão nesta pasta."));
        // Outra pasta encerrando não bloqueia esta.
        assert_eq!(
            folder_delete_blocker(&folder, &paths, &[], &[Some(PathBuf::from("/s/z.jsonl"))], &none),
            None,
        );
    }

    #[test]
    fn restore_failure_message_names_files_left_in_trash() {
        let item = |path: &str| DeletedItem { info: info(path, 1), trashed: PathBuf::from("/trash/x"), pinned: false };
        let exists = || std::io::Error::from(std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            restore_failure_message(3, &[(item("/s/a.jsonl"), exists())]),
            "Restauradas 2 de 3 sessões. Não voltaram: a.jsonl (já existe um arquivo no caminho original)",
        );
        let denied = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "acesso negado");
        assert!(restore_failure_message(2, &[(item("/s/b.jsonl"), denied)])
            .contains("b.jsonl (acesso negado)"));
        assert!(restore_failure_message(1, &[(item("/s/a.jsonl"), exists())])
            .starts_with("Não foi possível restaurar: já existe"));
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
            ("first".into(), None, Some(0), std::time::UNIX_EPOCH, std::time::UNIX_EPOCH),
            (saved.title.clone(), Some(saved.clone()), None, saved.modified, saved.modified),
            ("last".into(), None, Some(1), std::time::UNIX_EPOCH, std::time::UNIX_EPOCH),
        ];
        let mut previous = rows.iter().map(row_key).collect::<Vec<_>>();
        let (old, new) = super::promote_session_row(&mut previous, &saved.path, 2);
        assert_eq!(old, "saved-/middle.jsonl");
        assert_eq!(new, "live-2");
        rows[1].1 = None;
        rows[1].2 = Some(2);
        // Freeze applies while the mouse remains over the row just clicked.
        sort_session_rows(&mut rows, &previous, true, SessionOrder::Activity, |_| false);
        assert_eq!(rows.iter().map(row_key).collect::<Vec<_>>(), ["live-0", "live-2", "live-1"]);
        // Once interaction ends, normal activity ordering still applies.
        sort_session_rows(&mut rows, &previous, false, SessionOrder::Activity, |_| false);
        assert_eq!(row_key(&rows[0]), "live-2");
    }

    #[test]
    fn row_order_has_stable_ties_and_freezes_during_interaction() {
        use std::time::{Duration, UNIX_EPOCH};
        let old = UNIX_EPOCH + Duration::from_secs(1);
        let recent = UNIX_EPOCH + Duration::from_secs(2);
        let mut rows = vec![
            ("b".into(), None, Some(1), old, old),
            ("a".into(), None, Some(0), old, old),
        ];
        sort_session_rows(&mut rows, &[], false, SessionOrder::Activity, |_| false);
        assert_eq!(rows.iter().map(row_key).collect::<Vec<_>>(), ["live-0", "live-1"]);
        let previous = rows.iter().map(row_key).collect::<Vec<_>>();
        rows[1].3 = recent;
        rows.push(("c".into(), None, Some(2), recent, recent));
        sort_session_rows(&mut rows, &previous, true, SessionOrder::Activity, |_| false);
        assert_eq!(rows.iter().map(row_key).collect::<Vec<_>>(), ["live-0", "live-1", "live-2"]);
        sort_session_rows(&mut rows, &previous, false, SessionOrder::Activity, |_| false);
        assert_eq!(rows.iter().map(row_key).collect::<Vec<_>>(), ["live-1", "live-2", "live-0"]);
    }

    fn project_rank(running: bool, needs_input: bool, seconds: u64) -> ProjectRank {
        ProjectRank {
            running,
            needs_input,
            latest: std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds),
        }
    }

    fn project(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    fn group_names(groups: &[(PathBuf, ProjectRank, ())]) -> Vec<String> {
        groups.iter().map(|(path, _, _)| super::folder_of(path)).collect()
    }

    #[test]
    fn project_groups_put_running_before_input_before_recent_activity() {
        let mut groups = vec![
            (project("/idle"), project_rank(false, false, 9), ()),
            (project("/input"), project_rank(false, true, 1), ()),
            (project("/running"), project_rank(true, false, 0), ()),
        ];
        sort_project_groups(&mut groups, &[], false, ProjectOrder::Activity);
        assert_eq!(group_names(&groups), ["running", "input", "idle"]);
    }

    #[test]
    fn project_group_ties_fall_back_to_the_path() {
        let rank = project_rank(false, false, 3);
        let mut groups = vec![(project("/b"), rank, ()), (project("/a"), rank, ())];
        sort_project_groups(&mut groups, &[], false, ProjectOrder::Activity);
        assert_eq!(group_names(&groups), ["a", "b"]);
    }

    #[test]
    fn project_order_by_name_uses_the_folder_not_the_path() {
        let rank = project_rank(false, false, 1);
        let mut groups = vec![(project("/z/alpha"), rank, ()), (project("/a/beta"), rank, ())];
        sort_project_groups(&mut groups, &[], false, ProjectOrder::Name);
        assert_eq!(group_names(&groups), ["alpha", "beta"]);
        let alpha = project("/z/alpha");
        let beta = project("/a/beta");
        assert_eq!(
            compare_projects((alpha.as_path(), &rank), (beta.as_path(), &rank), ProjectOrder::Name),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn interacting_freezes_the_previous_project_order_even_when_the_rank_changes() {
        let mut groups = vec![
            (project("/a"), project_rank(false, false, 1), ()),
            (project("/b"), project_rank(false, false, 1), ()),
        ];
        let previous = vec![groups[1].0.clone(), groups[0].0.clone()];
        // `a` começa a executar, mas a lista não pode se mexer sob o ponteiro.
        groups[0].1 = project_rank(true, false, 0);
        sort_project_groups(&mut groups, &previous, true, ProjectOrder::Activity);
        assert_eq!(group_names(&groups), ["b", "a"]);
        sort_project_groups(&mut groups, &previous, false, ProjectOrder::Activity);
        assert_eq!(group_names(&groups), ["a", "b"]);
    }

    fn row(title: &str, live: usize, activity: u64, completed: u64) -> super::SessionRow {
        (
            title.into(),
            None,
            Some(live),
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(activity),
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(completed),
        )
    }

    fn row_keys(rows: &[super::SessionRow]) -> Vec<String> {
        rows.iter().map(row_key).collect()
    }

    #[test]
    fn pinned_rows_stay_first_in_every_order() {
        for order in [SessionOrder::Activity, SessionOrder::Completed, SessionOrder::Name] {
            let mut rows = vec![row("b", 1, 10, 1), row("a", 0, 20, 2), row("c", 2, 30, 3)];
            sort_session_rows(&mut rows, &[], false, order, |row| row.2 == Some(2));
            assert_eq!(row_key(&rows[0]), "live-2", "fixada primeiro em {order:?}");
        }
    }

    #[test]
    fn completed_order_uses_the_completion_time() {
        let mut rows = vec![row("a", 0, 1, 30), row("b", 1, 2, 20), row("c", 2, 3, 10)];
        sort_session_rows(&mut rows, &[], false, SessionOrder::Completed, |_| false);
        assert_eq!(row_keys(&rows), ["live-0", "live-1", "live-2"]);
        sort_session_rows(&mut rows, &[], false, SessionOrder::Activity, |_| false);
        assert_eq!(row_keys(&rows), ["live-2", "live-1", "live-0"]);
    }

    #[test]
    fn name_order_compares_titles_case_insensitively() {
        let mut rows = vec![row("Gama", 0, 3, 3), row("alfa", 1, 2, 2), row("beta", 2, 1, 1)];
        sort_session_rows(&mut rows, &[], false, SessionOrder::Name, |_| false);
        assert_eq!(row_keys(&rows), ["live-1", "live-2", "live-0"]);
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
