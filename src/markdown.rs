//! A deliberately small Markdown renderer.
//!
//! Assistant replies are Markdown, so we render the handful of constructs that
//! actually show up in coding conversations — fenced code, headings, lists,
//! quotes, tables and inline emphasis — without pulling in a full parser.

use gpui::{
    AnyElement, Font, FontWeight, Hsla, IntoElement, SharedString, TextRun,
    div, img, prelude::*, px,
    ObjectFit,

};

use crate::theme;

/// Everything the renderer needs to style a block of Markdown.
#[derive(Clone)]
pub struct MarkdownStyle {
    pub font: Font,
    pub color: Hsla,
    pub accent: Hsla,
    pub dim: Hsla,
    pub code_color: Hsla,
    pub code_bg: Hsla,
}

impl MarkdownStyle {
    pub fn base_run(&self) -> TextRun {
        TextRun {
            len: 0,
            font: self.font.clone(),
            color: self.color,
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Inline {
    bold: bool,
    italic: bool,
    code: bool,
    link: bool,
}

impl Inline {
    fn bold(self) -> Self {
        Self { bold: true, ..self }
    }
    fn italic(self) -> Self {
        Self { italic: true, ..self }
    }
    fn code(self) -> Self {
        Self { code: true, ..self }
    }
    fn link(self) -> Self {
        Self { link: true, ..self }
    }
}

fn run_for(style: &MarkdownStyle, inline: Inline) -> TextRun {
    let mut run = style.base_run();
    if inline.bold {
        run.font.weight = FontWeight::BOLD;
    }
    if inline.italic {
        run.font = run.font.italic();
    }
    if inline.code {
        run.font.family = SharedString::from(theme::FONT_MONO);
        run.font.weight = FontWeight::NORMAL;
        run.color = style.code_color;
        run.background_color = Some(style.code_bg);
    }
    if inline.link {
        run.color = style.accent;
    }
    run
}

/// Walk a line of Markdown and emit the *plain* text plus the runs that style
/// it. Markers are consumed here, so the runs always cover the emitted text
/// exactly — `StyledText::with_runs` requires that.
fn push_runs(
    text: &str,
    inline: Inline,
    style: &MarkdownStyle,
    out_text: &mut String,
    out_runs: &mut Vec<TextRun>,
) {
    if text.is_empty() {
        return;
    }
    let mut plain_start = 0usize;
    let mut index = 0usize;

    let push_plain = |from: usize, to: usize, out_text: &mut String, out_runs: &mut Vec<TextRun>| {
        if from >= to {
            return;
        }
        out_text.push_str(&text[from..to]);
        let mut run = run_for(style, inline);
        run.len = to - from;
        out_runs.push(run);
    };

    while index < text.len() {
        let rest = &text[index..];
        let mut consumed = None::<(usize, usize, String, Inline)>;

        if !inline.code {
            if let Some(after) = rest.strip_prefix("**") {
                if let Some(end) = after.find("**") {
                    let inner = &after[..end];
                    if !inner.is_empty() {
                        consumed = Some((index, index + 2 + end + 2, inner.to_string(), inline.bold()));
                    }
                }
            } else if let Some(after) = rest.strip_prefix("`") {
                if let Some(end) = after.find('`') {
                    let inner = &after[..end];
                    consumed = Some((index, index + 1 + end + 1, inner.to_string(), inline.code()));
                }
            } else if rest.starts_with('[') {
                if let Some(close) = rest.find("](") {
                    if let Some(end) = rest[close + 2..].find(')') {
                        let label = &rest[1..close];
                        let target = &rest[close + 2..close + 2 + end];
                        let label = if label.is_empty() { target } else { label };
                        consumed = Some((
                            index,
                            index + close + 2 + end + 1,
                            label.to_string(),
                            inline.link(),
                        ));
                    }
                }
            } else if rest.starts_with('*') || rest.starts_with('_') {
                let marker = &rest[..1];
                if let Some(end) = rest[1..].find(marker) {
                    let inner = &rest[1..1 + end];
                    if !inner.is_empty() && !inner.contains('\n') {
                        consumed = Some((index, index + 1 + end + 1, inner.to_string(), inline.italic()));
                    }
                }
            }
        }

        match consumed {
            Some((start, end, inner, nested)) => {
                push_plain(plain_start, start, out_text, out_runs);
                push_runs(&inner, nested, style, out_text, out_runs);
                index = end;
                plain_start = end;
            }
            None => {
                let character = rest.chars().next().unwrap_or('*');
                index += character.len_utf8();
            }
        }
    }

    push_plain(plain_start, text.len(), out_text, out_runs);
}

/// Render one line of inline Markdown as a styled element.
fn inline_element(source: &str, style: &MarkdownStyle, selection: &mut crate::ui::selectable::SelectionBuilder) -> AnyElement {
    let mut text = String::with_capacity(source.len());
    let mut runs = Vec::new();
    push_runs(source, Inline::default(), style, &mut text, &mut runs);

    // Defensive: `StyledText::with_runs` panics unless runs cover the text.
    let covered: usize = runs.iter().map(|run| run.len).sum();
    if covered != text.len() {
        let mut fallback = style.base_run();
        fallback.len = text.len();
        runs = vec![fallback];
    }

    selection.text(text, Some(runs))
}

fn code_block(language: Option<&str>, code: &str, style: &MarkdownStyle, selection: &mut crate::ui::selectable::SelectionBuilder, index: usize) -> AnyElement {
    let mut body = String::with_capacity(code.len());
    for (index, line) in code.lines().enumerate() {
        if index > 0 {
            body.push('\n');
        }
        body.push_str(line);
    }
    if body.is_empty() && !code.is_empty() {
        body.push_str(code);
    }

    let copy_code = code.to_string();
    div()
        .w_full()
        .rounded(px(4.))
        .border_1()
        .border_color(theme::line_soft())
        .bg(theme::inset())
        .overflow_hidden()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(12.))
                .py(px(6.))
                .child(
                    div()
                        .text_size(px(theme::TEXT_MICRO))
                        .letter_spacing(px(0.9))
                        .font_family(SharedString::from(theme::FONT_MONO))
                        .text_color(theme::faint())
                        .child(language.unwrap_or("code").to_uppercase()),
                )
                .child(crate::ui::copy_button::CopyButton {
                    id: ("copy-code", index).into(),
                    text: copy_code,
                    label: "Copiar código",
                }),
        )
        .child(
            div()
                .id(("md-code", index))
                .max_h(px(420.))
                .overflow_y_scroll()
                .px(px(12.))
                .py(px(10.))
                .text_size(px(13.))
                .line_height(px(20.))
                .font_family(SharedString::from(theme::FONT_MONO))
                .text_color(style.code_color)
                .whitespace_nowrap()
                .child(selection.text(body, None)),
        )
        .into_any_element()
}

fn table_cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let mut cells = vec![String::new()];
    let mut escaped = false;
    let mut code = false;
    for ch in line.chars() {
        if escaped {
            if ch != '|' {
                cells.last_mut().unwrap().push('\\');
            }
            cells.last_mut().unwrap().push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '`' {
            code = !code;
            cells.last_mut().unwrap().push(ch);
        } else if ch == '|' && !code {
            cells.push(String::new());
        } else {
            cells.last_mut().unwrap().push(ch);
        }
    }
    if escaped {
        cells.last_mut().unwrap().push('\\');
    }
    if line.starts_with('|') && cells.first().is_some_and(|c| c.is_empty()) {
        cells.remove(0);
    }
    if line.ends_with('|') && cells.last().is_some_and(|c| c.is_empty()) {
        cells.pop();
    }
    cells.into_iter().map(|cell| cell.trim().to_string()).collect()
}

fn table_header(lines: &[&str], index: usize) -> Option<Vec<gpui::TextAlign>> {
    let header = table_cells(lines.get(index)?);
    let separator = table_cells(lines.get(index + 1)?);
    if !lines[index].contains('|') || header.len() != separator.len() {
        return None;
    }
    separator.iter().map(|cell| {
        let dashes = cell.trim_matches(':');
        if dashes.is_empty() || !dashes.chars().all(|ch| ch == '-') {
            return None;
        }
        Some(match (cell.starts_with(':'), cell.ends_with(':')) {
            (true, true) => gpui::TextAlign::Center,
            (_, true) => gpui::TextAlign::Right,
            _ => gpui::TextAlign::Left,
        })
    }).collect()
}

fn table(rows: &[Vec<String>], alignments: &[gpui::TextAlign], style: &MarkdownStyle, selection: &mut crate::ui::selectable::SelectionBuilder) -> AnyElement {
    let widths: Vec<f32> = (0..alignments.len()).map(|col| {
        rows.iter().filter_map(|row| row.get(col))
            .map(|cell| cell.chars().count() as f32 * 7.5 + 24.)
            .fold(100., f32::max).min(360.)
    }).collect();
    let mut body = div().flex().flex_col().min_w(px(widths.iter().sum()));
    for (row_index, row) in rows.iter().enumerate() {
        let mut rendered = div().flex().flex_row().border_b_1().border_color(theme::line_soft());
        if row_index == 0 {
            rendered = rendered.bg(theme::surface_2()).font_weight(FontWeight::BOLD);
        }
        for (col, alignment) in alignments.iter().enumerate() {
            rendered = rendered.child(div().w(px(widths[col])).flex_none()
                .px(px(12.)).py(px(8.)).text_align(*alignment)
                .child(inline_element(row.get(col).map(String::as_str).unwrap_or(""), style, selection)));
        }
        body = body.child(rendered);
    }
    div().id(crate::ui::content_id("md-table", &format!("{rows:?}")))
        .w_full().overflow_x_scroll().border_1().border_color(theme::line_soft())
        .rounded(px(4.)).child(body).into_any_element()
}

/// Standalone Markdown images; incomplete streaming syntax stays ordinary text.
fn image_reference(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix("![")?;
    let (alt, target) = rest.split_once("](")?;
    let target = target.strip_suffix(')')?.trim();
    let target = target.strip_prefix('<').and_then(|s| s.strip_suffix('>')).unwrap_or(target);
    (!target.is_empty()).then_some((alt, target))
}

fn local_image_path(target: &str, cwd: &std::path::Path) -> Option<std::path::PathBuf> {
    // Never turn untrusted remote URLs into automatic network requests.
    if target.contains("://") || target.starts_with("data:") || target.starts_with("//") {
        return None;
    }
    let path = std::path::Path::new(target);
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg") {
        return None;
    }
    Some(if path.is_absolute() { path.to_owned() } else { cwd.join(path) })
}

