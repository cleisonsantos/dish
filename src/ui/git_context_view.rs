//! Compact read-only Git context: location and counters share one inline row.
use gpui::prelude::*;
use gpui::*;

use crate::git_context::GitContext;
use crate::theme;

fn stat(text: String, color: Hsla, hint: &'static str) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("git-stat-{text}")))
        .text_color(color)
        .child(SharedString::from(text))
        .tooltip(move |_, cx| {
            cx.new(|_| super::copy_button::TextTooltip(hint.into()))
                .into()
        })
}

pub fn footer(context: &GitContext) -> Stateful<Div> {
    let root = div()
        .id("git-context")
        .flex_none()
        .w_full()
        .min_w(px(0.))
        .px(px(theme::CHAT_INSET))
        .pb(px(theme::S2))
        .font_family(theme::FONT_MONO)
        .text_size(px(theme::TEXT_XS))
        .text_color(theme::dim());
    let GitContext::Repository(repo) = context else {
        return root.truncate().child(SharedString::from(context.summary()));
    };
    let mut counters = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(theme::S2))
        .child(stat(
            repo.changes_label(),
            theme::faint(),
            "Alterações no índice e na pasta de trabalho. Itens não rastreados podem ser pastas.",
        ));
    if let Some(lines) = &repo.lines {
        if lines.added > 0 {
            counters = counters.child(stat(format!("+{}", lines.added), theme::git_added(), "Linhas adicionadas: diff líquido de arquivos rastreados em relação a HEAD (staged + unstaged)."));
        }
        if lines.removed > 0 {
            counters = counters.child(stat(format!("−{}", lines.removed), theme::git_removed(), "Linhas removidas: diff líquido de arquivos rastreados em relação a HEAD (staged + unstaged)."));
        }
        if lines.binary_files > 0 {
            counters = counters.child(stat(
                format!("{} bin.", lines.binary_files),
                theme::faint(),
                "Arquivos binários alterados, sem contagem de linhas.",
            ));
        }
    }
    if repo.untracked > 0 {
        counters = counters.child(stat(
            format!("?{}", repo.untracked),
            theme::warn(),
            "Itens não rastreados: não incluídos na contagem de linhas.",
        ));
    }
    if repo.conflicts > 0 {
        counters = counters.child(stat(
            format!("{} conflitos", repo.conflicts),
            theme::danger(),
            "Arquivos com conflitos ainda não resolvidos.",
        ));
    }
    if repo.diff_error.is_some() {
        counters = counters.child(stat(
            "diff indisponível".into(),
            theme::faint(),
            "Não foi possível contar as linhas. Consulte Detalhes para ver o motivo.",
        ));
    }
    if let Some((ahead, behind)) = repo.ahead_behind {
        if ahead > 0 {
            counters = counters.child(stat(
                format!("↑{ahead}"),
                theme::faint(),
                "Commits à frente do upstream conhecido localmente, sem fetch.",
            ));
        }
        if behind > 0 {
            counters = counters.child(stat(
                format!("↓{behind}"),
                theme::faint(),
                "Commits atrás do upstream conhecido localmente, sem fetch.",
            ));
        }
    }
    root.flex()
        .items_center()
        .gap(px(theme::S2))
        .overflow_hidden()
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .child(SharedString::from(repo.location_label())),
        )
        .child(div().flex_none().child("·"))
        .child(counters)
}
