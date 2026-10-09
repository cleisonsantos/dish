//! Dish — a GPU-accelerated desktop interface for the Pi coding agent.
//!
//! Dish spawns `pi --mode rpc` as a child process, speaks its JSONL protocol,
//! and renders the conversation with GPUI. Nothing about Pi's behaviour is
//! reimplemented: the same session, tools, models and extensions run here as in
//! the terminal.

mod editor;
mod settings;
mod installation;
mod startup;
mod preferences;
mod markdown;
mod rpc;
mod state;
mod sessions;
mod workspace;
mod theme;
mod ui;

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::*;

use crate::editor::{
    Backspace, CopySelection, CutSelection, Delete, DeleteWordBack, DeleteWordForward, DocEnd,
    DocStart, End, Home, Left, Newline, Paste, Redo, Right, SelectAllText, SelectDocEnd,
    SelectDocStart, SelectDown, SelectEnd, SelectHome, SelectLeft, SelectRight, SelectUp,
    SelectWordLeft, SelectWordRight, SendPrompt, Undo, WordLeft, WordRight,
};
use crate::ui::{
    CloseWindow, CycleEffort, Dismiss, MenuAccept, MenuDown, MenuUp, ModalSubmit, NewSession,
    ToggleSidebar,
};

/// Everything a text field should answer to, bound for one key context.
///
/// Both `ctrl-` and `cmd-` spellings are registered so the same bindings work on
/// Linux, macOS and Windows without branching at runtime.
fn editor_bindings(context: &str) -> Vec<KeyBinding> {
    let ctx = Some(context);
    vec![
        // Caret movement.
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("shift-home", SelectHome, ctx),
        KeyBinding::new("shift-end", SelectEnd, ctx),
        KeyBinding::new("shift-up", SelectUp, ctx),
        KeyBinding::new("shift-down", SelectDown, ctx),
        KeyBinding::new("ctrl-home", DocStart, ctx),
        KeyBinding::new("ctrl-end", DocEnd, ctx),
        KeyBinding::new("cmd-up", DocStart, ctx),
        KeyBinding::new("cmd-down", DocEnd, ctx),
        KeyBinding::new("cmd-left", Home, ctx),
        KeyBinding::new("cmd-right", End, ctx),
        KeyBinding::new("ctrl-shift-home", SelectDocStart, ctx),
        KeyBinding::new("ctrl-shift-end", SelectDocEnd, ctx),
        // Word-wise movement.
        KeyBinding::new("ctrl-left", WordLeft, ctx),
        KeyBinding::new("ctrl-right", WordRight, ctx),
        KeyBinding::new("alt-left", WordLeft, ctx),
        KeyBinding::new("alt-right", WordRight, ctx),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, ctx),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, ctx),
        KeyBinding::new("alt-shift-left", SelectWordLeft, ctx),
        KeyBinding::new("alt-shift-right", SelectWordRight, ctx),
        // Editing.
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("ctrl-backspace", DeleteWordBack, ctx),
        KeyBinding::new("alt-backspace", DeleteWordBack, ctx),
        KeyBinding::new("ctrl-delete", DeleteWordForward, ctx),
        KeyBinding::new("alt-delete", DeleteWordForward, ctx),
        KeyBinding::new("ctrl-a", SelectAllText, ctx),
        KeyBinding::new("cmd-a", SelectAllText, ctx),
        KeyBinding::new("ctrl-z", Undo, ctx),
        KeyBinding::new("cmd-z", Undo, ctx),
        KeyBinding::new("ctrl-shift-z", Redo, ctx),
        KeyBinding::new("cmd-shift-z", Redo, ctx),
        KeyBinding::new("ctrl-y", Redo, ctx),
        // Clipboard.
        KeyBinding::new("ctrl-c", CopySelection, ctx),
        KeyBinding::new("cmd-c", CopySelection, ctx),
        KeyBinding::new("ctrl-x", CutSelection, ctx),
        KeyBinding::new("cmd-x", CutSelection, ctx),
        KeyBinding::new("ctrl-v", Paste, ctx),
        KeyBinding::new("ctrl-shift-v", Paste, ctx),
        KeyBinding::new("cmd-v", Paste, ctx),
    ]
}