fn image_block(alt: &str, target: &str, cwd: &std::path::Path, index: usize, style: &MarkdownStyle) -> AnyElement {
    let caption = if alt.is_empty() { target.to_owned() } else { format!("{alt} — {target}") };
    let mut block = div().w_full().flex().flex_col().gap(px(8.));
    if let Some(path) = local_image_path(target, cwd) {
        let external_path = path.clone();
        block = block
            .child(div().w(px(200.)).max_w_full().h(px(200.)).flex_none().overflow_hidden()
                .child(img(path).size_full().object_fit(ObjectFit::Contain)
                    .with_fallback(|| div().p(px(12.)).child("Imagem indisponível ou inválida.").into_any_element())))
            .child(div().id(("markdown-image-open", index)).cursor_pointer().text_color(style.accent)
                .on_click(move |_, _, cx| cx.open_with_system(&external_path))
                .child("Abrir externamente"));
    } else {
        block = block.child("Prévia não disponível: apenas imagens locais são carregadas.");
    }
    block.child(div().text_color(style.dim).child(SharedString::from(caption))).into_any_element()
}

/// Render Markdown into a list of block elements.
pub fn blocks(text: &str, style: &MarkdownStyle, cwd: &std::path::Path, id: gpui::ElementId) -> Vec<AnyElement> {
    let mut selection = crate::ui::selectable::SelectionBuilder::default();
    let lines: Vec<&str> = text.split('\n').collect();
    let mut elements: Vec<AnyElement> = Vec::new();
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();

        if let Some((alt, target)) = image_reference(trimmed) {
            elements.push(image_block(alt, target, cwd, index, style));
            index += 1;
            continue;
        }

        // Fenced code.
        if let Some(fence) = trimmed.strip_prefix("```").or_else(|| trimmed.strip_prefix("~~~")) {
            let code_index = index;
            let language = fence.trim();
            let language = (!language.is_empty()).then_some(language);
            let mut code = String::new();
            index += 1;
            while index < lines.len() {
                let candidate = lines[index];
                let candidate_trimmed = candidate.trim_start();
                if candidate_trimmed.starts_with("```") || candidate_trimmed.starts_with("~~~") {
                    index += 1;
                    break;
                }
                if !code.is_empty() {
                    code.push('\n');
                }
                code.push_str(candidate);
                index += 1;
            }
            elements.push(code_block(language, &code, style, &mut selection, code_index));
            if !code.is_empty() {
                // Breathing room after a code block.
                elements.push(div().h(px(2.)).into_any_element());
            }
            continue;
        }

        if trimmed.is_empty() {
            index += 1;
            continue;
        }

        if let Some(alignments) = table_header(&lines, index) {
            let mut rows = vec![table_cells(line)];
            index += 2;
            while index < lines.len() && !lines[index].trim().is_empty()
                && lines[index].contains('|')
            {
                rows.push(table_cells(lines[index]));
                index += 1;
            }
            elements.push(table(&rows, &alignments, style, &mut selection));
            continue;
        }

        // Horizontal rule.
        let dashes = trimmed.chars().all(|character| character == '-' || character == '*');
        if trimmed.len() >= 3 && dashes && (trimmed.starts_with("---") || trimmed.starts_with("***")) {
            elements.push(
                div()
                    .w_full()
                    .h(px(1.))
                    .my(px(6.))
                    .bg(theme::line())
                    .into_any_element(),
            );
            index += 1;
            continue;
        }

        // Headings.
        if let Some(rest) = trimmed.strip_prefix("#") {
            let level = rest.chars().take_while(|character| *character == '#').count() + 1;
            let title = rest.trim_start_matches('#').trim();
            let size = match level {
                1 => 20.0,
                2 => 17.0,
                _ => 15.0,
            };
            elements.push(
                div()
                    .w_full()
                    .mt(px(4.))
                    .text_size(px(size))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::text())
                    .child(selection.text(title.to_string(), None))
                    .into_any_element(),
            );
            index += 1;
            continue;
        }

        // Block quote.
        if trimmed.starts_with('>') {
            let mut quote = String::new();
            while index < lines.len() {
                let candidate = lines[index].trim_start();
                let Some(content) = candidate.strip_prefix('>') else {
                    break;
                };
                if !quote.is_empty() {
                    quote.push('\n');
                }
                quote.push_str(content.strip_prefix(' ').unwrap_or(content));
                index += 1;
            }
            elements.push(
                div()
                    .w_full()
                    .border_l_2()
                    .border_color(theme::line_strong())
                    .pl(px(12.))
                    .py(px(2.))
                    .text_color(style.dim)
                    .child(inline_element(&quote, style, &mut selection))
                    .into_any_element(),
            );
            continue;
        }

        // Unordered list.
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            let mut items: Vec<String> = Vec::new();
            while index < lines.len() {
                let candidate = lines[index].trim_start();
                if candidate.starts_with("- ") || candidate.starts_with("* ") || candidate.starts_with("+ ") {
                    items.push(candidate[2..].to_string());
                    index += 1;
                } else if !candidate.trim().is_empty()
                    && lines[index].starts_with([' ', '\t'])
                    && !items.is_empty()
                {
                    // A soft continuation line of the previous item.
                    if let Some(last) = items.last_mut() {
                        last.push(' ');
                        last.push_str(candidate.trim());
                    }
                    index += 1;
                } else {
                    break;
                }
            }
            for item in items {
                elements.push(
                    div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_start()
                        .gap(px(8.))
                        .child(
                            div()
                                .w(px(10.))
                                .flex_none()
                                .pt(px(1.))
                                .text_color(style.accent)
                                .child(SharedString::from("•")),
                        )
                        .child(div().flex_1().min_w(px(0.)).child(inline_element(&item, style, &mut selection)))
                        .into_any_element(),
                );
            }
            continue;
        }

        // Ordered list.
        let numbered = {
            let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
            digits > 0 && trimmed[digits..].starts_with(". ")
        };
        if numbered {
            let mut items: Vec<(String, String)> = Vec::new();
            while index < lines.len() {
                let candidate = lines[index].trim_start();
                let digits = candidate.chars().take_while(char::is_ascii_digit).count();
                if digits > 0 && candidate[digits..].starts_with(". ") {
                    let marker = candidate[..digits].to_string();
                    items.push((marker, candidate[digits + 2..].to_string()));
                    index += 1;
                } else {
                    break;
                }
            }
            for (marker, item) in items {
                elements.push(
                    div()
                        .w_full()
                        .flex()
                        .flex_row()
                        .items_start()
                        .gap(px(8.))
                        .child(
                            div()
                                .w(px(18.))
                                .flex_none()
                                .text_color(style.dim)
                                .text_size(px(13.))
                                .child(SharedString::from(format!("{marker}."))),
                        )
                        .child(div().flex_1().min_w(px(0.)).child(inline_element(&item, style, &mut selection)))
                        .into_any_element(),
                );
            }
            continue;
        }

        // Paragraph: gather until a blank line or another block starts.
        let mut paragraph = String::new();
        while index < lines.len() {
            let candidate = lines[index];
            let candidate_trimmed = candidate.trim_start();
            let is_block_start = candidate_trimmed.starts_with("```")
                || candidate_trimmed.starts_with("~~~")
                || candidate_trimmed.starts_with("# ")
                || candidate_trimmed.starts_with("## ")
                || candidate_trimmed.starts_with("> ")
                || candidate_trimmed.starts_with("- ")
                || candidate_trimmed.starts_with("* ")
                || candidate_trimmed.starts_with("+ ")
                || image_reference(candidate_trimmed).is_some();
            if candidate.trim().is_empty()
                || ((is_block_start || table_header(&lines, index).is_some()) && !paragraph.is_empty()) {
                break;
            }
            if !paragraph.is_empty() {
                paragraph.push('\n');
            }
            paragraph.push_str(candidate.trim_end());
            index += 1;
        }
        if !paragraph.is_empty() {
            elements.push(
                div()
                    .w_full()
                    .child(inline_element(&paragraph, style, &mut selection))
                    .into_any_element(),
            );
        } else {
            index += 1;
        }
    }

    vec![selection.finish(id, elements)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_images_parse_without_consuming_partial_streams() {
        assert_eq!(image_reference("![Dish](assets/icon.svg)"), Some(("Dish", "assets/icon.svg")));
        assert_eq!(image_reference(" ![](<assets/my icon.png>) "), Some(("", "assets/my icon.png")));
        assert!(image_reference("![Dish](assets/icon.svg").is_none());
        assert!(image_reference("[Dish](assets/icon.svg)").is_none());
    }

    #[test]
    fn image_paths_use_session_directory_and_reject_remote_sources() {
        let cwd = std::path::Path::new("/projects/dish");
        assert_eq!(local_image_path("assets/icon.svg", cwd).unwrap(), cwd.join("assets/icon.svg"));
        assert_eq!(local_image_path("/tmp/icon.PNG", cwd).unwrap(), std::path::PathBuf::from("/tmp/icon.PNG"));
        for target in ["https://example.com/icon.png", "file:///tmp/icon.png", "//example.com/icon.svg", "data:image/png;base64,abc", "assets/file.rs"] {
            assert!(local_image_path(target, cwd).is_none(), "{target}");
        }
    }

    #[test]
    fn table_cells_preserve_inline_code_and_escaped_pipes() {
        assert_eq!(table_cells("| arquivo | avaliação |"), vec!["arquivo", "avaliação"]);
        assert_eq!(table_cells("| `a|b` | a\\|b |"), vec!["`a|b`", "a|b"]);
        assert_eq!(table_cells("a | b"), vec!["a", "b"]);
        assert_eq!(table_cells("| a | |"), vec!["a", ""]);
    }

    #[test]
    fn tables_require_matching_header_and_delimiter_columns() {
        let alignments = table_header(&["| A | B | C |", "| :-- | :-: | --: |"], 0).unwrap();
        assert_eq!(alignments, vec![gpui::TextAlign::Left, gpui::TextAlign::Center, gpui::TextAlign::Right]);
        assert!(table_header(&["A | B", "---"], 0).is_none());
        assert!(table_header(&["A | B", "a | b"], 0).is_none());
        assert!(table_header(&["A | B"], 0).is_none());
    }
}
