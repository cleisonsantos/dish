//! Dish — a GPU-accelerated desktop interface for the Pi coding agent.
//!
//! Dish spawns `pi --mode rpc` as a child process, speaks its JSONL protocol,
//! and renders the conversation with GPUI. Nothing about Pi's behaviour is
//! reimplemented: the same session, tools, models and extensions run here as in
//! the terminal.
//!
//! No Windows, o release usa o subsistema GUI: sem isso o `dish.exe` abre um
//! console junto com a janela (issue #14). Os filhos escondem o próprio console
//! via `platform::hide_console`, e `logging::warn` escreve em stderr sem pânico.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod mcp_registry;
mod mcp_server;
mod demand;
mod limits;
mod logging;
mod platform;
mod process;
mod fs_security;
mod editor;
mod settings;
mod installation;
mod pi_update;
mod providers;
mod provider_login;
mod startup;
mod preferences;
mod notifications;
mod version;
mod update;
mod markdown;
mod metadata;
mod rpc;
mod state;
mod model_sync;
mod sessions;
mod content_search;
mod git_context;
mod session_activity;
mod sound;
mod workspace;
mod theme;
mod ui;

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::*;

use crate::editor::{
    Backspace, CopySelection, CutSelection, Delete, DeleteWordBack, DeleteWordForward, DocEnd,
    DocStart, End, Home, Left, Newline, PageDown, PageUp, Paste, Redo, Right, SelectAllText,
    SelectDocEnd, SelectDocStart, SelectDown, SelectEnd, SelectHome, SelectLeft, SelectRight,
    SelectUp, SelectWordLeft, SelectWordRight, SendPrompt, Undo, WordLeft, WordRight,
};
use crate::ui::{
    CloseWindow, CycleEffort, Dismiss, MenuAccept, MenuDown, MenuUp, ModalSubmit, NewSession,
    PreviousFocus, ToggleSidebar,
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
        KeyBinding::new("pageup", PageUp, ctx),
        KeyBinding::new("pagedown", PageDown, ctx),
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
    "--session", "--session-id", "--fork", "--session-dir", "--agent-dir", "-n", "--name", "--models",
    "--tools", "-t", "--exclude-tools", "-xt", "--thinking", "--extension", "-e", "--skill",
    "--prompt-template", "--theme", "--use-theme", "--export", "--tui-mode",
];

const PI_FLAGS_WITHOUT_REQUIRED_VALUES: &[&str] = &[
    "--print", "-p", "--continue", "-c", "--resume", "-r", "--no-session", "--no-tools", "-nt",
    "--no-builtin-tools", "-nbt", "--no-extensions", "-ne", "--no-mcp", "--no-skills", "-ns",
    "--no-prompt-templates", "-np", "--no-themes", "--no-context-files", "-nc", "--list-models",
    "--verbose", "--approve", "-a", "--no-approve", "-na", "--offline", "--",
];

const CLI_HELP: &str = "\
Uso: dish [opções do Pi] [diretório] [prompt...]
     dish --install-mcp|--uninstall-mcp [--project]
     dish --mcp-serve

Opções do Dish:
  -h, --help             Mostra esta ajuda
  -v, --version          Mostra a versão

Opções do Pi incluem --session <arquivo>, --continue, --no-session e --model <modelo>.
";

/// Um argumento com cara de caminho: absoluto, `~`, relativo explícito ou com
/// separador. Só vale antes do primeiro prompt, para que texto comum continue
/// sendo texto.
fn looks_like_path(argument: &str) -> bool {
    argument.starts_with('/')
        || argument.starts_with('~')
        || argument.starts_with("./")
        || argument.starts_with("../")
        || argument.contains('/')
        || (cfg!(windows) && argument.contains('\\'))
}

enum CliAction {
    Help,
    Version,
    Launch(LaunchArgs),
}

struct LaunchArgs {
    pi_flags: Vec<String>,
    cwd: Option<PathBuf>,
    words: Vec<String>,
    notice: Option<String>,
}

