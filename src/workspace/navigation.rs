//! A navegação de sessões: busca, filtros e a lista agrupada por projeto.
//!
//! Vive num módulo próprio só para manter `workspace.rs` legível; o estado e o
//! comportamento continuam sendo do `Workspace`.

use super::*;

impl Workspace {
    /// Navegação: cabeçalho integrado, busca, filtros e a lista de projetos.
    /// A identidade, a busca e "nova sessão" ficam no mesmo lugar — não há mais
    /// uma barra "Sessions" separada gastando uma linha inteira.
    pub(super) fn navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let query = self.query.read(cx).text(cx).to_lowercase();
        let filter = self.filter;

        let mut groups: BTreeMap<PathBuf, Vec<SessionRow>> = BTreeMap::new();
        for (index, conversation) in self.conversations.iter().enumerate() {
            let state = conversation.state.read(cx);
            let title = self.conversation_title(index, cx);
            // Recência observada nos limites de atividade, nunca nos deltas.
            // Sem atividade ao vivo: prompt, mtime salvo ou criação da conversa.
            let last_run = state
                .last_activity.or(state.last_run)
                .or_else(|| {
                    conversation.path.as_ref().and_then(|path| {
                        self.catalog
                            .iter()
                            .find(|info| &info.path == path)
                            .map(|info| info.modified)
                    })
                })
                .unwrap_or(state.created_at);
            groups.entry(state.cwd.clone()).or_default().push((
                title,
                None,
                Some(index),
                last_run,
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
            groups.entry(info.cwd.clone()).or_default().push((
                info.title.clone(),
                Some(info.clone()),
                None,
                self.preferences.unread_sessions.get(&info.path).copied().map(from_epoch_millis).unwrap_or(info.modified),
            ));
        }

        // Filtros independentes de atenção, leitura e atividade.
        if !query.is_empty() || filter != SessionFilter::All {
            groups.retain(|cwd, items| {
                let project_matches = cwd
                    .file_name()
                    .map(|name| name.to_string_lossy().to_lowercase().contains(&query))
                    .unwrap_or(false);
                items.retain(|(title, info, live, _)| {
                    let signals = live.map(|index| {
                        let conversation = &self.conversations[index];
                        SessionSignals::from_state(conversation.state.read(cx), conversation.unread_completion)
                    }).unwrap_or(SessionSignals {
                        saved: true,
                        unread: info.as_ref().is_some_and(|info| self.preferences.unread_sessions.contains_key(&info.path)),
                        ..Default::default()
                    });
                    if !signals.matches(filter) {
                        return false;
                    }
                    if project_matches || query.is_empty() {
                        return true;
                    }
                    title.to_lowercase().contains(&query)
                });
                !items.is_empty()
            });
        }

        // Novos resultados de busca/filtro são revelados; recolher manualmente
        // continua funcionando e a seta direita pode reabrir o último projeto.
        if self.nav_query != query || self.nav_filter != filter {
            for cwd in groups.keys() { self.collapsed.remove(cwd); }
            self.nav_filter = filter;
        }
        // A mesma ordem estável alimenta o desenho e a navegação por teclado.
        for items in groups.values_mut() {
            sort_session_rows(items, &self.row_order,
                self.nav_hovered || self.query.read(cx).focus_handle.is_focused(window));
            // Fixadas primeiro dentro do grupo, preservando a ordem por
            // atividade entre elas. `sort_by_key` é estável.
            let pinned = &self.preferences.pinned_sessions;
            items.sort_by_key(|row| {
                let path = match row.2 {
                    Some(index) => self.conversations[index].path.clone(),
                    None => row.1.as_ref().map(|info| info.path.clone()),
                };
                !path.is_some_and(|path| pinned.contains(&path))
            });
        }
        let mut entries: Vec<NavEntry> = Vec::new();
        for (cwd, items) in &groups {
            if !self.collapsed.contains(cwd) {
                for (_, info, live, _) in items {
                    if let Some(index) = live {
                        entries.push(NavEntry::Open(*index));
                    } else if let Some(info) = info {
                        entries.push(NavEntry::Saved(info.path.clone()));
                    } else {
                        continue;
                    }
                }
            }
        }
        let previous = self.nav_entries.get(self.nav_index);
        if self.nav_query != query {
            self.nav_query = query.clone();
            self.nav_index = 0;
        } else if let Some(index) = previous.and_then(|previous| entries.iter().position(|entry| entry == previous)) {
            self.nav_index = index;
        }
        self.nav_entries = entries;
        self.nav_index = self.nav_index.min(self.nav_entries.len().saturating_sub(1));
        let highlighted = self.nav_entries.get(self.nav_index).cloned();

        let open_count = self.conversations.len();
        let running_count = self
            .conversations
            .iter()
            .filter(|c| c.state.read(cx).is_executing())
            .count();
        let counts = self.session_counts(cx);
        let focus = self.query.read(cx).focus_handle.clone();
        let query_editor = self.query.clone();
        let focus_for_click = focus.clone();

        let mut rail = div()
            .id("sessions-navigation")
            .on_hover(cx.listener(|workspace, hovered: &bool, _, cx| { workspace.nav_hovered = *hovered; cx.notify(); }))
            .w(px(self.nav_width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme::surface())
            .border_r_1()
            .border_color(theme::line_soft())
            // cabeçalho integrado
            .child(
                div()
                    .flex_none()
                    .h(px(52.))
                    .pl(px(theme::S4))
                    .pr(px(theme::S2))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("nav-collapse")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S2))
                            .px(px(theme::S2))
                            .py(px(theme::S1))
                            .rounded(theme::r_control())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::hover()))
                            .on_click(cx.listener(|workspace, _: &ClickEvent, _, cx| {
                                workspace.visible = false;
                                workspace.nav_user_choice = true;
                                cx.notify();
                            }))
                            .child(img("dish-icon.png").flex_none().w(px(26.)).h(px(26.)))
                            .child(
                                div()
                                    .text_size(px(theme::TITLE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Dish"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S1))
                            .child(nav_icon_button(
                                "nav-open-project",
                                crate::ui::icons::Icon::Folder,
                                cx,
                                |workspace, _window, cx| workspace.choose_project(cx),
                            ))
                            .child(nav_icon_button(
                                "nav-top-help",
                                crate::ui::icons::Icon::Dots,
                                cx,
                                |workspace, window, cx| {
                                    workspace.open_settings(crate::settings::Section::App, window, cx);
                                },
                            )),
                    ),
            )
            .child(
                div().id("nav-new-session")
                    .test_probe(|| "nav-new-session".into(), || serde_json::json!({}))
                    .mx(px(theme::S3)).mb(px(theme::S3))
                    .px(px(theme::S3)).py(px(theme::S2))
                    .flex().items_center().gap(px(theme::S2))
                    .rounded(theme::r_control())
                    .border_1().border_color(theme::line_strong())
                    .bg(theme::surface_2()).text_color(theme::text())
                    .cursor_pointer().focusable().tab_index(0)
                    .hover(|style| style.bg(theme::hover()))
                    .focus(|style| style.shadow(theme::focus_ring()))
                    .on_key_down(cx.listener(|workspace, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            let cwd = workspace.conversations[workspace.active].state.read(cx).cwd.clone();
                            workspace.spawn_conversation(cwd, None, None, cx);
                            cx.stop_propagation();
                        } else if event.keystroke.key == "tab" {
                            if event.keystroke.modifiers.shift { window.focus_prev(cx); } else { window.focus_next(cx); }
                            cx.stop_propagation();
                        }
                    }))
                    .on_click(cx.listener(|workspace, _: &ClickEvent, _, cx| {
                        let cwd = workspace.conversations[workspace.active].state.read(cx).cwd.clone();
                        workspace.spawn_conversation(cwd, None, None, cx);
                    }))
                    .child(crate::ui::icons::icon(crate::ui::icons::Icon::Plus, 14., theme::dim()))
                    .child(div().flex_1().text_size(px(theme::TEXT)).child("Nova sessão"))
                    .when(self.nav_width >= 250., |el| el.child(crate::ui::micro("Ctrl+N"))),
            )
            // busca + filtros
            .child(
                div()
                    .flex_none()
                    .px(px(theme::S3))
                    .pb(px(theme::S3))
                    .flex()
                    .flex_col()
                    .gap(px(theme::S2))
                    .child(
                        div()
                            .id("nav-search")
                            .key_context("DishNavSearch")
                            .track_focus(&focus)
                            .cursor_text()
                            .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                                focus_for_click.focus(window, cx);
                            })
                            .map(crate::editor::standard_actions(query_editor.clone()))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S2))
                            .px(px(theme::S2 + theme::S1))
                            .py(px(theme::S1 + 2.))
                            .rounded(theme::r_control())
                            .bg(theme::inset())
                            .border_1()
                            .border_color(theme::line_soft())
                            .child(crate::ui::icons::icon(
                                crate::ui::icons::Icon::Search,
                                14.,
                                theme::faint(),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(theme::TEXT_SM))
                                    .child(query_editor.clone()),
                            ),
                    )
                    .child(
                        div()
                            // Controle segmentado: um só poço com um cursor
                            // claro. Rótulos compactos de propósito: a métrica
                            // da fonte de fallback varia entre sistemas, e
                            // nenhum modo pode truncar; o nome completo fica
                            // no tooltip e as contagens ficam no rodapé.
                            .flex()
                            .flex_row()
                            .items_stretch()
                            .p(px(2.))
                            .gap(px(1.))
                            .rounded(theme::r_control())
                            .bg(theme::inset())
                            .border_1()
                            .border_color(theme::line_soft())
                            .child(nav_segment(
                                "nav-filter-all",
                                "Todas",
                                "todas as sessões abertas e salvas",
                                self.filter == SessionFilter::All,
                                cx,
                                |workspace, cx| {
                                    workspace.filter = SessionFilter::All;
                                    cx.notify();
                                },
                            ))
                            .child(nav_segment("nav-filter-input", "Pend.", "sessões que precisam de você", self.filter == SessionFilter::NeedsInput, cx, |workspace, cx| {
                                workspace.filter = SessionFilter::NeedsInput; cx.notify();
                            }))
                            .child(nav_segment("nav-filter-unread", "Novas", "respostas ainda não lidas", self.filter == SessionFilter::Unread, cx, |workspace, cx| {
                                workspace.filter = SessionFilter::Unread; cx.notify();
                            }))
                            .child(nav_segment(
                                "nav-filter-running",
                                "Exec.",
                                "tarefas em execução agora",
                                self.filter == SessionFilter::Running,
                                cx,
                                |workspace, cx| {
                                    workspace.filter = SessionFilter::Running;
                                    cx.notify();
                                },
                            )),
                    ),
            );

        if let Some(error) = self.error.clone() {
            let _ = window;
            rail = rail.child(
                div()
                    .px(px(theme::S3))
                    .pb(px(theme::S2))
                    .text_size(px(theme::TEXT_XS))
                    .text_color(theme::failure())
                    .child(SharedString::from(error)),
            );
        }
        if let Some(notice) = self.notice.clone() {
            rail = rail.child(
                div()
                    .px(px(theme::S3))
                    .pb(px(theme::S2))
                    .text_size(px(theme::TEXT_XS))
                    .text_color(theme::warn())
                    .child(SharedString::from(notice)),
            );
        }

        let mut list = div()
            .id("nav-list")
            .test_probe(|| "nav-list".into(), || serde_json::json!({}))
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .track_scroll(&self.nav_scroll)
            .px(px(theme::S2))
            .pb(px(theme::S4))
            .flex()
            .flex_col()
            .gap(px(theme::S1));

        if groups.is_empty() {
            let label = if !query.is_empty() {
                format!("Nada corresponde a “{query}”")
            } else {
                match filter {
                    SessionFilter::All => "Nenhuma sessão ainda".to_string(),
                    SessionFilter::NeedsInput => "Nenhuma sessão precisa de você".to_string(),
                    SessionFilter::Unread => "Nenhuma resposta nova".to_string(),
                    SessionFilter::Running => "Nenhuma sessão em execução".to_string(),
                }
            };
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme::S2))
                    .px(px(theme::S2))
                    .py(px(theme::S4))
                    .child(
                        div()
                            .text_size(px(theme::TEXT_SM))
                            .text_color(theme::dim())
                            .child(SharedString::from(label)),
                    )
                    .child(
                        div()
                            .id("nav-clear-filter")
                            .text_size(px(theme::TEXT_SM))
                            .text_color(theme::running())
                            .cursor_pointer()
                            .hover(|style| style.text_color(theme::text()))
                            .on_click(cx.listener(|workspace, _: &ClickEvent, _, cx| {
                                workspace.filter = SessionFilter::All;
                                workspace.query.update(cx, |editor, cx| editor.clear(cx));
                                cx.notify();
                            }))
                            .child("Limpar filtro"),
                    ),
            );
        }

        let grouped_folders: Vec<PathBuf> = groups.keys().cloned().collect();
        for (cwd, items) in groups {
            let collapsed = self.collapsed.contains(&cwd);
            let click_cwd = cwd.clone();
            let running = items
                .iter()
                .filter(|(_, _, live, _)| match live {
                    Some(index) => self.conversations[*index].state.read(cx).is_executing(),
                    None => false,
                })
                .count();
            let needs_input = items.iter().filter(|(_, _, live, _)| live.is_some_and(|i| self.conversations[i].state.read(cx).modal.is_some())).count();
            let unread_count = items.iter().filter(|(_, info, live, _)| live.map(|i| self.conversations[i].unread_completion).unwrap_or_else(|| info.as_ref().is_some_and(|info| self.preferences.unread_sessions.contains_key(&info.path)))).count();
            let project_tooltip = {
                let path = cwd.display().to_string();
                let attention = attention_summary(needs_input, unread_count);
                if attention.is_empty() { path } else { format!("{path}\n{attention}") }
            };
            if !collapsed {
                for item in &items {
                    self.nav_anchors.entry(row_key(item)).or_insert_with(|| ScrollAnchor::for_handle(self.nav_scroll.clone()));
                }
            }

            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(theme::S1))
                    .mb(px(theme::S2))
                    .child(
                        div()
                            .id(SharedString::from(format!("project-{cwd:?}")))
                            .tooltip(move |_, cx| {
                                cx.new(|_| crate::ui::copy_button::TextTooltip(project_tooltip.clone())).into()
                            })
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S2))
                            .px(px(theme::S2))
                            .py(px(theme::S1 + 2.))
                            .rounded(theme::r_control())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::hover()))
                            .on_click(cx.listener(move |workspace, _: &ClickEvent, _, cx| {
                                if !workspace.collapsed.remove(&click_cwd) {
                                    workspace.collapsed.insert(click_cwd.clone());
                                }
                                cx.notify();
                            }))
                            .child(crate::ui::icons::icon(
                                if collapsed {
                                    crate::ui::icons::Icon::ChevronRight
                                } else {
                                    crate::ui::icons::Icon::ChevronDown
                                },
                                13.,
                                theme::faint(),
                            ))
                            .child(crate::ui::icons::icon(
                                crate::ui::icons::Icon::Folder,
                                14.,
                                theme::faint(),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(px(theme::TEXT_SM))
                                    .text_color(theme::dim())
                                    .child(SharedString::from(folder_of(&cwd))),
                            )
                            .when(needs_input > 0, |el| {
                                el.child(div().text_size(px(theme::TEXT_SM)).text_color(theme::warn())
                                    .child(SharedString::from(format!("{needs_input} pend."))))
                            })
                            .when(unread_count > 0, |el| {
                                el.child(div().text_size(px(theme::TEXT_SM)).text_color(theme::attention())
                                    .child(SharedString::from(format!("{unread_count} {}", if unread_count == 1 { "nova" } else { "novas" }))))
                            })
                            .when(running > 0, |el| {
                                el.child(crate::ui::icons::icon(
                                    crate::ui::icons::Icon::Activity,
                                    12.,
                                    theme::running(),
                                ))
                                .child(
                                    div()
                                        .text_size(px(theme::TEXT_XS))
                                        .text_color(theme::running())
                                        .child(SharedString::from(running.to_string())),
                                )
                            })
                            .child(
                                div()
                                    .text_size(px(theme::TEXT_XS))
                                    .text_color(theme::faint())
                                    .child(SharedString::from(items.len().to_string())),
                            ),
                    )
                    .when(!collapsed, |el| {
                        el.children(items.into_iter().map(
                            |(title, info, live, activity_time)| {
                                let key = row_key(&(title.clone(), info.clone(), live, activity_time));
                                let highlighted_here = match &highlighted {
                                    Some(NavEntry::Open(index)) => live == Some(*index),
                                    Some(NavEntry::Saved(path)) => info.as_ref().is_some_and(|info| &info.path == path),
                                    None => false,
                                };
                                let keyboard_selected = highlighted_here && self.query.read(cx).focus_handle.is_focused(window);
                                let signals = live.map(|i| {
                                    let conversation = &self.conversations[i];
                                    SessionSignals::from_state(conversation.state.read(cx), conversation.unread_completion)
                                }).unwrap_or(SessionSignals { saved: true,
                                    unread: info.as_ref().is_some_and(|info| self.preferences.unread_sessions.contains_key(&info.path)),
                                    ..Default::default() });
                                let opening_saved = info.as_ref().is_some_and(|info| {
                                    self.opening
                                        .as_ref()
                                        .is_some_and(|opening| opening.path.as_ref() == Some(&info.path))
                                });
                                let loading_live = live.is_some_and(|i| {
                                    self.conversations[i].state.read(cx).is_loading_session()
                                });
                                let loading_session = opening_saved || loading_live;
                                let any_opening_saved = self
                                    .opening
                                    .as_ref()
                                    .is_some_and(|opening| opening.path.is_some());
                                let running = signals.running;
                                let active = opening_saved || (!any_opening_saved && live == Some(self.active));
                                let unread = signals.unread;
                                let needs_input = signals.needs_input;
                                let weak = cx.entity().downgrade();
                                let info = info.clone();
                                let live_index = live;
                                let anchor = self.nav_anchors.get(&key).cloned();
                                let tooltip = format!(
                                    "{}\n{} · {}",
                                    title,
                                    signals.labels(),
                                    relative_time(activity_time, std::time::SystemTime::now())
                                );
                                let pulse_id = format!("nav-pulse-{key}");
                                let spinner_id = format!("nav-spinner-{key}");
                                let session_path = match live {
                                    Some(index) => self.conversations[index].path.clone(),
                                    None => info.as_ref().map(|info| info.path.clone()),
                                };
                                let pinned = session_path.as_ref().is_some_and(|path| {
                                    self.preferences.pinned_sessions.contains(path)
                                });
                                let pin_button = {
                                    let id = SharedString::from(format!("pin-{key}"));
                                    let pin_weak = weak.clone();
                                    match session_path.clone() {
                                        Some(path) => {
                                            let pinned_now = pinned;
                                            let weak = pin_weak;
                                            Some(div()
                                                .id(id)
                                                .w(px(24.))
                                                .h(px(theme::S6))
                                                .rounded(theme::r_control())
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .when(!pinned_now, |el| el.opacity(0.0).hover(|s| s.opacity(1.0)))
                                                .when(pinned_now, |el| el.opacity(1.0))
                                                .cursor_pointer()
                                                .tooltip(move |_, cx| {
                                                    let label = if pinned_now { "Desafixar sessão" } else { "Fixar sessão" };
                                                    cx.new(|_| crate::ui::copy_button::TextTooltip(label.into())).into()
                                                })
                                                .on_click(move |_event: &ClickEvent, _, cx: &mut App| {
                                                    cx.stop_propagation();
                                                    weak.update(cx, |workspace, cx| {
                                                        workspace.toggle_pin(path.clone(), cx)
                                                    })
                                                    .ok();
                                                })
                                                .child(crate::ui::icons::icon(
                                                    crate::ui::icons::Icon::Pin,
                                                    12.,
                                                    if pinned_now { theme::accent() } else { theme::faint() },
                                                ))
                                                .into_any_element())
                                        }
                                        None => {
                                            let weak = pin_weak;
                                            Some(div()
                                                .id(id)
                                                .w(px(24.))
                                                .h(px(theme::S6))
                                                .rounded(theme::r_control())
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .opacity(0.0)
                                                .hover(|s| s.opacity(1.0))
                                                .cursor_pointer()
                                                .tooltip(move |_, cx| {
                                                    cx.new(|_| crate::ui::copy_button::TextTooltip(
                                                        "A sessão é salva pelo Pi no primeiro turno".into(),
                                                    )).into()
                                                })
                                                .on_click(move |_event: &ClickEvent, _, cx: &mut App| {
                                                    cx.stop_propagation();
                                                    weak.update(cx, |workspace, cx| {
                                                        workspace.explain_unpinned_session(cx)
                                                    })
                                                    .ok();
                                                })
                                                .child(crate::ui::icons::icon(
                                                    crate::ui::icons::Icon::Pin,
                                                    12.,
                                                    theme::faint(),
                                                ))
                                                .into_any_element())
                                        }
                                    }
                                };
                                let action_target = live.map(|index| SessionTarget::Live(self.conversations[index].state.entity_id()))
                                    .or_else(|| info.as_ref().map(|info| SessionTarget::Saved(info.path.clone())));
                                let action_id = action_target.as_ref().map(|target| format!("session-actions-{}", target.key()));
                                let action_button = action_target.map(|target| self.session_actions_button(target, cx));
                                div()
                                    .test_probe(|| format!("nav-{key}"), || serde_json::json!({
                                        "title": title, "active": active, "actions": action_id,
                                    }))
                                    .id(SharedString::from(key))
                                    .anchor_scroll(anchor)
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(theme::S2))
                                    .pl(px(theme::S3))
                                    .pr(px(theme::S2))
                                    .py(px(theme::S2 + 1.))
                                    .rounded(theme::r_control())
                                    // Foco de teclado é um anel por fora, para não
                                    // competir com as cores de estado da linha. O
                                    // fundo opaco evita que a sombra vire um bloco.
                                    .when(keyboard_selected, |el| {
                                        el.bg(theme::hover()).shadow(theme::focus_ring())
                                    })
                                    .when(active, |el| {
                                        el.bg(theme::surface_2())
                                            .border_l_2()
                                            .border_color(theme::accent())
                                    })
                                    .when(!active && unread, |el| {
                                        el.border_l_2()
                                            .border_color(theme::attention())
                                    })
                                    // Bloqueada esperando você é mais urgente que
                                    // conteúdo novo: o âmbar vence o verde. Sem
                                    // wash: sob o modal, o título precisa ficar
                                    // legível.
                                    .when(!active && needs_input, |el| {
                                        el.border_l_2().border_color(theme::warn())
                                    })
                                    .when(!active && !keyboard_selected, |el| el.hover(|s| s.bg(theme::hover())))
                                    .cursor_pointer()
                                    .tooltip(move |_, cx| {
                                        cx.new(|_| crate::ui::copy_button::TextTooltip(tooltip.clone())).into()
                                    })
                                    .on_click(move |_event: &ClickEvent, window, cx: &mut App| {
                                        weak.update(cx, |workspace, cx| {
                                            workspace.focus_nav(live_index, info.as_ref().map(|info| info.path.clone()));
                                            match live_index {
                                                Some(index) => {
                                                    workspace.select(index, window, cx)
                                                }
                                                None => {
                                                    if let Some(info) = info.clone() {
                                                        workspace.open(info, window, cx)
                                                    }
                                                }
                                            }
                                        })
                                        .ok();
                                    })
                                    // O marcador repete o estado: forma e cor,
                                    // nunca só cor.
                                    .child(if loading_session {
                                        crate::ui::icons::spinner(spinner_id, 14., theme::accent()).into_any_element()
                                    } else if needs_input {
                                        crate::ui::icons::icon(crate::ui::icons::Icon::Alert, 12., theme::warn()).into_any_element()
                                    } else if signals.error {
                                        crate::ui::icons::icon(crate::ui::icons::Icon::Alert, 12., theme::failure()).into_any_element()
                                    } else if running {
                                        crate::ui::pulse(pulse_id, 7., theme::running()).into_any_element()
                                    } else if unread {
                                        crate::ui::icons::icon(crate::ui::icons::Icon::File, 12., theme::attention()).into_any_element()
                                    } else if active {
                                        crate::ui::icons::icon(crate::ui::icons::Icon::CurrentSession, 16., theme::accent()).into_any_element()
                                    } else if live.is_some() {
                                        crate::ui::icons::icon(crate::ui::icons::Icon::OpenSession, 16., theme::dim()).into_any_element()
                                    } else {
                                        crate::ui::icons::icon(crate::ui::icons::Icon::SavedSession, 16., theme::faint()).into_any_element()
                                    })
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
                                                    .text_size(px(theme::TEXT_SM))
                                                    .text_color(if active || unread {
                                                        theme::text()
                                                    } else {
                                                        theme::dim()
                                                    })
                                                    .when(unread && !active, |el| {
                                                        el.font_weight(FontWeight::MEDIUM)
                                                    })
                                                    .child(SharedString::from(title)),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .flex_wrap()
                                                    .items_center()
                                                    .gap(px(theme::S1 + 1.))
                                                    .when(loading_session || (!running && !needs_input && !signals.error && !signals.interrupted && !unread), |el| {
                                                        el.child(div().text_size(px(theme::TEXT_XS))
                                                            .text_color(if active { theme::accent() } else { theme::dim() })
                                                            .child(if loading_session {
                                                                "carregando…"
                                                            } else {
                                                                lifecycle_label(live.is_some(), active)
                                                            }))
                                                    })
                                                    .when(pinned, |el| el.child(crate::ui::icons::icon(crate::ui::icons::Icon::Pin, 12., theme::accent())))
                                                    .when(running && !signals.needs_input, |el| {
                                                        el.child(crate::ui::icons::icon(
                                                            crate::ui::icons::Icon::Activity,
                                                            11.,
                                                            theme::running(),
                                                        ))
                                                        .child(
                                                            div()
                                                                .text_size(px(theme::TEXT_SM))
                                                                .text_color(theme::running())
                                                                .child("executando"),
                                                        )
                                                    })
                                                    .when(unread, |el| {
                                                        el.child(crate::ui::icons::icon(
                                                            crate::ui::icons::Icon::File, 11., theme::attention(),
                                                        )).child(
                                                            div()
                                                                .flex()
                                                                .flex_row()
                                                                .items_center()
                                                                .gap(px(5.))
                                                                .text_size(px(theme::TEXT_SM))
                                                                .text_color(theme::attention())
                                                                .child("resposta")
                                                                .child("nova"),
                                                        )
                                                    })
                                                    .when(signals.needs_input, |el| {
                                                        el.child(crate::ui::icons::icon(crate::ui::icons::Icon::Alert, 11., theme::warn())).child(
                                                            div()
                                                                .text_size(px(theme::TEXT_XS))
                                                                .text_color(theme::warn())
                                                                .child("precisa de você"),
                                                        )
                                                    })
                                                    .when(signals.queued > 0, |el| {
                                                        el.child(
                                                            div()
                                                                .flex()
                                                                .flex_row()
                                                                .items_center()
                                                                .gap(px(5.))
                                                                .text_size(px(theme::TEXT_SM))
                                                                .text_color(theme::text())
                                                                .child(SharedString::from(format!("{}", signals.queued)))
                                                                .child("em")
                                                                .child("fila"),
                                                        )
                                                    })
                                                    .when(signals.error, |el| {
                                                        el.child(crate::ui::icons::icon(crate::ui::icons::Icon::Alert, 11., theme::failure())).child(div().text_size(px(theme::TEXT))
                                                            .text_color(theme::failure()).child("erro"))
                                                    })
                                                    .when(signals.interrupted, |el| {
                                                        el.child(crate::ui::icons::icon(crate::ui::icons::Icon::Stop, 11., theme::dim())).child(div().text_size(px(theme::TEXT_SM)).text_color(theme::dim()).child("interrompida"))
                                                    })
                                                    .when(!running && !needs_input && !signals.error && !signals.interrupted && !unread, |el| {
                                                        if signals.completed {
                                                            el.child(crate::ui::icons::icon(crate::ui::icons::Icon::Check, 11., theme::ok()))
                                                                .child(div().text_size(px(theme::TEXT_XS)).text_color(theme::ok()).child("concluída"))
                                                        } else { el }
                                                    })
                                                    .when(!signals.needs_input, |el| {
                                                        el.child(div().text_size(px(theme::TEXT_XS)).text_color(theme::faint())
                                                            .child(SharedString::from(relative_time(activity_time, std::time::SystemTime::now()))))
                                                    }),
                                            )
                                            .when(running, |el| {
                                                el.child(div().truncate().text_size(px(theme::TEXT_XS)).text_color(theme::dim())
                                                    .child(SharedString::from(live.and_then(|i| self.conversations[i].state.read(cx).activity.clone()).unwrap_or_default())))
                                            })
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(2.))
                                            .when_some(pin_button, |el, button| el.child(button))
                                            .when_some(action_button, |el, button| el.child(button)),
                                    )
                            },
                        ))
                    }),
            );
        }

        // Projetos recentes que ainda não aparecem na lista de sessões: uma
        // conversa nova começa aqui sem reiniciar o Dish. A pasta em uso entra
        // na frente quando a conversa ativa muda.
        if query.is_empty() {
            let recents: Vec<PathBuf> = self
                .preferences
                .recent_projects
                .iter()
                .filter(|path| path.is_dir() && !grouped_folders.contains(path))
                .take(5)
                .cloned()
                .collect();
            if !recents.is_empty() {
                let mut section = div()
                    .flex()
                    .flex_col()
                    .gap(px(theme::S1))
                    .pt(px(theme::S2))
                    .child(
                        div()
                            .px(px(theme::S2))
                            .py(px(theme::S1))
                            .text_size(px(theme::TEXT_XS))
                            .text_color(theme::faint())
                            .child("Projetos recentes"),
                    );
                for path in recents {
                    let label = folder_of(&path);
                    let tooltip = path.display().to_string();
                    let target = path.clone();
                    section = section.child(
                        div()
                            .id(SharedString::from(format!("recent-{}", path.display())))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S2))
                            .px(px(theme::S2))
                            .py(px(theme::S1 + 2.))
                            .rounded(theme::r_control())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::hover()))
                            .tooltip(move |_, cx| {
                                cx.new(|_| crate::ui::copy_button::TextTooltip(tooltip.clone())).into()
                            })
                            .on_click(cx.listener(move |workspace, _: &ClickEvent, _, cx| {
                                workspace.open_project(target.clone(), cx);
                            }))
                            .child(crate::ui::icons::icon(
                                crate::ui::icons::Icon::Folder,
                                13.,
                                theme::faint(),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(px(theme::TEXT_SM))
                                    .text_color(theme::dim())
                                    .child(SharedString::from(label)),
                            ),
                    );
                }
                list = list.child(section);
            }
        }

        self.row_order = self.nav_entries.iter().map(entry_key).collect();
        self.nav_anchors.retain(|key, _| self.row_order.contains(key));
        rail = rail.child(list);
        if self.undo_delete.is_some() {
            rail = rail.child(self.undo_bar(cx));
        }
        rail.child(
            div()
                .flex_none()
                .px(px(theme::S3))
                .py(px(theme::S2))
                .border_t_1()
                .border_color(theme::line_soft())
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(theme::S2))
                .child(
                    div()
                        .text_size(px(theme::TEXT_XS))
                        .text_color(theme::faint())
                        .child(SharedString::from(format!("{open_count} {}", if open_count == 1 { "sessão aberta" } else { "sessões abertas" }))),
                )
                .when(running_count > 0, |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S1))
                            .child(crate::ui::icons::icon(
                                crate::ui::icons::Icon::Activity,
                                12.,
                                theme::running(),
                            ))
                            .child(
                                div()
                                    .text_size(px(theme::TEXT_XS))
                                    .text_color(theme::running())
                                    .child(SharedString::from(format!("{running_count} executando"))),
                            ),
                    )
                })
                .when(counts[2] > 0, |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S1))
                            .child(crate::ui::icons::icon(
                                crate::ui::icons::Icon::File,
                                12.,
                                theme::attention(),
                            ))
                            .child(
                                div()
                                    .text_size(px(theme::TEXT_XS))
                                    .text_color(theme::attention())
                                    .child(SharedString::from(format!(
                                        "{} {}",
                                        counts[2],
                                        if counts[2] == 1 { "nova" } else { "novas" }
                                    ))),
                            ),
                    )
                })
                .when(counts[1] > 0, |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(theme::S1))
                            .child(crate::ui::icons::icon(
                                crate::ui::icons::Icon::Alert,
                                12.,
                                theme::warn(),
                            ))
                            .child(
                                div()
                                    .text_size(px(theme::TEXT_XS))
                                    .text_color(theme::warn())
                                    .child(SharedString::from(format!(
                                        "{} {}",
                                        counts[1],
                                        if counts[1] == 1 { "pendente" } else { "pendentes" }
                                    ))),
                            ),
                    )
                }),
        )
    }
}

