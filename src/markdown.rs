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
///
/// The scanner remembers a delimiter that has no closing partner and stops
/// searching for that construct. This keeps malformed streaming text linear:
/// a line with thousands of unmatched `[` or `*` cannot restart a full scan at
/// every character. Recursion is bounded for the same reason.
const MAX_INLINE_DEPTH: usize = 32;

fn push_runs(
    text: &str,
    inline: Inline,
    style: &MarkdownStyle,
    out_text: &mut String,
    out_runs: &mut Vec<TextRun>,
) {
    push_runs_depth(text, inline, style, out_text, out_runs, 0);
}

fn push_runs_depth(
    text: &str,
    inline: Inline,
    style: &MarkdownStyle,
    out_text: &mut String,
    out_runs: &mut Vec<TextRun>,
    depth: usize,
) {
    if text.is_empty() {
        return;
    }
    if depth >= MAX_INLINE_DEPTH {
        let mut run = run_for(style, inline);
        run.len = text.len();
        out_text.push_str(text);
        out_runs.push(run);
        return;
    }
    let mut plain_start = 0usize;
    let mut index = 0usize;
    let mut bold_open = true;
    let mut code_open = true;
    let mut link_open = true;
    let mut italic_star = true;
    let mut italic_under = true;

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
            if bold_open && rest.starts_with("**") {
                let after = &rest[2..];
                match after.find("**") {
                    Some(end) => {
                        let inner = &after[..end];
                        if !inner.is_empty() {
                            consumed =
                                Some((index, index + 2 + end + 2, inner.to_string(), inline.bold()));
                        }
                    }
                    None => bold_open = false,
                }
            } else if code_open && rest.starts_with('`') {
                let after = &rest[1..];
                match after.find('`') {
                    Some(end) => {
                        let inner = &after[..end];
                        consumed =
                            Some((index, index + 1 + end + 1, inner.to_string(), inline.code()));
                    }
                    None => code_open = false,
                }
            } else if link_open && rest.starts_with('[') {
                match rest.find("](") {
                    Some(close) => match rest[close + 2..].find(')') {
                        Some(end) => {
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
                        None => link_open = false,
                    },
                    None => link_open = false,
                }
            } else if italic_star && rest.starts_with('*') {
                match rest[1..].find('*') {
                    Some(end) => {
                        let inner = &rest[1..1 + end];
                        if !inner.is_empty() && !inner.contains('\n') {
                            consumed =
                                Some((index, index + 1 + end + 1, inner.to_string(), inline.italic()));
                        }
                    }
                    None => italic_star = false,
                }
            } else if italic_under && rest.starts_with('_') {
                match rest[1..].find('_') {
                    Some(end) => {
                        let inner = &rest[1..1 + end];
                        if !inner.is_empty() && !inner.contains('\n') {
                            consumed =
                                Some((index, index + 1 + end + 1, inner.to_string(), inline.italic()));
                        }
                    }
                    None => italic_under = false,
                }
            }
        }

        match consumed {
            Some((start, end, inner, nested)) => {
                push_plain(plain_start, start, out_text, out_runs);
                push_runs_depth(&inner, nested, style, out_text, out_runs, depth + 1);
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
        .rounded(theme::r_technical())
        .border_1()
        .border_color(theme::line())
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
                .border_b_1()
                .border_color(theme::line_soft())
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
        .w_full().overflow_x_scroll().border_1().border_color(theme::line())
        .rounded(theme::r_technical()).child(body).into_any_element()
}

/// Standalone Markdown images; incomplete streaming syntax stays ordinary text.
fn image_reference(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix("![")?;
    let (alt, target) = rest.split_once("](")?;
    let target = target.strip_suffix(')')?.trim();
    let target = target.strip_prefix('<').and_then(|s| s.strip_suffix('>')).unwrap_or(target);
    (!target.is_empty()).then_some((alt, target))
}

/// A local image can be opened externally, but previews are restricted to
/// regular files inside the session project and limited in size. SVG stays
/// out of the preview path because the decoder can resolve external resources.
const MAX_IMAGE_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImagePathError {
    Remote,
    Unsupported,
    Missing,
    NotFile,
    OutsideProject,
}

impl ImagePathError {
    fn message(self) -> &'static str {
        match self {
            Self::Remote => "Prévia indisponível: apenas imagens locais são carregadas.",
            Self::Unsupported => "Prévia indisponível: formato de imagem não suportado.",
            Self::Missing => "Prévia indisponível: imagem não encontrada.",
            Self::NotFile => "Prévia indisponível: o caminho não é um arquivo comum.",
            Self::OutsideProject => "Prévia indisponível: a imagem está fora do projeto.",
        }
    }
}

fn local_image_path(
    target: &str,
    cwd: &std::path::Path,
) -> Result<std::path::PathBuf, ImagePathError> {
    // Never turn untrusted remote URLs into automatic network requests.
    if target.contains("://") || target.starts_with("data:") || target.starts_with("//") {
        return Err(ImagePathError::Remote);
    }
    let path = std::path::Path::new(target);
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .ok_or(ImagePathError::Unsupported)?;
    if !matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg"
    ) {
        return Err(ImagePathError::Unsupported);
    }
    let candidate = if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    };
    let root = std::fs::canonicalize(cwd).map_err(|_| ImagePathError::Missing)?;
    let path = std::fs::canonicalize(candidate).map_err(|_| ImagePathError::Missing)?;
    if !path.starts_with(&root) {
        return Err(ImagePathError::OutsideProject);
    }
    let metadata = std::fs::metadata(&path).map_err(|_| ImagePathError::Missing)?;
    if !metadata.is_file() {
        return Err(ImagePathError::NotFile);
    }
    Ok(path)
}