fn parse_cli(mut arguments: impl Iterator<Item = String>) -> Result<CliAction, String> {
    let mut pi_flags: Vec<String> = Vec::new();
    let mut cwd: Option<PathBuf> = None;
    let mut words: Vec<String> = Vec::new();
    let mut notice: Option<String> = None;
    while let Some(argument) = arguments.next() {
        let still_leading = cwd.is_none() && words.is_empty();
        if still_leading && argument.starts_with('-') {
            match argument.as_str() {
                "--help" | "-h" => return Ok(CliAction::Help),
                "--version" | "-v" => return Ok(CliAction::Version),
                _ => {}
            }
            let inline_value = argument.split_once('=');
            let flag = inline_value.map_or(argument.as_str(), |(flag, _)| flag);
            let takes_value = PI_FLAGS_WITH_VALUES.contains(&flag);
            if !takes_value && !PI_FLAGS_WITHOUT_REQUIRED_VALUES.contains(&argument.as_str()) {
                return Err(argument);
            }
            let separate_value = takes_value && inline_value.is_none();
            pi_flags.push(argument);
            if separate_value {
                pi_flags.extend(arguments.next());
            }
        } else if still_leading && PathBuf::from(&argument).is_dir() {
            cwd = Some(PathBuf::from(&argument));
        } else if still_leading && looks_like_path(&argument) {
            // Um caminho que não existe não vira prompt em silêncio. O resto da
            // linha era um pedido para aquela pasta, então não é enviado em
            // outra; a janela avisa e segue com a pasta atual.
            notice = Some(format!(
                "A pasta “{argument}” não existe ou não é um diretório. O Dish abriu a pasta atual; use “Abrir projeto…” para escolher outra."
            ));
            break;
        } else {
            words.push(argument);
        }
    }
    Ok(CliAction::Launch(LaunchArgs {
        pi_flags,
        cwd,
        words,
        notice,
    }))
}

