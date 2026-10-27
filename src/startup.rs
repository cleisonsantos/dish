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
    workspace::{Workspace, WorkspaceEvent},
};
use gpui::{prelude::*, *};
use std::{path::PathBuf, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Welcome,
    VerifyPi,
    Install,
    /// Só aparece no caminho npm. O instalador oficial não exige Node.
    VerifyNode,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InstallPath {
    Official,
    Npm,
}

impl Step {
    fn title(self) -> &'static str {
        match self {
            Step::Welcome => "Bem-vindo ao Dish",
            Step::VerifyPi => "Verificar o Pi",
            Step::Install => "Instalar ou escolher",
            Step::VerifyNode => "Verificar o Node",
            Step::Done => "Pronto",
        }
    }

    /// Reabrir o assistente volta ao começo: o estado de uma instalação
    /// anterior não deve reaparecer como se ainda estivesse em andamento.
    fn reopen() -> Self {
        Step::Welcome
    }

    fn back(self) -> Self {
        match self {
            Step::Welcome => Step::Welcome,
            Step::VerifyPi => Step::Welcome,
            Step::Install => Step::VerifyPi,
            Step::VerifyNode => Step::Install,
            Step::Done => Step::VerifyPi,
        }
    }
}

/// Pi encontrado dispensa instalação; sem Pi, a próxima decisão é o caminho.
fn step_after_pi(found: bool) -> Step {
    if found {
        Step::Done
    } else {
        Step::Install
    }
}

/// O npm precisa de Node; o instalador oficial e um executável existente não.
fn step_after_install_path(path: InstallPath) -> Step {
    match path {
        InstallPath::Official => Step::Install,
        InstallPath::Npm => Step::VerifyNode,
    }
}

