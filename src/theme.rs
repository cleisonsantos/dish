//! Visual language for Dish.
//!
//! Three rules hold the design together:
//!
//! 1. **Graphite with separation.** Surfaces step up from the conversation to
//!    the panels; nothing is outlined just to be outlined. Steps between
//!    layers carry a touch more blue as they rise, so the depth is felt
//!    instead of only measured.
//! 2. **The accent is the effort meter.** Pi's own effort palette drives the
//!    accent, so the window looks different when the model is thinking hard.
//!    It stays concentrated on the effort control, focus and small indicators.
//! 3. **Effort colours are not status colours.** Execution, success and failure
//!    live on their own hues so that `max` never reads as an error — enforced by
//!    the tests at the bottom of this file, not by opinion.
//!
//! The palette is a [`Theme`] value. `dish-dark` is the original look and
//! `dish-light` is its light counterpart. Six additional presets cover warm,
//! dim, blue, plum, lavender and sage surfaces. User themes live in
//! `$XDG_CONFIG_HOME/dish/themes/*.json` (see
//! `docs/themes.md`). The active one is resolved at render time from the
//! user's choice and the desktop, so switching never needs a restart.

use gpui::{BoxShadow, Hsla, Pixels, WindowAppearance, point, px, rgb_to_hsla, rgba};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// Fonts compiled into the binary: the reading serif and the interface sans.
pub fn bundled_fonts() -> Vec<std::borrow::Cow<'static, [u8]>> {
    vec![
        std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/source-serif-4/SourceSerif4-Regular.ttf").as_slice()),
        std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/source-serif-4/SourceSerif4-It.ttf").as_slice()),
        std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/source-serif-4/SourceSerif4-Semibold.ttf").as_slice()),
        std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/geist/Geist-Regular.ttf").as_slice()),
        std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/geist/Geist-Medium.ttf").as_slice()),
        std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/geist/Geist-SemiBold.ttf").as_slice()),
    ]
}

/// Reading font for prose (answers, requests, titles). Bundled, so it always resolves.
pub const FONT_PROSE: &str = "Source Serif 4";

/// Interface fonts, in order of preference. The first installed one wins.
pub const UI_FONT_CANDIDATES: &[&str] = &[
    "Geist", "Noto Sans", "Inter", "Segoe UI", "Cantarell", "Ubuntu", "DejaVu Sans",
    "Helvetica Neue", "Liberation Sans", "Arial",
];
/// Monospace fonts, in order of preference. Anything a machine wrote must stay
/// monospaced even when JetBrains Mono is missing: GPUI has no generic
/// `monospace` fallback, so an absent family lands on a proportional face.
pub const MONO_FONT_CANDIDATES: &[&str] = &[
    "JetBrains Mono", "Cascadia Mono", "Cascadia Code", "Fira Code", "Source Code Pro",
    "DejaVu Sans Mono", "Ubuntu Mono", "Liberation Mono", "Noto Sans Mono", "Consolas",
    "Menlo", "Courier New",
];

static FONT_UI_FAMILY: OnceLock<&'static str> = OnceLock::new();
static FONT_MONO_FAMILY: OnceLock<&'static str> = OnceLock::new();

/// First candidate present in `installed` (case-insensitive), else the first candidate.
fn pick_font(candidates: &[&'static str], installed: &[String]) -> &'static str {
    candidates
        .iter()
        .copied()
        .find(|candidate| installed.iter().any(|name| name.eq_ignore_ascii_case(candidate)))
        .unwrap_or(candidates[0])
}

/// Resolve both families once, from the fonts the platform reports.
pub fn resolve_fonts(installed: &[String]) {
    let _ = FONT_UI_FAMILY.set(pick_font(UI_FONT_CANDIDATES, installed));
    let _ = FONT_MONO_FAMILY.set(pick_font(MONO_FONT_CANDIDATES, installed));
}

/// Interface font.
pub fn font_ui() -> &'static str {
    FONT_UI_FAMILY.get().copied().unwrap_or(UI_FONT_CANDIDATES[0])
}
/// Monospace font, used for anything a machine wrote.
pub fn font_mono() -> &'static str {
    FONT_MONO_FAMILY.get().copied().unwrap_or(MONO_FONT_CANDIDATES[0])
}

// ----------------------------------------------------------------- tipografia

/// Reading text: the assistant's answer and the user's request.
pub const PROSE: f32 = 15.0;
/// Leading for reading text: 1.55 of the prose size, comfortable for long answers.
pub const PROSE_LINE: f32 = 23.0;
/// Session title.
pub const TITLE: f32 = 20.0;
/// Default interface text.
pub const TEXT: f32 = 13.0;
/// Welcome heading: the only display-sized type in the workspace.
pub const DISPLAY: f32 = 32.0;
/// Fixed inset, shared by header, transcript and composer at any window width.
pub const CHAT_INSET: f32 = 16.0;
/// Reading measure: transcript, composer and the rows around them share one
/// centred column, so prose stays near 90 characters per line on wide windows.
/// Includes the `CHAT_INSET` on both sides.
pub const READING_WIDTH: f32 = 840.0;
/// Popup controls keep their own measure when the writing area grows.
pub const EFFORT_MENU_WIDTH: f32 = 360.0;
pub const COMPOSER_PICKER_WIDTH: f32 = 640.0;
/// Code stays readable on wide windows and reveals overflow in long snippets.
/// Code fills the reading column, aligned with tables and prose.
pub const CODE_BLOCK_MAX_WIDTH: f32 = READING_WIDTH - 2.0 * CHAT_INSET;
pub const CODE_BLOCK_MAX_HEIGHT: f32 = 340.0;
/// User bubbles stay compact but long requests can use a wide workspace.
pub const REQUEST_MAX_FRACTION: f32 = 0.9;
/// The writing surface reserves space without growing on every short draft.
pub const WRITING_HEIGHT: f32 = 52.0;
/// Header rhythm shared by the conversation and inspector.
pub const HEADER_HEIGHT: f32 = 72.0;
/// Default expanded navigation width.
pub const NAV_WIDTH: f32 = 320.0;
/// The fixed icon rail on the left edge; the sessions drawer opens beside it.
pub const RAIL_WIDTH: f32 = 56.0;
/// From this window width the sessions drawer docks beside the conversation;
/// below it the drawer floats over the conversation.
pub const DRAWER_DOCK_WIDTH: f32 = 1120.0;
/// Secondary interface text: rows, values.
pub const TEXT_SM: f32 = 12.0;
/// Metadata: paths, durations, counts.
pub const TEXT_XS: f32 = 11.5;
/// Micro labels, used sparingly.
pub const TEXT_MICRO: f32 = 11.0;
/// Technical values inside monospace blocks.
pub const MONO: f32 = 12.5;

