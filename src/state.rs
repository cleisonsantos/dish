//! Application state: the conversation model, the Pi session, and every
//! interaction the UI can trigger.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use gpui::*;
use serde_json::{Value, json};
use base64::Engine as _;

use crate::editor::Editor;
use crate::rpc::{PiClient, RpcResponse};
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    User,
    Assistant,
    Bash,
    System,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolStatus {
    Pending,
    Running,
    Ok,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone)]
pub struct ToolCard {
    pub id: String,
    pub name: String,
    pub args: Value,
    pub args_text: String,
    pub output: String,
    pub status: ToolStatus,
    pub expanded: bool,
    pub details: Option<Value>,
    pub started_at: Option<i64>,
    pub elapsed_ms: Option<u64>,
}

impl ToolCard {
    fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            args: Value::Null,
            args_text: String::new(),
            output: String::new(),
            status: ToolStatus::Pending,
            expanded: false,
            details: None,
            started_at: None,
            elapsed_ms: None,
        }
    }


    /// The most useful single-line summary of the call's arguments.
    pub fn summary(&self) -> String {
        let args = &self.args;
        let candidate = |keys: &[&str]| -> Option<String> {
            for key in keys {
                if let Some(value) = args.get(*key).and_then(Value::as_str) {
                    return Some(value.to_string());
                }
            }
            None
        };
        let text = match self.name.as_str() {
            "bash" | "shell" | "exec" => candidate(&["command", "cmd"]),
            "read" | "write" | "edit" | "view" | "create" => candidate(&["file_path", "path", "file"]),
            "grep" | "search" => candidate(&["pattern", "query"]),
            "glob" | "find" | "ls" | "list" => candidate(&["pattern", "path", "dir"]),
            _ => None,
        };
        if let Some(text) = text {
            return text;
        }
        if !self.args_text.is_empty() {
            return truncate(&self.args_text.replace('\n', " "), 160);
        }
        match &self.args {
            Value::Null => String::new(),
            other => truncate(&other.to_string(), 160),
        }
    }

}

#[derive(Clone)]
pub struct BashCard {
    pub command: String,
    pub output: String,
    pub exit_code: Option<i64>,
    pub cancelled: bool,
    pub truncated: bool,
    pub status: ToolStatus,
}

pub enum Block {
    Text(String),
    Thinking {
        text: String,
        done: bool,
        expanded: bool,
    },
    Tool(Box<ToolCard>),
    Bash(Box<BashCard>),
    Note {
        label: String,
        text: String,
        tone: Tone,
    },
}

pub struct Message {
    pub role: Role,
    pub blocks: Vec<Block>,
    pub streaming: bool,
    /// Quando o Pi criou a mensagem.
    pub timestamp: i64,
    /// Quando o Dish viu esta mensagem terminar, em ms locais. `0` quando a
    /// mensagem veio de uma sessão restaurada e o dado não existe.
    pub finished_at: i64,
    pub usage: Option<Value>,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
    pub local: bool,
    pub model: Option<String>,
}

impl Message {
    fn new(role: Role) -> Self {
        Self {
            role,
            blocks: Vec::new(),
            streaming: false,
            timestamp: 0,
            finished_at: 0,
            usage: None,
            stop_reason: None,
            error: None,
            local: false,
            model: None,
        }
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            if let Block::Text(text) = block {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
        }
        out
    }

}

#[derive(Clone, Default)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    #[allow(dead_code)]
    pub reasoning: bool,
    pub context_window: u64,
}

impl ModelInfo {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: value.get("id")?.as_str()?.to_string(),
            provider: value
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            reasoning: value.get("reasoning").and_then(Value::as_bool).unwrap_or(false),
            context_window: value
                .get("contextWindow")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        })
    }

}

#[derive(Clone)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    /// Where Pi found the command: `prompt`, `skill`, or `extension`.
    #[allow(dead_code)]
    pub source: String,
}

#[derive(Clone)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub tone: Tone,
}

#[derive(Clone)]
pub struct Banner {
    pub text: String,
    pub tone: Tone,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    Select,
    Confirm,
    Input,
    Editor,
}

pub struct Modal {
    pub id: String,
    pub kind: ModalKind,
    pub title: String,
    pub message: Option<String>,
    pub options: Vec<String>,
    /// Shown as the editor's placeholder text.
    #[allow(dead_code)]
    pub placeholder: Option<String>,
    /// True for `Input` and `Editor` dialogs, which collect free text.
    pub wants_input: bool,
}

/// Session status as reported by `get_state`.
#[derive(Default, Clone)]
pub struct SessionState {
    pub id: Option<String>,
    pub file: Option<String>,
    pub name: Option<String>,
    pub message_count: usize,
    pub is_compacting: bool,
    pub auto_compaction: bool,
    pub steering_mode: String,
    pub follow_up_mode: String,
}

pub struct AppState {
    pub visible: bool,
    pub disconnected: bool,
    pub new_session_requested: bool,
    pub loading: bool,
    /// Quando esta conversa foi aberta nesta janela.
    pub created_at: std::time::SystemTime,
    /// Última execução: o prompt mais recente enviado. `None` até o primeiro.
    pub last_run: Option<std::time::SystemTime>,
    pub client: PiClient,
    pub cwd: PathBuf,
    pub editor: Entity<Editor>,
    /// A second editor used by extension dialogs.
    pub modal_editor: Entity<Editor>,
    pub model_search: Entity<Editor>,
    pub model_search_focus_pending: bool,
    /// Linha destacada no seletor de modelos e a busca da qual ela veio.
    model_index: usize,
    model_query: String,
    pub model_scroll: ScrollHandle,
    /// Busca dentro da janela de atalhos.
    pub help_search: Entity<Editor>,
    /// Foco pendente para a busca de atalhos, consumido no próximo frame.
    pub help_focus_pending: bool,
    pub composer_scroll: ScrollHandle,
    pub pasted_images: Vec<(String, Value)>,
    pub paste_loading: bool,
    pending_image_prompts: HashMap<String, (String, Vec<(String, Value)>)>,

    pub messages: Vec<Message>,
    pub list_state: ListState,
    list_count: usize,
    /// toolCallId -> (message index, block index)
    tool_slots: HashMap<String, (usize, usize)>,
    /// content index -> block index, for the message currently streaming
    content_slots: HashMap<usize, usize>,
    streaming_message: Option<usize>,
    /// Command id -> local bash message index
    local_bash: HashMap<String, usize>,
    /// Index of an optimistically-rendered user message awaiting its echo.
    pending_local_user: Option<usize>,

    pub streaming: bool,
    agent_active: bool,
    /// Monotonic counter of completed agent runs (not individual tool turns).
    pub completed_runs: u64,
    /// Completed runs with live assistant text, not merely an agent_end event.
    pub completed_content_runs: u64,
    response_tracker: crate::session_activity::ResponseTracker,
    pub last_activity: Option<std::time::SystemTime>,
    pub activity_error: bool,
    pub activity_interrupted: bool,
    pub activity: Option<String>,

    pub session: SessionState,
    pub model: Option<ModelInfo>,
    pub models: Vec<ModelInfo>,
    pub thinking_level: Option<String>,
    pub thinking_levels: Vec<String>,
    pub stats: Option<Value>,
    pub commands: Vec<CommandInfo>,

    pub steering_queue: Vec<String>,
    pub follow_up_queue: Vec<String>,

    pub sidebar: bool,
    pub model_menu: bool,
    /// Se a abertura automática do inspetor já foi decidida nesta janela.
    pub inspector_initialized: bool,
    /// Menu de esforço, ancorado no compositor.
    pub effort_menu: bool,
    /// Fila de mensagens pendentes aberta.
    pub queue_open: bool,
    /// Highlighted row in the slash menu, and the query it was chosen for.
    slash_index: usize,
    slash_query: String,
    slash_dismissed: bool,
    pub thinking_menu: bool,
    pub help_open: bool,

    pub toasts: Vec<Toast>,
    pub banner: Option<Banner>,
    pub modal: Option<Modal>,
    /// Linha destacada nos diálogos de seleção de extensões.
    modal_index: usize,
    pub composer_focus_pending: bool,
    pub ext_status: Vec<(String, String)>,
    pub ext_widget: Vec<String>,
    pub title_override: Option<String>,
    pub show_thinking: bool,

    /// Escolha explícita do usuário sobre abrir o grupo de atividade de um turno.
    activity_override: HashMap<usize, bool>,
    toast_seq: u64,
    /// A prompt supplied on the command line, sent once the session is up.
    initial_prompt: Option<String>,
}

pub struct AppEditors {
    pub composer: Entity<Editor>,
    pub modal: Entity<Editor>,
    pub model_search: Entity<Editor>,
    pub help_search: Entity<Editor>,
}

impl AppState {
    pub fn new(
        cwd: PathBuf,
        client: PiClient,
        editors: AppEditors,
        initial_prompt: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let AppEditors { composer: editor, modal: modal_editor, model_search, help_search } = editors;
        let list_state = ListState::new(0, ListAlignment::Top, px(600.));
        list_state.set_follow_mode(FollowMode::Tail);
        // O botão "ir para o fim" precisa saber quando o usuário rolou para cima.
        let weak = cx.entity().downgrade();
        list_state.set_scroll_handler(move |_event, _window, cx| {
            weak.update(cx, |_state, cx| cx.notify()).ok();
        });
        cx.observe(&model_search, |_, _, cx| cx.notify()).detach();
        cx.observe(&help_search, |_, _, cx| cx.notify()).detach();

        Self {
            visible: false,
            disconnected: false,
            new_session_requested: false,
            loading: true,
            created_at: std::time::SystemTime::now(),
            last_run: None,
            client,
            cwd,
            editor,
            modal_editor,
            model_search,
            model_search_focus_pending: false,
            model_index: 0,
            model_query: String::new(),
            model_scroll: ScrollHandle::new(),
            help_search,
            help_focus_pending: false,
            composer_scroll: ScrollHandle::new(),
            pasted_images: Vec::new(),
            paste_loading: false,
            pending_image_prompts: HashMap::new(),
            messages: Vec::new(),
            list_state,
            list_count: 0,
            tool_slots: HashMap::new(),
            content_slots: HashMap::new(),
            streaming_message: None,
            local_bash: HashMap::new(),
            pending_local_user: None,
            streaming: false,
            agent_active: false,
            completed_runs: 0,
            completed_content_runs: 0,
            response_tracker: crate::session_activity::ResponseTracker::default(),
            last_activity: None,
            activity_error: false,
            activity_interrupted: false,
            activity: None,
            session: SessionState {
                auto_compaction: true,
                ..Default::default()
            },
            model: None,
            models: Vec::new(),
            thinking_level: None,
            thinking_levels: Vec::new(),
            stats: None,
            commands: Vec::new(),
            steering_queue: Vec::new(),
            follow_up_queue: Vec::new(),
            sidebar: true,
            model_menu: false,
            inspector_initialized: false,
            effort_menu: false,
            queue_open: false,
            slash_index: 0,
            slash_query: String::new(),
            slash_dismissed: false,
            thinking_menu: false,
            help_open: false,
            toasts: Vec::new(),
            banner: None,
            modal: None,
            modal_index: 0,
            composer_focus_pending: false,
            ext_status: Vec::new(),
            ext_widget: Vec::new(),
            title_override: None,
            show_thinking: true,
            activity_override: HashMap::new(),
            toast_seq: 1,
            initial_prompt,
        }
    }

