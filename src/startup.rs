//! Onboarding em poucos passos: Pi continua sendo do workspace, nunca um daemon.
//!
//! A regra aqui é simplicidade: cada tela mostra **uma** ação principal e o
//! resto fica atrás de um clique, em vez de uma parede de botões. Nada é
//! instalado sem consentimento explícito e o comando executado fica visível no
//! terminal (issues #59 e #58).
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Verificação e resultado. Ação principal depende do que foi encontrado.
    Welcome,
    /// Só aparece quando o Pi não está instalado.
    Install,
    /// Confirmação depois de uma instalação bem-sucedida.
    Done,
}

impl Step {
    fn title(self) -> &'static str {
        match self {
            Step::Welcome => "Configurar Pi",
            Step::Install => "Instalar o Pi",
            Step::Done => "Pronto",
        }
    }
}

/// O que a verificação encontrou. `None` enquanto ela não terminou.
struct Probe {
    program: Option<String>,
    version: Option<String>,
    error: Option<String>,
}

impl Probe {
    fn found(&self) -> bool {
        self.program.is_some()
    }

}

pub struct Startup {
    cwd: PathBuf,
    flags: Vec<String>,
    prompt: Option<String>,
    busy: bool,
    message: String,
    step: Step,
    probe: Option<Probe>,
    /// Versão do Node, ou a razão pela qual ele não serve para o caminho npm.
    /// Verificado só quando o passo de instalação é aberto: quem já tem Pi não
    /// precisa que o Dish rode `node`/`npm` no caminho de abertura.
    node: Option<String>,
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
            step: Step::Welcome,
            probe: None,
            node: None,
            ready: None,
            focus: cx.focus_handle(),
            window: window.window_handle(),
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.step = Step::Welcome;
        self.probe(None, cx);
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

