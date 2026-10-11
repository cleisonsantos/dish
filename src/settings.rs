use crate::{editor::{self, Editor}, theme};
use gpui::{prelude::*, *};
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq)]
pub enum Section { App, Models, Keys, About }

pub enum SettingsEvent {
    Closed,
    Preference(&'static str, bool),
    /// Um modelo passou a visível (`true`) ou oculto (`false`) no seletor.
    ModelVisibility(String, bool),
    /// Limpar todos os modelos ocultos.
    ShowAllModels,
}
impl EventEmitter<SettingsEvent> for Settings {}

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
    /// `(provider, id)` de cada modelo que o Pi reportou.
    models: Vec<(String, String)>,
    hidden: BTreeSet<String>,
}

impl Settings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| Editor::new("", "Buscar atalhos…", window, cx));
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        Self {
            open: false, section: Section::App, focus: cx.focus_handle(),
            previous_focus: None, search,
            details: true, navigation: true, thinking: true, titles: false,
            models: Vec::new(), hidden: BTreeSet::new(),
        }
    }

    pub fn show(
        &mut self,
        section: Section,
        options: [bool; 4],
        models: Vec<(String, String)>,
        hidden: BTreeSet<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.open { self.previous_focus = window.focused(cx); }
        self.open = true;
        self.section = section;
        [self.details, self.navigation, self.thinking, self.titles] = options;
        self.models = models;
        self.hidden = hidden;
        self.focus.focus(window, cx);
        if section == Section::Keys { self.search.read(cx).focus_handle.clone().focus(window, cx); }
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = false;
        self.search.update(cx, |editor, cx| editor.clear(cx));
        if let Some(focus) = self.previous_focus.take() { focus.focus(window, cx); }
        cx.emit(SettingsEvent::Closed);
        cx.notify();
    }

    pub fn suspend(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.previous_focus = None;
        self.search.update(cx, |editor, cx| editor.clear(cx));
        cx.notify();
    }
}

