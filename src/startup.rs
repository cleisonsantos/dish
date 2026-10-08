//! Interactive onboarding; Pi remains owned by the workspace, never a daemon.
use crate::{
    editor::Editor,
    installation,
    rpc::{PiClient, PiConfig},
    state::AppState,
    theme,
    workspace::Workspace,
};
use gpui::{prelude::*, *};
use std::path::PathBuf;

pub struct Startup {
    cwd: PathBuf,
    flags: Vec<String>,
    prompt: Option<String>,
    busy: bool,
    message: String,
    prerequisites: bool,
    ready: Option<Entity<Workspace>>,
    focus: FocusHandle,
    window: AnyWindowHandle,
}

impl Startup {
    pub fn new(
        cwd: PathBuf,
        flags: Vec<String>,
        prompt: Option<String>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            cwd,
            flags,
            prompt,
            busy: false,
            message: String::new(),
            prerequisites: false,
            ready: None,
            focus: cx.focus_handle(),
            window: window.window_handle(),
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.detect(None, false, cx);
    }

    pub fn can_close(&mut self, cx: &mut Context<Self>) -> bool {
        if self.busy {
            self.message =
                "Aguarde a verificação ou instalação terminar antes de fechar o app.".into();
            cx.notify();
            return false;
        }
        true
    }

    fn detect(&mut self, chosen: Option<String>, install: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if (chosen.is_some() || install) && std::env::var_os("DISH_PI_BIN").is_some() {
            self.message = "DISH_PI_BIN está definido. Corrija ou remova esse override antes de escolher outra instalação.".into();
            cx.notify();
            return;
        }
        self.busy = true;
        self.message = if install {
            "Instalando Pi… Isso pode levar alguns minutos. Aguarde antes de fechar o app."
        } else {
            "Verificando instalação do Pi…"
        }
        .into();
        let cwd = self.cwd.clone();
        let flags = self.flags.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (result, prerequisites) = cx.background_executor().spawn(async move {
                let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
                let prerequisites = installation::prerequisites("node", npm).is_ok();
                let result = (|| -> anyhow::Result<(PiClient, String)> {
                    let program = if install {
                        installation::prerequisites("node", npm)?;
                        let prefix = installation::prefix().ok_or_else(|| anyhow::anyhow!("Não foi possível localizar a pasta do usuário."))?;
                        installation::install(npm, &prefix)?
                    } else { chosen.unwrap_or_else(installation::program) };
                    installation::validate(&program)?;
                    let mut config = PiConfig::new(cwd).with_args(flags);
                    config.program = program.clone();
                    let client = PiClient::spawn(&config)
                        .map_err(|_| anyhow::anyhow!("Pi encontrado, mas não foi possível iniciar o processo RPC. Verifique o projeto e as permissões."))?;
                    Ok((client, program))
                })();
                (result, prerequisites)
            }).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.prerequisites = prerequisites;
                match result {
                    Ok((client, program)) => {
                        // Store an absolute path where possible, not a shell command.
                        if PathBuf::from(&program).is_absolute() && std::env::var_os("DISH_PI_BIN").is_none() {
                            installation::remember(&program);
                        }
                        let handle = this.window;
                        let _ = handle.update(cx, |_, window, cx| {
                            this.ready = Some(this.connect(client, window, cx));
                            window.refresh();
                        });
                    }
                    Err(error) => this.message = format!("Pi indisponível: {error:#}"),
                }
                cx.notify();
                let _ = this.window.update(cx, |_, window, _| window.refresh());
            });
        }).detach();
    }

    fn install_official(&mut self, cx: &mut Context<Self>) {
        if std::env::var_os("DISH_PI_BIN").is_some() {
            self.message = "Corrija ou remova DISH_PI_BIN antes de instalar outra versão.".into();
        } else {
            self.message = match installation::launch_official_installer() {
                Ok(()) => "Instalador aberto no terminal. Conclua as etapas lá e clique em Tentar novamente. Se o Pi não for encontrado, escolha o executável instalado.".into(),
                Err(error) => error.to_string(),
            };
        }
        cx.notify();
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Escolher executável do Pi".into()),
        });
        cx.spawn(async move |this, cx| {
            match picker.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.first() {
                        let _ = this.update(cx, |this, cx| this.detect(Some(path.to_string_lossy().into_owned()), false, cx));
                    }
                }
                Ok(Ok(None)) => {}
                _ => { let _ = this.update(cx, |this, cx| {
                    this.message = "Não foi possível abrir o seletor de arquivos. Configure DISH_PI_BIN e tente novamente.".into();
                    cx.notify();
                }); }
            }
        }).detach();
    }
    fn connect(&self, client: PiClient, window: &mut Window, cx: &mut App) -> Entity<Workspace> {
        let editor = cx.new(|cx| Editor::new("", "Ask anything", window, cx));
        let modal = cx.new(|cx| Editor::new("", "Type here…", window, cx));
        let search = cx.new(|cx| Editor::new("", "Search models or providers…", window, cx));
        let help = cx.new(|cx| Editor::new("", "Buscar atalhos…", window, cx));
        let nav = cx.new(|cx| Editor::new("", "Filtrar sessões", window, cx));
        let state = cx.new(|cx| {
            AppState::new(
                self.cwd.clone(),
                client,
                crate::state::AppEditors {
                    composer: editor,
                    modal,
                    model_search: search,
                    help_search: help,
                },
                self.prompt.clone(),
                cx,
            )
        });
        state.update(cx, |state, cx| {
            state.start(cx);
            state.editor.read(cx).focus_handle.clone().focus(window, cx);
        });
        let workspace = cx.new(|cx| Workspace::new(state, self.flags.clone(), nav, cx));
        workspace.update(cx, |workspace, cx| workspace.start(window, cx));
        workspace
    }
}