pub fn body_line_height() -> Pixels {
    px(20.)
}

// -------------------------------------------------------------------- espaço

pub const S1: f32 = 4.;
pub const S2: f32 = 8.;
pub const S3: f32 = 12.;
pub const S4: f32 = 16.;
pub const S5: f32 = 24.;
pub const S6: f32 = 32.;

/// Radius for controls.
pub fn r_control() -> Pixels {
    px(8.)
}
/// Radius for inner cards and technical blocks.
pub fn r_small() -> Pixels {
    px(6.)
}
/// Radius for surfaces: composer, code, panels.
pub fn r_surface() -> Pixels {
    px(14.)
}
/// The paint inside a 1px surface border shares its corner's centre.
/// GPUI's overflow mask is rectangular, so opaque children need their own radius.
pub fn r_surface_inner() -> Pixels {
    px(13.)
}
/// Radius for the isolated actions: send, stop, primary setup action.
pub fn r_action() -> Pixels {
    px(10.)
}
/// Radius for technical fields: recessed search and detail wells.
pub fn r_technical() -> Pixels {
    px(4.)
}

#[inline]
fn c(hex: u32) -> Hsla {
    rgb_to_hsla(rgba(hex))
}

// ---------------------------------------------------------------------- temas
//
// Um tema é um valor: cada papel semântico da interface é um campo. Os temas
// embutidos são construídos uma única vez; o índice ativo fica num átomo
// porque a interface lê cores centenas de vezes por quadro.

/// Como o usuário quer a aparência da janela.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppearanceMode {
    Dark,
    Light,
    /// Segue o tema do desktop.
    #[default]
    System,
}

/// Um esquema de cores completo. `effort` guarda a rampa na ordem
/// `off, minimal, low, medium, high, xhigh, max`.
#[derive(Clone, PartialEq, Debug)]
pub struct Theme {
    /// Identificador estável de um tema embutido, ou `user:<arquivo>`.
    pub id: String,
    /// Nome exibido na tela de Aparência.
    pub name: String,
    pub dark: bool,
    /// Veio de `themes/*.json`, e não do catálogo embutido.
    pub user: bool,
    pub bg: Hsla,
    pub surface: Hsla,
    pub surface_2: Hsla,
    pub inset: Hsla,
    pub hover: Hsla,
    pub line: Hsla,
    pub line_soft: Hsla,
    pub line_strong: Hsla,
    pub text: Hsla,
    pub dim: Hsla,
    pub faint: Hsla,
    pub on_accent: Hsla,
    /// O acento da identidade: fixo por tema, independente do esforço.
    pub brand: Hsla,
    pub running: Hsla,
    pub failure: Hsla,
    pub warn: Hsla,
    pub attention: Hsla,
    pub ok: Hsla,
    pub git_added: Hsla,
    pub git_removed: Hsla,
    pub effort: [Hsla; 7],
}

/// Níveis de raciocínio, na ordem da rampa de esforço.
pub const LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

fn base_theme(id: &str, name: &str, dark: bool) -> Theme {
    Theme {
        id: id.to_owned(),
        name: name.to_owned(),
        dark,
        user: false,
        bg: c(0x000000ff),
        surface: c(0x000000ff),
        surface_2: c(0x000000ff),
        inset: c(0x000000ff),
        hover: c(0x000000ff),
        line: c(0x000000ff),
        line_soft: c(0x000000ff),
        line_strong: c(0x000000ff),
        text: c(0x000000ff),
        dim: c(0x000000ff),
        faint: c(0x000000ff),
        on_accent: c(0x000000ff),
        brand: c(0x000000ff),
        running: c(0x000000ff),
        failure: c(0x000000ff),
        warn: c(0x000000ff),
        attention: c(0x000000ff),
        ok: c(0x000000ff),
        git_added: c(0x000000ff),
        git_removed: c(0x000000ff),
        effort: [c(0x000000ff); 7],
    }
}

/// Noite: papel escuro levemente esverdeado, tinta clara e acento verdete.
/// Mantém a id `dish-dark` para preferências salvas antes do redesign.
fn dark_theme() -> Theme {
    Theme {
        bg: c(0x131614ff),
        surface: c(0x171a18ff),
        surface_2: c(0x1d211eff),
        inset: c(0x0e100fff),
        hover: c(0x252a27ff),
        line: c(0x2a2f2cff),
        line_soft: c(0x212522ff),
        line_strong: c(0x3a403cff),
        text: c(0xe9ece7ff),
        dim: c(0xb5bcb5ff),
        faint: c(0x8e968fff),
        on_accent: c(0x0f1a16ff),
        brand: c(0x86c2acff),
        running: c(0x86c2acff),
        failure: c(0xe0876aff),
        warn: c(0xd9a441ff),
        attention: c(0xe9ece7ff),
        ok: c(0xb5bcb5ff),
        git_added: c(0x81b88bff),
        git_removed: c(0xd98282ff),
        effort: [
            c(0x6c767bff),
            c(0x68808dff),
            c(0x5489a4ff),
            c(0x6185ccff),
            c(0x9776e5ff),
            c(0xde54c1ff),
            c(0xfe5462ff),
        ],
        ..base_theme("dish-dark", "Noite", true)
    }
}