fn main() {
    if let Some(code) = mcp_command() {
        std::process::exit(code);
    }

    let arguments = std::env::args();
    let restore_last = arguments.len() == 1;
    let LaunchArgs {
        mut pi_flags,
        mut cwd,
        words,
        notice,
    } = match parse_cli(arguments.skip(1)) {
        Ok(CliAction::Help) => {
            print!("{CLI_HELP}");
            return;
        }
        Ok(CliAction::Version) => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Ok(CliAction::Launch(launch)) => launch,
        Err(argument) => {
            eprintln!("dish: argumento desconhecido: {argument}");
            std::process::exit(2);
        }
    };

    logging::init();
    // `DISH_TEST_PANIC` existe só para o smoke de privacidade dos diagnósticos
    // (tests/diagnostic_privacy_smoke.py), no mesmo espírito de `DISH_TEST_UI_LOG`:
    // provoca um pânico numa thread nomeada antes do GPUI subir e encerra, para
    // conferir o relatório redigido em `crash.log`.
    if let Ok(value) = std::env::var("DISH_TEST_PANIC") {
        let spawned = std::thread::Builder::new()
            .name("dish-test-panic".into())
            .spawn(move || panic!("dish: pânico de teste: {value}"));
        if let Ok(handle) = spawned {
            let _ = handle.join();
        }
        std::process::exit(101);
    }
    ui::test_probe::init();
    // `DISH_BACKEND=x11` força o XWayland no Linux: é o caminho para ter as
    // decorações do sistema onde o compositor Wayland não as fornece (GNOME).
    // Ignorado em macOS e Windows.
    if cfg!(target_os = "linux")
        && std::env::var("DISH_BACKEND").is_ok_and(|value| value.eq_ignore_ascii_case("x11"))
    {
        std::env::remove_var("WAYLAND_DISPLAY");
    }

    let preferences = preferences::load();
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
        cx.set_app_identity("dish", "Dish");
        // Fontes da identidade (OFL-1.1, ver assets/fonts/README.md). Sem elas
        // a resolução abaixo cai nas famílias instaladas no sistema.
        if let Err(error) = cx.text_system().add_fonts(theme::bundled_fonts()) {
            eprintln!("dish: fontes embutidas indisponíveis: {error:#}");
        }
        theme::resolve_fonts(&cx.text_system().all_font_names());
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
        keys.extend(editor_bindings("DishContentSearch"));
        keys.extend(editor_bindings("DishHelpSearch"));
        keys.extend(editor_bindings("DishSettings"));
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
            // Esquerda/direita recolhem e expandem o projeto destacado.
            KeyBinding::new("left", workspace::NavCollapse, Some("DishNavSearch")),
            KeyBinding::new("right", workspace::NavExpand, Some("DishNavSearch")),
            KeyBinding::new("up", ui::ModalUp, Some("DishModal")),
            KeyBinding::new("down", ui::ModalDown, Some("DishModal")),
            // GPUI keymap.rs: binding_enabled returns contexts.len() for globals, so global ctrl-f wins.
            KeyBinding::new("alt-f", ui::FindInComposer, Some("DishInput")),
            KeyBinding::new("f3", ui::NextComposerMatch, Some("DishInput")),
            KeyBinding::new("shift-f3", ui::PreviousComposerMatch, Some("DishInput")),
            KeyBinding::new("enter", SendPrompt, Some("DishInput")),
            KeyBinding::new("shift-enter", Newline, Some("DishInput")),
            KeyBinding::new("enter", ModalSubmit, Some("DishModal")),
            KeyBinding::new("shift-enter", Newline, Some("DishModal")),
            KeyBinding::new("tab", MenuAccept, Some("DishInput")),
            KeyBinding::new("shift-tab", PreviousFocus, Some("DishInput")),
            KeyBinding::new("shift-alt-down", ui::DuplicateLineDown, Some("DishInput")),
            KeyBinding::new("shift-alt-up", ui::DuplicateLineUp, Some("DishInput")),
            KeyBinding::new("alt-shift-e", ui::ToggleComposer, Some("DishInput")),
            KeyBinding::new("escape", Dismiss, None),
            KeyBinding::new("ctrl-b", ToggleSidebar, None),
            KeyBinding::new("ctrl-n", NewSession, None),
            KeyBinding::new("ctrl-shift-e", CycleEffort, None),
            KeyBinding::new("ctrl-shift-b", workspace::ToggleSessions, None),
            KeyBinding::new("ctrl-alt-f", workspace::CycleSessionFilter, None),
            KeyBinding::new("ctrl-tab", workspace::NextConversation, None),
            KeyBinding::new("ctrl-shift-tab", workspace::PreviousConversation, None),
            KeyBinding::new("ctrl-w", workspace::CloseConversation, None),
            KeyBinding::new("ctrl-k", workspace::SearchSessions, None),
            KeyBinding::new("ctrl-f", workspace::FindCurrent, None),
            KeyBinding::new("ctrl-shift-f", workspace::FindProject, None),
            KeyBinding::new("escape", workspace::ExitSearch, Some("DishNavSearch")),
            KeyBinding::new("tab", workspace::NavDown, Some("DishNavSearch")),
            KeyBinding::new("shift-tab", workspace::NavUp, Some("DishNavSearch")),
            KeyBinding::new("escape", workspace::ExitSearch, Some("DishContentSearch")),
            KeyBinding::new("tab", workspace::NextMatch, Some("DishContentSearch")),
            KeyBinding::new("shift-tab", workspace::PreviousMatch, Some("DishContentSearch")),
            KeyBinding::new("down", workspace::NextMatch, Some("DishContentSearch")),
            KeyBinding::new("up", workspace::PreviousMatch, Some("DishContentSearch")),
            KeyBinding::new("enter", workspace::AcceptMatch, Some("DishContentSearch")),
            KeyBinding::new("shift-enter", workspace::AcceptMatch, Some("DishContentSearch")),
            KeyBinding::new("ctrl-l", workspace::FocusPrompt, None),
            KeyBinding::new("ctrl-shift-m", ui::OpenModels, None),
            KeyBinding::new("alt-shift-e", ui::ToggleComposer, None),
            KeyBinding::new("ctrl-q", ui::CloseWindow, None),
            KeyBinding::new("cmd-q", ui::CloseWindow, None),
            KeyBinding::new("f1", ui::ToggleHelp, None),
        ]);
        cx.bind_keys(keys);

        // A preferência de aparência vale desde a primeira pintura; o render
        // resolve o modo Sistema a cada quadro.
        theme::set_mode(preferences.appearance);
        theme::set_theme(preferences.theme.clone());
        if let Some(reduce_motion) = preferences.reduce_motion {
            cx.set_reduce_motion(reduce_motion);
        }

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
            // No Linux a barra integrada é desenhada pelo Dish, como no Zed.
            // No Windows o backend pode ou não criar a legenda nativa; a
            // decisão é tomada em tempo de execução, pelo estilo real do
            // HWND, em `ui::window_frame::client_side`.
            window_decorations: cfg!(target_os = "linux")
                .then_some(WindowDecorations::Client),
            window_background: WindowBackgroundAppearance::Opaque,
            app_id: Some("dish".to_string()),
            ..Default::default()
        };

        cx.open_window(options, move |window, cx| {
            window.set_window_title("Dish");
            // Modo Sistema: quando o desktop troca de tema, a janela repinta.
            window
                .observe_window_appearance(|window, _| window.refresh())
                .detach();
            let root = cx.new(|cx| startup::Startup::new(cwd.clone(), pi_flags.clone(), initial_prompt.clone(), notice.clone(), window, cx));
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

/// MCP commands are consumed before any GUI or Pi argument handling.
fn mcp_command() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let commands = ["--install-mcp", "--uninstall-mcp", "--mcp-serve"];
    let command = args
        .iter()
        .find_map(|arg| commands.iter().find(|flag| arg == *flag).copied())?;
    let result = (|| -> anyhow::Result<()> {
        if std::env::consts::OS != "linux" {
            anyhow::bail!("MCP do Dish ainda não é suportado no Windows/macOS; use Linux.");
        }
        let project = args.iter().any(|arg| arg == "--project");
        if args.len() != 1 + usize::from(project) || (command == "--mcp-serve" && project) {
            anyhow::bail!("Argumentos inválidos para o comando MCP.");
        }
        if command == "--mcp-serve" {
            return mcp_server::serve().map_err(Into::into);
        }
        let path = if project {
            std::env::current_dir()?.join(".pi/mcp.json")
        } else {
            let home = std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("HOME não está definido."))?;
            PathBuf::from(home).join(".pi/agent/mcp.json")
        };
        let outcome = if command == "--install-mcp" {
            mcp_registry::install(&path)?
        } else {
            mcp_registry::uninstall(&path)?
        };
        println!(
            "MCP do Dish: {}.{}",
            match outcome {
                mcp_registry::Outcome::Changed => "registro atualizado",
                mcp_registry::Outcome::Unchanged => "registro já está correto",
            },
            if project {
                " O Pi só lê este registro após confiar no projeto."
            } else {
                ""
            }
        );
        Ok(())
    })();
    Some(match result {
        Ok(()) => 0,
        Err(error) => {
            if command == "--mcp-serve" {
                eprintln!("MCP do Dish: {error}");
            } else {
                println!("MCP do Dish: {error}");
            }
            1
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{looks_like_path, parse_cli, CliAction};
    use std::path::Path;

    #[test]
    fn cli_handles_its_own_flags_and_rejects_unknown_options() {
        for flag in ["--help", "-h"] {
            assert!(matches!(
                parse_cli(std::iter::once(flag.to_owned())),
                Ok(CliAction::Help)
            ));
        }
        for flag in ["--version", "-v"] {
            assert!(matches!(
                parse_cli(std::iter::once(flag.to_owned())),
                Ok(CliAction::Version)
            ));
        }
        assert!(matches!(
            parse_cli(["--continue", "--xyz"].map(str::to_owned).into_iter()),
            Err(flag) if flag == "--xyz"
        ));
    }

    #[test]
    fn cli_keeps_project_and_session_launch_arguments() {
        for (args, expected_pi_flags) in [
            (&["."][..], &[][..]),
            (
                &["--session", "saved session.jsonl", "."],
                &["--session", "saved session.jsonl"],
            ),
            (
                &["--session=saved session.jsonl", "."],
                &["--session=saved session.jsonl"],
            ),
            (
                &["--session", "-session.jsonl", "."],
                &["--session", "-session.jsonl"],
            ),
        ] {
            let Ok(CliAction::Launch(launch)) = parse_cli(args.iter().copied().map(str::to_owned))
            else {
                panic!("expected a graphical launch for {args:?}");
            };
            assert_eq!(launch.cwd.as_deref(), Some(Path::new(".")));
            assert_eq!(launch.pi_flags, expected_pi_flags);
            assert!(launch.words.is_empty());
            assert!(launch.notice.is_none());
        }
    }

    #[test]
    fn path_like_arguments_are_recognized() {
        for value in ["/tmp/projeto", "~/projetos/novo", "./app", "../app", "pasta/app", "/"] {
            assert!(looks_like_path(value), "{value}");
        }
        for value in ["explique", "este", "código", "--continue", "fix the tests"] {
            assert!(!looks_like_path(value), "{value}");
        }
    }
}