fn column() -> Div { div().flex().flex_col().gap(px(12.)).min_w(px(0.)) }
fn note(value: &str) -> Div { div().text_size(px(theme::TEXT_SM)).text_color(theme::dim()).child(SharedString::from(value.to_owned())) }
fn control(id: impl Into<ElementId>, label: impl Into<SharedString>, click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Stateful<Div> {
    div().id(id).px(px(12.)).py(px(8.)).rounded(theme::r_control())
        .bg(theme::surface_2()).cursor_pointer().hover(|s| s.bg(theme::hover()))
        .on_click(click).child(label.into())
}

impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open { return div().into_any_element(); }
        let height = (f32::from(window.bounds().size.height) - 48.).max(300.);
        let width = (f32::from(window.bounds().size.width) - 48.).min(840.);
        let mut menu = column().w(px(184.)).flex_none();
        for (section, label, id) in [
            (Section::App, "Aplicativo", "settings-app"),
            (Section::Models, "Modelos", "settings-models"),
            (Section::Keys, "Atalhos de teclado", "settings-keys"),
            (Section::About, "Sobre / Pi", "settings-about"),
        ] {
            menu = menu.child(control(id, label, cx.listener(move |this, _, window, cx| {
                this.section = section;
                if section == Section::Keys { this.search.read(cx).focus_handle.clone().focus(window, cx); }
                else { this.focus.focus(window, cx); }
                cx.notify();
            })).when(self.section == section, |el| el.border_l_2().border_color(theme::accent())));
        }
        let content = match self.section {
            Section::App => {
                let mut page = column().child("Aplicativo");
                for (id, label, current) in [
                    ("details", "Painel de detalhes", self.details),
                    ("navigation", "Navegação de sessões", self.navigation),
                    ("thinking", "Raciocínio expandido", self.thinking),
                    ("generated_titles", "Títulos gerados por modelo", self.titles),
                ] {
                    page = page.child(control(id, SharedString::from(format!("{label}: {}", if current { "ativado" } else { "desativado" })),
                        cx.listener(move |this, _, _, cx| {
                            match id {
                                "details" => this.details = !current,
                                "navigation" => this.navigation = !current,
                                "thinking" => this.thinking = !current,
                                _ => this.titles = !current,
                            }
                            cx.emit(SettingsEvent::Preference(id, !current)); cx.notify();
                        })));
                }
                page = page.child(note("Títulos por modelo usam o modelo da sessão a cada rodada e gastam tokens. Desligado, o Dish mostra a última demanda por extenso."));
                page.into_any_element()
            }
            Section::Models => {
                let mut page = column().child("Modelos").child(note(
                    "Oculta modelos apenas no seletor do Dish. O Pi continua com a lista completa; nada é alterado na configuração dele.",
                ));
                if self.models.is_empty() {
                    page = page.child(note(
                        "Nenhum modelo disponível ainda: abra uma conversa para o Pi informar a lista.",
                    ));
                }
                if !self.hidden.is_empty() {
                    page = page.child(control("models-show-all", "Mostrar todos", cx.listener(
                        |this, _, _, cx| {
                            this.hidden.clear();
                            cx.emit(SettingsEvent::ShowAllModels);
                            cx.notify();
                        },
                    )));
                }
                for (provider, id) in &self.models {
                    let key = format!("{provider}/{id}");
                    let row_id = SharedString::from(format!("model-visibility-{key}"));
                    let label = if self.hidden.contains(&key) { "oculto" } else { "visível" };
                    let toggled = key.clone();
                    page = page.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(12.))
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
                                            .font_family(theme::FONT_MONO)
                                            .text_size(px(theme::TEXT_SM))
                                            .text_color(theme::text())
                                            .child(SharedString::from(id.clone())),
                                    )
                                    .child(
                                        div()
                                            .font_family(theme::FONT_MONO)
                                            .text_size(px(theme::TEXT_MICRO))
                                            .text_color(theme::faint())
                                            .child(SharedString::from(provider.clone())),
                                    ),
                            )
                            .child(control(row_id, label, cx.listener(move |this, _, _, cx| {
                                let visible = !this.hidden.contains(&toggled);
                                if visible { this.hidden.insert(toggled.clone()); } else { this.hidden.remove(&toggled); }
                                cx.emit(SettingsEvent::ModelVisibility(toggled.clone(), !visible));
                                cx.notify();
                            }))),
                    );
                }
                page.into_any_element()
            }
            Section::Keys => {
                let focus = self.search.read(cx).focus_handle.clone();
                let click_focus = focus.clone();
                let query = self.search.read(cx).text(cx).trim().to_lowercase();
                let mut page = column().child("Atalhos de teclado").child(div().id("settings-key-search")
                    .key_context("DishHelpSearch").track_focus(&focus)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| click_focus.focus(window, cx))
                    .map(editor::standard_actions(self.search.clone())).p(px(8.)).bg(theme::inset()).child(self.search.clone()));
                let mut count = 0;
                for (key, description) in crate::ui::overlays::keyboard_shortcuts() {
                    if !query.is_empty() && !format!("{key} {description}").to_lowercase().contains(&query) { continue; }
                    count += 1;
                    page = page.child(div().flex().gap(px(12.)).child(crate::ui::keycap_all(key)).child(note(description)));
                }
                if count == 0 { page = page.child(note("Nenhum atalho encontrado.")); }
                page.into_any_element()
            }
            Section::About => column().child("Dish")
                .child(note(&format!("Versão do Dish: {}", env!("CARGO_PKG_VERSION"))))
                .child(note(&format!("Pi selecionado: {}", crate::installation::program())))
                .child(note("Pi mantém as sessões e credenciais. O Dish não executa um serviço em segundo plano."))
                .child(control("settings-docs", "Documentação oficial do Pi", |_, _, cx| cx.open_url("https://github.com/earendil-works/pi"))).into_any_element(),
        };
        let card = column().id("settings-card").w(px(width)).h(px(height.min(620.)))
            .p(px(20.)).rounded(px(6.)).border_1().border_color(theme::line_strong())
            .bg(theme::surface()).shadow(theme::shadow_overlay())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().flex().justify_between().items_center()
                .child("Configurações")
                .child(control("settings-close", "Fechar · Esc", cx.listener(|this, _, window, cx| this.close(window, cx)))))
            .child(crate::ui::rule())
            .child(div().flex().flex_1().gap(px(20.)).min_h(px(0.)).child(menu)
                .child(div().id("settings-content").flex_1().min_w(px(0.)).min_h(px(0.)).overflow_y_scroll().child(content)));
        div().id("settings-scrim").absolute().inset(px(0.)).flex().items_center().justify_center()
            .track_focus(&self.focus).key_context("DishSettings")
            .bg(theme::wash(theme::bg(), 0.78))
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
            .child(card).into_any_element()
    }
}