/// Papel: o par claro da Noite, papel quase branco e tinta verde-escura.
/// Mantém a id `dish-light`.
fn light_theme() -> Theme {
    Theme {
        bg: c(0xf6f6f2ff),
        surface: c(0xeeefeaff),
        surface_2: c(0xffffffff),
        inset: c(0xecede6ff),
        hover: c(0xe6e8e1ff),
        line: c(0xd6d9d0ff),
        line_soft: c(0xe2e4dcff),
        line_strong: c(0xc3c7bcff),
        text: c(0x1b1e1cff),
        dim: c(0x444a45ff),
        faint: c(0x5e655fff),
        on_accent: c(0xffffffff),
        brand: c(0x2e6b58ff),
        running: c(0x2e6b58ff),
        failure: c(0xa8432bff),
        warn: c(0x8a6212ff),
        attention: c(0x1b1e1cff),
        ok: c(0x444a45ff),
        git_added: c(0x2f6f38ff),
        git_removed: c(0xa83c3cff),
        effort: [
            c(0x6a7378ff),
            c(0x53778bff),
            c(0x4a778fff),
            c(0x4670c3ff),
            c(0x7d54deff),
            c(0xca27a8ff),
            c(0xe50c1eff),
        ],
        ..base_theme("dish-light", "Papel", false)
    }
}

/// Ardósia: escuro azulado, para quem prefere o frio ao verde.
fn slate_theme() -> Theme {
    Theme {
        bg: c(0x1a1d24ff),
        surface: c(0x15181eff),
        surface_2: c(0x21252dff),
        inset: c(0x101217ff),
        hover: c(0x2a2f38ff),
        line: c(0x30353fff),
        line_soft: c(0x252a33ff),
        line_strong: c(0x434a57ff),
        text: c(0xe6e9f0ff),
        dim: c(0xb3bac8ff),
        faint: c(0x959dadff),
        ok: c(0xb3bac8ff),
        ..base_theme("dish-slate", "Ardósia", true)
    }
    .inherit(&dark_theme())
    .with_brand(c(0x8fb0e8ff), c(0x10151fff))
}

/// Névoa: claro de cinza quente, acento azul-tinta.
fn mist_theme() -> Theme {
    Theme {
        bg: c(0xf3f1ecff),
        surface: c(0xeae7e0ff),
        surface_2: c(0xfcfbf8ff),
        inset: c(0xe6e2d9ff),
        hover: c(0xe3dfd6ff),
        line: c(0xd5d0c4ff),
        line_soft: c(0xe2ded4ff),
        line_strong: c(0xc2bcaeff),
        text: c(0x2a2722ff),
        dim: c(0x4a453dff),
        faint: c(0x5f594fff),
        ok: c(0x4a453dff),
        ..base_theme("dish-mist", "Névoa", false)
    }
    .inherit(&light_theme())
    .with_brand(c(0x3e5c8aff), c(0xffffffff))
}

impl Theme {
    /// Completa com os semânticos, a rampa e o `on-accent` do tema base.
    fn inherit(mut self, base: &Theme) -> Theme {
        self.on_accent = base.on_accent;
        self.running = base.running;
        self.failure = base.failure;
        self.warn = base.warn;
        self.attention = base.attention;
        self.git_added = base.git_added;
        self.git_removed = base.git_removed;
        self.effort = base.effort;
        self.brand = base.brand;
        self
    }

    /// Acento próprio de um preset; `running` acompanha o acento.
    fn with_brand(mut self, brand: Hsla, on_accent: Hsla) -> Theme {
        self.brand = brand;
        self.running = brand;
        self.on_accent = on_accent;
        self
    }
}

/// Os temas embutidos, na ordem em que a tela de Aparência os mostra.
pub fn built_in_themes() -> Vec<Theme> {
    vec![dark_theme(), slate_theme(), light_theme(), mist_theme()]
}

// ------------------------------------------------------------------- catálogo

static THEMES: OnceLock<Vec<Theme>> = OnceLock::new();
static THEME_ERRORS: OnceLock<Vec<String>> = OnceLock::new();

/// O catálogo em uso: embutidos e temas do usuário, nessa ordem.
pub fn themes() -> &'static [Theme] {
    THEMES.get_or_init(|| {
        let (list, errors) = build_themes();
        let _ = THEME_ERRORS.set(errors);
        list
    })
}

/// Garante o catálogo carregado e devolve os erros dos arquivos de tema.
///
/// Um arquivo malformado nunca impede a inicialização: o tema é ignorado e o
/// aviso vira um toast, com o nome do arquivo e o erro.
pub fn preload() -> &'static [String] {
    let _ = themes();
    THEME_ERRORS.get().map(Vec::as_slice).unwrap_or(&[])
}

/// O tema do catálogo com essa id, se existir.
pub fn theme_by_id(id: &str) -> Option<&'static Theme> {
    themes().iter().find(|theme| theme.id == id)
}

/// Lê os temas do usuário do diretório de configuração. Em testes o diretório
/// real não é tocado: os testes de carregamento usam um diretório próprio.
#[cfg(not(test))]
fn user_themes_dir() -> Option<std::path::PathBuf> {
    crate::preferences::themes_dir()
}
#[cfg(test)]
fn user_themes_dir() -> Option<std::path::PathBuf> {
    None
}

fn build_themes() -> (Vec<Theme>, Vec<String>) {
    match user_themes_dir() {
        Some(directory) => build_themes_from(&directory),
        None => (built_in_themes(), Vec::new()),
    }
}