/// Nenhum caminho de instalação roda sem consentimento. O npm também exige
/// Node 22.19+ já verificado; escolher um executável existente não instala.
fn install_allowed(path: InstallPath, consent: bool, node_ready: bool) -> bool {
    consent
        && match path {
            InstallPath::Official => true,
            InstallPath::Npm => node_ready,
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

/// `Start` preserves the old returning-user shortcut; a manual check stays in
/// the wizard and only advances when the executable is valid.
#[derive(Clone, Copy)]
enum ProbeMode {
    Start,
    Stay(Step),
}

pub struct Startup {
    cwd: PathBuf,
    flags: Vec<String>,
    prompt: Option<String>,
    /// Aviso da linha de comando (por exemplo, uma pasta que não existe).
    /// Vira toast quando o espaço de trabalho abre.
    notice: Option<String>,
    busy: bool,
    message: String,
    step: Step,
    probe: Option<Probe>,
    /// O assistente está visível. Depois da primeira abertura ele pode ser
    /// reaberto por Configurações → Sobre / Pi sem perder o workspace atual.
    show_wizard: bool,
    /// A verificação foi pedida pelo usuário ("já instalei"), não automática.
    explicit_check: bool,
    /// Versão do Node, ou a razão pela qual ele não serve para o caminho npm.
    /// Verificado só quando esse caminho é escolhido: quem já tem Pi não
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
        notice: Option<String>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            cwd,
            flags,
            prompt,
            notice,
            busy: false,
            message: String::new(),
            step: Step::Welcome,
            probe: None,
            show_wizard: true,
            explicit_check: false,
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
        self.probe(None, ProbeMode::Start, cx);
    }

    /// Entra no primeiro passo de verificação. Se a detecção automática ainda
    /// estiver rodando, ela termina e aparece aqui.
    fn begin(&mut self, cx: &mut Context<Self>) {
        self.step = Step::VerifyPi;
        if self.probe.is_none() && !self.busy {
            self.probe(None, ProbeMode::Stay(Step::VerifyPi), cx);
        }
        cx.notify();
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

    fn probe(&mut self, chosen: Option<String>, mode: ProbeMode, cx: &mut Context<Self>) {
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
                if found && returning && matches!(mode, ProbeMode::Start) {
                    this.connect_from_background(cx);
                    return;
                }
                this.step = match mode {
                    ProbeMode::Start => Step::Welcome,
                    ProbeMode::Stay(fallback) => {
                        if found {
                            Step::Done
                        } else {
                            fallback
                        }
                    }
                };
                if !found && this.explicit_check {
                    this.message = "Pi ainda não encontrado. Se você acabou de instalar, feche e abra o Dish (o PATH só é lido na abertura) ou use \"Escolher executável existente\".".into();
                }
                this.explicit_check = false;
                cx.notify();
                let _ = this.window.update(cx, |_, window, _| window.refresh());
            });
        })
        .detach();
    }

    /// Roda `node --version`/`npm --version` em segundo plano, só quando o
    /// caminho npm é escolhido. O instalador oficial nunca passa por aqui.
    fn check_node(&mut self, cx: &mut Context<Self>) {
        self.node = None;
        cx.notify();
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

    /// Verificação pedida pelo usuário: além de procurar de novo, explica o que
    /// fazer quando o Pi recém-instalado não aparece.
    fn recheck(&mut self, cx: &mut Context<Self>) {
        self.explicit_check = true;
        self.probe(None, ProbeMode::Stay(self.step), cx);
    }

    fn node_ready(&self) -> bool {
        self.node.as_deref().is_some_and(|node| node.starts_with('v'))
    }

    fn install_npm(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if !install_allowed(InstallPath::Npm, true, self.node_ready()) {
            self.step = Step::VerifyNode;
            self.message = "Confirme o Node 22.19+ antes de instalar via npm.".into();
            cx.notify();
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
                    this.probe(Some(program), ProbeMode::Stay(Step::VerifyNode), cx);
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
        if !install_allowed(InstallPath::Official, true, false) {
            return;
        }
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
                            this.probe(
                                Some(path.to_string_lossy().into_owned()),
                                ProbeMode::Stay(Step::Install),
                                cx,
                            )
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

    /// Reabre o assistente depois da primeira configuração. O workspace atual
    /// continua vivo por trás; cancelar volta para ele.
    fn reopen(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            self.message =
                "Aguarde a verificação ou instalação terminar antes de reabrir o assistente.".into();
            cx.notify();
            return;
        }
        self.step = Step::reopen();
        self.show_wizard = true;
        self.probe = None;
        self.node = None;
        self.message.clear();
        self.explicit_check = false;
        let handle = self.window;
        let focus = self.focus.clone();
        let _ = handle.update(cx, |_, window, cx| {
            focus.focus(window, cx);
            window.refresh();
        });
        cx.notify();
    }

    /// `esc` e "Agora não" fecham o assistente sem deixar a janela presa.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            self.message =
                "Aguarde a verificação ou instalação terminar antes de fechar o assistente.".into();
            cx.notify();
            return;
        }
        if self.ready.is_some() {
            self.show_wizard = false;
            self.message.clear();
            cx.notify();
        } else {
            // Primeiro uso sem Pi: o Dish não tem como abrir uma conversa. A
            // escolha fica para a próxima abertura, sem inventar modo somente
            // leitura. Configurações → Sobre / Pi reabre o assistente.
            window.remove_window();
        }
    }

    /// Enter no foco da tela equivale ao botão principal do passo. Com um
    /// botão focado, o próprio botão trata o atalho.
    fn activate_primary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.step {
            Step::Welcome => self.begin(cx),
            Step::VerifyPi => {
                self.step = step_after_pi(self.probe.as_ref().is_some_and(Probe::found));
                cx.notify();
            }
            Step::Install => {
                self.step = step_after_install_path(InstallPath::Official);
                self.install_official(cx);
            }
            Step::VerifyNode => self.install_npm(cx),
            Step::Done => self.finish(window, cx),
        }
    }

    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ready.is_some() {
            // Reabrir o assistente pode trocar o executável. O workspace atual
            // continua no processo que já está falando com o Pi; a escolha nova
            // vale para as próximas aberturas, sem derrubar conversas em curso.
            if let Some(program) = self.probe.as_ref().and_then(|probe| probe.program.as_ref()) {
                if PathBuf::from(program).is_absolute()
                    && std::env::var_os("DISH_PI_BIN").is_none()
                {
                    installation::remember(program);
                }
            }
            self.show_wizard = false;
            cx.notify();
        } else {
            self.connect_now(window, cx);
        }
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
        let weak = cx.entity().downgrade();
        self.ready = Some(self.connect(client, window, cx, weak));
        self.show_wizard = false;
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
        let weak = cx.entity().downgrade();
        let _ = handle.update(cx, |_, window, cx| {
            self.show_wizard = false;
            self.ready = Some(self.connect(client, window, cx, weak));
            window.refresh();
        });
    }

    fn connect(
        &self,
        client: PiClient,
        window: &mut Window,
        cx: &mut App,
        weak: WeakEntity<Startup>,
    ) -> Entity<Workspace> {
        let editor = cx.new(|cx| Editor::new("", "Pergunte algo ao agente", window, cx));
        let modal = cx.new(|cx| Editor::new("", "Escreva aqui…", window, cx));
        let search = cx.new(|cx| Editor::new("", "Buscar modelos ou provedores…", window, cx));
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
        let workspace = cx.new(|cx| Workspace::new(state, self.flags.clone(), nav, self.notice.clone(), cx));
        cx.subscribe(&workspace, move |_, event: &WorkspaceEvent, cx| {
            if matches!(event, WorkspaceEvent::ReopenSetup) {
                let _ = weak.update(cx, |this, cx| this.reopen(cx));
            }
        })
        .detach();
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
            if !self.show_wizard {
                return workspace.clone().into_any_element();
            }
        }
        theme::apply(window.appearance());
        let mut view = div()
            .id("pi-setup")
            .size_full()
            .min_h(px(720.))
            .flex()
            .flex_col()
            .items_center()
            .track_focus(&self.focus)
            .justify_center()
            .gap(px(14.))
            .p(px(24.))
            .bg(theme::bg())
            .text_color(theme::text())
            .font_family(theme::FONT_UI)
            .text_size(px(14.))
            .line_height(px(20.))
            .on_action(cx.listener(|this, _: &crate::ui::Dismiss, window, cx| {
                cx.stop_propagation();
                this.cancel(window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                } else if matches!(event.keystroke.key.as_str(), "enter" | "space")
                    && this.focus.is_focused(window)
                {
                    this.activate_primary(window, cx);
                    cx.stop_propagation();
                }
            }))
            // O pratinho dá as boas-vindas; o passo aparece por cima dele,
            // sem rótulo pequeno explicando o que é a tela.
            .child(
                img("dish-icon.png").flex_none().w(px(64.)).h(px(64.)).opacity(0.9),
            )
            .child(
                div()
                    .text_size(px(20.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::text())
                    .child(self.step.title()),
            )
            .child(
                div()
                    .max_w(px(680.))
                    .text_center()
                    .text_size(px(theme::TEXT_SM))
                    .text_color(theme::faint())
                    .child(step_caption(self.step)),
            );

        if !self.message.is_empty() {
            view = view.child(
                div()
                    .max_w(px(680.))
                    .px(px(theme::S4))
                    .py(px(theme::S2 + 1.))
                    .rounded(theme::r_control())
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::line())
                    .text_color(theme::dim())
                    .text_center()
                    .child(self.message.clone()),
            );
        }

        let cancel_label = if self.ready.is_some() {
            "Agora não — voltar ao Dish"
        } else {
            "Agora não — fechar"
        };

        view = match self.step {
            Step::Welcome => view
                .child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(
                    "O Dish é um cliente do Pi: o Pi é obrigatório e continua separado, com as mesmas sessões, ferramentas e modelos do terminal. Nada é instalado sem a sua escolha.",
                ))
                .child(primary_button("setup-start", "Começar", cx, |this, _, cx| {
                    this.begin(cx);
                }))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .justify_center()
                        .gap(px(8.))
                        .child(button("setup-later", cancel_label, cx, |this, window, cx| {
                            this.cancel(window, cx);
                        })),
                ),
            Step::VerifyPi => {
                let found = self.probe.as_ref().is_some_and(Probe::found);
                let status = match self.probe.as_ref() {
                    None if self.busy => "Procurando o Pi neste computador…".to_string(),
                    None => "Ainda não verificado.".to_string(),
                    Some(probe) => match (&probe.program, &probe.version) {
                        (Some(program), Some(version)) => {
                            format!("Pi {version} encontrado em {program}")
                        }
                        _ => probe
                            .error
                            .clone()
                            .unwrap_or_else(|| "Pi não encontrado neste computador.".to_string()),
                    },
                };
                let mut step = view
                    .child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(
                        "O Dish procura, nesta ordem, DISH_PI_BIN, a escolha salva, o PATH e os locais comuns de instalação. Nada é alterado durante a verificação.",
                    ))
                    .child(
                        div()
                            .max_w(px(680.))
                            .text_center()
                            .font_family(theme::FONT_MONO)
                            .text_size(px(theme::TEXT_SM))
                            .text_color(if found { theme::text() } else { theme::dim() })
                            .child(status),
                    );
                if found {
                    step = step.child(primary_button("setup-continue", "Continuar", cx, |this, _, cx| {
                        this.step = step_after_pi(true);
                        cx.notify();
                    }));
                } else {
                    step = step.child(primary_button("setup-install-step", "Instalar o Pi", cx, |this, _, cx| {
                        this.step = step_after_pi(false);
                        cx.notify();
                    }));
                }
                step.child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .justify_center()
                        .gap(px(8.))
                        .child(button("setup-retry", "Verificar novamente", cx, |this, _, cx| {
                            this.recheck(cx);
                        }))
                        .child(button("setup-back", "Voltar", cx, |this, _, cx| {
                            this.step = this.step.back();
                            cx.notify();
                        }))
                        .child(button("setup-later", cancel_label, cx, |this, window, cx| {
                            this.cancel(window, cx);
                        })),
                )
            }
            Step::Install => {
                let destination = installation::prefix()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(pasta do usuário indisponível)".into());
                let command = installation::manual_command();
                let mut step = view
                    .child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(
                        "O instalador oficial abre em um terminal externo. O comando abaixo é o mesmo que será executado, e nada roda antes do seu consentimento.",
                    ))
                    .child(
                        div()
                            .max_w(px(680.))
                            .px(px(theme::S4))
                            .py(px(theme::S2 + 1.))
                            .rounded(theme::r_control())
                            .bg(theme::inset())
                            .border_1()
                            .border_color(theme::line_soft())
                            .font_family(theme::FONT_MONO)
                            .text_size(px(theme::TEXT_SM))
                            .text_color(theme::text())
                            .child(SharedString::from(format!("$ {command}"))),
                    )
                    .child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(
                        "O instalador pode instalar dependências (Node; no Windows, também Git via winget) e pedir permissões elevadas. O Dish não escreve na configuração, nas sessões nem nas credenciais do Pi.",
                    ))
                    .child(primary_button("setup-official", "Aceitar e abrir instalador oficial", cx, |this, _, cx| {
                        this.step = step_after_install_path(InstallPath::Official);
                        this.install_official(cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .justify_center()
                            .gap(px(8.))
                            .child(button("setup-npm-path", "Instalar via npm", cx, |this, _, cx| {
                                this.step = step_after_install_path(InstallPath::Npm);
                                this.check_node(cx);
                                cx.notify();
                            }))
                            .child(button("setup-choose", "Escolher executável existente", cx, |this, _, cx| {
                                this.choose(cx);
                            })),
                    )
                    .child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(format!(
                        "O caminho npm instala só para o seu usuário, sem sudo, em \"{destination}\"."
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .justify_center()
                            .gap(px(8.))
                            .child(button("setup-retry", "Já instalei, verificar", cx, |this, _, cx| {
                                this.recheck(cx);
                            }))
                            .child(button("setup-back", "Voltar", cx, |this, _, cx| {
                                this.step = this.step.back();
                                cx.notify();
                            }))
                            .child(button("setup-later", cancel_label, cx, |this, window, cx| {
                                this.cancel(window, cx);
                            })),
                    );
                step = step.child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .justify_center()
                        .gap(px(8.))
                        .child(button("setup-copy", "Copiar comando manual", cx, |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                installation::manual_command().into(),
                            ));
                        }))
                        .child(button("setup-docs", "Instruções oficiais", cx, |_, _, cx| {
                            cx.open_url("https://github.com/earendil-works/pi/blob/main/packages/coding-agent/README.md");
                        })),
                );
                step
            }
            Step::VerifyNode => {
                let destination = installation::prefix()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(pasta do usuário indisponível)".into());
                let node_ready = self.node_ready();
                let status = match self.node.as_deref() {
                    None => "Verificando Node e npm…".to_string(),
                    Some(node) if node_ready => format!("Node {node} e npm encontrados."),
                    Some(error) => error.to_string(),
                };
                let mut step = view
                    .child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(
                        "Node 22.19 ou superior e npm são necessários apenas para instalar o Pi via npm. O instalador oficial não exige Node.",
                    ))
                    .child(
                        div()
                            .max_w(px(680.))
                            .text_center()
                            .font_family(theme::FONT_MONO)
                            .text_size(px(theme::TEXT_SM))
                            .text_color(if node_ready { theme::text() } else { theme::dim() })
                            .child(status),
                    )
                    .child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(format!(
                        "Instalação privada via npm, sem sudo, em \"{destination}\"."
                    )));
                if node_ready {
                    let installing = self.busy;
                    step = step.child(primary_button(
                        "setup-install",
                        if installing { "Instalando…" } else { "Instalar via npm" },
                        cx,
                        |this, _, cx| {
                            this.install_npm(cx);
                        },
                    ));
                } else {
                    step = step.child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(
                        "Sem Node compatível, volte e use o instalador oficial ou escolha um Pi que já exista.",
                    ));
                }
                step.child(div().max_w(px(680.)).text_center().text_color(theme::faint()).child(
                    "A saída bruta do npm não é exibida porque pode conter credenciais de registry; o progresso aparece nesta tela e o resultado é validado antes de abrir.",
                ))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .justify_center()
                        .gap(px(8.))
                        .child(button("setup-node-retry", "Verificar Node novamente", cx, |this, _, cx| {
                            this.check_node(cx);
                        }))
                        .child(button("setup-back", "Voltar", cx, |this, _, cx| {
                            this.step = this.step.back();
                            cx.notify();
                        }))
                        .child(button("setup-later", cancel_label, cx, |this, window, cx| {
                            this.cancel(window, cx);
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
                let reopened = self.ready.is_some();
                let open_label = if reopened {
                    "Voltar ao Dish"
                } else {
                    "Abrir o Dish"
                };
                let done_note = if reopened {
                    "Tudo certo. A escolha fica salva para a próxima abertura; a conversa atual continua no processo que já está aberto."
                } else {
                    "Tudo certo. O Dish vai usar esse executável e guardar a escolha nas preferências."
                };
                view.child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(done_note))
                .child(
                    div()
                        .max_w(px(680.))
                        .text_center()
                        .font_family(theme::FONT_MONO)
                        .text_size(px(theme::TEXT_SM))
                        .text_color(theme::text())
                        .child(summary),
                )
                .child(div().max_w(px(680.)).text_center().text_color(theme::dim()).child(format!(
                    "Primeiro projeto: {}",
                    self.cwd.display()
                )))
                .child(primary_button("setup-open", open_label, cx, |this, window, cx| {
                    this.finish(window, cx);
                }))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .justify_center()
                        .gap(px(8.))
                        .child(button("setup-retry", "Verificar novamente", cx, |this, _, cx| {
                            this.recheck(cx);
                        }))
                        .child(button("setup-back", "Voltar", cx, |this, _, cx| {
                            this.step = this.step.back();
                            cx.notify();
                        }))
                        .child(button("setup-later", cancel_label, cx, |this, window, cx| {
                            this.cancel(window, cx);
                        })),
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

fn step_caption(step: Step) -> &'static str {
    match step {
        Step::Welcome => "Dish e Pi são programas separados.",
        Step::VerifyPi => "Verificar antes de instalar.",
        Step::Install => "Uma escolha por vez; nada roda sem consentimento.",
        Step::VerifyNode => "Node só é exigido no caminho npm.",
        Step::Done => "Versão, local e primeiro projeto.",
    }
}

fn button(
    id: &'static str,
    label: &'static str,
    cx: &mut Context<Startup>,
    activate: impl Fn(&mut Startup, &mut Window, &mut Context<Startup>) + 'static,
) -> Stateful<Div> {
    let activate = Rc::new(activate);
    let clicked = activate.clone();
    let keyed = activate;
    div()
        .id(id)
        .px(px(16.))
        .py(px(8.))
        .rounded(theme::r_control())
        .bg(theme::surface_2())
        .border_1()
        .border_color(theme::line())
        .text_size(px(theme::TEXT_SM))
        .text_color(theme::dim())
        .cursor_pointer()
        .focusable()
        .tab_index(0)
        .focus(|s| s.shadow(theme::focus_ring()))
        .hover(|s| {
            s.bg(theme::hover())
                .border_color(theme::line_strong())
                .text_color(theme::text())
        })
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            clicked(this, window, cx);
        }))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                keyed(this, window, cx);
            } else if event.keystroke.key == "tab" {
                if event.keystroke.modifiers.shift {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
                cx.stop_propagation();
            }
        }))
        .child(label)
}