struct Root(Entity<startup::Startup>);

/// Ícones embutidos no binário: o app não depende de arquivos soltos.
struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        match path {
            "dish-icon.png" => Ok(Some(std::borrow::Cow::Borrowed(include_bytes!(
                "../assets/dish-icon.png"
            )))),
            "dish-mark.svg" => Ok(Some(std::borrow::Cow::Borrowed(include_bytes!(
                "../assets/dish-mark.svg"
            )))),
            _ => Ok(None),
        }
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .on_action(cx.listener(|this, _: &CloseWindow, window, cx| {
                let allowed = this
                    .0
                    .update(cx, |startup, cx| startup.can_close(cx));
                if allowed {
                    window.remove_window();
                }
            }))
            .child(self.0.clone())
    }
}

/// Pi options that consume the following argument, so `dish --model foo` keeps
/// `foo` with the flag instead of treating it as prompt text.
const PI_FLAGS_WITH_VALUES: &[&str] = &[
    "--provider", "--model", "--api-key", "--system-prompt", "--append-system-prompt", "--mode",
    "--session", "--session-id", "--fork", "--session-dir", "-n", "--name", "--models", "--tools",
    "-t", "--exclude-tools", "-xt", "--thinking", "--extension", "-e", "--skill",
    "--prompt-template", "--theme", "--use-theme", "--export", "--tui-mode",
];