/// Lê o diretório em ordem de nome de arquivo, para o catálogo ser estável.
fn build_themes_from(directory: &std::path::Path) -> (Vec<Theme>, Vec<String>) {
    let mut list = built_in_themes();
    let mut errors = Vec::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return (list, errors);
    };
    let mut paths: Vec<std::path::PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .collect();
    paths.sort();
    for path in paths {
        match load_theme(&path) {
            Ok(theme) => list.push(theme),
            Err(error) => errors.push(error),
        }
    }
    (list, errors)
}

/// Um arquivo de tema, no formato de `docs/themes.md`.
#[derive(Deserialize)]
struct ThemeFile {
    #[serde(default, rename = "$schema")]
    #[allow(dead_code)]
    schema: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    appearance: Option<String>,
    #[serde(default)]
    colors: ThemeColors,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ThemeColors {
    bg: Option<String>,
    surface: Option<String>,
    surface2: Option<String>,
    inset: Option<String>,
    hover: Option<String>,
    line: Option<String>,
    line_soft: Option<String>,
    line_strong: Option<String>,
    text: Option<String>,
    dim: Option<String>,
    faint: Option<String>,
    on_accent: Option<String>,
    accent: Option<String>,
    running: Option<String>,
    failure: Option<String>,
    warn: Option<String>,
    attention: Option<String>,
    ok: Option<String>,
    git_added: Option<String>,
    git_removed: Option<String>,
}

/// `#rrggbb` ou `#rrggbbaa`.
fn parse_hex(value: &str) -> Option<Hsla> {
    let digits = value.trim().trim_start_matches('#');
    let full = match digits.len() {
        6 => format!("{digits}ff"),
        8 => digits.to_owned(),
        _ => return None,
    };
    u32::from_str_radix(&full, 16).ok().map(c)
}

fn load_theme(path: &std::path::Path) -> Result<Theme, String> {
    let label = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let bytes = std::fs::read(path)
        .map_err(|error| format!("Tema {label}: não foi possível ler o arquivo ({error})"))?;
    let file: ThemeFile = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Tema {label}: JSON inválido ({error})"))?;
    let dark = match file.appearance.as_deref() {
        None | Some("dark") => true,
        Some("light") => false,
        Some(other) => {
            return Err(format!(
                "Tema {label}: aparência desconhecida `{other}` (use `dark` ou `light`)"
            ))
        }
    };
    let base = if dark { dark_theme() } else { light_theme() };
    let colors = file.colors;
    let pick = |value: Option<String>, key: &str, fallback: Hsla| -> Result<Hsla, String> {
        match value {
            None => Ok(fallback),
            Some(text) => parse_hex(&text).ok_or_else(|| {
                format!("Tema {label}: cor inválida em `{key}`: {text} (use #rrggbb)")
            }),
        }
    };
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| label.clone());
    let name = file
        .name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| stem.clone());
    let theme = Theme {
        id: format!("user:{stem}"),
        name,
        dark,
        user: true,
        bg: pick(colors.bg, "bg", base.bg)?,
        surface: pick(colors.surface, "surface", base.surface)?,
        surface_2: pick(colors.surface2, "surface2", base.surface_2)?,
        inset: pick(colors.inset, "inset", base.inset)?,
        hover: pick(colors.hover, "hover", base.hover)?,
        line: pick(colors.line, "line", base.line)?,
        line_soft: pick(colors.line_soft, "lineSoft", base.line_soft)?,
        line_strong: pick(colors.line_strong, "lineStrong", base.line_strong)?,
        text: pick(colors.text, "text", base.text)?,
        dim: pick(colors.dim, "dim", base.dim)?,
        faint: pick(colors.faint, "faint", base.faint)?,
        on_accent: pick(colors.on_accent, "onAccent", base.on_accent)?,
        brand: pick(colors.accent, "accent", base.brand)?,
        running: pick(colors.running, "running", base.running)?,
        failure: pick(colors.failure, "failure", base.failure)?,
        warn: pick(colors.warn, "warn", base.warn)?,
        attention: pick(colors.attention, "attention", base.attention)?,
        ok: pick(colors.ok, "ok", base.ok)?,
        git_added: pick(colors.git_added, "gitAdded", base.git_added)?,
        git_removed: pick(colors.git_removed, "gitRemoved", base.git_removed)?,
        effort: base.effort,
    };
    Ok(theme)
}

static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static MODE: AtomicU8 = AtomicU8::new(0);
static PINNED: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn selection() -> &'static Mutex<Option<String>> {
    PINNED.get_or_init(|| Mutex::new(None))
}

/// Fixa (ou solta) um tema do catálogo pela id escolhida pelo usuário.
pub fn set_theme(id: Option<String>) {
    let mut slot = match selection().lock() {
        Ok(guard) => guard,
        // Um pânico alheio não invalida a escolha guardada.
        Err(poisoned) => poisoned.into_inner(),
    };
    *slot = id;
}

/// O tema fixado pela preferência, se ainda existir no catálogo.
pub fn pinned_theme() -> Option<&'static Theme> {
    let id = {
        let slot = match selection().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        slot.clone()?
    };
    theme_by_id(&id)
}

/// Guarda a preferência do usuário; [`apply`] resolve a aparência efetiva.
pub fn set_mode(mode: AppearanceMode) {
    MODE.store(
        match mode {
            AppearanceMode::System => 0,
            AppearanceMode::Dark => 1,
            AppearanceMode::Light => 2,
        },
        Ordering::Relaxed,
    );
}

/// A preferência em vigor.
pub fn mode() -> AppearanceMode {
    match MODE.load(Ordering::Relaxed) {
        1 => AppearanceMode::Dark,
        2 => AppearanceMode::Light,
        _ => AppearanceMode::System,
    }
}