/// A ação principal de cada passo: contorno forte com a cor de texto —
/// é o único peso visual do passo, sem virar um bloco claro que o
/// reconhecimento de tela (e olhos cansados) precisam adivinhar.
fn primary_button(
    id: &'static str,
    label: &'static str,
    cx: &mut Context<Startup>,
    activate: impl Fn(&mut Startup, &mut Window, &mut Context<Startup>) + 'static,
) -> Stateful<Div> {
    let activate = Rc::new(activate);
    let clicked = activate.clone();
    let keyed = activate;
    div()
        .id(id)
        .px(px(theme::S4))
        .py(px(9.))
        .rounded(theme::r_action())
        .bg(theme::surface_2())
        .border_1()
        .border_color(theme::line_strong())
        .text_color(theme::text())
        .cursor_pointer()
        .focusable()
        .tab_index(0)
        .focus(|s| s.shadow(theme::focus_ring()))
        .hover(|s| s.bg(theme::hover()).border_color(theme::text()))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            clicked(this, window, cx);
        }))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                keyed(this, window, cx);
            } else if event.keystroke.key == "tab" {
                if event.keystroke.modifiers.shift {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
                cx.stop_propagation();
            }
        }))
        .child(label)
}

#[cfg(test)]
mod tests {
    use super::{install_allowed, step_after_install_path, step_after_pi, InstallPath, Step};