fn main() {
    // `DISH_BACKEND=x11` força o XWayland no Linux: é o caminho para ter as
    // decorações do sistema onde o compositor Wayland não as fornece (GNOME).
    // Ignorado em macOS e Windows.
    if cfg!(target_os = "linux")
        && std::env::var("DISH_BACKEND").is_ok_and(|value| value.eq_ignore_ascii_case("x11"))
    {
        std::env::remove_var("WAYLAND_DISPLAY");
    }

    // `dish [pi-flags] [path] [prompt...]`
    //
    // Leading options are handed straight to Pi (`--continue`, `--model`, …),
    // an existing directory selects the project folder, and the remaining words
    // are sent as the first prompt.
    let preferences = preferences::load();
    let restore_last = std::env::args_os().len() == 1;
    let mut pi_flags: Vec<String> = Vec::new();
    let mut cwd: Option<PathBuf> = None;
    let mut words: Vec<String> = Vec::new();
    let mut arguments = std::env::args().skip(1).peekable();
    while let Some(argument) = arguments.next() {
        let still_leading = cwd.is_none() && words.is_empty();
        if still_leading && argument.starts_with('-') {
            let takes_value = PI_FLAGS_WITH_VALUES.contains(&argument.as_str());
            pi_flags.push(argument);
            if takes_value {
                // `--model foo`, not `--model=foo`
                if let Some(value) = arguments.peek() {
                    if !value.starts_with('-') {
                        let value = arguments.next().unwrap_or_default();
                        pi_flags.push(value);
                    }
                }
            }
        } else if still_leading && PathBuf::from(&argument).is_dir() {
            cwd = Some(PathBuf::from(&argument));
        } else {
            words.push(argument);
        }
    }
    if restore_last {
        if let Some(project) = preferences.last_project.as_ref().filter(|p| p.is_dir()) {
            cwd = Some(project.clone());
            if let Some(session) = preferences.last_session.as_ref().filter(|p| p.is_file()) {
                pi_flags.extend(["--session".into(), session.to_string_lossy().into_owned()]);
            }
        }
    }
    let cwd = cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let initial_prompt: Option<String> = {
        let trimmed = words.join(" ").trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    };

    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
        let cwd = cwd.clone();
        let initial_prompt = initial_prompt.clone();
        let pi_flags = pi_flags.clone();
        let mut keys = editor_bindings("DishInput");
        keys.extend([
            KeyBinding::new("ctrl-c", CopySelection, Some("DishSelection")),
            KeyBinding::new("cmd-c", CopySelection, Some("DishSelection")),
            KeyBinding::new("ctrl-a", SelectAllText, Some("DishSelection")),
            KeyBinding::new("cmd-a", SelectAllText, Some("DishSelection")),
        ]);
        keys.extend(editor_bindings("DishModal"));
        keys.extend(editor_bindings("DishModelSearch"));
        keys.extend(editor_bindings("DishNavSearch"));
        keys.extend(editor_bindings("DishHelpSearch"));
        keys.extend([
            // The composer owns up/down so the slash menu can take them over;
            // the menu handler forwards to the caret when it is closed.
            KeyBinding::new("up", MenuUp, Some("DishInput")),
            KeyBinding::new("down", MenuDown, Some("DishInput")),
            // No seletor de modelos as setas percorrem a lista e Enter escolhe.
            KeyBinding::new("up", ui::ModelUp, Some("DishModelSearch")),
            KeyBinding::new("down", ui::ModelDown, Some("DishModelSearch")),
            KeyBinding::new("enter", ui::ModelAccept, Some("DishModelSearch")),
            // Na busca de sessões as setas destacam e Enter abre.
            KeyBinding::new("up", workspace::NavUp, Some("DishNavSearch")),
            KeyBinding::new("down", workspace::NavDown, Some("DishNavSearch")),
            KeyBinding::new("enter", workspace::NavAccept, Some("DishNavSearch")),
            KeyBinding::new("up", ui::ModalUp, Some("DishModal")),
            KeyBinding::new("down", ui::ModalDown, Some("DishModal")),
            KeyBinding::new("enter", SendPrompt, Some("DishInput")),
            KeyBinding::new("shift-enter", Newline, Some("DishInput")),
            KeyBinding::new("enter", ModalSubmit, Some("DishModal")),
            KeyBinding::new("shift-enter", Newline, Some("DishModal")),
            KeyBinding::new("tab", MenuAccept, Some("DishInput")),
            KeyBinding::new("escape", Dismiss, None),
            KeyBinding::new("ctrl-b", ToggleSidebar, None),
            KeyBinding::new("ctrl-n", NewSession, None),
            KeyBinding::new("ctrl-shift-e", CycleEffort, None),
            KeyBinding::new("ctrl-shift-b", workspace::ToggleSessions, None),
            KeyBinding::new("ctrl-tab", workspace::NextConversation, None),
            KeyBinding::new("ctrl-shift-tab", workspace::PreviousConversation, None),
            KeyBinding::new("ctrl-w", workspace::CloseConversation, None),
            KeyBinding::new("ctrl-k", workspace::SearchSessions, None),
            KeyBinding::new("ctrl-l", workspace::FocusPrompt, None),
            KeyBinding::new("ctrl-shift-m", ui::OpenModels, None),
            KeyBinding::new("ctrl-q", ui::CloseWindow, None),
            KeyBinding::new("cmd-q", ui::CloseWindow, None),
            KeyBinding::new("f1", ui::ToggleHelp, None),
        ]);
        cx.bind_keys(keys);

        let [width, height] = preferences.window_size
            .filter(|s| s.iter().all(|v| v.is_finite()) && s[0] >= 720. && s[1] >= 520.)
            .unwrap_or([1240., 860.]);
        let bounds = Bounds::centered(None, size(px(width.min(3840.)), px(height.min(2160.))), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(720.), px(520.))),
            titlebar: Some(TitlebarOptions {
                title: Some("Dish".into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            window_background: WindowBackgroundAppearance::Opaque,
            app_id: Some("dish".to_string()),
            ..Default::default()
        };

        cx.open_window(options, move |window, cx| {
            window.set_window_title("Dish");
            let root = cx.new(|cx| startup::Startup::new(cwd.clone(), pi_flags.clone(), initial_prompt.clone(), window, cx));
            let weak = root.downgrade();
            window.on_window_should_close(cx, move |_, cx| {
                weak.update(cx, |root, cx| root.can_close(cx)).unwrap_or(true)
            });
            root.update(cx, |root, cx| {
                root.focus(window, cx);
                root.start(cx);
            });
            window.refresh();
            cx.new(|_| Root(root))
        })
        .expect("failed to open the Dish window");

        cx.activate(true);
    });
}