/// Decide se a aparência efetiva é escura: `Dark`/`Light` mandam no desktop;
/// `System` segue o tema dele (e o `NoPreference` do GPUI conta como claro).
pub fn prefers_dark(mode: AppearanceMode, system: WindowAppearance) -> bool {
    match mode {
        AppearanceMode::Dark => true,
        AppearanceMode::Light => false,
        AppearanceMode::System => matches!(
            system,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
    }
}

/// Resolve o tema efetivo a partir da preferência, do tema fixado e do
/// desktop. Chamado a cada render, como o esforço: a troca é imediata.
pub fn apply(system: WindowAppearance) {
    let list = themes();
    let index = pinned_theme()
        .and_then(|pinned| list.iter().position(|theme| theme.id == pinned.id))
        .or_else(|| {
            let wanted = if prefers_dark(mode(), system) {
                "dish-dark"
            } else {
                "dish-light"
            };
            list.iter().position(|theme| theme.id == wanted)
        })
        .unwrap_or(0);
    ACTIVE.store(index, Ordering::Relaxed);
}

/// O tema em uso agora.
pub fn active() -> &'static Theme {
    let list = themes();
    let index = ACTIVE.load(Ordering::Relaxed).min(list.len().saturating_sub(1));
    &list[index]
}

/// Se a aparência efetiva é escura.
pub fn is_dark() -> bool {
    active().dark
}

// ---------------------------------------------------------------- superfícies

/// The window itself, behind the conversation.
pub fn bg() -> Hsla {
    active().bg
}
/// Panels: navigation, inspector.
pub fn surface() -> Hsla {
    active().surface
}
/// Raised surfaces: composer, the user's request, the active row.
pub fn surface_2() -> Hsla {
    active().surface_2
}
/// Recessed surfaces: code, tool output, search fields.
pub fn inset() -> Hsla {
    active().inset
}
pub fn hover() -> Hsla {
    active().hover
}

pub fn line() -> Hsla {
    active().line
}
pub fn line_soft() -> Hsla {
    active().line_soft
}
pub fn line_strong() -> Hsla {
    active().line_strong
}

// ------------------------------------------------------------------- texto

pub fn text() -> Hsla {
    active().text
}
pub fn dim() -> Hsla {
    active().dim
}
pub fn faint() -> Hsla {
    active().faint
}
/// Texto sobre o accent (o botão de enviar, por exemplo).
pub fn on_accent() -> Hsla {
    active().on_accent
}

// --------------------------------------------------------------- semântica
//
// Fora da rampa de effort, que vai do cinza-azulado ao coral passando pelo
// magenta. Estas ficam a ΔE ≥ 25 de todos os níveis (ver testes).

/// Executando. Verde-água.
pub fn running() -> Hsla {
    active().running
}
/// Falha. Magenta-carmim, longe do coral do `max`.
pub fn failure() -> Hsla {
    active().failure
}
/// Aviso de recurso (contexto cheio), o âmbar que o Pi usa para isso.
pub fn warn() -> Hsla {
    active().warn
}
/// Sucesso: neutro de propósito — a marca é o ícone e o texto, não a cor.
pub fn ok() -> Hsla {
    active().ok
}
/// Conteúdo novo: a resposta terminou e ainda não foi lida. Verde vivo,
/// longe do teal de execução e do âmbar de aviso.
pub fn attention() -> Hsla {
    active().attention
}
/// Mantido para os pontos que ainda pedem a cor de perigo antiga.
pub fn danger() -> Hsla {
    failure()
}

/// Diff Git: cores semânticas próprias, sem alterar o acento do esforço.
pub fn git_added() -> Hsla {
    active().git_added
}
pub fn git_removed() -> Hsla {
    active().git_removed
}

pub fn wash(color: Hsla, alpha: f32) -> Hsla {
    Hsla {
        color: color.color,
        alpha,
    }
}

// ------------------------------------------------------------ effort accent
//
// Reference: Pi's modes/interactive/theme/dark.json thinking* tokens,
// converted from OKHSL with pi-tui's parseColor/colorToHex. The light theme
// keeps those hues and lowers lightness until each level reads on white.

/// The accent in use right now: the theme's own, never the effort level.
pub fn accent() -> Hsla {
    active().brand
}

/// The accent lifted for hover states. No tema escuro "levantar" é clarear;
/// no claro, o accent já é escuro e o hover escurece um pouco mais.
pub fn accent_hover() -> Hsla {
    let accent = accent();
    let mut hsl = accent.color;
    hsl.lightness = if is_dark() {
        (hsl.lightness + 0.10).min(0.92)
    } else {
        (hsl.lightness - 0.06).max(0.12)
    };
    Hsla {
        color: hsl,
        alpha: accent.alpha,
    }
}

/// A half-strength accent, for rules and borders.
pub fn accent_edge() -> Hsla {
    Hsla {
        color: accent().color,
        alpha: 0.42,
    }
}


fn effort_index(level: &str) -> usize {
    LEVELS.iter().position(|name| *name == level).unwrap_or(0)
}

/// The colour of a single reasoning level, so the selector can show the ramp.
pub fn effort_color(level: &str, _available: &[String]) -> Hsla {
    active().effort[effort_index(level)]
}

/// Kept for callers that still report the level: since the editorial
/// redesign the accent belongs to the theme and the level is shown as text.
pub fn set_effort(_level: Option<&str>, _available: &[String]) {}

/// Anel de foco do teclado. Fica fora da caixa, para não competir com as
/// bordas e cores de estado das linhas. Usa o acento do esforço: o foco é o
/// lugar onde a cor significa "é aqui que você está".
pub fn focus_ring() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: wash(accent(), 0.9),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(2.),
        inset: false,
    }]
}

/// Brilho suave na cor do esforço, para a superfície de escrita. `strength`
/// vai de 0 (apagado) a 1 (pico do pulso durante o trabalho do agente).
pub fn accent_glow(strength: f32) -> Vec<BoxShadow> {
    let strength = strength.clamp(0.0, 1.0);
    vec![
        BoxShadow {
            color: wash(accent(), (if is_dark() { 0.10 } else { 0.07 }) + 0.22 * strength),
            offset: point(px(0.), px(0.)),
            blur_radius: px(10. + 18. * strength),
            spread_radius: px(0.),
            inset: false,
        },
        BoxShadow {
            color: wash(inset(), if is_dark() { 0.32 } else { 0.16 }),
            offset: point(px(0.), px(6.)),
            blur_radius: px(20.),
            spread_radius: px(-4.),
            inset: false,
        },
    ]
}