    fn probe(&mut self, chosen: Option<String>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message = "Procurando o Pi neste computador…".into();
        self.probe = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let probe = cx
                .background_executor()
                .spawn(async move {
                    let program = chosen.unwrap_or_else(installation::program);
                    match installation::validate(&program) {
                        Ok(version) => Probe {
                            program: Some(program),
                            version: Some(version),
                            error: None,
                        },
                        Err(error) => Probe {
                            program: None,
                            version: None,
                            error: Some(format!("{error:#}")),
                        },
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                this.message.clear();
                let found = probe.found();
                this.probe = Some(probe);
                // Quem já tem uma escolha salva (ou DISH_PI_BIN) não vê o
                // assistente de novo: abre direto, como antes. O assistente
                // existe para a primeira vez e para quem perdeu o Pi.
                // A preferência salva sobrevive ao processo; `selected()` só
                // existe nesta execução, então não serve para decidir isso.
                let returning = crate::preferences::load().pi_executable.is_some()
                    || std::env::var_os("DISH_PI_BIN").is_some();
                if found && returning {
                    this.connect_from_background(cx);
                    return;
                }
                this.step = if found { Step::Done } else { Step::Welcome };
                cx.notify();
                let _ = this.window.update(cx, |_, window, _| window.refresh());
            });
        })
        .detach();
    }

    /// Roda `node --version`/`npm --version` em segundo plano, só quando o
    /// passo de instalação precisa saber se o caminho npm está disponível.
    fn check_node(&mut self, cx: &mut Context<Self>) {
        if self.node.is_some() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let node = cx
                .background_executor()
                .spawn(async move {
                    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
                    installation::prerequisites("node", npm)
                        .map(|_| node_version())
                        .unwrap_or_else(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.node = Some(node);
                cx.notify();
            });
        })
        .detach();
    }

    fn install_npm(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if std::env::var_os("DISH_PI_BIN").is_some() {
            self.message = "DISH_PI_BIN está definido. Corrija ou remova esse override antes de instalar outra versão.".into();
            cx.notify();
            return;
        }
        self.busy = true;
        self.message = "Instalando o Pi via npm… Isso pode levar alguns minutos.".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
                    installation::prerequisites("node", npm)?;
                    let prefix = installation::prefix().ok_or_else(|| {
                        anyhow::anyhow!("Não foi possível localizar a pasta do usuário.")
                    })?;
                    installation::install(npm, &prefix)
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                // `probe` ignora a chamada enquanto `busy`; a instalação terminou.
                // Verifica exatamente o que foi instalado, e não o que a busca
                // genérica encontraria primeiro (pode haver outro Pi no PATH).
                Ok(program) => {
                    this.busy = false;
                    this.probe(Some(program), cx);
                }
                Err(error) => {
                    this.busy = false;
                    this.message = format!("Não foi possível instalar via npm: {error:#}");
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn install_official(&mut self, cx: &mut Context<Self>) {
        if std::env::var_os("DISH_PI_BIN").is_some() {
            self.message = "Corrija ou remova DISH_PI_BIN antes de instalar outra versão.".into();
        } else {
            self.message = match installation::launch_official_installer() {
                Ok(()) => "Instalador aberto no terminal. Conclua as etapas por lá e clique em Já instalei, verificar.".into(),
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
                        let _ = this.update(cx, |this, cx| {
                            this.probe(Some(path.to_string_lossy().into_owned()), cx)
                        });
                    }
                }
                Ok(Ok(None)) => {}
                _ => {
                    let _ = this.update(cx, |this, cx| {
                        this.message = "Não foi possível abrir o seletor de arquivos. Configure DISH_PI_BIN e tente novamente.".into();
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// Sobe o processo RPC do Pi já verificado, guardando a escolha.
    fn spawn_client(&mut self) -> Option<PiClient> {
        let program = self.probe.as_ref().and_then(|probe| probe.program.clone())?;
        if PathBuf::from(&program).is_absolute() && std::env::var_os("DISH_PI_BIN").is_none() {
            installation::remember(&program);
        }
        let mut config = PiConfig::new(self.cwd.clone()).with_args(self.flags.clone());
        config.program = program;
        match PiClient::spawn(&config) {
            Ok(client) => Some(client),
            Err(_) => {
                self.message = "Pi encontrado, mas não foi possível iniciar o processo RPC. Verifique o projeto e as permissões.".into();
                None
            }
        }
    }

    fn connect_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.spawn_client() else {
            cx.notify();
            return;
        };
        self.ready = Some(self.connect(client, window, cx));
        window.refresh();
    }

    /// Mesma abertura, a partir de um contexto sem `Window` (verificação em
    /// segundo plano): a janela é atualizada pelo handle guardado.
    fn connect_from_background(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.spawn_client() else {
            cx.notify();
            return;
        };
        let handle = self.window;
        let _ = handle.update(cx, |_, window, cx| {
            self.ready = Some(self.connect(client, window, cx));
            window.refresh();
        });
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

fn node_version() -> String {
    match std::process::Command::new("node").arg("--version").output() {
        Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
        Err(_) => String::new(),
    }
}

impl Render for Startup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(workspace) = &self.ready {
            return workspace.clone().into_any_element();
        }
        let mut view = div()
            .id("pi-setup")
            .size_full()
            .min_h(px(720.))
            .flex()
            .flex_col()
            .items_center()
            .track_focus(&self.focus)
            .justify_center()
            .gap(px(16.))
            .p(px(24.))
            .bg(theme::bg())
            .text_color(theme::text())
            .font_family(theme::FONT_UI)
            .text_size(px(14.))
            .line_height(px(20.))
            .child(div().text_size(px(22.)).child(self.step.title()));

        if !self.message.is_empty() {
            view = view.child(
                div()
                    .max_w(px(680.))
                    .text_center()
                    .child(self.message.clone()),
            );
        }

        view = match self.step {
            Step::Welcome => {
                let status = match self.probe.as_ref() {
                    None => "Procurando o Pi…".to_string(),
                    Some(probe) => match (&probe.program, &probe.version) {
                        (Some(program), Some(version)) => format!("Pi {version} encontrado em {program}"),
                        _ => probe
                            .error
                            .clone()
                            .unwrap_or_else(|| "Pi não encontrado neste computador.".to_string()),
                    },
                };
                let mut step = view
                    .child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(
                        "O Dish é um cliente do Pi: usa as mesmas sessões, ferramentas e modelos do terminal. Nada é instalado sem a sua escolha.",
                    ))
                    .child(div().max_w(px(680.)).text_center().child(status));
                match self.probe.as_ref() {
                    Some(probe) if probe.found() => {
                        step = step
                            .child(button("setup-open", "Abrir o Dish", cx.listener(|this, _: &ClickEvent, window, cx| this.connect_now(window, cx))))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap(px(8.))
                                    .child(button("setup-choose", "Usar outro executável", cx.listener(|this, _: &ClickEvent, _, cx| this.choose(cx))))
                                    .child(button("setup-retry", "Verificar novamente", cx.listener(|this, _: &ClickEvent, _, cx| this.probe(None, cx)))),
                            );
                    }
                    Some(_) => {
                        step = step
                            .child(button("setup-install-step", "Instalar o Pi", cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.step = Step::Install;
                                this.check_node(cx);
                                cx.notify();
                            })))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap(px(8.))
                                    .child(button("setup-retry", "Já instalei, verificar", cx.listener(|this, _: &ClickEvent, _, cx| this.probe(None, cx))))
                                    .child(button("setup-choose", "Escolher executável existente", cx.listener(|this, _: &ClickEvent, _, cx| this.choose(cx)))),
                            );
                    }
                    None => {}
                }
                step
            }
            Step::Install => {
                let destination = installation::prefix()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(pasta do usuário indisponível)".into());
                let node = self.node.clone();
                let node_ready = self.node.as_ref().is_some_and(|node| node.starts_with('v'));
                let mut step = view
                    .child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(
                        "O instalador oficial abre em um terminal, baixa o Pi, escolhe o destino e pode instalar dependências (o Node, quando falta). O comando fica visível lá.",
                    ))
                    .child(button("setup-official", "Aceitar e abrir instalador oficial", cx.listener(|this, _: &ClickEvent, _, cx| this.install_official(cx))))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .child(button("setup-retry", "Já instalei, verificar", cx.listener(|this, _: &ClickEvent, _, cx| this.probe(None, cx))))
                            .child(button("setup-back", "Voltar", cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.step = Step::Welcome;
                                cx.notify();
                            }))),
                    );
                if node_ready && installation::prefix().is_some() {
                    step = step
                        .child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(format!(
                            "Sem terminal: instalação privada via npm, sem sudo, em \"{destination}\"."
                        )))
                        .child(button("setup-install", "Instalar via npm (sem terminal)", cx.listener(|this, _: &ClickEvent, _, cx| this.install_npm(cx))));
                } else if let Some(node) = node {
                    step = step.child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(
                        format!("Para o caminho npm seria preciso Node 22.19 ou superior ({node}). O instalador oficial não precisa disso."),
                    ));
                }
                step.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.))
                        .child(button("setup-copy", "Copiar comando manual", |_: &ClickEvent, _, cx: &mut App| {
                            cx.write_to_clipboard(ClipboardItem::new_string(installation::manual_command().into()));
                        }))
                        .child(button("setup-docs", "Instruções oficiais", |_: &ClickEvent, _, cx: &mut App| {
                            cx.open_url("https://github.com/earendil-works/pi/blob/main/packages/coding-agent/README.md");
                        })),
                )
            }
            Step::Done => {
                let summary = self
                    .probe
                    .as_ref()
                    .map(|probe| match (&probe.program, &probe.version) {
                        (Some(program), Some(version)) => format!("Pi {version}\n{program}"),
                        _ => "Pi pronto.".to_string(),
                    })
                    .unwrap_or_else(|| "Pi pronto.".to_string());
                view.child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(
                    "Tudo certo. O Dish vai usar esse executável e guardar a escolha nas preferências.",
                ))
                .child(div().max_w(px(680.)).text_center().font_family(theme::FONT_MONO).child(summary))
                .child(button("setup-open", "Abrir o Dish", cx.listener(|this, _: &ClickEvent, window, cx| this.connect_now(window, cx))))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.))
                        .child(button("setup-choose", "Usar outro executável", cx.listener(|this, _: &ClickEvent, _, cx| this.choose(cx))))
                        .child(button("setup-later", "Agora não — fechar", |_: &ClickEvent, window, _| window.remove_window())),
                )
            }
        };

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