    #[test]
    fn pi_already_found_skips_installation_and_node() {
        assert_eq!(step_after_pi(true), Step::Done);
        assert_eq!(step_after_pi(false), Step::Install);
    }

    #[test]
    fn official_path_never_requires_node_or_silent_consent() {
        assert_eq!(step_after_install_path(InstallPath::Official), Step::Install);
        assert!(install_allowed(InstallPath::Official, true, false));
        assert!(!install_allowed(InstallPath::Official, false, false));
        assert!(!install_allowed(InstallPath::Official, false, true));
    }

    #[test]
    fn npm_path_checks_node_and_requires_consent() {
        assert_eq!(step_after_install_path(InstallPath::Npm), Step::VerifyNode);
        assert!(!install_allowed(InstallPath::Npm, false, true));
        assert!(!install_allowed(InstallPath::Npm, true, false));
        assert!(install_allowed(InstallPath::Npm, true, true));
    }

    #[test]
    fn back_walks_the_linear_flow() {
        assert_eq!(Step::Done.back(), Step::VerifyPi);
        assert_eq!(Step::VerifyNode.back(), Step::Install);
        assert_eq!(Step::Install.back(), Step::VerifyPi);
        assert_eq!(Step::VerifyPi.back(), Step::Welcome);
        assert_eq!(Step::Welcome.back(), Step::Welcome);
    }

    #[test]
    fn reopening_returns_to_welcome_instead_of_stale_busy_state() {
        let finished = step_after_pi(true);
        assert_eq!(finished, Step::Done);
        assert_eq!(Step::reopen(), Step::Welcome);
        assert_eq!(Step::reopen().back(), Step::Welcome);
    }
}