/// Halo vertical do estado vazio: o acento esmaece até a superfície.
pub fn accent_wash() -> gpui::Background {
    gpui::linear_gradient(
        180.,
        gpui::linear_color_stop(wash(accent(), if is_dark() { 0.10 } else { 0.08 }), 0.0),
        gpui::linear_color_stop(wash(accent(), 0.0), 1.0),
    )
}

/// Fundo do código inline: um véu do próprio texto, não o `inset`. O poço
/// recuado (quase preto no escuro) partia a frase em blocos duros.
pub fn code_wash() -> Hsla {
    wash(text(), if is_dark() { 0.10 } else { 0.07 })
}

/// Cobertura translúcida atrás de diálogos; combinada com `backdrop_blur`.
pub fn scrim() -> Hsla {
    wash(bg(), if is_dark() { 0.62 } else { 0.55 })
}

/// Raio do desfoque do fundo atrás de diálogos e sobreposições.
pub const SCRIM_BLUR: f32 = 10.0;

/// A shallow lift for the writing surface, never for transcript rows.
pub fn shadow_composer() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: wash(inset(), if is_dark() { 0.32 } else { 0.18 }),
        offset: point(px(0.), px(6.)),
        blur_radius: px(20.),
        spread_radius: px(-4.),
        inset: false,
    }]
}

/// Shadows for things that float above the workspace.
pub fn shadow_overlay() -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: rgb_to_hsla(rgba(0x000000a6)),
            offset: point(px(0.), px(18.)),
            blur_radius: px(48.),
            spread_radius: px(0.),
            inset: false,
        },
        BoxShadow {
            color: rgb_to_hsla(rgba(0x00000059)),
            offset: point(px(0.), px(2.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
            inset: false,
        },
    ]
}