impl Render for Startup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(workspace) = &self.ready {
            return workspace.clone().into_any_element();
        }
        let mut view = div().id("pi-setup").size_full().min_h(px(720.)).flex().flex_col().items_center()
            .track_focus(&self.focus)
            .justify_center().gap(px(16.)).p(px(24.)).bg(theme::bg())
            .text_color(theme::text()).font_family(theme::FONT_UI)
            .text_size(px(14.)).line_height(px(20.))
            .child(div().text_size(px(22.)).child("Configurar Pi"))
            .child(div().max_w(px(680.)).text_center().child(self.message.clone()))
            .child(div().max_w(px(680.)).text_center().text_color(theme::dim())
                .child("O Dish precisa do Pi. Nada será instalado sem sua escolha. As tarefas não são gerenciadas por um serviço em segundo plano."));
        if !self.busy {
            view = view
                .child(div().max_w(px(680.)).text_center().child(
                    "Recomendado: instalador oficial em terminal externo. Ele baixa e executa o script do Pi, escolhe o destino e pode pedir instalação de dependências ou sudo. Revise as etapas no terminal."
                ))
                .child(button("setup-official", "Aceitar e abrir instalador oficial", cx.listener(|this, _: &ClickEvent, _, cx| this.install_official(cx))));
            let destination = installation::prefix()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(pasta do usuário indisponível)".into());
            view = view.child(div().max_w(px(680.)).text_center().child(format!(
                "Alternativa: instalação privada via npm, sem sudo. Comando: npm install -g --ignore-scripts --no-audit --no-fund --prefix \"{destination}\" {}", installation::PACKAGE)));
            if self.prerequisites && installation::prefix().is_some() {
                view = view.child(button(
                    "setup-install",
                    "Alternativa: aceitar e instalar via npm",
                    cx.listener(|this, _: &ClickEvent, _, cx| this.detect(None, true, cx)),
                ));
            } else {
                view = view.child(
                    "Instale Node.js 22.19 ou superior e npm usando as instruções oficiais.",
                );
            }
            view = view
                .child(button("setup-retry", "Tentar novamente", cx.listener(|this, _: &ClickEvent, _, cx| this.start(cx))))
                .child(button("setup-choose", "Escolher executável existente", cx.listener(|this, _: &ClickEvent, _, cx| this.choose(cx))))
                .child(div().max_w(px(680.)).text_center().child(installation::manual_command()))
                .child(button("setup-copy", "Copiar comando de instalação manual", |_: &ClickEvent, _, cx: &mut App| {
                    cx.write_to_clipboard(ClipboardItem::new_string(installation::manual_command().into()));
                }))
                .child(button("setup-docs", "Abrir instruções oficiais", |_: &ClickEvent, _, cx: &mut App| {
                    cx.open_url("https://github.com/earendil-works/pi/blob/main/packages/coding-agent/README.md");
                }))
                .child(button("setup-later", "Agora não — fechar", |_: &ClickEvent, window, _| window.remove_window()));
        }
        let mut frame = div().relative().size_full();
        if let Some(layer) = crate::ui::window_frame::layer(window) {
            frame = frame.child(layer);
        }
        frame
            .child(
                div()
                    .id("setup-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .child(view),
            )
            .into_any_element()
    }
}

fn button(
    id: &'static str,
    label: &'static str,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(16.))
        .py(px(8.))
        .rounded(px(6.))
        .bg(theme::surface())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .on_click(click)
        .child(label)
}