/// Botão de ícone do cabeçalho da navegação.
pub(super) fn nav_icon_button(
    id: &'static str,
    glyph: crate::ui::icons::Icon,
    cx: &mut Context<Workspace>,
    run: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .size(px(28.))
        .rounded(theme::r_control())
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(theme::hover()))
        .on_click(cx.listener(move |workspace, _: &ClickEvent, window, cx| {
            run(workspace, window, cx);
        }))
        .child(crate::ui::icons::icon(glyph, 15., theme::faint()))
        .into_any_element()
}

/// Um segmento do filtro de sessões: modo de leitura, ocupando a faixa toda.
/// O nome completo vive no tooltip; o número da contagem vive no rodapé.
fn nav_segment(
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    active: bool,
    cx: &mut Context<Workspace>,
    run: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .min_w(px(0.))
        .flex_1()
        .flex()
        .justify_center()
        .px(px(theme::S2))
        .py(px(3.))
        .rounded(theme::r_small())
        .when(active, |el| {
            el.bg(theme::surface_2())
                .border_1()
                .border_color(theme::line())
        })
        .when(!active, |el| {
            el.cursor_pointer()
                .border_1()
                .border_color(gpui::transparent_black())
                .hover(|s| s.bg(theme::hover()))
        })
        .tooltip(move |_, cx| {
            cx.new(|_| crate::ui::copy_button::TextTooltip(hint.into())).into()
        })
        .text_size(px(theme::TEXT_XS))
        .text_color(if active { theme::text() } else { theme::dim() })
        .when(active, |el| el.font_weight(FontWeight::MEDIUM))
        .truncate()
        .on_click(cx.listener(move |workspace, _: &ClickEvent, _window, cx| {
            run(workspace, cx);
        }))
        .child(SharedString::from(label.to_owned()))
        .into_any_element()
}