    /// Subscribe to the Pi process and pull the initial session state.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        let records = self.client.records.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(record) = records.recv().await {
                let alive = this.update(cx, |state, cx| state.handle_record(record, cx));
                if alive.is_err() {
                    break;
                }
            }
            let _ = this.update(cx, |state, cx| {
                state.activity = None;
                state.streaming = false;
                state.agent_active = false;
                state.disconnected = true;
                state.activity_error = true;
                state.last_activity = Some(std::time::SystemTime::now());
                state.loading = false;
                state.push_toast("Pi exited. Restart Dish to reconnect.".to_string(), Tone::Error, cx);
            });
        })
        .detach();

        let logs = self.client.logs.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(line) = logs.recv().await {
                if line.trim().is_empty() {
                    continue;
                }
                eprintln!("pi: {line}");
                let alive = this.update(cx, |state, cx| {
                    if line.contains("Error") || line.contains("error") {
                        state.push_toast(line.clone(), Tone::Warning, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();

        // Prime the UI with everything it needs to render a full frame.
        self.client.call("get_state", Value::Null);
        self.client.call("get_available_models", Value::Null);
        self.client.call("get_available_thinking_levels", Value::Null);
        self.client.call("get_session_stats", Value::Null);
        self.client.call("get_commands", Value::Null);
        self.client.call("get_messages", Value::Null);

        if let Some(prompt) = self.initial_prompt.take() {
            self.submit_text(prompt, cx);
        }
    }


    // ----------------------------------------------------------------- records

    fn handle_record(&mut self, record: Value, cx: &mut Context<Self>) {
        let kind = record
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let dirty = self.apply_record(&kind, &record, cx);
        self.sync_list();
        if let Some(index) = dirty {
            if index < self.list_count {
                self.list_state.remeasure_items(index..index + 1);
            }
        }
        cx.notify();
    }

    fn apply_record(
        &mut self,
        kind: &str,
        record: &Value,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.response_tracker.observe(kind, record);
        self.completed_content_runs = self.response_tracker.completed;
        // Only boundaries affect navigator ordering; streaming deltas do not.
        if matches!(kind, "agent_start" | "agent_end" | "tool_execution_start" | "tool_execution_end" | "extension_ui_request" | "queue_update" | "compaction_start" | "compaction_end") {
            self.last_activity = Some(std::time::SystemTime::now());
        }
        if kind == "extension_error"
            || (kind == "compaction_end" && record["aborted"] != true && record.get("errorMessage").and_then(Value::as_str).is_some())
            || (kind == "auto_retry_end" && record.get("success").and_then(Value::as_bool) == Some(false)) {
            self.activity_error = true;
        }
        if kind == "compaction_end" && record.get("aborted").and_then(Value::as_bool) == Some(true) {
            self.activity_interrupted = true;
        }
        match kind {
            "response" => {
                if let Some(response) = RpcResponse::parse(record) {
                    return self.apply_response(response, cx);
                }
                None
            }
            "agent_start" => {
                self.agent_active = true;
                self.activity_error = false;
                self.activity_interrupted = false;
                self.streaming = true;
                self.activity = Some("Thinking…".into());
                None
            }
            "turn_start" => {
                self.streaming = true;
                self.activity = Some("Thinking…".into());
                None
            }
            "agent_end" => {
                self.agent_active = false;
                self.completed_runs = self.completed_runs.saturating_add(1);
                self.streaming = false;
                self.activity = None;
                None
            }
            "agent_settled" => {
                self.agent_active = false;
                self.streaming = false;
                self.activity = None;
                self.refresh_after_settle();
                None
            }
            "message_start" => self.apply_message_start(record),
            "message_update" => self.apply_message_update(record),
            "message_end" => self.apply_message_end(record),
            "turn_end" => {
                if let Some(message) = record.get("message") {
                    let (failed, interrupted) = crate::session_activity::message_outcome(message);
                    self.activity_interrupted |= interrupted;
                    self.activity_error |= failed;
                    if let Some(error) = message.get("errorMessage").and_then(Value::as_str) {
                        let aborted = message["stopReason"] == "aborted";
                        self.activity_error |= !aborted;
                        self.banner = Some(Banner {
                            text: error.to_string(),
                            tone: if aborted { Tone::Warning } else { Tone::Error },
                        });
                    }
                }
                self.streaming = false;
                self.activity = None;
                None
            }
            "tool_execution_start" => self.apply_tool_start(record),
            "tool_execution_update" => self.apply_tool_update(record),
            "tool_execution_end" => {
                self.activity_error |= record.get("isError").and_then(Value::as_bool).unwrap_or(false);
                self.apply_tool_end(record)
            }
            "bash_execution_update" => self.apply_bash_delta(record),
            "queue_update" => {
                self.steering_queue = string_array(record.get("steering"));
                self.follow_up_queue = string_array(record.get("followUp"));
                None
            }
            "thinking_level_changed" => {
                self.thinking_level = record
                    .get("level")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.sync_effort_accent();
                None
            }
            "session_info_changed" => {
                self.session.name = record
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                None
            }
            "compaction_start" => {
                let reason = record
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("manual");
                self.banner = Some(Banner {
                    text: format!("Compacting context ({reason})…"),
                    tone: Tone::Info,
                });
                self.activity = Some("Compacting…".into());
                self.session.is_compacting = true;
                None
            }
            "compaction_end" => {
                self.session.is_compacting = false;
                if record.get("aborted").and_then(Value::as_bool).unwrap_or(false) {
                    self.banner = None;
                    self.push_toast("Compaction cancelled".into(), Tone::Warning, cx);
                } else if let Some(error) = record.get("errorMessage").and_then(Value::as_str) {
                    self.banner = None;
                    self.push_toast(format!("Compaction failed: {error}"), Tone::Error, cx);
                } else if let Some(result) = record.get("result") {
                    let before = result.get("tokensBefore").and_then(Value::as_u64).unwrap_or(0);
                    let after = result
                        .get("estimatedTokensAfter")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let summary = result
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    self.banner = None;
                    self.push_message(self.summary_message("Context compacted", &summary, before, after));
                    self.push_toast(
                        format!("Compacted {} → {} tokens", compact_number(before), compact_number(after)),
                        Tone::Success,
                        cx,
                    );
                } else {
                    self.banner = None;
                }
                self.refresh_after_settle();
                None
            }
            "auto_retry_start" => {
                let attempt = record.get("attempt").and_then(Value::as_u64).unwrap_or(1);
                let max = record.get("maxAttempts").and_then(Value::as_u64).unwrap_or(3);
                let message = record
                    .get("errorMessage")
                    .and_then(Value::as_str)
                    .unwrap_or("transient error");
                self.banner = Some(Banner {
                    text: format!("Retrying ({attempt}/{max}) — {message}"),
                    tone: Tone::Warning,
                });
                None
            }
            "auto_retry_end" => {
                let success = record.get("success").and_then(Value::as_bool).unwrap_or(false);
                if success {
                    self.banner = None;
                } else {
                    let error = record
                        .get("finalError")
                        .and_then(Value::as_str)
                        .unwrap_or("request failed");
                    self.banner = Some(Banner {
                        text: format!("Request failed: {error}"),
                        tone: Tone::Error,
                    });
                }
                None
            }
            "summarization_retry_scheduled" => {
                self.banner = Some(Banner {
                    text: "Retrying summarization…".into(),
                    tone: Tone::Warning,
                });
                None
            }
            "summarization_retry_finished" => {
                self.banner = None;
                None
            }
            "extension_error" => {
                let path = record
                    .get("extensionPath")
                    .and_then(Value::as_str)
                    .unwrap_or("extension");
                let error = record
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                self.push_toast(format!("{path}: {error}"), Tone::Error, cx);
                None
            }
            "extension_ui_request" => {
                self.apply_extension_ui(record, cx);
                None
            }
            _ => None,
        }
    }

    fn apply_response(&mut self, response: RpcResponse, cx: &mut Context<Self>) -> Option<usize> {
        if let Some((text, images)) = response.id.as_ref().and_then(|id| self.pending_image_prompts.remove(id)) {
            if !response.success {
                self.pasted_images.extend(images);
                if self.editor.read(cx).is_empty(cx) {
                    self.editor.update(cx, |editor, cx| editor.set_text(text, cx));
                }
                self.streaming = false;
                self.activity = None;
            }
        }
        if !response.success {
            self.activity_error = true;
            if response.command == "get_messages" {
                self.loading = false;
            }
            if let Some(error) = &response.error {
                self.banner = Some(Banner {
                    text: format!("{}: {error}", response.command),
                    tone: Tone::Error,
                });
            }
            // A rejected prompt should not leave a phantom message behind.
            if response.command == "prompt" {
                if let Some(index) = self.pending_local_user.take() {
                    if index < self.messages.len() {
                        self.messages.remove(index);
                        self.list_state.reset(self.messages.len());
                        self.list_count = self.messages.len();
                    }
                }
            }
            return None;
        }
        match response.command.as_str() {
            "get_state" => {
                let data = &response.data;
                if let Some(model) = data.get("model").and_then(ModelInfo::from_value) {
                    self.model = Some(model);
                }
                self.thinking_level = data
                    .get("thinkingLevel")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.reconcile_effort();
                self.sync_effort_accent();
                self.session.id = data
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.session.file = data
                    .get("sessionFile")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.session.name = data
                    .get("sessionName")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.session.message_count = data
                    .get("messageCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                self.session.is_compacting = data
                    .get("isCompacting")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.session.auto_compaction = data
                    .get("autoCompactionEnabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                self.streaming = data
                    .get("isStreaming")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.agent_active = self.streaming;
                self.session.steering_mode = data
                    .get("steeringMode")
                    .and_then(Value::as_str)
                    .unwrap_or("one-at-a-time")
                    .to_string();
                self.session.follow_up_mode = data
                    .get("followUpMode")
                    .and_then(Value::as_str)
                    .unwrap_or("one-at-a-time")
                    .to_string();
                None
            }
            "get_available_models" => {
                self.models = response
                    .data
                    .get("models")
                    .and_then(Value::as_array)
                    .map(|models| models.iter().filter_map(ModelInfo::from_value).collect())
                    .unwrap_or_default();
                None
            }
            "get_available_thinking_levels" => {
                self.thinking_levels = string_array(response.data.get("levels"));
                self.reconcile_effort();
                self.sync_effort_accent();
                None
            }
            "get_session_stats" => {
                self.stats = Some(response.data);
                None
            }
            "get_commands" => {
                self.commands = response
                    .data
                    .get("commands")
                    .and_then(Value::as_array)
                    .map(|commands| {
                        commands
                            .iter()
                            .filter_map(|command| {
                                Some(CommandInfo {
                                    name: command.get("name")?.as_str()?.to_string(),
                                    description: command
                                        .get("description")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                    source: command
                                        .get("source")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                None
            }
            "get_messages" => {
                self.loading = false;
                // Rebuild the transcript from scratch; this only runs on start.
                let messages = response
                    .data
                    .get("messages")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if self.messages.is_empty() {
                    for message in &messages {
                        self.apply_message_end_value(message);
                    }
                }
                None
            }
            "bash" => {
                let id = response.id.clone().unwrap_or_default();
                if let Some(index) = self.local_bash.remove(&id) {
                    if let Some(message) = self.messages.get_mut(index) {
                        if let Some(Block::Bash(card)) = message.blocks.first_mut() {
                            card.output = response
                                .data
                                .get("output")
                                .and_then(Value::as_str)
                                .unwrap_or(&card.output)
                                .to_string();
                            card.exit_code = response.data.get("exitCode").and_then(Value::as_i64);
                            card.cancelled = response
                                .data
                                .get("cancelled")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            card.truncated = response
                                .data
                                .get("truncated")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            card.status = if card.cancelled {
                                ToolStatus::Failed
                            } else if card.exit_code.unwrap_or(0) == 0 {
                                ToolStatus::Ok
                            } else {
                                ToolStatus::Failed
                            };
                        }
                    }
                    return Some(index);
                }
                None
            }
            "new_session" => {
                let cancelled = response
                    .data
                    .get("cancelled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if !cancelled {
                    self.reset_transcript();
                    self.push_toast("Started a new session".into(), Tone::Success, cx);
                }
                None
            }
            "compact" => None,
            "export_html" => {
                if let Some(path) = response.data.get("path").and_then(Value::as_str) {
                    self.push_toast(
                        format!("Exported session to {path}"),
                        Tone::Success,
                        cx,
                    );
                }
                None
            }
            _ => None,
        }
    }

    // ---------------------------------------------------------------- messages

    fn apply_message_start(&mut self, record: &Value) -> Option<usize> {
        let message = record.get("message")?;
        let role = message.get("role").and_then(Value::as_str).unwrap_or_default();
        let timestamp = message.get("timestamp").and_then(Value::as_i64).unwrap_or(0);
        match role {
            "user" => {
                let text = message_text(message);
                if let Some(index) = self.pending_local_user.take() {
                    if let Some(existing) = self.messages.get_mut(index) {
                        if existing.text().trim() == text.trim() {
                            existing.local = false;
                            existing.timestamp = timestamp;
                            return Some(index);
                        }
                    }
                    self.push_local_user(&text);
                    return self.messages.len().checked_sub(1);
                }
                let mut entry = Message::new(Role::User);
                entry.blocks.push(Block::Text(text));
                entry.timestamp = timestamp;
                Some(self.push_message(entry))
            }
            "assistant" => {
                let mut entry = Message::new(Role::Assistant);
                entry.streaming = true;
                entry.timestamp = timestamp;
                entry.model = message.get("model").and_then(Value::as_str).map(str::to_string);
                self.content_slots.clear();
                let index = self.push_message(entry);
                self.streaming_message = Some(index);
                self.activity = Some("Thinking…".into());
                Some(index)
            }
            "bashExecution" => {
                let card = BashCard {
                    command: message
                        .get("command")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    output: message
                        .get("output")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    exit_code: message.get("exitCode").and_then(Value::as_i64),
                    cancelled: message
                        .get("cancelled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    truncated: message
                        .get("truncated")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    status: ToolStatus::Ok,
                };
                let mut entry = Message::new(Role::Bash);
                entry.blocks.push(Block::Bash(Box::new(card)));
                entry.timestamp = timestamp;
                Some(self.push_message(entry))
            }
            "compactionSummary" => {
                let summary = message
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let before = message
                    .get("tokensBefore")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                Some(self.push_message(self.summary_message("Context compacted", summary, before, 0)))
            }
            "branchSummary" => {
                let summary = message
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                Some(self.push_message(self.summary_message("Branch summary", summary, 0, 0)))
            }
            "custom" => {
                if message.get("display").and_then(Value::as_bool).unwrap_or(true) {
                    let text = message_text(message);
                    let mut entry = Message::new(Role::System);
                    entry.blocks.push(Block::Note {
                        label: message
                            .get("customType")
                            .and_then(Value::as_str)
                            .unwrap_or("extension")
                            .to_string(),
                        text,
                        tone: Tone::Info,
                    });
                    Some(self.push_message(entry))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn apply_message_update(&mut self, record: &Value) -> Option<usize> {
        let update = record.get("assistantMessageEvent")?;
        let kind = update.get("type").and_then(Value::as_str).unwrap_or_default();
        let content_index = update
            .get("contentIndex")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;

        let index = match self.streaming_message {
            Some(index) => index,
            None => {
                let mut entry = Message::new(Role::Assistant);
                entry.streaming = true;
                self.content_slots.clear();
                self.push_message(entry)
            }
        };

        // Usage arrives alongside every delta.
        if let Some(usage) = record.get("usage") {
            if !usage.is_null() {
                if let Some(message) = self.messages.get_mut(index) {
                    message.usage = Some(usage.clone());
                }
            }
        }

        let message = self.messages.get_mut(index)?;
        match kind {
            "text_start" => {
                message.blocks.push(Block::Text(String::new()));
                self.content_slots.insert(content_index, message.blocks.len() - 1);
            }
            "text_delta" => {
                let delta = update.get("delta").and_then(Value::as_str).unwrap_or_default();
                if let Some(slot) = self.content_slots.get(&content_index).copied() {
                    if let Some(Block::Text(text)) = message.blocks.get_mut(slot) {
                        text.push_str(delta);
                    }
                }
            }
            "text_end" => {
                let content = update.get("content").and_then(Value::as_str).unwrap_or_default();
                if let Some(slot) = self.content_slots.get(&content_index).copied() {
                    if let Some(Block::Text(text)) = message.blocks.get_mut(slot) {
                        *text = content.to_string();
                    }
                }
            }
            "thinking_start" => {
                message.blocks.push(Block::Thinking {
                    text: String::new(),
                    done: false,
                    // O padrão é escolha do usuário (`/thinking-view`):
                    // acompanhar o raciocínio ou mantê-lo recolhido.
                    expanded: self.show_thinking,
                });
                self.content_slots.insert(content_index, message.blocks.len() - 1);
            }
            "thinking_delta" => {
                let delta = update.get("delta").and_then(Value::as_str).unwrap_or_default();
                if let Some(slot) = self.content_slots.get(&content_index).copied() {
                    if let Some(Block::Thinking { text, .. }) = message.blocks.get_mut(slot) {
                        text.push_str(delta);
                    }
                }
            }
            "thinking_end" => {
                let content = update.get("content").and_then(Value::as_str).unwrap_or_default();
                if let Some(slot) = self.content_slots.get(&content_index).copied() {
                    if let Some(Block::Thinking { text, done, .. }) = message.blocks.get_mut(slot) {
                        *text = content.to_string();
                        *done = true;
                    }
                }
            }
            "toolcall_start" => {
                let id = update
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("pending-tool")
                    .to_string();
                let name = update
                    .get("toolName")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                let card = ToolCard::new(id.clone(), name);
                message.blocks.push(Block::Tool(Box::new(card)));
                let block_index = message.blocks.len() - 1;
                self.content_slots.insert(content_index, block_index);
                // `message` borrow ends here; record the slot for later events.
                self.tool_slots.insert(id, (index, block_index));
            }
            "toolcall_delta" => {
                let delta = update.get("delta").and_then(Value::as_str).unwrap_or_default();
                if let Some(slot) = self.content_slots.get(&content_index).copied() {
                    if let Some(Block::Tool(card)) = message.blocks.get_mut(slot) {
                        card.args_text.push_str(delta);
                    }
                }
            }
            "toolcall_end" => {
                let tool_call = update.get("toolCall").cloned().unwrap_or(Value::Null);
                if let Some(slot) = self.content_slots.get(&content_index).copied() {
                    if let Some(Block::Tool(card)) = message.blocks.get_mut(slot) {
                        if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                            card.id = id.to_string();
                        }
                        if let Some(name) = tool_call.get("name").and_then(Value::as_str) {
                            card.name = name.to_string();
                        }
                        if let Some(arguments) = tool_call.get("arguments") {
                            card.args = arguments.clone();
                            card.args_text = arguments.to_string();
                        }
                        let id = card.id.clone();
                        let slot = (index, slot);
                        self.tool_slots.insert(id, slot);
                    }
                }
            }
            _ => {}
        }
        Some(index)
    }

    fn apply_message_end(&mut self, record: &Value) -> Option<usize> {
        let message = record.get("message")?;
        let role = message.get("role").and_then(Value::as_str).unwrap_or_default();
        if role == "assistant" {
            let (failed, interrupted) = crate::session_activity::message_outcome(message);
            self.activity_interrupted |= interrupted;
            self.activity_error |= failed;
        }
        match role {
            "assistant" => {
                let index = self.streaming_message.take().or_else(|| {
                    self.messages
                        .iter()
                        .rposition(|entry| entry.role == Role::Assistant && entry.streaming)
                });
                let Some(index) = index else {
                    let index = self.apply_message_end_value(message)?;
                    if let Some(entry) = self.messages.get_mut(index) {
                        entry.finished_at = now_ms().unwrap_or(0);
                    }
                    return Some(index);
                };
                self.rebuild_from_value(index, message);
                if let Some(entry) = self.messages.get_mut(index) {
                    entry.finished_at = now_ms().unwrap_or(0);
                }
                Some(index)
            }
            "bashExecution" => {
                // Update the matching bash message if we have one, else add it.
                let command = message
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let index = self.messages.iter().rposition(|entry| {
                    entry.role == Role::Bash
                        && entry
                            .blocks
                            .iter()
                            .any(|block| matches!(block, Block::Bash(card) if card.command == command))
                });
                if let Some(index) = index {
                    if let Some(Block::Bash(card)) = self.messages[index].blocks.first_mut() {
                        card.output = message
                            .get("output")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        card.exit_code = message.get("exitCode").and_then(Value::as_i64);
                        card.cancelled = message
                            .get("cancelled")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        card.truncated = message
                            .get("truncated")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        card.status = if card.cancelled {
                            ToolStatus::Failed
                        } else if card.exit_code.unwrap_or(0) == 0 {
                            ToolStatus::Ok
                        } else {
                            ToolStatus::Failed
                        };
                    }
                    return Some(index);
                }
                self.apply_message_start(&json!({ "message": message }))
            }
            "user" | "toolResult" | "system" => None,
            "custom" => self.apply_message_start(&json!({ "message": message })),
            _ => None,
        }
    }

    /// Append a message from a `get_messages` payload.
    fn apply_message_end_value(&mut self, message: &Value) -> Option<usize> {
        let role = message.get("role").and_then(Value::as_str).unwrap_or_default();
        match role {
            "assistant" => {
                let mut entry = Message::new(Role::Assistant);
                entry.timestamp = message.get("timestamp").and_then(Value::as_i64).unwrap_or(0);
                entry.model = message.get("model").and_then(Value::as_str).map(str::to_string);
                entry.usage = message.get("usage").cloned();
                entry.stop_reason = message
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                entry.error = message
                    .get("errorMessage")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let index = self.push_message(entry);
                self.rebuild_from_value(index, message);
                Some(index)
            }
            "user" => {
                let mut entry = Message::new(Role::User);
                entry.timestamp = message.get("timestamp").and_then(Value::as_i64).unwrap_or(0);
                entry.blocks.push(Block::Text(message_text(message)));
                Some(self.push_message(entry))
            }
            "bashExecution" => self.apply_message_start(&json!({ "message": message })),
            "toolResult" => {
                // Restoring a session: attach the recorded result to the tool
                // card created for the matching call.
                let id = message.get("toolCallId").and_then(Value::as_str)?;
                let (message_index, block_index) = *self.tool_slots.get(id)?;
                let is_error = message
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let output = tool_result_text(message).unwrap_or_default();
                let details = message.get("details").cloned();
                if let Some(Block::Tool(card)) = self
                    .messages
                    .get_mut(message_index)
                    .and_then(|entry| entry.blocks.get_mut(block_index))
                {
                    card.output = output;
                    card.status = if is_error { ToolStatus::Failed } else { ToolStatus::Ok };
                    card.details = details;
                }
                Some(message_index)
            }
            "compactionSummary" => {
                let summary = message
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let before = message.get("tokensBefore").and_then(Value::as_u64).unwrap_or(0);
                Some(self.push_message(self.summary_message("Context compacted", summary, before, 0)))
            }
            "branchSummary" => {
                let summary = message
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                Some(self.push_message(self.summary_message("Branch summary", summary, 0, 0)))
            }
            _ => None,
        }
    }

    /// Replace a streaming message's blocks with the authoritative content,
    /// preserving live tool state.
    fn rebuild_from_value(&mut self, index: usize, message: &Value) {
        let previous = std::mem::take(&mut self.messages[index].blocks);
        let content = message.get("content").cloned().unwrap_or(Value::Null);
        let mut blocks: Vec<Block> = Vec::new();
        let mut thinking_seen = 0usize;

        let items: Vec<Value> = match content {
            Value::Array(items) => items,
            Value::String(text) => vec![json!({ "type": "text", "text": text })],
            _ => Vec::new(),
        };

        for item in items {
            match item.get("type").and_then(Value::as_str).unwrap_or_default() {
                "text" => {
                    let text = item.get("text").and_then(Value::as_str).unwrap_or_default();
                    if !text.is_empty() {
                        blocks.push(Block::Text(text.to_string()));
                    }
                }
                "thinking" => {
                    let text = item
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let expanded = previous
                        .iter()
                        .filter_map(|block| match block {
                            Block::Thinking { expanded, .. } => Some(*expanded),
                            _ => None,
                        })
                        .nth(thinking_seen)
                        .unwrap_or(false);
                    thinking_seen += 1;
                    if !text.is_empty() && !item.get("redacted").and_then(Value::as_bool).unwrap_or(false) {
                        blocks.push(Block::Thinking {
                            text,
                            done: true,
                            expanded,
                        });
                    }
                }
                "toolCall" => {
                    let id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string();
                    let existing = previous.iter().find_map(|block| match block {
                        Block::Tool(card) if card.id == id => Some((**card).clone()),
                        _ => None,
                    });
                    let mut card = existing.unwrap_or_else(|| {
                        ToolCard::new(
                            id.clone(),
                            item.get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("tool")
                                .to_string(),
                        )
                    });
                    if let Some(name) = item.get("name").and_then(Value::as_str) {
                        card.name = name.to_string();
                    }
                    if let Some(arguments) = item.get("arguments") {
                        card.args = arguments.clone();
                        if card.args_text.is_empty() {
                            card.args_text = arguments.to_string();
                        }
                    }
                    let block_index = blocks.len();
                    blocks.push(Block::Tool(Box::new(card)));
                    self.tool_slots.insert(id, (index, block_index));
                }
                _ => {}
            }
        }

        let entry = &mut self.messages[index];
        let existing_error = entry.error.clone();
        entry.blocks = blocks;
        entry.streaming = false;
        entry.timestamp = message
            .get("timestamp")
            .and_then(Value::as_i64)
            .unwrap_or(entry.timestamp);
        if let Some(usage) = message.get("usage") {
            if !usage.is_null() {
                entry.usage = Some(usage.clone());
            }
        }
        if let Some(reason) = message.get("stopReason").and_then(Value::as_str) {
            entry.stop_reason = Some(reason.to_string());
        }
        if let Some(error) = message.get("errorMessage").and_then(Value::as_str) {
            entry.error = Some(error.to_string());
        } else {
            entry.error = existing_error;
        }
        if let Some(model) = message.get("model").and_then(Value::as_str) {
            entry.model = Some(model.to_string());
        }
    }

    // ------------------------------------------------------------------- tools

    fn apply_tool_start(&mut self, record: &Value) -> Option<usize> {
        let id = record.get("toolCallId").and_then(Value::as_str)?.to_string();
        let name = record
            .get("toolName")
            .and_then(Value::as_str)
            .unwrap_or("tool")
            .to_string();
        let args = record.get("args").cloned().unwrap_or(Value::Null);
        let (message_index, block_index) = *self.tool_slots.get(&id)?;
        if let Some(message) = self.messages.get_mut(message_index) {
            if let Some(Block::Tool(card)) = message.blocks.get_mut(block_index) {
                card.status = ToolStatus::Running;
                card.name = name.clone();
                card.args = args.clone();
                card.args_text = args.to_string();
                card.started_at = now_ms();
            }
        }
        self.activity = Some(format!("Running {name}…"));
        Some(message_index)
    }

    fn apply_tool_update(&mut self, record: &Value) -> Option<usize> {
        let id = record.get("toolCallId").and_then(Value::as_str)?.to_string();
        let (message_index, block_index) = *self.tool_slots.get(&id)?;
        let partial = record.get("partialResult").cloned().unwrap_or(Value::Null);
        if let Some(message) = self.messages.get_mut(message_index) {
            if let Some(Block::Tool(card)) = message.blocks.get_mut(block_index) {
                if let Some(text) = tool_result_text(&partial) {
                    card.output = text;
                }
                if card.status == ToolStatus::Pending {
                    card.status = ToolStatus::Running;
                }
            }
        }
        Some(message_index)
    }

    fn apply_tool_end(&mut self, record: &Value) -> Option<usize> {
        let id = record.get("toolCallId").and_then(Value::as_str)?.to_string();
        let (message_index, block_index) = *self.tool_slots.get(&id)?;
        let result = record.get("result").cloned().unwrap_or(Value::Null);
        let is_error = record.get("isError").and_then(Value::as_bool).unwrap_or(false);
        if let Some(message) = self.messages.get_mut(message_index) {
            if let Some(Block::Tool(card)) = message.blocks.get_mut(block_index) {
                if let Some(text) = tool_result_text(&result) {
                    card.output = text;
                }
                card.details = result.get("details").cloned();
                card.status = if is_error { ToolStatus::Failed } else { ToolStatus::Ok };
                card.elapsed_ms = card
                    .started_at
                    .and_then(|started| now_ms().map(|now| (now - started).max(0) as u64));
            }
        }
        self.activity = Some("Thinking…".into());
        Some(message_index)
    }

    fn apply_bash_delta(&mut self, record: &Value) -> Option<usize> {
        let id = record.get("id").and_then(Value::as_str)?;
        let delta = record.get("delta").and_then(Value::as_str).unwrap_or_default();
        let index = *self.local_bash.get(id)?;
        if let Some(message) = self.messages.get_mut(index) {
            if let Some(Block::Bash(card)) = message.blocks.first_mut() {
                card.output.push_str(delta);
            }
        }
        Some(index)
    }

    // -------------------------------------------------------------- extensions

    fn apply_extension_ui(&mut self, record: &Value, cx: &mut Context<Self>) {
        let id = record.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        let method = record.get("method").and_then(Value::as_str).unwrap_or_default();
        match method {
            "notify" => {
                let message = record
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let tone = match record.get("notifyType").and_then(Value::as_str) {
                    Some("warning") => Tone::Warning,
                    Some("error") => Tone::Error,
                    _ => Tone::Info,
                };
                self.push_toast(message, tone, cx);
            }
            "setStatus" => {
                let key = record
                    .get("statusKey")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                match record.get("statusText").and_then(Value::as_str) {
                    Some(text) if !text.is_empty() => {
                        if let Some(slot) = self.ext_status.iter_mut().find(|(k, _)| *k == key) {
                            slot.1 = text.to_string();
                        } else {
                            self.ext_status.push((key, text.to_string()));
                        }
                    }
                    _ => self.ext_status.retain(|(k, _)| *k != key),
                }
            }
            "setTitle" => {
                self.title_override = record
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            "setWidget" => {
                self.ext_widget = string_array(record.get("widgetLines"));
            }
            "set_editor_text" => {
                let text = record
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.editor.update(cx, |editor, cx| editor.set_text(text, cx));
            }
            "select" | "confirm" | "input" | "editor" => {
                let kind = match method {
                    "select" => ModalKind::Select,
                    "confirm" => ModalKind::Confirm,
                    "input" => ModalKind::Input,
                    _ => ModalKind::Editor,
                };
                if matches!(kind, ModalKind::Input | ModalKind::Editor) {
                    let prefill = record
                        .get("prefill")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let placeholder = record
                        .get("placeholder")
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                        .unwrap_or("Type here…")
                        .to_string();
                    let editor = self.modal_editor.clone();
                    editor.update(cx, |editor, cx| {
                        editor.set_placeholder(placeholder);
                        editor.set_text(prefill, cx);
                    });
                }
                self.modal = Some(Modal {
                    id,
                    kind,
                    title: record
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("Pi needs your input")
                        .to_string(),
                    message: record
                        .get("message")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    options: string_array(record.get("options")),
                    placeholder: record
                        .get("placeholder")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    wants_input: matches!(kind, ModalKind::Input | ModalKind::Editor),
                });
                self.modal_index = 0;
            }
            _ => {}
        }
    }

    /// Um diálogo de seleção está aberto?
    pub fn modal_select_open(&self) -> bool {
        self.modal
            .as_ref()
            .is_some_and(|modal| modal.kind == ModalKind::Select)
    }

    /// Move o destaque no diálogo de seleção.
    pub fn modal_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(count) = self.modal.as_ref().map(|modal| modal.options.len()) else {
            return;
        };
        if count == 0 {
            return;
        }
        self.modal_index =
            (self.modal_index as isize + delta).rem_euclid(count as isize) as usize;
        cx.notify();
    }

    /// Responde o diálogo de seleção com a opção destacada.
    pub fn submit_modal_select(&mut self, cx: &mut Context<Self>) {
        let Some(option) = self
            .modal
            .as_ref()
            .filter(|modal| modal.kind == ModalKind::Select)
            .and_then(|modal| modal.options.get(self.modal_index).cloned())
        else {
            return;
        };
        self.respond_modal(ModalAnswer::Value(json!(option)), cx);
    }

    /// Linha destacada no diálogo de seleção atual.
    pub fn modal_highlight(&self) -> usize {
        self.modal_index
    }

    /// Respond to a dialog request from an extension.
    pub fn respond_modal(&mut self, value: ModalAnswer, cx: &mut Context<Self>) {
        let Some(modal) = self.modal.take() else {
            return;
        };
        let payload = match value {
            ModalAnswer::Cancelled => json!({
                "type": "extension_ui_response",
                "id": modal.id,
                "cancelled": true,
            }),
            ModalAnswer::Value(value) => json!({
                "type": "extension_ui_response",
                "id": modal.id,
                "value": value,
            }),
            ModalAnswer::Confirmed(confirmed) => json!({
                "type": "extension_ui_response",
                "id": modal.id,
                "confirmed": confirmed,
            }),
        };
        self.client.send(payload);
        self.composer_focus_pending = true;
        cx.notify();
    }

    // ------------------------------------------------------------ transcript

    fn summary_message(&self, label: &str, summary: &str, before: u64, after: u64) -> Message {
        let text = if after > 0 {
            format!(
                "{} → {} tokens\n\n{}",
                compact_number(before),
                compact_number(after),
                summary
            )
        } else if before > 0 {
            format!("{}\n\n{}", compact_number(before), summary)
        } else {
            summary.to_string()
        };
        let mut message = Message::new(Role::System);
        message.blocks.push(Block::Note {
            label: label.to_string(),
            text,
            tone: Tone::Info,
        });
        message
    }

    fn push_message(&mut self, message: Message) -> usize {
        self.messages.push(message);
        self.messages.len() - 1
    }

    fn push_local_user(&mut self, text: &str) {
        let mut message = Message::new(Role::User);
        message.local = true;
        message.timestamp = now_ms().unwrap_or(0);
        message.blocks.push(Block::Text(text.to_string()));
        self.messages.push(message);
        self.pending_local_user = Some(self.messages.len() - 1);
    }

    fn reset_transcript(&mut self) {
        self.messages.clear();
        self.tool_slots.clear();
        self.content_slots.clear();
        self.local_bash.clear();
        self.streaming_message = None;
        self.pending_local_user = None;
        self.list_state.reset(0);
        self.list_count = 0;
        self.banner = None;
        self.steering_queue.clear();
        self.follow_up_queue.clear();
    }


    fn sync_list(&mut self) {
        let count = self.messages.len();
        if count > self.list_count {
            self.list_state
                .splice(self.list_count..self.list_count, count - self.list_count);
        } else if count < self.list_count {
            self.list_state.splice(count..self.list_count, 0);
        }
        self.list_count = count;
    }

    /// Re-point the interface accent at the current reasoning level.
    fn sync_effort_accent(&self) {
        if self.visible {
            theme::set_effort(self.thinking_level.as_deref(), &self.thinking_levels);
        }
    }

    fn refresh_after_settle(&mut self) {
        self.client.call("get_session_stats", Value::Null);
        self.client.call("get_state", Value::Null);
    }

    // --------------------------------------------------------------- actions

    pub fn push_toast(&mut self, text: String, tone: Tone, cx: &mut Context<Self>) {
        let id = self.toast_seq;
        self.toast_seq += 1;
        self.toasts.push(Toast { id, text, tone });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(5200))
                .await;
            this.update(cx, |state, cx| {
                state.toasts.retain(|toast| toast.id != id);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.paste_loading || self.disconnected {
            self.push_toast("Aguarde a colagem terminar e verifique a conexão com o Pi.".into(), Tone::Warning, cx);
            return;
        }
        if !self.pasted_images.is_empty() {
            let text = self.editor.read(cx).text(cx);
            if text.trim_start().starts_with(['/', '!']) {
                self.push_toast("Envie imagens com uma mensagem normal, sem comando / ou !.".into(), Tone::Warning, cx);
                return;
            }
            let images = std::mem::take(&mut self.pasted_images);
            let payload: Vec<Value> = images.iter().map(|(_, value)| value.clone()).collect();
            let behavior = self.streaming.then_some("steer");
            let id = self.client.prompt_with_images(&text, behavior, &payload);
            let display = format!("{}\n{}", text, vec!["[image]"; images.len()].join("\n"));
            self.push_local_user(&display);
            self.pending_image_prompts.insert(id, (text, images));
            if behavior.is_some() {
                self.steering_queue.push(display);
            } else {
                self.streaming = true;
                self.activity = Some("Thinking…".into());
            }
            self.editor.update(cx, |editor, cx| editor.clear(cx));
            self.last_run = Some(std::time::SystemTime::now());
            self.sync_list();
            cx.notify();
            return;
        }
        // Enter with the slash menu open takes the highlighted command.
        let (matches, _) = self.slash_state(cx);
        if !matches.is_empty() {
            self.menu_accept(true, cx);
            return;
        }

        let text = self.editor.read(cx).text(cx);
        if text.trim().is_empty() {
            return;
        }
        self.editor.update(cx, |editor, cx| editor.clear(cx));
        self.dispatch(text, cx);
    }

    /// Send a prompt programmatically (command line, suggestion chips).
    pub fn submit_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.dispatch(text, cx);
    }

    /// Route one line of input: shell escape, slash command, or model prompt.
    fn dispatch(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        if self.disconnected {
            self.push_toast("This session's Pi process has exited. Close and reopen the session.".into(), Tone::Error, cx);
            return;
        }
        self.last_run = Some(std::time::SystemTime::now());

        if let Some(command) = text.strip_prefix('!') {
            let command = command.trim().to_string();
            if !command.is_empty() {
                self.run_bash(command);
            }
            return;
        }
        if text.starts_with('/') && self.run_slash_command(&text, cx) {
            return;
        }

        // While a turn is running, Pi needs to know what to do with the message.
        let behavior = self.streaming.then_some("steer");
        self.client.prompt(&text, behavior);
        self.push_local_user(&text);
        if behavior.is_some() {
            self.steering_queue.push(text);
        } else {
            self.streaming = true;
            self.activity = Some("Thinking…".into());
        }
        cx.notify();
    }

    /// Returns true when the input was handled locally.
    fn run_slash_command(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let mut parts = text.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or_default();
        let argument = parts.next().unwrap_or_default().trim().to_string();
        match name {
            "/new" => {
                self.new_session();
                cx.notify();
                true
            }
            "/compact" => {
                let mut command = json!({ "type": "compact" });
                if !argument.is_empty() {
                    command["customInstructions"] = json!(argument);
                }
                self.client.send(command);
                self.banner = Some(Banner {
                    text: "Compacting context…".into(),
                    tone: Tone::Info,
                });
                true
            }
            "/model" => {
                if argument.is_empty() {
                    self.open_model_menu(cx);
                } else {
                    let mut fields = argument.split_whitespace();
                    let provider = fields.next().unwrap_or_default();
                    let id = fields.next();
                    match id {
                        Some(id) => {
                            self.client.call(
                                "set_model",
                                json!({ "provider": provider, "modelId": id }),
                            );
                        }
                        None => {
                            // Match by model id alone.
                            if let Some(model) = self
                                .models
                                .iter()
                                .find(|model| model.id == *provider)
                                .cloned()
                            {
                                self.client.call(
                                    "set_model",
                                    json!({ "provider": model.provider, "modelId": model.id }),
                                );
                            } else {
                                self.push_toast(
                                    format!("Unknown model: {provider}"),
                                    Tone::Warning,
                                    cx,
                                );
                            }
                        }
                    }
                }
                true
            }
            "/thinking" => {
                if argument.is_empty() {
                    self.thinking_menu = true;
                } else {
                    self.client
                        .call("set_thinking_level", json!({ "level": argument }));
                }
                true
            }
            "/export" => {
                let mut command = json!({ "type": "export_html" });
                if !argument.is_empty() {
                    command["outputPath"] = json!(argument);
                }
                self.client.send(command);
                true
            }
            "/name" => {
                if argument.is_empty() {
                    self.push_toast("Usage: /name <session name>".into(), Tone::Info, cx);
                } else {
                    self.client
                        .call("set_session_name", json!({ "name": argument }));
                    self.session.name = Some(argument);
                }
                true
            }
            "/sidebar" => {
                self.sidebar = !self.sidebar;
                true
            }
            "/thinking-view" => {
                self.show_thinking = !self.show_thinking;
                true
            }
            "/auto-compact" => {
                self.toggle_auto_compaction();
                let enabled = if self.session.auto_compaction { "on" } else { "off" };
                self.push_toast(format!("Auto-compaction {enabled}"), Tone::Info, cx);
                true
            }
            "/clear" => {
                self.reset_transcript();
                true
            }
            "/help" => {
                self.help_open = true;
                self.help_focus_pending = true;
                true
            }
            _ => {
                // Extension commands, skills and prompt templates are expanded
                // by Pi itself, but only if Pi knows the command.
                let known = self
                    .commands
                    .iter()
                    .any(|command| format!("/{}", command.name) == name);
                if known {
                    let behavior = self.streaming.then_some("steer");
                    self.client.prompt(text, behavior);
                    self.push_local_user(text);
                    if behavior.is_some() {
                        self.steering_queue.push(text.to_string());
                    }
                    true
                } else {
                    self.push_toast(
                        format!("Unknown command {name} — try /help"),
                        Tone::Warning,
                        cx,
                    );
                    true
                }
            }
        }
    }

    pub fn run_bash(&mut self, command: String) {
        let id = self.client.bash(&command);
        let mut message = Message::new(Role::Bash);
        message.timestamp = now_ms().unwrap_or(0);
        message.blocks.push(Block::Bash(Box::new(BashCard {
            command,
            output: String::new(),
            exit_code: None,
            cancelled: false,
            truncated: false,
            status: ToolStatus::Running,
        })));
        let index = self.push_message(message);
        self.local_bash.insert(id, index);
    }

    /// Replace the composer text, moving focus to the end of it.
    pub fn set_composer_text(&mut self, text: String, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| editor.set_text(text, cx));
        cx.notify();
    }

    /// Título da conversa: o nome da sessão, senão o primeiro pedido do usuário.
    pub fn display_title(&self) -> String {
        if let Some(name) = self.session.name.clone() {
            if !name.trim().is_empty() {
                return name;
            }
        }
        if let Some(message) = self.messages.iter().find(|m| m.role == Role::User) {
            let text = message
                .text()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if !text.is_empty() {
                return text.chars().take(72).collect();
            }
        }
        "Nova conversa".to_string()
    }

    /// Arquivos que o turno em curso tocou: leituras e edições, sem repetir,
    /// com o estado mais forte que apareceu para cada caminho.
    pub fn current_files(&self) -> Vec<TouchedFile> {
        let start = self
            .messages
            .iter()
            .rposition(|message| message.role == Role::User)
            .unwrap_or(0);
        let mut files: Vec<TouchedFile> = Vec::new();
        for message in self.messages.iter().skip(start) {
            if message.role != Role::Assistant {
                continue;
            }
            for block in &message.blocks {
                let Block::Tool(card) = block else {
                    continue;
                };
                let edited = matches!(card.name.as_str(), "write" | "create" | "edit" | "patch");
                if !edited && !matches!(card.name.as_str(), "read" | "view" | "open") {
                    continue;
                }
                let path = card.summary();
                if path.is_empty() || path.starts_with('{') {
                    continue;
                }
                match files.iter_mut().find(|file| file.path == path) {
                    Some(file) => {
                        file.edited |= edited;
                        file.status = match (file.status, card.status) {
                            (ToolStatus::Failed, _) | (_, ToolStatus::Failed) => ToolStatus::Failed,
                            (ToolStatus::Running, _) | (_, ToolStatus::Running) => {
                                ToolStatus::Running
                            }
                            _ => ToolStatus::Ok,
                        };
                    }
                    None => files.push(TouchedFile {
                        path,
                        edited,
                        status: card.status,
                    }),
                }
            }
        }
        // Edições primeiro: é o que interessa saber.
        files.sort_by_key(|file| !file.edited);
        files
    }

    /// Linha do tempo da sessão: cada pedido do usuário com a resposta que veio.
    /// Dado real — o `limit` corta pela cauda, mais recente primeiro.
    pub fn timeline(&self, limit: usize) -> Vec<TurnSummary> {
        let mut turns: Vec<TurnSummary> = Vec::new();
        for (index, message) in self.messages.iter().enumerate() {
            if message.role != Role::User {
                continue;
            }
            let prompt = message
                .text()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let answer = self.messages[index + 1..]
                .iter()
                .take_while(|entry| entry.role != Role::User)
                .filter(|entry| entry.role == Role::Assistant)
                .last();
            let is_last = !self.messages[index + 1..]
                .iter()
                .any(|entry| entry.role == Role::User);
            let (duration, state) = match answer {
                Some(answer) => {
                    // O Pi datou a mensagem quando ela começou; `finished_at`
                    // é o instante local em que ela terminou.
                    let duration = if answer.finished_at > message.timestamp
                        && message.timestamp > 0
                    {
                        let seconds = (answer.finished_at - message.timestamp) as f64 / 1000.;
                        Some(if seconds < 10. {
                            format!("{seconds:.1}s")
                        } else {
                            format!("{seconds:.0}s")
                        })
                    } else {
                        None
                    };
                    let state = if answer.streaming || (is_last && self.streaming) {
                        TurnState::Running
                    } else if answer.error.is_some()
                        || answer.stop_reason.as_deref() == Some("error")
                    {
                        TurnState::Failed
                    } else if answer.stop_reason.as_deref() == Some("aborted") {
                        TurnState::Stopped
                    } else {
                        TurnState::Done
                    };
                    (duration, state)
                }
                None => (
                    None,
                    if message.local {
                        TurnState::Running
                    } else {
                        TurnState::Done
                    },
                ),
            };
            turns.push(TurnSummary {
                number: turns.len() + 1,
                prompt: prompt.chars().take(80).collect(),
                duration,
                state,
            });
        }
        if turns.len() > limit {
            turns = turns.split_off(turns.len() - limit);
        }
        turns.reverse();
        turns
    }

    /// Se o grupo de atividade do turno está aberto; `None` = usar o padrão
    /// (aberto enquanto o turno está em execução).
    pub fn activity_open(&self, message_index: usize) -> Option<bool> {
        self.activity_override.get(&message_index).copied()
    }

    pub fn set_activity(&mut self, message_index: usize, open: bool, cx: &mut Context<Self>) {
        self.activity_override.insert(message_index, open);
        if message_index < self.list_count {
            self.list_state
                .remeasure_items(message_index..message_index + 1);
        }
        cx.notify();
    }

    pub fn toggle_tool(&mut self, message_index: usize, block_index: usize, cx: &mut Context<Self>) {
        if let Some(message) = self.messages.get_mut(message_index) {
            if let Some(Block::Tool(card)) = message.blocks.get_mut(block_index) {
                card.expanded = !card.expanded;
            }
        }
        if message_index < self.list_count {
            self.list_state
                .remeasure_items(message_index..message_index + 1);
        }
        cx.notify();
    }

    pub fn toggle_thinking(
        &mut self,
        message_index: usize,
        block_index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(message) = self.messages.get_mut(message_index) {
            if let Some(Block::Thinking { expanded, .. }) = message.blocks.get_mut(block_index) {
                *expanded = !*expanded;
            }
        }
        if message_index < self.list_count {
            self.list_state
                .remeasure_items(message_index..message_index + 1);
        }
        cx.notify();
    }

    pub fn abort(&mut self, cx: &mut Context<Self>) {
        self.client.call("abort", Value::Null);
        self.streaming = false;
        self.activity = None;
        self.banner = None;
        cx.notify();
    }

    pub fn clear_queue(&mut self) {
        self.client.call("clear_queue", Value::Null);
    }

    pub fn new_session(&mut self) {
        self.new_session_requested = true;
    }

    pub fn is_executing(&self) -> bool {
        !self.disconnected && (self.agent_active || self.streaming || self.session.is_compacting || !self.local_bash.is_empty())
    }

    pub fn is_busy(&self) -> bool {
        self.is_executing() || (!self.disconnected && self.modal.is_some())
    }

    pub fn compact(&mut self) {
        self.client.call("compact", Value::Null);
        self.banner = Some(Banner {
            text: "Compacting context…".into(),
            tone: Tone::Info,
        });
    }

    pub fn open_model_menu(&mut self, cx: &mut Context<Self>) {
        self.model_search.update(cx, |editor, cx| editor.clear(cx));
        self.model_menu = true;
        self.model_index = 0;
        self.model_query.clear();
        self.model_search_focus_pending = true;
        cx.notify();
    }


    pub fn set_model(&mut self, model: &ModelInfo) {
        self.client.call(
            "set_model",
            json!({ "provider": model.provider, "modelId": model.id }),
        );
        self.model = Some(model.clone());
        self.model_menu = false;
        // A escada do novo modelo chega em seguida; até lá os níveis atuais
        // continuam visíveis, em vez de o controle esvaziar.
        self.client.call("get_available_thinking_levels", Value::Null);
    }

    /// Modelos visíveis para a busca atual, na ordem exibida (o atual primeiro).
    pub fn filtered_models(&self, query: &str) -> Vec<usize> {
        let mut indices: Vec<usize> = self
            .models
            .iter()
            .enumerate()
            .filter(|(_, model)| model_matches(model, query))
            .map(|(index, _)| index)
            .collect();
        if let Some(current) = &self.model {
            indices.sort_by_key(|index| {
                let model = &self.models[*index];
                !(model.id == current.id && model.provider == current.provider)
            });
        }
        indices
    }

    /// A posição destacada no seletor para a busca informada.
    pub fn model_highlight(&self, query: &str) -> Option<usize> {
        let count = self.filtered_models(query).len();
        if count == 0 {
            return None;
        }
        if self.model_query == query {
            Some(self.model_index.min(count - 1))
        } else {
            Some(0)
        }
    }

    /// A lista filtrada e o destaque atual; reinicia o destaque quando a busca muda.
    fn model_state(&mut self, cx: &App) -> (Vec<usize>, usize) {
        let query = self.model_search.read(cx).text(cx);
        if query != self.model_query {
            self.model_query = query.clone();
            self.model_index = 0;
        }
        let order = self.filtered_models(&query);
        let index = self.model_index.min(order.len().saturating_sub(1));
        (order, index)
    }

    pub fn model_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let (order, index) = self.model_state(cx);
        if order.is_empty() {
            return;
        }
        let next = (index as isize + delta).rem_euclid(order.len() as isize) as usize;
        self.model_index = next;
        self.model_scroll.scroll_to_item(next);
        cx.notify();
    }

    pub fn model_accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (order, index) = self.model_state(cx);
        let Some(model_index) = order.get(index).copied() else {
            return;
        };
        let model = self.models[model_index].clone();
        self.set_model(&model);
        let focus = self.editor.read(cx).focus_handle.clone();
        focus.focus(window, cx);
        cx.notify();
    }

    pub fn cycle_effort(&mut self, cx: &mut Context<Self>) {
        let Some(level) = next_effort_level(&self.thinking_levels, self.thinking_level.as_deref())
            .map(str::to_string)
        else {
            self.push_toast("No effort levels available for this model".into(), Tone::Info, cx);
            return;
        };
        self.set_thinking_level(&level);
        self.push_toast(format!("Effort: {level}"), Tone::Info, cx);
    }

    pub fn set_thinking_level(&mut self, level: &str) {
        self.client
            .call("set_thinking_level", json!({ "level": level }));
        self.thinking_level = Some(level.to_string());
        self.thinking_menu = false;
        self.sync_effort_accent();
    }

    /// Alinha o nível atual com a escada do modelo: um nível que a nova lista
    /// não suporta vira `off` (ou o primeiro disponível) em vez de ficar pendurado.
    fn reconcile_effort(&mut self) {
        let Some(level) =
            reconcile_effort_level(&self.thinking_levels, self.thinking_level.as_deref())
                .map(str::to_string)
        else {
            return;
        };
        if self.thinking_level.as_deref() != Some(level.as_str()) {
            self.set_thinking_level(&level);
        }
    }

    pub fn toggle_auto_compaction(&mut self) {
        self.session.auto_compaction = !self.session.auto_compaction;
        self.client.call(
            "set_auto_compaction",
            json!({ "enabled": self.session.auto_compaction }),
        );
    }

    pub fn export_html(&mut self) {
        self.client.call("export_html", Value::Null);
    }

    // ------------------------------------------------------------------ reads

    pub fn context_usage(&self) -> Option<(f64, f64, f64)> {
        let stats = self.stats.as_ref()?;
        let usage = stats.get("contextUsage")?;
        let tokens = usage.get("tokens").and_then(Value::as_f64);
        let window = usage.get("contextWindow").and_then(Value::as_f64);
        let percent = usage.get("percent").and_then(Value::as_f64);
        match (tokens, window, percent) {
            (Some(tokens), Some(window), Some(percent)) => Some((tokens, window, percent)),
            (Some(tokens), Some(window), None) => {
                Some((tokens, window, (tokens / window.max(1.0)) * 100.0))
            }
            _ => None,
        }
    }

    pub fn total_cost(&self) -> Option<f64> {
        self.stats
            .as_ref()
            .and_then(|stats| stats.get("cost"))
            .and_then(Value::as_f64)
    }

    /// The slash menu: the filtered commands and the highlighted row.
    ///
    /// The menu is derived from the composer text, so it is recomputed here and
    /// the highlight is reset whenever the query itself changes.
    pub fn slash_state(&mut self, cx: &App) -> (Vec<(String, String)>, usize) {
        let query = self.slash_query(cx);
        match &query {
            Some(query) if *query != self.slash_query => {
                self.slash_query = query.clone();
                self.slash_index = 0;
                self.slash_dismissed = false;
            }
            None => {
                self.slash_query.clear();
                self.slash_index = 0;
                self.slash_dismissed = false;
                return (Vec::new(), 0);
            }
            _ => {}
        }
        if self.slash_dismissed {
            return (Vec::new(), 0);
        }

        let needle = query.unwrap_or_default();
        let mut matches: Vec<(String, String)> = Vec::new();
        for (name, description) in BUILTIN_COMMANDS {
            if name[1..].starts_with(&needle) {
                matches.push(((*name).to_string(), (*description).to_string()));
            }
        }
        for command in &self.commands {
            if command.name.to_lowercase().starts_with(&needle) {
                matches.push((
                    format!("/{}", command.name),
                    if command.description.is_empty() {
                        command.source.clone()
                    } else {
                        command.description.clone()
                    },
                ));
            }
        }
        matches.truncate(9);
        let index = self.slash_index.min(matches.len().saturating_sub(1));
        (matches, index)
    }

    /// The text being typed after a leading slash, while it is still one word.
    fn slash_query(&self, cx: &App) -> Option<String> {
        let text = self.editor.read(cx).text(cx);
        let rest = text.strip_prefix('/')?;
        if rest.contains(char::is_whitespace) {
            return None;
        }
        Some(rest.to_lowercase())
    }

    /// Hide the slash menu until the query changes again.
    pub fn dismiss_slash(&mut self, cx: &mut Context<Self>) {
        self.slash_dismissed = true;
        cx.notify();
    }

    /// Move the slash-menu highlight, or move the caret when no menu is open.
    pub fn menu_move(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let (matches, index) = self.slash_state(cx);
        if matches.is_empty() {
            let editor = self.editor.clone();
            editor.update(cx, |editor, cx| {
                if delta < 0 {
                    editor.up(&crate::editor::Up, window, cx);
                } else {
                    editor.down(&crate::editor::Down, window, cx);
                }
            });
            return;
        }
        let count = matches.len() as isize;
        self.slash_index = (index as isize + delta).rem_euclid(count) as usize;
        cx.notify();
    }

    /// Take a specific menu row (used by clicks, so it behaves exactly like
    /// pressing Enter on it would).
    pub fn select_slash(&mut self, index: usize, cx: &mut Context<Self>) {
        self.slash_index = index;
        self.menu_accept(false, cx);
    }

    /// Take the highlighted command. `run` sends it, otherwise it is only
    /// written into the composer for editing.
    pub fn menu_accept(&mut self, run: bool, cx: &mut Context<Self>) {
        let (matches, index) = self.slash_state(cx);
        let Some((name, _)) = matches.get(index).cloned() else {
            return;
        };
        self.slash_dismissed = true;

        if takes_argument(&name) {
            // Leave the caret after the command so arguments can be typed.
            self.editor
                .update(cx, |editor, cx| editor.set_text(format!("{name} "), cx));
        } else if run {
            self.editor.update(cx, |editor, cx| editor.clear(cx));
            self.dispatch(name, cx);
        } else {
            self.editor
                .update(cx, |editor, cx| editor.set_text(name, cx));
        }
        cx.notify();
    }

    /// The editor that editing actions should act on.
    fn focused_editor(&self, window: &Window, cx: &App) -> Entity<Editor> {
        let modal_wants_input = self
            .modal
            .as_ref()
            .map(|modal| modal.wants_input)
            .unwrap_or(false);
        if modal_wants_input && self.modal_editor.read(cx).focus_handle.is_focused(window) {
            self.modal_editor.clone()
        } else if self.model_menu && self.model_search.read(cx).focus_handle.is_focused(window) {
            self.model_search.clone()
        } else {
            self.editor.clone()
        }
    }

    /// Paste the clipboard over the selection in the focused editor.
    pub fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut item = cx.read_from_clipboard().unwrap_or(ClipboardItem { entries: Vec::new() });
        let target = self.focused_editor(window, cx);
        if target == self.editor {
            if let Some(paths) = item.text().as_deref().and_then(clipboard_paths) {
                item = ClipboardItem { entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(paths.into()))] };
            }
        }
        if target == self.editor && self.paste_loading {
            return;
        }
        if target == self.editor && !self.paste_loading
            && (item.entries().is_empty() || item.entries().iter().any(|entry| matches!(entry, ClipboardEntry::Image(_) | ClipboardEntry::ExternalPaths(_))))
        {
            self.paste_loading = true;
            cx.notify();
            cx.spawn(async move |this, cx| {
                let result = cx.background_executor().spawn(async move {
                    // GPUI's Linux clipboard backend does not request URI-list
                    // MIME types. Optional desktop utilities fill that gap.
                    if item.entries().is_empty() {
                        let output = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                            std::process::Command::new("wl-paste").args(["--no-newline", "--type", "text/uri-list"]).output()
                        } else {
                            std::process::Command::new("xclip").args(["-selection", "clipboard", "-out", "-target", "text/uri-list"]).output()
                        };
                        if let Ok(output) = output {
                            if output.status.success() {
                                if let Ok(text) = String::from_utf8(output.stdout) {
                                    if let Some(paths) = clipboard_paths(&text) {
                                        item.entries.push(ClipboardEntry::ExternalPaths(ExternalPaths(paths.into())));
                                    }
                                }
                            }
                        }
                    }
                    let mut images = Vec::new();
                    let mut paths = Vec::new();
                    let mut errors = Vec::new();
                    for entry in item.into_entries() {
                        match entry {
                            ClipboardEntry::Image(image) => {
                                match clipboard_image(image.format(), image.bytes()) {
                                    Ok(value) => images.push(("imagem colada".into(), value)),
                                    Err(error) => errors.push(error),
                                }
                            }
                            ClipboardEntry::ExternalPaths(files) => for path in files.0 {
                                let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
                                let format = match extension.as_str() {
                                    "png" => Some(ImageFormat::Png),
                                    "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
                                    "gif" => Some(ImageFormat::Gif),
                                    "webp" => Some(ImageFormat::Webp),
                                    _ => None,
                                };
                                if let Some(format) = format {
                                    use std::io::Read;
                                    let bytes = std::fs::File::open(&path).and_then(|file| {
                                        let mut bytes = Vec::new();
                                        file.take(10 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                                        Ok(bytes)
                                    });
                                    match bytes.map_err(|e| e.to_string()).and_then(|bytes| clipboard_image(format, &bytes)) {
                                        Ok(value) => images.push((path.file_name().unwrap_or_default().to_string_lossy().into_owned(), value)),
                                        Err(error) => errors.push(format!("{}: {error}", path.display())),
                                    }
                                } else {
                                    paths.push(format!("{:?}", path.to_string_lossy()));
                                }
                            },
                            ClipboardEntry::String(_) => {}
                        }
                    }
                    (images, paths, errors)
                }).await;
                let _ = this.update(cx, |state, cx| {
                    state.paste_loading = false;
                    state.pasted_images.extend(result.0);
                    if !result.1.is_empty() {
                        state.editor.update(cx, |editor, cx| editor.insert_text(&result.1.join("\n"), cx));
                    }
                    for error in result.2 {
                        state.push_toast(error, Tone::Warning, cx);
                    }
                    cx.notify();
                });
            }).detach();
            return;
        }
        let Some(text) = item.text() else { return; };
        if text.is_empty() {
            return;
        }
        target.update(cx, |editor, cx| editor.insert_text(&text, cx));
        self.slash_dismissed = false;
        cx.notify();
    }

    /// Copy the selection, or the whole prompt when nothing is selected.
    pub fn copy_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.focused_editor(window, cx);
        let text = editor
            .read(cx)
            .selected_text(cx)
            .unwrap_or_else(|| editor.read(cx).text(cx));
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Copy and remove the selection.
    pub fn cut_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.focused_editor(window, cx);
        let Some(text) = editor.read(cx).selected_text(cx) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        editor.update(cx, |editor, cx| editor.delete_selection(cx));
        cx.notify();
    }
}

fn clipboard_paths(text: &str) -> Option<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        if matches!(line, "copy" | "cut") && paths.is_empty() {
            continue;
        }
        let uri = url::Url::parse(line).ok()?;
        paths.push(uri.to_file_path().ok()?);
    }
    (!paths.is_empty()).then_some(paths)
}

fn clipboard_image(format: ImageFormat, bytes: &[u8]) -> Result<Value, String> {
    if bytes.is_empty() || bytes.len() > 10 * 1024 * 1024 {
        return Err("A imagem deve ter conteúdo e no máximo 10 MiB.".into());
    }
    let mime = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Gif => "image/gif",
        ImageFormat::Webp => "image/webp",
        _ => return Err("Cole uma imagem PNG, JPEG, GIF ou WebP.".into()),
    };
    Ok(json!({"type": "image", "mimeType": mime, "data": base64::engine::general_purpose::STANDARD.encode(bytes)}))
}

/// Advance through the levels reported by Pi, wrapping after the last.
/// Nível de esforço a usar quando a escada de um modelo chega: mantém o atual
/// se ainda for válido; senão cai para `off` (ou o primeiro disponível).
pub fn reconcile_effort_level<'a>(
    levels: &'a [String],
    current: Option<&str>,
) -> Option<&'a str> {
    if levels.is_empty() {
        return None;
    }
    if let Some(current) = current {
        if let Some(level) = levels.iter().find(|level| level.as_str() == current) {
            return Some(level.as_str());
        }
    }
    levels
        .iter()
        .find(|level| level.as_str() == "off")
        .or_else(|| levels.first())
        .map(String::as_str)
}

fn next_effort_level<'a>(levels: &'a [String], current: Option<&str>) -> Option<&'a str> {
    if levels.is_empty() {
        return None;
    }
    let next = current
        .and_then(|current| levels.iter().position(|level| level == current))
        .map(|index| (index + 1) % levels.len())
        .unwrap_or(0);
    Some(levels[next].as_str())
}

#[cfg(test)]
mod effort_tests {
    use super::next_effort_level;

    #[test]
    fn reconcile_effort_keeps_valid_and_falls_back_deterministically() {
        let levels = vec!["off".to_string(), "low".to_string(), "high".to_string()];
        assert_eq!(super::reconcile_effort_level(&levels, Some("low")), Some("low"));
        assert_eq!(super::reconcile_effort_level(&levels, Some("max")), Some("off"));
        assert_eq!(super::reconcile_effort_level(&levels, None), Some("off"));
        assert_eq!(super::reconcile_effort_level(&[], Some("low")), None);
        let without_off = vec!["low".to_string(), "high".to_string()];
        assert_eq!(
            super::reconcile_effort_level(&without_off, Some("max")),
            Some("low")
        );
    }

    #[test]
    fn image_clipboard_uses_pi_image_content_contract() {
        let payload = super::clipboard_image(gpui::ImageFormat::Png, &[1, 2, 3]).unwrap();
        assert_eq!(payload["type"], "image");
        assert_eq!(payload["mimeType"], "image/png");
        assert_eq!(payload["data"], "AQID");
        assert!(super::clipboard_image(gpui::ImageFormat::Png, &[]).is_err());
        assert!(super::clipboard_image(gpui::ImageFormat::Svg, b"<svg/>").is_err());
        assert!(super::clipboard_image(gpui::ImageFormat::Png, &vec![0; 10 * 1024 * 1024 + 1]).is_err());
    }

    #[test]
    fn file_uris_are_decoded_without_reinterpreting_normal_text() {
        let paths = super::clipboard_paths("copy\nfile:///tmp/a%20b.png\r\nfile:///tmp/file.txt").unwrap();
        assert_eq!(paths, vec![std::path::PathBuf::from("/tmp/a b.png"), std::path::PathBuf::from("/tmp/file.txt")]);
        assert!(super::clipboard_paths("some normal text").is_none());
        assert!(super::clipboard_paths("https://example.com/image.png").is_none());
        assert!(super::clipboard_paths("file://remote/tmp/a").is_none());
    }

    #[test]
    fn scrolling_up_disengages_tail_even_with_unmeasured_messages() {
        let list = gpui::ListState::new(20, gpui::ListAlignment::Top, gpui::px(600.));
        list.set_follow_mode(gpui::FollowMode::Tail);
        assert!(list.is_following_tail());
        assert_eq!(list.is_scrolled_to_end(), None);
        list.scroll_to(gpui::ListOffset { item_ix: 2, offset_in_item: gpui::px(10.) });
        list.splice(20..20, 1);
        list.remeasure_items(20..21);
        assert!(!list.is_following_tail());
        assert_eq!(list.logical_scroll_top().item_ix, 2);
        assert_eq!(list.logical_scroll_top().offset_in_item, gpui::px(10.));
    }

    #[test]
    fn cycles_only_supported_levels_and_wraps() {
        let levels = vec!["low".into(), "high".into(), "max".into()];
        assert_eq!(next_effort_level(&levels, Some("low")), Some("high"));
        assert_eq!(next_effort_level(&levels, Some("high")), Some("max"));
        assert_eq!(next_effort_level(&levels, Some("max")), Some("low"));
        assert_eq!(next_effort_level(&levels, None), Some("low"));
        assert_eq!(next_effort_level(&levels, Some("off")), Some("low"));
    }

    #[test]
    fn handles_empty_and_single_level_lists() {
        assert_eq!(next_effort_level(&[], Some("high")), None);
        assert_eq!(next_effort_level(&["off".into()], Some("off")), Some("off"));
    }
}

/// Um turno, para a linha do tempo do inspetor.
pub struct TurnSummary {
    /// Número sequencial do turno (1 é o primeiro pedido).
    pub number: usize,
    pub prompt: String,
    pub duration: Option<String>,
    pub state: TurnState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TurnState {
    Running,
    Done,
    Failed,
    /// Interrompido pelo usuário (`esc`).
    Stopped,
}

/// Um arquivo tocado no turno em curso.
pub struct TouchedFile {
    pub path: String,
    pub edited: bool,
    pub status: ToolStatus,
}

/// Nível de esforço em português, para os controles.
pub fn effort_label(level: &str) -> &'static str {
    match level {
        "off" => "desligado",
        "minimal" => "mínimo",
        "low" => "baixo",
        "medium" => "médio",
        "high" => "alto",
        "xhigh" => "muito alto",
        "max" => "máximo",
        _ => "—",
    }
}

/// Commands that expect an argument, so accepting them leaves the composer
/// ready for one instead of firing immediately.
fn takes_argument(name: &str) -> bool {
    matches!(name, "/model" | "/name" | "/thinking")
}

/// Commands Dish handles itself.
pub const BUILTIN_COMMANDS: &[(&str, &str)] = &[
    // Ordered safest first: typing `/` and pressing Enter takes the top row, so
    // the commands that destroy the view come last.
    ("/help", "Keyboard shortcuts and commands"),
    ("/compact", "Summarise and shrink the context window"),
    ("/model", "Switch model"),
    ("/thinking", "Set the reasoning level"),
    ("/thinking-view", "Show or hide reasoning by default"),
    ("/auto-compact", "Toggle automatic compaction"),
    ("/export", "Write the session to an HTML file"),
    ("/name", "Name this session"),
    ("/sidebar", "Toggle the details rail"),
    ("/clear", "Clear the transcript view"),
    ("/new", "Start a fresh session"),
];

#[derive(Clone)]
pub enum ModalAnswer {
    Cancelled,
    Confirmed(bool),
    Value(Value),
}

// -------------------------------------------------------------------- helpers

fn now_ms() -> Option<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as i64)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn compact_number(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Flatten a message's `content` into text.
pub fn message_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => {
            let mut out = String::new();
            for item in items {
                match item.get("type").and_then(Value::as_str).unwrap_or_default() {
                    "text" => {
                        if let Some(text) = item.get("text").and_then(Value::as_str) {
                            if !out.is_empty() {
                                out.push('\n');
                            }
                            out.push_str(text);
                        }
                    }
                    "image" => {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str("[image]");
                    }
                    _ => {}
                }
            }
            out
        }
        _ => String::new(),
    }
}

/// Flatten a tool result's `content` into text.
fn tool_result_text(result: &Value) -> Option<String> {
    match result.get("content") {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Array(items)) => {
            let mut out = String::new();
            for item in items {
                match item.get("type").and_then(Value::as_str).unwrap_or_default() {
                    "text" => {
                        if let Some(text) = item.get("text").and_then(Value::as_str) {
                            if !out.is_empty() {
                                out.push('\n');
                            }
                            out.push_str(text);
                        }
                    }
                    "image" => {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str("[image]");
                    }
                    _ => {}
                }
            }
            Some(out)
        }
        Some(other) => Some(other.to_string()),
        None => None,
    }
}

/// A busca do seletor de modelos: id e provedor, sem diferenciar maiúsculas.
pub fn model_matches(model: &ModelInfo, query: &str) -> bool {
    let searchable = format!("{} {}", model.id, model.provider).to_lowercase();
    query
        .split_whitespace()
        .all(|term| searchable.contains(&term.to_lowercase()))
}

/// Format a token count for the status bar.
pub fn format_tokens(value: f64) -> String {
    compact_number(value.max(0.0) as u64)
}
