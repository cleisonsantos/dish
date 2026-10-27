use crate::{editor::{self, Editor}, theme::{self, AppearanceMode}};
use gpui::{prelude::*, *};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq)]
pub enum Section { App, Appearance, Providers, Models, Keys, About }

pub enum SettingsEvent {
    Closed,
    Preference(&'static str, bool),
    /// O usuário escolheu escuro, claro ou seguir o desktop.
    Appearance(AppearanceMode),
    /// O usuário escolheu um tema do catálogo, por id.
    Theme(String),
    /// Abrir a pasta de temas do usuário no gerenciador de arquivos.
    OpenThemes,
    /// Um modelo passou a visível (`true`) ou oculto (`false`) no seletor.
    ModelVisibility(String, bool),
    /// Vários modelos de uma vez (toggle de provedor, ocultar todos).
    ModelsVisibility(Vec<String>, bool),
    /// Limpar todos os modelos ocultos.
    ShowAllModels,
    /// Trocar o executável do Pi escolhido.
    ChoosePi,
    /// Reabrir o assistente de primeiro uso (Configurações → Sobre / Pi).
    OpenPiSetup,
    /// Consultar o registro npm do Pi agora (ação manual, ignora a cadência).
    RequestPiCheck,
    /// Abrir o instalador oficial do Pi (ação manual).
    RequestPiUpdate,
    /// Uma credencial de provedor foi adicionada ou removida no Pi.
    ///
    /// As conversas abertas precisam reler a lista de modelos: o processo do Pi
    /// não avisa sozinho quando uma credencial muda, e o usuário deve conseguir
    /// escolher o novo modelo sem reiniciar a conversa.
    ProvidersChanged,
}
impl EventEmitter<SettingsEvent> for Settings {}

/// Estado da consulta ao Pi sobre provedores.
///
/// A leitura é somente leitura e roda fora da thread da interface: enquanto
/// responde, a tela diz que está consultando em vez de mostrar uma lista vazia
/// que pareceria "nenhum provedor" (issue #69).
#[derive(Clone)]
enum ProvidersView {
    /// Ainda não consultado nesta sessão.
    Idle,
    Loading,
    Ready(crate::providers::Snapshot),
    /// Sem Node, sem pacote do Pi ou consulta falhou.
    Unavailable(String),
}

/// Onde o login está. Um passo por vez, com a lista sempre a um clique atrás.
#[derive(Clone)]
enum LoginStep {
    /// Escolher como entrar nesse provedor.
    ChoosingMethod { provider: String },
    /// Login em andamento: uma pergunta pendente ou os avisos já recebidos.
    Running {
        provider: String,
        ask: Option<crate::provider_login::Ask>,
        notices: Vec<crate::provider_login::Notice>,
    },
    Done { provider: String },
    Failed { message: String, provider: String },
}

pub struct Settings {
    pub open: bool,
    section: Section,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    search: Entity<Editor>,
    details: bool,
    navigation: bool,
    thinking: bool,
    titles: bool,
    appearance: AppearanceMode,
    /// Id do tema fixado, ou `None` para o padrão da aparência.
    theme: Option<String>,
    /// `(provider, id)` de cada modelo que o Pi reportou.
    models: Vec<(String, String)>,
    hidden: BTreeSet<String>,
    providers: ProvidersView,
    /// Aviso da última ação na seção Provedores (ex.: nenhum terminal).
    provider_notice: Option<String>,
    /// Fluxo de login em andamento, se houver.
    login: Option<LoginStep>,
    /// O processo do adaptador enquanto um login corre; matá-lo é o cancelamento.
    session: Option<crate::provider_login::LoginSession>,
    /// Campo da resposta atual — nunca guardado depois de enviado.
    answer: Entity<Editor>,
    /// Pedido de foco para o campo, aplicado no próximo quadro (o foco precisa
    /// de uma janela, e quem recebe a pergunta não tem uma).
    answer_focus: bool,
    /// Provedor esperando confirmação de remoção.
    confirming_logout: Option<String>,
    /// Versão do Pi e opções de atualização mostradas em Sobre / Pi.
    pi: PiStatus,
    /// Consulta de versão em andamento (não é a checagem de rede).
    pi_version_loading: bool,
}

/// O que a tela de configurações precisa mostrar quando abre.
pub struct SettingsView {
    /// `[detalhes, navegação, raciocínio, títulos]`.
    pub options: [bool; 4],
    pub appearance: AppearanceMode,
    /// Tema fixado pelo usuário, se houver.
    pub theme: Option<String>,
    /// `(provider, id)` de cada modelo que o Pi reportou.
    pub models: Vec<(String, String)>,
    pub hidden: BTreeSet<String>,
    pub pi: PiStatus,
}

/// Estado apresentado na seção Sobre / Pi. A versão instalada é consultada com
/// o próprio executável; quando falha, a interface diz "desconhecida".
#[derive(Clone, Default)]
pub struct PiStatus {
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub last_check: Option<i64>,
    pub checking: bool,
    pub notice: Option<String>,
    pub auto_check: bool,
    pub auto_install: bool,
}

impl Settings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| Editor::new("", "Buscar atalhos…", window, cx));
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        let answer = cx.new(|cx| Editor::new("", "", window, cx));
        cx.observe(&answer, |_, _, cx| cx.notify()).detach();
        Self {
            open: false, section: Section::App, focus: cx.focus_handle(),
            previous_focus: None, search,
            details: true, navigation: true, thinking: true, titles: false,
            appearance: AppearanceMode::System,
            theme: None,
            models: Vec::new(), hidden: BTreeSet::new(),
            providers: ProvidersView::Idle,
            provider_notice: None,
            login: None,
            session: None,
            answer,
            answer_focus: false,
            confirming_logout: None,
            pi: PiStatus::default(),
            pi_version_loading: false,
        }
    }

    pub fn show(
        &mut self,
        section: Section,
        view: SettingsView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.open { self.previous_focus = window.focused(cx); }
        self.open = true;
        self.section = section;
        [self.details, self.navigation, self.thinking, self.titles] = view.options;
        self.appearance = view.appearance;
        self.theme = view.theme;
        self.models = view.models;
        self.hidden = view.hidden;
        self.pi = view.pi;
        self.focus.focus(window, cx);
        if section == Section::Keys { self.search.read(cx).focus_handle.clone().focus(window, cx); } else { self.focus.focus(window, cx); }
        self.provider_notice = None;
        if section == Section::Providers { self.refresh_providers(cx); }
        if section == Section::About { self.refresh_pi_version(cx); }
        cx.notify();
    }

    /// Atualiza os dados de Pi vindos de quem executa a checagem de rede.
    pub fn set_pi_status(&mut self, status: PiStatus, cx: &mut Context<Self>) {
        self.pi = status;
        cx.notify();
    }

    /// Pergunta a versão ao executável do Pi, fora da thread da interface.
    pub fn refresh_pi_version(&mut self, cx: &mut Context<Self>) {
        if self.pi_version_loading {
            return;
        }
        self.pi_version_loading = true;
        let program = crate::installation::program();
        cx.spawn(async move |this, cx| {
            let version = cx
                .background_executor()
                .spawn(async move { crate::pi_update::installed_version(&program) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.pi.installed = version;
                this.pi_version_loading = false;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Consulta o catálogo e o estado dos provedores no Pi.
    ///
    /// Uma consulta por vez: a tela pode ser reaberta enquanto a anterior
    /// responde, e duas leituras concorrentes só gastariam processos.
    pub fn refresh_providers(&mut self, cx: &mut Context<Self>) {
        if matches!(self.providers, ProvidersView::Loading) {
            return;
        }
        self.providers = ProvidersView::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let probed = cx
                .background_executor()
                .spawn(async { crate::providers::probe() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.providers = match probed {
                    crate::providers::Probed::Ready(snapshot) => ProvidersView::Ready(snapshot),
                    crate::providers::Probed::Unavailable(reason) => ProvidersView::Unavailable(reason),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.search.update(cx, |editor, cx| editor.clear(cx));
        if let Some(focus) = self.previous_focus.take() { focus.focus(window, cx); }
        cx.emit(SettingsEvent::Closed);
        cx.notify();
    }

    /// Mantém a lista de modelos atualizada enquanto a tela está aberta.
    pub fn set_models(&mut self, models: Vec<(String, String)>, cx: &mut Context<Self>) {
        if self.models == models {
            return;
        }
        self.models = models;
        cx.notify();
    }

    /// Cópia da lista em exibição, usada para evitar atualizações repetidas.
    pub fn visible_models(&self) -> Vec<(String, String)> {
        self.models.clone()
    }

    /// A seção Provedores: a lista, ou o login em andamento.
    fn providers_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // O foco precisa de uma janela, e quem recebe a pergunta não tem uma.
        if self.answer_focus {
            self.answer_focus = false;
            self.answer.read(cx).focus_handle.clone().focus(window, cx);
        }
        let mut page = column().child(page_title("Provedores")).child(note(
            "O estado vem do Pi. O Dish não guarda nem mostra o valor de nenhuma credencial: a resposta vai direto para o Pi e o campo é limpo em seguida.",
        ));
        if let Some(notice) = self.provider_notice.clone() {
            page = page.child(note(&notice));
        }
        match self.login.clone() {
            Some(LoginStep::ChoosingMethod { provider }) => self.method_chooser(page, &provider, cx),
            Some(LoginStep::Running { provider, ask, notices }) => {
                self.login_running(page, &provider, ask, &notices, cx)
            }
            Some(LoginStep::Done { provider }) => {
                page.child(note(&format!("Provedor {provider} conectado.")))
                    .child(actions_row(vec![
                        control("providers-back", "Voltar à lista", cx.listener(|this, _, window, cx| {
                            this.login = None;
                            this.focus.focus(window, cx);
                            cx.notify();
                        })),
                    ]))
                    .into_any_element()
            }
            Some(LoginStep::Failed { message, provider }) => page
                .child(note(&message))
                .child(actions_row(vec![
                    control("providers-retry-login", "Tentar de novo", cx.listener(
                        move |this, _, window, cx| {
                            let provider = provider.clone();
                            this.login = Some(LoginStep::ChoosingMethod { provider });
                            this.focus.focus(window, cx);
                            cx.notify();
                        },
                    )),
                    control("providers-back", "Voltar à lista", cx.listener(|this, _, window, cx| {
                        this.login = None;
                        this.focus.focus(window, cx);
                        cx.notify();
                    })),
                ]))
                .into_any_element(),
            None => self.provider_list(page, cx),
        }
    }

    /// Lista de provedores com estado, origem e a ação de cada linha.
    fn provider_list(&mut self, mut page: Div, cx: &mut Context<Self>) -> AnyElement {
        // Três controles numa linha só não cabem na largura da seção com a
        // sidebar aberta: sem quebra, os rótulos comprimem e ficam ilegíveis.
        let mut actions = div().flex().flex_row().flex_wrap().items_center().gap(px(8.));
        match self.providers.clone() {
            ProvidersView::Idle | ProvidersView::Loading => {
                page = page.child(note("Consultando o Pi…"));
            }
            ProvidersView::Unavailable(reason) => {
                page = page.child(note(&reason));
                // O caminho de saída quando o pacote não foi encontrado: a
                // pessoa aponta o executável sem precisar editar variável.
                actions = actions.child(control(
                    "providers-choose-pi",
                    "Escolher executável do Pi",
                    cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::ChoosePi)),
                ).flex_none());
                // Sem Node ou sem pacote, o terminal ainda resolve: o Dish
                // abre o Pi e a pessoa usa /login lá.
                actions = actions.child(control("providers-terminal", "Abrir o Pi num terminal", cx.listener(
                    |this, _, _, cx| {
                        this.provider_notice = crate::installation::launch_pi_login()
                            .err()
                            .map(|error| format!("{error:#}"));
                        cx.notify();
                    },
                )).flex_none());
            }
            ProvidersView::Ready(snapshot) => {
                if snapshot.empty() {
                    // Primeiro uso sem credencial: o Pi instalado não basta, e o
                    // usuário precisa saber o que fazer agora.
                    page = page.child(note(
                        "Nenhum provedor conectado, então ainda não há modelo para conversar. Comece pela linha do provedor que você usa.",
                    ));
                }
                let mut providers = snapshot.providers.clone();
                providers.sort_by(|a, b| b.configured.cmp(&a.configured).then_with(|| a.id.cmp(&b.id)));
                for provider in providers {
                    let status = if provider.configured {
                        provider.source.map(|source| source.label()).unwrap_or("conectado")
                    } else {
                        "sem credencial"
                    };
                    let methods = provider.methods();
                    let mut row = div().flex().flex_row().items_center().gap(px(12.))
                        .px(px(12.)).py(px(8.)).rounded(theme::r_control())
                        .bg(theme::surface_2()).border_1().border_color(theme::line_soft())
                        .child(div().flex_1().min_w(px(0.)).truncate()
                            .font_family(theme::FONT_MONO).text_size(px(theme::TEXT_SM))
                            .text_color(if provider.configured { theme::text() } else { theme::faint() })
                            .child(SharedString::from(provider.id.clone())))
                        .child(div().flex_none().text_size(px(theme::TEXT_XS))
                            .text_color(if provider.configured { theme::dim() } else { theme::faint() })
                            .child(SharedString::from(status)))
                        .child(div().flex_none().font_family(theme::FONT_MONO)
                            .text_size(px(theme::TEXT_XS)).text_color(theme::faint())
                            .child(SharedString::from(model_count(provider.models))));
                    if provider.configured {
                        if self.confirming_logout.as_deref() == Some(provider.id.as_str()) {
                            let id = provider.id.clone();
                            row = row
                                .child(control(
                                    SharedString::from(format!("providers-logout-confirm-{}", provider.id)),
                                    "Confirmar remoção",
                                    cx.listener(move |this, _, _, cx| this.logout_provider(id.clone(), cx)),
                                ))
                                .child(control(
                                    SharedString::from(format!("providers-logout-cancel-{}", provider.id)),
                                    "Cancelar",
                                    cx.listener(|this, _, _, cx| {
                                        this.confirming_logout = None;
                                        cx.notify();
                                    }),
                                ));
                        } else {
                            let id = provider.id.clone();
                            row = row.child(control(
                                SharedString::from(format!("providers-logout-{}", provider.id)),
                                "Remover credencial",
                                cx.listener(move |this, _, _, cx| {
                                    this.confirming_logout = Some(id.clone());
                                    this.provider_notice = None;
                                    cx.notify();
                                }),
                            ));
                        }
                    } else if !methods.is_empty() {
                        let id = provider.id.clone();
                        row = row.child(control(
                            SharedString::from(format!("providers-connect-{}", provider.id)),
                            "Conectar",
                            cx.listener(move |this, _, _, cx| {
                                this.login = Some(LoginStep::ChoosingMethod { provider: id.clone() });
                                this.provider_notice = None;
                                cx.notify();
                            }),
                        ));
                    }
                    page = page.child(row);
                }
                let mut summary = match snapshot.pi_version.clone() {
                    Some(version) => format!("Catálogo do Pi {version}"),
                    None => "Catálogo do Pi".to_string(),
                };
                summary.push_str(&format!(
                    " · {} provedores · {} conectados",
                    snapshot.providers.len(),
                    snapshot.ready(),
                ));
                page = page.child(note(&summary));
            }
        }
        actions = actions.child(control("providers-retry", "Verificar de novo", cx.listener(
            |this, _, _, cx| this.refresh_providers(cx),
        )).flex_none());
        page.child(actions).child(note(
            "Provedores locais e endpoints compatíveis são configurados em models.json, como o Pi descreve em docs/models.md.",
        )).into_any_element()
    }

    /// Escolha de como entrar: chave de API ou conta do provedor.
    fn method_chooser(&self, mut page: Div, provider: &str, cx: &mut Context<Self>) -> AnyElement {
        let methods = match &self.providers {
            ProvidersView::Ready(snapshot) => snapshot
                .providers
                .iter()
                .find(|entry| entry.id == provider)
                .map(crate::providers::Provider::methods)
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        page = page.child(note(&format!("Como entrar em {provider}?")));
        let provider_owned = provider.to_string();
        for (kind, label) in methods {
            let provider = provider_owned.clone();
            page = page.child(
                div().flex().flex_row().items_center().gap(px(12.))
                    .px(px(12.)).py(px(8.)).rounded(theme::r_control())
                    .bg(theme::surface_2()).border_1().border_color(theme::line_soft())
                    .child(div().flex_1().min_w(px(0.)).truncate()
                        .text_size(px(theme::TEXT_SM)).text_color(theme::text())
                        .child(SharedString::from(label)))
                    .child(control(
                        SharedString::from(format!("providers-method-{}", kind.wire())),
                        "Usar",
                        cx.listener(move |this, _, _, cx| {
                            this.start_login(provider.clone(), kind, cx);
                        }),
                    )),
            );
        }
        page.child(actions_row(vec![control("providers-back", "Voltar", cx.listener(
            |this, _, _, cx| {
                this.login = None;
                cx.notify();
            },
        ))])).into_any_element()
    }

    /// O login em andamento: pergunta pendente, avisos e a saída de emergência.
    fn login_running(
        &self,
        mut page: Div,
        provider: &str,
        ask: Option<crate::provider_login::Ask>,
        notices: &[crate::provider_login::Notice],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::provider_login::{AskKind, Notice};
        page = page.child(note(&format!("Conectando {provider}…")));
        for notice in notices {
            match notice {
                Notice::Info { message, links } => {
                    if let Some(message) = message {
                        page = page.child(note(message));
                    }
                    for link in links {
                        let url = link.clone();
                        page = page.child(actions_row(vec![control(
                            "providers-open-link",
                            "Abrir ajuda no navegador",
                            cx.listener(move |_, _, _, _| crate::platform::open_url(&url)),
                        )]));
                    }
                }
                Notice::AuthUrl { url, instructions } => {
                    if let Some(instructions) = instructions {
                        page = page.child(note(instructions));
                    }
                    let open = url.clone();
                    page = page.child(note(url)).child(actions_row(vec![control(
                        "providers-open-url",
                        "Abrir no navegador",
                        cx.listener(move |_, _, _, _| crate::platform::open_url(&open)),
                    )]));
                }
                Notice::DeviceCode { user_code, verification_uri, .. } => {
                    page = page.child(note(&format!("Código: {user_code}")));
                    if let Some(uri) = verification_uri {
                        let uri = uri.clone();
                        page = page.child(note(&format!("Em {uri}"))).child(actions_row(vec![control(
                            "providers-open-device",
                            "Abrir a página do código",
                            cx.listener(move |_, _, _, _| crate::platform::open_url(&uri)),
                        )]));
                    }
                }
                Notice::Progress { message } => {
                    page = page.child(note(message.as_deref().unwrap_or("Aguardando o provedor…")));
                }
            }
        }
        match ask {
            Some(ask) => {
                page = page.child(note(&ask.message));
                if ask.kind == AskKind::Select {
                    // Cada opção é uma resposta: o valor vai como o id do Pi.
                    for option in &ask.options {
                        let id = option.id.clone();
                        let mut row = div().flex().flex_row().items_center().gap(px(12.))
                            .px(px(12.)).py(px(8.)).rounded(theme::r_control())
                            .bg(theme::surface_2()).border_1().border_color(theme::line_soft())
                            .child(div().flex_1().min_w(px(0.)).child(div().truncate()
                                .text_size(px(theme::TEXT_SM)).text_color(theme::text())
                                .child(SharedString::from(option.label.clone()))));
                        if let Some(description) = &option.description {
                            row = row.child(div().flex_none().font_family(theme::FONT_MONO)
                                .text_size(px(theme::TEXT_XS)).text_color(theme::faint())
                                .child(SharedString::from(description.clone())));
                        }
                        page = page.child(row.child(control(
                            SharedString::from(format!("providers-option-{}", option.id)),
                            "Escolher",
                            cx.listener(move |this, _, _, cx| this.submit_answer(Some(id.clone()), cx)),
                        )));
                    }
                } else {
                    let placeholder = ask.placeholder.clone().unwrap_or_default();
                    let hint = match ask.kind {
                        AskKind::Secret => "O valor não é guardado pelo Dish nem aparece em registro.",
                        AskKind::ManualCode => "Cole o código devolvido pelo provedor.",
                        _ => "",
                    };
                    page = page.child(
                        div().id("providers-answer").p(px(8.)).rounded(theme::r_technical())
                            .bg(theme::inset()).border_1().border_color(theme::line_soft())
                            .child(self.answer.clone()),
                    );
                    if !placeholder.is_empty() {
                        page = page.child(note(&format!("Exemplo: {placeholder}")));
                    }
                    if !hint.is_empty() {
                        page = page.child(note(hint));
                    }
                    page = page.child(actions_row(vec![control(
                        "providers-send",
                        "Enviar",
                        cx.listener(|this, _, window, cx| {
                            this.focus.focus(window, cx);
                            this.submit_answer(None, cx);
                        }),
                    )]));
                }
            }
            None => {
                page = page.child(note("Aguardando o provedor…"));
            }
        }
        page.child(actions_row(vec![control(
            "providers-cancel",
            "Cancelar",
            cx.listener(|this, _, window, cx| {
                this.focus.focus(window, cx);
                this.cancel_login(cx);
            }),
        )]))
        .into_any_element()
    }

    /// Abre o processo do adaptador e começa o login de um provedor.
    fn start_login(
        &mut self,
        provider: String,
        kind: crate::provider_login::AuthKind,
        cx: &mut Context<Self>,
    ) {
        match crate::provider_login::LoginSession::start(&provider, kind) {
            Ok(session) => {
                let events = session.events();
                self.session = Some(session);
                self.login = Some(LoginStep::Running {
                    provider,
                    ask: None,
                    notices: Vec::new(),
                });
                self.answer.update(cx, |editor, cx| editor.clear(cx));
                cx.notify();
                cx.spawn(async move |this, cx| {
                    while let Ok(message) = events.recv().await {
                        let finished = this
                            .update(cx, |this, cx| this.apply_login_message(message, cx))
                            .unwrap_or(true);
                        if finished {
                            break;
                        }
                    }
                })
                .detach();
            }
            Err(error) => {
                self.login = Some(LoginStep::Failed {
                    provider,
                    message: format!("{error:#}"),
                });
                cx.notify();
            }
        }
    }

    /// Aplica uma mensagem do adaptador. Devolve `true` quando o login terminou.
    fn apply_login_message(
        &mut self,
        message: crate::provider_login::Message,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::provider_login::Message;
        match message {
            Message::Ask(ask) => {
                if let Some(LoginStep::Running { ask: slot, .. }) = self.login.as_mut() {
                    *slot = Some(ask);
                }
                self.answer.update(cx, |editor, cx| editor.clear(cx));
                self.answer_focus = true;
            }
            Message::Notice(notice) => {
                if let Some(LoginStep::Running { notices, .. }) = self.login.as_mut() {
                    notices.push(notice);
                }
            }
            Message::Done(outcome) => {
                self.session = None;
                self.login = Some(LoginStep::Done {
                    provider: outcome.provider,
                });
                self.refresh_providers(cx);
                // Os modelos das conversas abertas só são atualizados quando
                // pedimos de novo ao Pi; ver `SettingsEvent::ProvidersChanged`.
                cx.emit(SettingsEvent::ProvidersChanged);
                return true;
            }
            Message::Failed(message) => {
                let provider = self.current_provider().unwrap_or_default();
                self.session = None;
                self.login = Some(LoginStep::Failed { message, provider });
                return true;
            }
            Message::Ignored => {}
        }
        cx.notify();
        false
    }

    fn current_provider(&self) -> Option<String> {
        match self.login.as_ref()? {
            LoginStep::Running { provider, .. }
            | LoginStep::Failed { provider, .. }
            | LoginStep::Done { provider } => Some(provider.clone()),
            LoginStep::ChoosingMethod { .. } => None,
        }
    }

    /// Envia a resposta de uma pergunta. `None` usa o que está no campo.
    ///
    /// O valor sai daqui direto para o Pi: não é copiado para o estado, para as
    /// preferências nem para registro, e o campo é limpo logo depois.
    fn submit_answer(&mut self, value: Option<String>, cx: &mut Context<Self>) {
        let Some(LoginStep::Running { ask: Some(ask), .. }) = self.login.as_ref() else {
            return;
        };
        let prompt = ask.prompt;
        let value = value.unwrap_or_else(|| self.answer.read(cx).text(cx));
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if let Err(error) = session.reply(prompt, &value) {
            let provider = self.current_provider().unwrap_or_default();
            self.session = None;
            self.login = Some(LoginStep::Failed {
                provider,
                message: format!("{error:#}"),
            });
            cx.notify();
            return;
        }
        self.answer.update(cx, |editor, cx| editor.clear(cx));
        if let Some(LoginStep::Running { ask, .. }) = self.login.as_mut() {
            *ask = None;
        }
        cx.notify();
    }

    /// Cancela o login e encerra o processo do adaptador.
    fn cancel_login(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.session.as_mut() {
            let _ = session.cancel();
        }
        // Descartar a sessão mata o processo filho (ver `Drop`).
        self.session = None;
        self.answer.update(cx, |editor, cx| editor.clear(cx));
        // Cancelar volta para a lista: a seção não fecha nem fica presa.
        self.login = None;
        cx.notify();
    }

    /// Remove a credencial guardada de um provedor, no Pi.
    fn logout_provider(&mut self, provider: String, cx: &mut Context<Self>) {
        self.confirming_logout = None;
        self.provider_notice = Some(format!("Removendo a credencial de {provider}…"));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let target = provider.clone();
            let result = cx
                .background_executor()
                .spawn(async move { crate::provider_login::logout(&target) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let removed = result.is_ok();
                this.provider_notice = Some(match result {
                    Ok(()) => format!(
                        "Credencial de {provider} removida do Pi. Remover aqui não revoga a chave no serviço."
                    ),
                    Err(error) => format!("{error:#}"),
                });
                if removed {
                    cx.emit(SettingsEvent::ProvidersChanged);
                }
                this.refresh_providers(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn suspend(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.previous_focus = None;
        self.search.update(cx, |editor, cx| editor.clear(cx));
        // Fechar a tela não pode deixar um login pendurado nem um processo vivo.
        self.session = None;
        self.login = None;
        self.confirming_logout = None;
        self.answer.update(cx, |editor, cx| editor.clear(cx));
        // Reabrir a tela relê o estado: a credencial pode ter mudado no Pi.
        self.providers = ProvidersView::Idle;
        cx.notify();
    }
}

fn column() -> Div { div().flex().flex_col().gap(px(12.)).min_w(px(0.)) }

/// Agrupa os modelos por provedor, com os nomes em ordem alfabética dentro de
/// cada grupo. O `BTreeMap` mantém os provedores em ordem alfabética também.
fn grouped_models(models: &[(String, String)]) -> BTreeMap<&str, Vec<&str>> {
    let mut providers: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (provider, id) in models {
        providers.entry(provider.as_str()).or_default().push(id.as_str());
    }
    for ids in providers.values_mut() {
        ids.sort_unstable();
    }
    providers
}

/// Estado de um grupo no seletor: tudo ligado, tudo desligado ou misto.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SwitchState { Off, Mixed, On }

/// Estado do grupo a partir das chaves dos seus modelos.
fn group_state(keys: &[String], hidden: &BTreeSet<String>) -> SwitchState {
    let hidden_count = keys.iter().filter(|key| hidden.contains(*key)).count();
    if hidden_count == 0 {
        SwitchState::On
    } else if hidden_count == keys.len() {
        SwitchState::Off
    } else {
        SwitchState::Mixed
    }
}

/// Chave de liga/desliga: trilho com o botão, na linguagem da interface
/// (acento quando ligada, linha quando desligada, botão centrado quando misto).
fn switch(id: impl Into<ElementId>, state: SwitchState, click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Stateful<Div> {
    let on = state == SwitchState::On;
    let id: ElementId = id.into();
    let knob_color = match state {
        SwitchState::On => theme::on_accent(),
        SwitchState::Mixed => theme::dim(),
        SwitchState::Off => theme::faint(),
    };
    div()
        .id(id.clone())
        .relative()
        .flex_none()
        .w(px(34.))
        .h(px(18.))
        .rounded(px(9.))
        .cursor_pointer()
        .bg(if on { theme::accent() } else { theme::inset() })
        .border_1()
        .border_color(if on { theme::accent() } else { theme::line_strong() })
        .child(match state {
            SwitchState::Mixed => crate::ui::switch_knob_mixed(knob_color).into_any_element(),
            _ => crate::ui::switch_knob((id, if on { "knob-on" } else { "knob-off" }), on, knob_color)
                .into_any_element(),
        })
        // O interruptor é o alvo próprio; em linhas toda-clicáveis ele não
        // pode repetir o mesmo giro duas vezes.
        .on_click(move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            click(event, window, cx);
        })
}
fn note(value: &str) -> Div {
    div()
        .text_size(px(theme::TEXT_SM))
        .text_color(theme::dim())
        .child(SharedString::from(value.to_owned()))
}
/// Fileira de ações: mesma medida em toda a seção.
fn actions_row(actions: Vec<Stateful<Div>>) -> Div {
    div().flex().flex_row().items_center().gap(px(8.)).children(actions)
}

fn control(id: impl Into<ElementId>, label: impl Into<SharedString>, click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Stateful<Div> {
    // O padding desconta a borda: a caixa final fica com as mesmas medidas da
    // versão sem fio, e o conteúdo abaixo não desliza um pixel.
    div().id(id).px(px(11.)).py(px(7.)).rounded(theme::r_control())
        .bg(theme::surface_2()).border_1().border_color(theme::line_soft())
        .text_color(theme::dim())
        .cursor_pointer().hover(|s| s.bg(theme::hover()).text_color(theme::text()))
        .on_click(click).child(label.into())
}
/// Uma linha do catálogo de temas: amostras, nome, aparência e a escolha.
fn theme_row(item: &theme::Theme, selected: bool, cx: &mut Context<Settings>) -> AnyElement {
    let mut swatches = div().flex().flex_row().gap(px(2.)).flex_none();
    for color in [item.bg, item.surface_2, item.text, item.effort[3]] {
        swatches = swatches.child(
            div()
                .size(px(14.))
                .rounded(px(3.))
                .bg(color)
                .border_1()
                .border_color(item.line),
        );
    }
    let appearance = if item.dark { "Escuro" } else { "Claro" };
    let detail = if item.user {
        format!("{appearance} · {}.json", item.id.trim_start_matches("user:"))
    } else {
        appearance.to_string()
    };
    let id = item.id.clone();
    div()
        .id(SharedString::from(format!("theme-{id}")))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.))
        .px(px(12.))
        .py(px(8.))
        .rounded(theme::r_control())
        .border_1()
        .cursor_pointer()
        .when(selected, |el| {
            el.bg(theme::surface_2()).border_color(theme::line())
        })
        .when(!selected, |el| {
            el.border_color(theme::line_soft())
                .hover(|style| style.bg(theme::surface_2()))
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            this.theme = Some(id.clone());
            cx.emit(SettingsEvent::Theme(id.clone()));
            cx.notify();
        }))
        .child(swatches)
        .child(
            div().flex_1().min_w(px(0.)).flex().flex_col().gap(px(2.))
                .child(
                    div().text_size(px(theme::TEXT_SM)).text_color(theme::text())
                        .truncate().child(SharedString::from(item.name.clone())),
                )
                .child(
                    div().text_size(px(theme::TEXT_XS)).text_color(theme::faint())
                        .truncate().child(SharedString::from(detail)),
                ),
        )
        .when(selected, |el| el.child(crate::ui::icons::icon(
            crate::ui::icons::Icon::Check,
            13.,
            theme::accent(),
        )))
        .into_any_element()
}

/// Um cursor do controle segmentado da Aparência, na mesma linguagem dos
/// filtros da navegação: poço escuro com o item ativo em superfície clara.
fn appearance_segment(
    id: &'static str,
    label: &'static str,
    active: bool,
    cx: &mut Context<Settings>,
    run: impl Fn(&mut Settings, &mut Context<Settings>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .min_w(px(0.))
        .px(px(theme::S2))
        .py(px(3.))
        .rounded(theme::r_small())
        .when(active, |el| {
            el.bg(theme::surface_2())
                .border_1()
                .border_color(theme::line())
                .text_color(theme::text())
                .font_weight(FontWeight::MEDIUM)
        })
        .when(!active, |el| {
            el.cursor_pointer()
                .border_1()
                .border_color(transparent_black())
                .text_color(theme::faint())
                .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
        })
        .text_size(px(theme::TEXT_XS))
        .truncate()
        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
            run(this, cx);
        }))
        .child(SharedString::from(label.to_owned()))
        .into_any_element()
}

/// "1 modelo" não é "1 modelos": a contagem aparece em toda linha.
fn model_count(models: usize) -> String {
    if models == 1 {
        "1 modelo".to_string()
    } else {
        format!("{models} modelos")
    }
}

/// Título de página: uma linha, sem rótulo de seção novo — é a mesma palavra
/// do menu à esquerda.
fn page_title(label: &str) -> Div {
    div()
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::text())
        .child(SharedString::from(label.to_owned()))
}

impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open { return div().into_any_element(); }
        let height = (f32::from(window.bounds().size.height) - 48.).max(300.);
        let width = (f32::from(window.bounds().size.width) - 48.).min(840.);
        let compact = width < 680.;
        let mut menu = column().w(px(184.)).flex_none()
            .when(compact, |el| el.w_full().flex_row().flex_wrap().gap(px(theme::S1)));
        for (section, label, id) in [
            (Section::App, "Aplicativo", "settings-app"),
            (Section::Appearance, "Aparência", "settings-appearance"),
            (Section::Providers, "Provedores", "settings-providers"),
            (Section::Models, "Modelos", "settings-models"),
            (Section::Keys, "Atalhos de teclado", "settings-keys"),
            (Section::About, "Sobre / Pi", "settings-about"),
        ] {
            menu = menu.child(control(id, label, cx.listener(move |this, _, window, cx| {
                this.section = section;
                if section == Section::Keys { this.search.read(cx).focus_handle.clone().focus(window, cx); }
                else { this.focus.focus(window, cx); }
                if section == Section::Providers { this.refresh_providers(cx); }
                if section == Section::About { this.refresh_pi_version(cx); }
                cx.notify();
            })).when(self.section == section, |el| el
                // A escolha na esquerda é neutra: o accent é do raciocínio,
                // e não muda de cor porque uma tela de configurações abriu.
                .bg(theme::surface_2()).border_color(theme::line()).text_color(theme::text())));
        }
        let content = match self.section {
            Section::App => {
                let mut page = column().child(page_title("Aplicativo"));
                for (id, label, current) in [
                    ("details", "Painel de detalhes", self.details),
                    ("navigation", "Navegação de sessões", self.navigation),
                    ("thinking", "Raciocínio expandido", self.thinking),
                    ("generated_titles", "Títulos gerados por modelo", self.titles),
                ] {
                    // Linha inteira clicável — o interruptor é o estado, não
                    // o único alvo.
                    page = page.child(
                        div().id(id).flex().flex_row().items_center().gap(px(12.))
                            .px(px(12.)).py(px(8.)).rounded(theme::r_control())
                            .bg(theme::surface_2()).border_1().border_color(theme::line_soft())
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::hover()).border_color(theme::line()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                match id {
                                    "details" => this.details = !current,
                                    "navigation" => this.navigation = !current,
                                    "thinking" => this.thinking = !current,
                                    _ => this.titles = !current,
                                }
                                cx.emit(SettingsEvent::Preference(id, !current)); cx.notify();
                            }))
                            .child(div().flex_1().min_w(px(0.))
                                .text_size(px(theme::TEXT_SM)).text_color(theme::dim())
                                .child(SharedString::from(label.to_owned())))
                            .child(switch(SharedString::from(format!("{id}-switch")),
                                if current { SwitchState::On } else { SwitchState::Off },
                                cx.listener(move |this, _, _, cx| {
                                    match id {
                                        "details" => this.details = !current,
                                        "navigation" => this.navigation = !current,
                                        "thinking" => this.thinking = !current,
                                        _ => this.titles = !current,
                                    }
                                    cx.emit(SettingsEvent::Preference(id, !current)); cx.notify();
                                }))),
                    );
                }
                page = page.child(note("Gera um nome curto para o primeiro pedido relevante com o modelo da sessão (gasta tokens) e salva no Pi ao terminar a rodada. Nomes já definidos são preservados. Se falhar, mantém o texto do pedido e avisa; desative e reative para tentar novamente."));
                page.into_any_element()
            }
            Section::Appearance => {
                let mut page = column().child(page_title("Aparência")).child(note(
                    "Vale para todas as janelas e é salva entre execuções. O modo Sistema acompanha o tema do desktop; sem preferência do sistema, o Dish fica claro.",
                ));
                let mut segments = div()
                    .flex()
                    .flex_row()
                    .items_stretch()
                    .p(px(2.))
                    .gap(px(1.))
                    .rounded(theme::r_control())
                    .bg(theme::inset())
                    .border_1()
                    .border_color(theme::line_soft());
                for (mode, label, id) in [
                    (AppearanceMode::System, "Sistema", "appearance-system"),
                    (AppearanceMode::Dark, "Escuro", "appearance-dark"),
                    (AppearanceMode::Light, "Claro", "appearance-light"),
                ] {
                    // O segmento mostra a aparência em vigor; a lista mostra
                    // o tema fixado (ou o padrão dela, quando não há fixado).
                    let active = self.appearance == mode;
                    segments = segments.child(appearance_segment(
                        id,
                        label,
                        active,
                        cx,
                        move |this, cx| {
                            this.appearance = mode;
                            this.theme = None;
                            cx.emit(SettingsEvent::Appearance(mode));
                            cx.notify();
                        },
                    ));
                }
                page = page.child(
                    div().flex().flex_row().items_center().gap(px(12.))
                        .px(px(12.)).py(px(8.)).rounded(theme::r_control())
                        .bg(theme::surface_2()).border_1().border_color(theme::line_soft())
                        .child(div().flex_1().min_w(px(0.)).text_size(px(theme::TEXT_SM)).text_color(theme::dim()).child("Tema padrão"))
                        .child(segments),
                );
                let reduced = cx.reduce_motion();
                page = page.child(
                    div().id("reduce-motion").flex().items_center().gap(px(theme::S4))
                        .py(px(theme::S3)).border_b_1().border_color(theme::line_soft())
                        .cursor_pointer().focusable().tab_index(0)
                        .hover(|style| style.bg(theme::hover()))
                        .focus(|style| style.shadow(theme::focus_ring()))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.set_reduce_motion(!reduced);
                            cx.emit(SettingsEvent::Preference("reduce_motion", !reduced));
                            cx.notify();
                        }))
                        .on_key_down(cx.listener(move |_, event: &KeyDownEvent, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                cx.set_reduce_motion(!reduced);
                                cx.emit(SettingsEvent::Preference("reduce_motion", !reduced));
                                cx.notify();
                                cx.stop_propagation();
                            } else if event.keystroke.key == "tab" {
                                if event.keystroke.modifiers.shift { window.focus_prev(cx); } else { window.focus_next(cx); }
                                cx.stop_propagation();
                            }
                        }))
                        .child(div().flex_1().min_w(px(0.)).flex().flex_col().gap(px(theme::S1))
                            .child(div().text_color(theme::text()).child("Reduzir movimento"))
                            .child(note("Desativa transições e mantém indicadores de atividade estáticos.")))
                        .child(switch("reduce-motion-switch",
                            if reduced { SwitchState::On } else { SwitchState::Off },
                            cx.listener(move |_, _, _, cx| {
                                cx.set_reduce_motion(!reduced);
                                cx.emit(SettingsEvent::Preference("reduce_motion", !reduced));
                                cx.notify();
                            }))),
                );
                let mut themes = div().flex().flex_col().gap(px(6.));
                for (label, dark) in [("Escuros", true), ("Claros", false)] {
                    let mut group = div().flex().flex_col().gap(px(6.))
                        .child(div().text_size(px(theme::TEXT_SM)).text_color(theme::dim())
                            .child(label));
                    for item in theme::themes().iter().filter(|item| item.dark == dark) {
                        // Sem tema fixado, o marcado é o que está em uso agora —
                        // inclusive quando `Sistema` escolheu pelo desktop.
                        let selected = self
                            .theme
                            .as_deref()
                            .map_or(theme::active().id == item.id, |id| id == item.id);
                        group = group.child(theme_row(item, selected, cx));
                    }
                    themes = themes.child(group);
                }
                page = page.child(themes).child(
                    div().flex().flex_row().items_center().gap(px(12.)).flex_wrap()
                        .child(control("themes-open", "Abrir pasta de temas", cx.listener(
                            |_, _, _, cx| cx.emit(SettingsEvent::OpenThemes),
                        )))
                        .child(note("Um arquivo .json por tema, lido quando o Dish abre (docs/themes.md).")),
                );
                page.into_any_element()
            }
            Section::Providers => self.providers_page(window, cx),
            Section::Models => {
                let mut page = column().child("Modelos").child(note(
                    "Oculta modelos apenas no seletor do Dish. O Pi continua com a lista completa; nada é alterado na configuração dele.",
                ));
                if self.models.is_empty() {
                    page = page.child(note(
                        "Nenhum modelo disponível ainda: abra uma conversa para o Pi informar a lista.",
                    ));
                }
                let hidden_count = self
                    .models
                    .iter()
                    .filter(|(provider, id)| self.hidden.contains(&format!("{provider}/{id}")))
                    .count();
                let mut actions = div().flex().flex_row().gap(px(8.));
                if hidden_count > 0 {
                    actions = actions.child(control("models-show-all", "Mostrar todos", cx.listener(
                        |this, _, _, cx| {
                            this.hidden.clear();
                            cx.emit(SettingsEvent::ShowAllModels);
                            cx.notify();
                        },
                    )));
                }
                if hidden_count < self.models.len() {
                    let all: Vec<String> = self
                        .models
                        .iter()
                        .map(|(provider, id)| format!("{provider}/{id}"))
                        .collect();
                    actions = actions.child(control("models-hide-all", "Ocultar todos", cx.listener(
                        move |this, _, _, cx| {
                            for key in &all {
                                this.hidden.insert(key.clone());
                            }
                            cx.emit(SettingsEvent::ModelsVisibility(all.clone(), false));
                            cx.notify();
                        },
                    )));
                }
                if hidden_count > 0 || hidden_count < self.models.len() {
                    page = page.child(actions);
                }
                // Agrupado por provedor, em ordem alfabética dentro do grupo.
                for (provider, ids) in grouped_models(&self.models) {
                    let keys: Vec<String> = ids.iter().map(|id| format!("{provider}/{id}")).collect();
                    let state = group_state(&keys, &self.hidden);
                    let group_keys = keys.clone();
                    let mut group = column().gap(px(2.)).child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .font_family(theme::FONT_MONO)
                                    .text_size(px(theme::TEXT_MICRO))
                                    .text_color(theme::faint())
                                    .child(SharedString::from(provider.to_string())),
                            )
                            .child(switch(
                                SharedString::from(format!("provider-visibility-{provider}")),
                                state,
                                cx.listener(move |this, _, _, cx| {
                                    // Ligado esconde o grupo inteiro; qualquer outro
                                    // estado devolve todos os modelos do provedor.
                                    let hide = state == SwitchState::On;
                                    for key in &group_keys {
                                        if hide { this.hidden.insert(key.clone()); } else { this.hidden.remove(key); }
                                    }
                                    cx.emit(SettingsEvent::ModelsVisibility(group_keys.clone(), !hide));
                                    cx.notify();
                                }),
                            )),
                    );
                    for id in ids {
                        let key = format!("{provider}/{id}");
                        let row_id = SharedString::from(format!("model-visibility-{key}"));
                        let visible = !self.hidden.contains(&key);
                        let toggled = key.clone();
                        group = group.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .truncate()
                                        .font_family(theme::FONT_MONO)
                                        .text_size(px(theme::TEXT_SM))
                                        .text_color(if visible { theme::text() } else { theme::faint() })
                                        .child(SharedString::from(id.to_string())),
                                )
                                .child(switch(
                                    row_id,
                                    if visible { SwitchState::On } else { SwitchState::Off },
                                    cx.listener(move |this, _, _, cx| {
                                        let visible = !this.hidden.contains(&toggled);
                                        if visible { this.hidden.insert(toggled.clone()); } else { this.hidden.remove(&toggled); }
                                        cx.emit(SettingsEvent::ModelVisibility(toggled.clone(), !visible));
                                        cx.notify();
                                    }),
                                )),
                        );
                    }
                    page = page.child(group);
                }
                page.into_any_element()
            }
            Section::Keys => {
                let focus = self.search.read(cx).focus_handle.clone();
                let click_focus = focus.clone();
                let query = self.search.read(cx).text(cx).trim().to_lowercase();
                let mut page = column().child(page_title("Atalhos de teclado")).child(div().id("settings-key-search")
                    .key_context("DishHelpSearch").track_focus(&focus)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| click_focus.focus(window, cx))
                    .map(editor::standard_actions(self.search.clone()))
                    .p(px(8.)).rounded(theme::r_technical()).bg(theme::inset())
                    .border_1().border_color(theme::line_soft()).child(self.search.clone()));
                let mut count = 0;
                for (key, description) in crate::ui::overlays::keyboard_shortcuts() {
                    if !query.is_empty() && !format!("{key} {description}").to_lowercase().contains(&query) { continue; }
                    count += 1;
                    page = page.child(div().flex().gap(px(12.)).child(crate::ui::keycap_all(key)).child(note(description)));
                }
                if count == 0 { page = page.child(note("Nenhum atalho encontrado.")); }
                page.into_any_element()
            }
            Section::About => {
                let installed = if self.pi_version_loading && self.pi.installed.is_none() {
                    "consultando…".to_string()
                } else {
                    crate::pi_update::display_version(self.pi.installed.as_deref())
                };
                let last_check = crate::pi_update::format_last_check(self.pi.last_check);
                let mut status = self.pi.notice.clone().unwrap_or_default();
                if status.is_empty() {
                    status = if self.pi.checking {
                        "Verificando atualizações do Pi…".to_string()
                    } else {
                        match (self.pi.installed.as_deref(), self.pi.latest.as_deref()) {
                            (Some(installed), Some(latest))
                                if crate::pi_update::is_newer(installed, latest) =>
                            {
                                format!("Pi {latest} disponível (instalado: {installed}).")
                            }
                            (Some(installed), Some(latest)) if installed == latest => {
                                format!("Você está na versão mais recente do Pi ({installed}).")
                            }
                            (None, Some(latest)) => format!(
                                "Última versão publicada do Pi: {latest}. Versão instalada desconhecida."
                            ),
                            (_, Some(latest)) => format!("Última versão publicada do Pi: {latest}."),
                            _ => String::new(),
                        }
                    };
                }
                let auto_check = self.pi.auto_check;
                let auto_install = self.pi.auto_install;
                // A página Sobre precisa caber sem rolagem na janela padrão:
                // cada explicação fica em uma linha e cada comando secundário
                // mantém a sua própria linha.
                let mut page = column()
                    .child(page_title("Dish"))
                    .child(note(&format!("Versão do Dish: {}", crate::version::display_version())))
                    // Os pares de valor分离tens: rótulo em uma linha, path na
                    // seguinte — caminhos longos não roubam a leitura do rótulo.
                    .child(about_pair("Pi selecionado", &crate::installation::program()))
                    .child(div().flex().flex_row().flex_wrap().gap(px(theme::S5))
                        .child(about_pair("Versão do Pi", &installed))
                        .child(about_pair("Última verificação do Pi", &last_check)))
                    .child(note("Pi mantém as sessões e as credenciais; o Dish não altera os arquivos do Pi."));
                page = page
                    .child(div().flex().flex_col().gap(px(theme::S1))
                        .child(div().id("settings-pi-auto-check-row").flex().flex_col().gap(px(theme::S1))
                            .px(px(12.)).py(px(8.)).rounded(theme::r_control())
                            .bg(theme::surface_2()).border_1().border_color(theme::line_soft())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::hover()).border_color(theme::line()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pi.auto_check = !auto_check;
                                cx.emit(SettingsEvent::Preference("pi_auto_check", !auto_check));
                                cx.notify();
                            }))
                            .child(div().text_size(px(theme::TEXT_SM))
                                .text_color(theme::text()).child("Verificar automaticamente"))
                            .child(div().flex().flex_row().items_center().gap(px(12.))
                                .child(div().flex_1().min_w(px(0.)).text_size(px(theme::TEXT_XS))
                                    .text_color(theme::dim())
                                    .child("Desligado por padrão; consulta o npm do Pi uma vez por dia e falhas ficam no log."))
                                .child(switch("settings-pi-auto-check",
                                    if auto_check { SwitchState::On } else { SwitchState::Off },
                                    cx.listener(move |this, _, _, cx| {
                                        this.pi.auto_check = !auto_check;
                                        cx.emit(SettingsEvent::Preference("pi_auto_check", !auto_check));
                                        cx.notify();
                                    }))))))
                    .child(div().flex().flex_col().gap(px(theme::S1))
                        .child(div().id("settings-pi-auto-install-row").flex().flex_col().gap(px(theme::S1))
                            .px(px(12.)).py(px(8.)).rounded(theme::r_control())
                            .bg(theme::surface_2()).border_1().border_color(theme::line_soft())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::hover()).border_color(theme::line()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pi.auto_install = !auto_install;
                                cx.emit(SettingsEvent::Preference("pi_auto_install", !auto_install));
                                cx.notify();
                            }))
                            .child(div().text_size(px(theme::TEXT_SM))
                                .text_color(theme::text()).child("Instalar automaticamente"))
                            .child(div().flex().flex_row().items_center().gap(px(12.))
                                .child(div().flex_1().min_w(px(0.)).text_size(px(theme::TEXT_XS))
                                    .text_color(theme::dim())
                                    .child("Nunca instala em silêncio: o instalador oficial abre num terminal e avisa ao terminar."))
                                .child(switch("settings-pi-auto-install",
                                    if auto_install { SwitchState::On } else { SwitchState::Off },
                                    cx.listener(move |this, _, _, cx| {
                                        this.pi.auto_install = !auto_install;
                                        cx.emit(SettingsEvent::Preference("pi_auto_install", !auto_install));
                                        cx.notify();
                                    }))))))
                    .child(actions_row(vec![
                        control("settings-pi-update", "Atualizar o Pi", cx.listener(
                            |_, _, _, cx| cx.emit(SettingsEvent::RequestPiUpdate),
                        )),
                        control("settings-pi-check", "Verificar agora", cx.listener(
                            |_, _, _, cx| cx.emit(SettingsEvent::RequestPiCheck),
                        )),
                    ]));
                if !status.is_empty() {
                    page = page.child(note(&status));
                }
                if !crate::pi_update::installer_supported() {
                    page = page.child(note("Neste sistema, o Dish não abre o instalador oficial do Pi. Atualize pelo instalador do Pi em https://pi.dev."));
                }
                page.child(control("settings-pi-choose", "Escolher executável do Pi", cx.listener(
                    |_, _, _, cx| cx.emit(SettingsEvent::ChoosePi),
                )))
                .child(control("settings-pi-setup", "Reabrir o assistente do Pi", cx.listener(
                    |_, _, _, cx| cx.emit(SettingsEvent::OpenPiSetup),
                )))
                .child(control("settings-docs", "Documentação oficial do Pi", |_, _, cx| cx.open_url("https://github.com/earendil-works/pi"))).into_any_element()
            }
        };
        let card = column().id("settings-card").w(px(width)).h(px(height.min(700.)))
            .p(px(theme::S5)).rounded(theme::r_surface()).border_1().border_color(theme::line_strong())
            .bg(theme::surface()).shadow(theme::shadow_overlay())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().flex().justify_between().items_center()
                .child(div().text_size(px(theme::TITLE)).font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text()).child("Configurações"))
                .child(control("settings-close", "Fechar · Esc", cx.listener(|this, _, window, cx| this.close(window, cx)))))
            .child(crate::ui::rule())
            .child(div().flex().flex_1().gap(px(theme::S5)).min_h(px(0.))
                .when(compact, |el| el.flex_col())
                .child(menu)
                .child(div().id("settings-content").flex_1().min_w(px(0.)).min_h(px(0.)).overflow_y_scroll()
                    .child(crate::ui::motion::reveal(div().child(content),
                        ElementId::NamedInteger("settings-page-enter".into(), self.section as u64)))));
        div().id("settings-scrim").occlude()
            // Overlays are input boundaries: the page behind must not scroll.
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .absolute().inset(px(0.)).flex().items_center().justify_center()
            .track_focus(&self.focus).key_context("DishSettings")
            .bg(theme::scrim()).backdrop_blur(px(theme::SCRIM_BLUR))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| { cx.stop_propagation(); this.close(window, cx); }))
            .on_action(cx.listener(|this, _: &crate::ui::Dismiss, window, cx| { cx.stop_propagation(); this.close(window, cx); }))
            .on_action(cx.listener(|this, _: &editor::Paste, window, cx| {
                cx.stop_propagation();
                let target = if this.search.read(cx).focus_handle.is_focused(window) {
                    Some(this.search.clone())
                } else { None };
                if let (Some(target), Some(text)) = (target, cx.read_from_clipboard().and_then(|item| item.text())) {
                    target.update(cx, |editor, cx| editor.insert_text(&text, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &editor::CopySelection, window, cx| {
                cx.stop_propagation();
                if !this.search.read(cx).focus_handle.is_focused(window) { return; }
                let target = &this.search;
                if let Some(text) = target.read(cx).selected_text(cx) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }))
            .on_action(cx.listener(|this, _: &editor::CutSelection, window, cx| {
                cx.stop_propagation();
                if !this.search.read(cx).focus_handle.is_focused(window) { return; }
                let target = &this.search;
                if let Some(text) = target.read(cx).selected_text(cx) {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    target.update(cx, |editor, cx| editor.delete_selection(cx));
                }
            }))
            // Capture actions from global shortcuts, without blocking field editing.
            .on_key_down(|event, _, cx| {
                if (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                    && ["n", "w", "k", "l", "b", "tab", "m", "e"].contains(&event.keystroke.key.as_str())
                {
                    cx.stop_propagation();
                }
            })
            .child(crate::ui::motion::reveal(card, "settings-enter")).into_any_element()
    }
}

/// Par rótulo/valor da tela "Sobre": o rótulo numa linha própria (curta,
/// legível) e o caminho qualquer abaixo, em fonte de máquina.
fn about_pair(label: &str, value: &str) -> Div {
    div().flex().flex_col().gap(px(2.)).min_w(px(0.))
        .child(div().text_size(px(theme::TEXT_SM)).text_color(theme::text()).child(SharedString::from(label.to_owned())))
        .child(div().min_w(px(0.)).truncate().font_family(theme::FONT_MONO)
            .text_size(px(theme::TEXT_XS)).text_color(theme::dim())
            .child(SharedString::from(value.to_owned())))
}


#[cfg(test)]
mod model_grouping_tests {
    use super::{group_state, grouped_models, SwitchState};
    use std::collections::BTreeSet;

    fn models(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(provider, id)| (provider.to_string(), id.to_string())).collect()
    }

    fn keys(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn group_state_reports_on_off_and_mixed() {
        let group = keys(&["a/1", "a/2"]);
        assert_eq!(group_state(&group, &BTreeSet::new()), SwitchState::On);
        assert_eq!(group_state(&group, &keys(&["a/1"]).into_iter().collect()), SwitchState::Mixed);
        assert_eq!(group_state(&group, &keys(&["a/1", "a/2"]).into_iter().collect()), SwitchState::Off);
        // Um grupo vazio aparece ligado, não misto.
        assert_eq!(group_state(&[], &BTreeSet::new()), SwitchState::On);
    }

    #[test]
    fn groups_by_provider_and_sorts_models_alphabetically() {
        let models = models(&[
            ("openai", "gpt-4"),
            ("anthropic", "sonnet"),
            ("openai", "gpt-3"),
            ("anthropic", "haiku"),
        ]);
        let grouped = grouped_models(&models);
        let providers: Vec<&str> = grouped.keys().copied().collect();
        assert_eq!(providers, ["anthropic", "openai"]);
        assert_eq!(grouped["anthropic"], ["haiku", "sonnet"]);
        assert_eq!(grouped["openai"], ["gpt-3", "gpt-4"]);
        assert!(grouped_models(&[]).is_empty());
    }
}