// --------------------------------------------------------------------- testes

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_pick_falls_back_to_an_installed_monospace() {
        let installed = vec!["DejaVu Sans".to_string(), "dejavu sans mono".to_string()];
        assert_eq!(pick_font(MONO_FONT_CANDIDATES, &installed), "DejaVu Sans Mono");
        assert_eq!(pick_font(UI_FONT_CANDIDATES, &installed), "DejaVu Sans");
        let preferred = vec!["JetBrains Mono".to_string(), "DejaVu Sans Mono".to_string()];
        assert_eq!(pick_font(MONO_FONT_CANDIDATES, &preferred), "JetBrains Mono");
        assert_eq!(pick_font(MONO_FONT_CANDIDATES, &[]), "JetBrains Mono");
    }

    fn built_ins() -> Vec<Theme> {
        built_in_themes()
    }

    fn built_in(id: &str) -> Theme {
        built_in_themes()
            .into_iter()
            .find(|theme| theme.id == id)
            .expect("tema embutido") // ids fixos do catálogo
    }

    fn luminance(color: Hsla) -> f32 {
        let rgba = gpui::hsla_to_rgba(color);
        let channel = |v: f32| {
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(rgba.color.red)
            + 0.7152 * channel(rgba.color.green)
            + 0.0722 * channel(rgba.color.blue)
    }

    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Perceptual distance (CIE76 in Lab).
    fn delta_e(a: Hsla, b: Hsla) -> f32 {
        fn lab(color: Hsla) -> (f32, f32, f32) {
            let rgba = gpui::hsla_to_rgba(color);
            let f = |v: f32| {
                if v > 0.04045 {
                    ((v + 0.055) / 1.055).powf(2.4)
                } else {
                    v / 12.92
                }
            };
            let (r, g, bl) = (f(rgba.color.red), f(rgba.color.green), f(rgba.color.blue));
            let x = 0.4124 * r + 0.3576 * g + 0.1805 * bl;
            let y = 0.2126 * r + 0.7152 * g + 0.0722 * bl;
            let z = 0.0193 * r + 0.1192 * g + 0.9505 * bl;
            let curve = |v: f32| {
                if v > 0.008856 {
                    v.cbrt()
                } else {
                    7.787 * v + 16.0 / 116.0
                }
            };
            let (fx, fy, fz) = (curve(x / 0.95047), curve(y), curve(z / 1.08883));
            (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
        }
        let (l1, a1, b1) = lab(a);
        let (l2, a2, b2) = lab(b);
        ((l1 - l2).powi(2) + (a1 - a2).powi(2) + (b1 - b2).powi(2)).sqrt()
    }

    #[test]
    fn delta_e_anchors_are_sane() {
        assert!(delta_e(c(0x000000ff), c(0xffffffff)) > 99.);
        let text = built_in("dish-dark").text;
        assert!(delta_e(text, text) < 0.01);
    }

    /// Todo texto normal precisa de 4,5:1 nas superfícies em que aparece.
    #[test]
    fn text_meets_contrast_requirement() {
        for theme in built_ins() {
            for surface in [theme.bg, theme.surface, theme.surface_2, theme.inset] {
                for (name, color) in [
                    ("text", theme.text),
                    ("dim", theme.dim),
                    ("faint", theme.faint),
                ] {
                    let ratio = contrast(color, surface);
                    assert!(ratio >= 4.5, "{} {name}: {ratio:.2}:1", theme.name);
                }
            }
        }
    }

    /// O critério "`max` não deve parecer uma falha", verificado cor a cor.
    ///
    /// Estas três *são* o sinal: se a cor se confundir com um nível de esforço,
    /// o estado deixa de se ler. O app antes desta mudança reprovava aqui — a
    /// cor de erro antiga (#e2655f) ficava a ΔE 16,9 do coral do `max`.
    #[test]
    fn git_diff_colors_are_readable_and_distinguishable() {
        for theme in built_ins() {
            for color in [theme.git_added, theme.git_removed] {
                let ratio = contrast(color, theme.bg);
                assert!(ratio >= 4.5, "{} git: {ratio:.2}:1", theme.name);
            }
            assert!(
                delta_e(theme.git_added, theme.git_removed) >= 20.,
                "{} git ΔE {:.1}",
                theme.name,
                delta_e(theme.git_added, theme.git_removed)
            );
        }
    }

    /// Desde o redesign editorial o esforço não pinta a interface (vira
    /// texto no composer), então o que precisa de separação são os estados
    /// entre si. `running` é o próprio acento: "o agente está vivo".
    #[test]
    fn status_colors_are_distinct_from_each_other() {
        for theme in built_ins() {
            let states = [
                ("running", theme.running),
                ("failure", theme.failure),
                ("warn", theme.warn),
                ("attention", theme.attention),
            ];
            for (index, (name, color)) in states.iter().enumerate() {
                for (other, other_color) in &states[index + 1..] {
                    let distance = delta_e(*color, *other_color);
                    assert!(
                        distance > 25.0,
                        "{} {name} vs {other}: ΔE {distance:.1} (mínimo 25)",
                        theme.name
                    );
                }
            }
            assert_eq!(theme.running, theme.brand, "{}: running acompanha o acento", theme.name);
        }
    }

    /// Sucesso é neutro de propósito (decisão de produto): o significado vem do
    /// ícone e do texto, não da cor. Ele só não pode encostar no neutro do
    /// esforço `off`, senão vira mingau.
    #[test]
    fn success_neutral_stays_off_the_neutral_effort() {
        for theme in built_ins() {
            for (level, effort) in [("off", theme.effort[0]), ("minimal", theme.effort[1])] {
                let distance = delta_e(theme.ok, effort);
                assert!(
                    distance > 15.0,
                    "{} ok vs effort {level}: ΔE {distance:.1}",
                    theme.name
                );
            }
        }
    }

    /// A cor de resposta nova é a mais saliente da lista: precisa ficar longe
    /// dos outros estados e legível sobre as superfícies em que aparece.
    #[test]
    fn attention_reads_as_new_content_not_as_running_or_warning() {
        for theme in built_ins() {
            // `ok` fica de fora: resposta nova é a tinta mais forte com peso e
            // ponto cheio; concluída é o ícone de check. A forma separa os dois.
            for (name, color) in [
                ("running", theme.running),
                ("failure", theme.failure),
                ("warn", theme.warn),
            ] {
                let distance = delta_e(theme.attention, color);
                assert!(
                    distance > 25.0,
                    "{} attention vs {name}: ΔE {distance:.1}",
                    theme.name
                );
            }
            for surface in [theme.bg, theme.surface, theme.surface_2, theme.hover] {
                let ratio = contrast(theme.attention, surface);
                assert!(ratio >= 4.5, "{} attention: {ratio:.2}:1", theme.name);
            }
        }
    }

    #[test]
    fn effort_colors_match_pi_dark_theme() {
        let dark = dark_theme();
        for (index, level) in LEVELS.iter().enumerate() {
            let actual = dark.effort[index].color;
            let hex = [
                0x6c767bff, 0x68808dff, 0x5489a4ff, 0x6185ccff, 0x9776e5ff, 0xde54c1ff, 0xfe5462ff,
            ][index];
            let expected = c(hex).color;
            assert!(
                (actual.hue.into_positive_degrees() - expected.hue.into_positive_degrees()).abs()
                    < 0.001,
                "{level}"
            );
            assert!((actual.saturation - expected.saturation).abs() < 0.00001, "{level}");
            assert!((actual.lightness - expected.lightness).abs() < 0.00001, "{level}");
        }
        // Nível desconhecido cai no `off`, como no Pi.
        assert_eq!(effort_index("desconhecido"), effort_index("off"));
    }

    /// A rampa clara é a mesma progressão do Pi com a luminosidade baixada: os
    /// matizes seguem o tema escuro e cada nível lê sobre o branco.
    #[test]
    fn light_effort_ramp_keeps_the_dark_hues_and_stays_readable() {
        let dark = built_in("dish-dark");
        let light = built_in("dish-light");
        for (index, level) in LEVELS.iter().enumerate() {
            let a = dark.effort[index].color.hue.into_positive_degrees();
            let b = light.effort[index].color.hue.into_positive_degrees();
            let difference = (a - b).abs().min(360.0 - (a - b).abs());
            assert!(difference < 2.0, "{level}: hue {a:.1} vs {b:.1}");
            let ratio = contrast(light.effort[index], light.surface_2);
            assert!(ratio >= 4.5, "{level} clear: {ratio:.2}:1");
        }
    }

    /// A preferência é serializada em minúsculas e o padrão é seguir o desktop.
    #[test]
    fn appearance_mode_round_trips_through_json() {
        assert_eq!(AppearanceMode::default(), AppearanceMode::System);
        assert_eq!(serde_json::to_string(&AppearanceMode::Dark).unwrap(), "\"dark\"");
        assert_eq!(
            serde_json::from_str::<AppearanceMode>("\"light\"").unwrap(),
            AppearanceMode::Light
        );
    }

    /// `Dark` e `Light` mandam no desktop; `System` obedece (e `Vibrant*`
    /// conta como a sua variante).
    #[test]
    fn appearance_mode_decides_the_palette() {
        use gpui::WindowAppearance::*;
        assert!(prefers_dark(AppearanceMode::Dark, Light));
        assert!(!prefers_dark(AppearanceMode::Light, Dark));
        assert!(prefers_dark(AppearanceMode::System, Dark));
        assert!(prefers_dark(AppearanceMode::System, VibrantDark));
        assert!(!prefers_dark(AppearanceMode::System, Light));
        assert!(!prefers_dark(AppearanceMode::System, VibrantLight));
    }

    fn theme_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dish-themes-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap(); // raiz própria do teste
        dir
    }

    /// Um arquivo completo vira tema com id próprio e as cores pedidas.
    #[test]
    fn user_theme_file_loads_with_its_colors() {
        let dir = theme_dir("completo");
        let path = dir.join("meu-tema.json");
        std::fs::write(
            &path,
            r##"{
                "$schema": "https://dish.dev/schemas/theme-v1.json",
                "name": "Meu tema",
                "appearance": "light",
                "colors": { "bg": "#101010", "surface2": "#202020", "lineSoft": "#303030",
                            "lineStrong": "#404040", "onAccent": "#ffffff", "attention": "#118844" }
            }"##,
        )
        .unwrap();
        let theme = load_theme(&path).unwrap();
        assert_eq!(theme.id, "user:meu-tema");
        assert_eq!(theme.name, "Meu tema");
        assert!(!theme.dark);
        assert!(theme.user);
        assert_eq!(theme.bg, c(0x101010ff));
        assert_eq!(theme.surface_2, c(0x202020ff));
        assert_eq!(theme.line_soft, c(0x303030ff));
        assert_eq!(theme.attention, c(0x118844ff));
        // O que falta vem do base claro, não do tema ativo.
        assert_eq!(theme.text, light_theme().text);
        assert_eq!(theme.effort, light_theme().effort);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Arquivo sem nome usa o nome do arquivo; sem aparência é escuro.
    #[test]
    fn user_theme_defaults_name_and_appearance() {
        let dir = theme_dir("minimo");
        let path = dir.join("so-um.json");
        std::fs::write(&path, r##"{ "colors": { "bg": "#010203" } }"##).unwrap();
        let theme = load_theme(&path).unwrap();
        assert_eq!(theme.id, "user:so-um");
        assert_eq!(theme.name, "so-um");
        assert!(theme.dark);
        assert_eq!(theme.bg, c(0x010203ff));
        assert_eq!(theme.text, dark_theme().text);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Arquivo malformado nomeia o arquivo e o problema, e nunca derruba nada.
    #[test]
    fn malformed_user_themes_report_the_file() {
        let dir = theme_dir("erros");
        let invalid_json = dir.join("quebrado.json");
        std::fs::write(&invalid_json, "{ not json }").unwrap();
        let error = load_theme(&invalid_json).unwrap_err();
        assert!(error.contains("quebrado.json"), "{error}");
        assert!(error.contains("JSON"), "{error}");

        let bad_color = dir.join("cor-ruim.json");
        std::fs::write(&bad_color, r#"{ "colors": { "bg": "roxo" } }"#).unwrap();
        let error = load_theme(&bad_color).unwrap_err();
        assert!(error.contains("cor-ruim.json"), "{error}");
        assert!(error.contains("bg"), "{error}");
        assert!(error.contains("#rrggbb"), "{error}");

        let bad_appearance = dir.join("meio.json");
        std::fs::write(&bad_appearance, r#"{ "appearance": "meio" }"#).unwrap();
        let error = load_theme(&bad_appearance).unwrap_err();
        assert!(error.contains("meio.json") && error.contains("dark"), "{error}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hex_parser_accepts_six_or_eight_digits_only() {
        assert_eq!(parse_hex("#0b0e13"), Some(c(0x0b0e13ff)));
        assert_eq!(parse_hex("0b0e13aa"), Some(c(0x0b0e13aa)));
        assert_eq!(parse_hex("#abc"), None);
        assert_eq!(parse_hex("#gggggg"), None);
        assert_eq!(parse_hex(""), None);
    }

    /// A varredura do diretório ordena por arquivo, ignora o que não é JSON e
    /// devolve os erros sem impedir os temas válidos de entrar no catálogo.
    #[test]
    fn directory_scan_orders_filters_and_reports() {
        let dir = theme_dir("varredura");
        std::fs::write(dir.join("zulu.json"), r#"{ "name": "Zulu" }"#).unwrap();
        std::fs::write(dir.join("alfa.json"), r#"{ "name": "Alfa" }"#).unwrap();
        std::fs::write(dir.join("notas.txt"), "não é tema").unwrap();
        std::fs::write(dir.join("quebrado.json"), "{ nope }").unwrap();
        let (list, errors) = build_themes_from(&dir);
        assert_eq!(list.len(), built_in_themes().len() + 2, "só os .json válidos");
        assert_eq!(list[built_in_themes().len()].name, "Alfa", "ordem por arquivo");
        assert_eq!(list[built_in_themes().len() + 1].name, "Zulu");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("quebrado.json"), "{}", errors[0]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Nenhum tema do catálogo divide id ou nome, e os embutidos vêm primeiro.
    #[test]
    fn built_in_catalog_is_unique_and_ordered() {
        let themes = built_in_themes();
        let ids: Vec<&str> = themes.iter().map(|theme| theme.id.as_str()).collect();
        assert_eq!(ids, vec!["dish-dark", "dish-slate", "dish-light", "dish-mist"]);
        // Escuros primeiro, claros depois: a lista se agrupa sem reordenar.
        let first_light = themes.iter().position(|theme| !theme.dark).unwrap();
        assert!(themes[..first_light].iter().all(|theme| theme.dark));
        assert!(themes[first_light..].iter().all(|theme| !theme.dark));
        assert_eq!(first_light, 2);
        assert_eq!(themes.len() - first_light, 2);
        for (index, theme) in themes.iter().enumerate() {
            assert!(!theme.user);
            assert!(!theme.name.trim().is_empty());
            assert!(
                !themes[..index].iter().any(|other| other.id == theme.id),
                "id repetida: {}",
                theme.id
            );
            assert!(
                !themes[..index].iter().any(|other| other.name == theme.name),
                "nome repetido: {}",
                theme.name
            );
        }
    }
}