fn preview_problem(path: &std::path::Path) -> Result<(), &'static str> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "svg" {
        return Err("Prévia SVG desativada por segurança; use Abrir externamente.");
    }
    let metadata = std::fs::metadata(path).map_err(|_| "Imagem indisponível ou inválida.")?;
    if !metadata.is_file() {
        return Err("Prévia indisponível: o caminho não é um arquivo comum.");
    }
    if metadata.len() > MAX_IMAGE_PREVIEW_BYTES {
        return Err("Imagem grande demais para prévia (máx. 10 MiB); use Abrir externamente.");
    }
    Ok(())
}

fn image_block(
    alt: &str,
    target: &str,
    cwd: &std::path::Path,
    index: usize,
    style: &MarkdownStyle,
) -> AnyElement {
    let caption = if alt.is_empty() { target.to_owned() } else { format!("{alt} — {target}") };
    let mut block = div().w_full().flex().flex_col().gap(px(8.));
    match local_image_path(target, cwd) {
        Ok(path) => {
            let external_path = path.clone();
            match preview_problem(&path) {
                Ok(()) => {
                    block = block.child(
                        div()
                            .w(px(200.))
                            .max_w_full()
                            .h(px(200.))
                            .flex_none()
                            .overflow_hidden()
                            .child(
                                img(path)
                                    .size_full()
                                    .object_fit(ObjectFit::Contain)
                                    .with_fallback(|| {
                                        div()
                                            .p(px(12.))
                                            .child("Imagem indisponível ou inválida.")
                                            .into_any_element()
                                    }),
                            ),
                    );
                }
                Err(reason) => {
                    block = block.child(div().p(px(12.)).child(reason));
                }
            }
            block = block.child(
                div()
                    .id(("markdown-image-open", index))
                    .cursor_pointer()
                    .text_color(style.accent)
                    .on_click(move |_, _, cx| cx.open_with_system(&external_path))
                    .child("Abrir externamente"),
            );
        }
        Err(error) => {
            block = block.child(div().p(px(12.)).child(error.message()));
        }
    }
    block
        .child(div().text_color(style.dim).child(SharedString::from(caption)))
        .into_any_element()
}


/// Render Markdown into a list of block elements.
pub fn blocks(text: &str, style: &MarkdownStyle, cwd: &std::path::Path, id: gpui::ElementId, search: Option<&str>) -> Vec<AnyElement> {
    let mut selection = crate::ui::selectable::SelectionBuilder::with_search(search);
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
    fn image_previews_stay_inside_the_project_and_reject_remote_sources() {
        let root = std::env::temp_dir().join(format!(
            "dish-markdown-images-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = root.join("project");
        let assets = project.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        let png = assets.join("icon.PNG");
        std::fs::write(&png, b"png").unwrap();
        let svg = assets.join("icon.svg");
        std::fs::write(&svg, b"<svg/>").unwrap();
        let outside = root.join("outside.png");
        std::fs::write(&outside, b"outside").unwrap();
        let canonical_png = std::fs::canonicalize(&png).unwrap();

        assert_eq!(
            local_image_path("assets/icon.PNG", &project).unwrap(),
            canonical_png
        );
        assert_eq!(
            local_image_path(&png.to_string_lossy(), &project).unwrap(),
            canonical_png
        );
        assert_eq!(
            local_image_path("assets/icon.svg", &project).unwrap(),
            std::fs::canonicalize(&svg).unwrap()
        );
        assert_eq!(
            local_image_path("../outside.png", &project),
            Err(ImagePathError::OutsideProject)
        );
        assert_eq!(
            local_image_path(&outside.to_string_lossy(), &project),
            Err(ImagePathError::OutsideProject)
        );
        for target in [
            "https://example.com/icon.png",
            "file:///tmp/icon.png",
            "//example.com/icon.svg",
            "data:image/png;base64,abc",
            "assets/file.rs",
        ] {
            assert!(local_image_path(target, &project).is_err(), "{target}");
        }
        assert_eq!(
            local_image_path("missing.png", &project),
            Err(ImagePathError::Missing)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = assets.join("link.png");
            symlink(&outside, &link).unwrap();
            assert_eq!(
                local_image_path("assets/link.png", &project),
                Err(ImagePathError::OutsideProject)
            );
        }
        assert_eq!(preview_problem(&png), Ok(()));
        assert!(preview_problem(&svg).is_err());
        let large = assets.join("large.png");
        let file = std::fs::File::create(&large).unwrap();
        file.set_len(MAX_IMAGE_PREVIEW_BYTES + 1).unwrap();
        assert!(preview_problem(&large).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inline_markup_still_styles_after_the_linear_scan() {
        let style = crate::ui::markdown_style();
        let mut out = String::new();
        let mut runs = Vec::new();
        push_runs(
            "**negrito** *itálico* `código` [link](https://example.invalid)",
            Inline::default(),
            &style,
            &mut out,
            &mut runs,
        );
        assert_eq!(out, "negrito itálico código link");
        assert!(runs.iter().any(|run| run.font.weight == FontWeight::BOLD));
        assert!(runs.iter().any(|run| run.background_color.is_some()));
        assert!(runs.iter().any(|run| run.color == style.accent));
    }

    #[test]
    fn malformed_inline_brackets_do_not_rescan_or_hide_later_markup() {
        let style = crate::ui::markdown_style();
        let text = format!("{} **bold**", "[".repeat(20_000));
        let mut out = String::new();
        let mut runs = Vec::new();
        push_runs(&text, Inline::default(), &style, &mut out, &mut runs);
        assert_eq!(out, format!("{} bold", "[".repeat(20_000)));
        assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), out.len());
        assert!(runs.iter().any(|run| run.font.weight == FontWeight::BOLD));
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
