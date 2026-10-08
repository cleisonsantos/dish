//! Visual language for Dish.
//!
//! Three rules hold the design together:
//!
//! 1. **Graphite with separation.** Surfaces step up from the conversation to
//!    the panels; nothing is outlined just to be outlined.
//! 2. **The accent is the effort meter.** Pi's own dark-theme effort palette
//!    drives the accent, so the window looks different when the model is
//!    thinking hard. It stays concentrated on the effort control, focus and
//!    small indicators.
//! 3. **Effort colours are not status colours.** Execution, success and failure
//!    live on their own hues so that `max` never reads as an error — enforced by
//!    the tests at the bottom of this file, not by opinion.

use gpui::{BoxShadow, Hsla, Pixels, point, px, rgb_to_hsla, rgba};
use std::sync::atomic::{AtomicU32, Ordering};

/// Interface font.
pub const FONT_UI: &str = "Roboto";
/// Monospace font, used for anything a machine wrote.
pub const FONT_MONO: &str = "JetBrains Mono";

// ----------------------------------------------------------------- tipografia

/// Reading text: the assistant's answer and the user's request.
pub const PROSE: f32 = 15.0;
/// Session title.
pub const TITLE: f32 = 16.5;
/// Default interface text.
pub const TEXT: f32 = 13.0;
/// Secondary interface text: rows, values.
pub const TEXT_SM: f32 = 12.0;
/// Metadata: paths, durations, counts.
pub const TEXT_XS: f32 = 11.5;
/// Micro labels, used sparingly.
pub const TEXT_MICRO: f32 = 11.0;
/// Technical values inside monospace blocks.
pub const MONO: f32 = 12.5;

pub fn body_line_height() -> Pixels {
    px(18.)
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
/// Radius for surfaces: composer, code, panels.
pub fn r_surface() -> Pixels {
    px(12.)
}

#[inline]
fn c(hex: u32) -> Hsla {
    rgb_to_hsla(rgba(hex))
}

// ---------------------------------------------------------------- superfícies

/// The window itself, behind the conversation.
pub fn bg() -> Hsla {
    c(0x0c0e12ff)
}
/// Panels: navigation, inspector.
pub fn surface() -> Hsla {
    c(0x101318ff)
}
/// Raised surfaces: composer, the user's request, the active row.
pub fn surface_2() -> Hsla {
    c(0x171b21ff)
}
/// Recessed surfaces: code, tool output, search fields.
pub fn inset() -> Hsla {
    c(0x08090dff)
}
pub fn hover() -> Hsla {
    c(0x1b1f26ff)
}

pub fn line() -> Hsla {
    c(0x22262eff)
}
pub fn line_soft() -> Hsla {
    c(0x191d23ff)
}
pub fn line_strong() -> Hsla {
    c(0x2e343dff)
}

// ------------------------------------------------------------------- texto

pub fn text() -> Hsla {
    c(0xe9ebeeff)
}
pub fn dim() -> Hsla {
    c(0xa6aeb8ff)
}
pub fn faint() -> Hsla {
    c(0x858d97ff)
}
pub fn on_accent() -> Hsla {
    c(0x0c0e12ff)
}
/// Text over a light control (the send button).
pub fn on_light() -> Hsla {
    c(0x0c0e12ff)
}

// --------------------------------------------------------------- semântica
//
// Fora da rampa de effort, que vai do cinza-azulado ao coral passando pelo
// magenta. Estas três ficam a ΔE ≥ 28 de todos os níveis (ver testes).

/// Executando. Verde-água.
pub fn running() -> Hsla {
    c(0x4fb3a5ff)
}
/// Falha. Magenta-carmim, longe do coral do `max`.
pub fn failure() -> Hsla {
    c(0xcf5f92ff)
}
/// Aviso de recurso (contexto cheio), o âmbar que o Pi usa para isso.
pub fn warn() -> Hsla {
    c(0xd8a24aff)
}
/// Sucesso: neutro de propósito — a marca é o ícone e o texto, não a cor.
pub fn ok() -> Hsla {
    c(0xa6aeb8ff)
}
/// Mantido para os pontos que ainda pedem a cor de perigo antiga.
pub fn danger() -> Hsla {
    failure()
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
// converted from OKHSL with pi-tui's parseColor/colorToHex.

/// Hue, saturation and lightness of the current accent, packed as f32 bits.
static ACCENT_H: AtomicU32 = AtomicU32::new(f32::to_bits(0.60));
static ACCENT_S: AtomicU32 = AtomicU32::new(f32::to_bits(0.18));
static ACCENT_L: AtomicU32 = AtomicU32::new(f32::to_bits(0.66));

fn store(h: f32, s: f32, l: f32) {
    ACCENT_H.store(h.to_bits(), Ordering::Relaxed);
    ACCENT_S.store(s.to_bits(), Ordering::Relaxed);
    ACCENT_L.store(l.to_bits(), Ordering::Relaxed);
}

fn load(cell: &AtomicU32) -> f32 {
    f32::from_bits(cell.load(Ordering::Relaxed))
}

/// The accent in use right now.
pub fn accent() -> Hsla {
    hsla_f(load(&ACCENT_H), load(&ACCENT_S), load(&ACCENT_L), 1.0)
}

/// The accent lifted for hover states.
pub fn accent_hover() -> Hsla {
    let accent = accent();
    let mut hsl = accent.color;
    hsl.lightness = (hsl.lightness + 0.10).min(0.92);
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


fn hsla_f(h: f32, s: f32, l: f32, a: f32) -> Hsla {
    gpui::hsla(h, s, l, a)
}

/// The colour of a single reasoning level, so the selector can show the ramp.
pub fn effort_color(level: &str, available: &[String]) -> Hsla {
    let (h, s, l) = effort_ramp(level, available);
    hsla_f(h, s, l, 1.0)
}

/// Point the accent at the current reasoning level.
pub fn set_effort(level: Option<&str>, available: &[String]) {
    let (h, s, l) = match level {
        Some(level) => effort_ramp(level, available),
        None => neutral(),
    };
    store(h, s, l);
}

/// No reasoning control: a cool, quiet slate.
fn neutral() -> (f32, f32, f32) {
    effort_ramp("off", &[])
}

/// Map a level to hue/saturation/lightness.
fn effort_ramp(level: &str, _available: &[String]) -> (f32, f32, f32) {
    let hex = match level {
        "minimal" => 0x68808dff,
        "low" => 0x5489a4ff,
        "medium" => 0x6185ccff,
        "high" => 0x9776e5ff,
        "xhigh" => 0xde54c1ff,
        "max" => 0xfe5462ff,
        // Pi also falls back to thinkingOff for unknown levels.
        _ => 0x6c767bff,
    };
    let color = c(hex).color;
    (
        color.hue.into_positive_degrees() / 360.0,
        color.saturation,
        color.lightness,
    )
}

/// The only shadow in the app: things that float above it.
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
        assert!(delta_e(text(), text()) < 0.01);
    }

    /// Todo texto normal precisa de 4,5:1 nas superfícies em que aparece.
    #[test]
    fn text_meets_contrast_requirement() {
        for surface in [bg(), surface(), surface_2(), inset()] {
            for (name, color) in [("text", text()), ("dim", dim()), ("faint", faint())] {
                let ratio = contrast(color, surface);
                assert!(ratio >= 4.5, "{name}: {ratio:.2}:1");
            }
        }
    }

    /// O critério "`max` não deve parecer uma falha", verificado cor a cor.
    ///
    /// Estas três *são* o sinal: se a cor se confundir com um nível de esforço,
    /// o estado deixa de se ler. O app antes desta mudança reprovava aqui — a
    /// cor de erro antiga (#e2655f) ficava a ΔE 16,9 do coral do `max`.
    #[test]
    fn status_colors_are_distinct_from_the_effort_ramp() {
        let levels = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
        for (name, semantic) in [
            ("running", running()),
            ("failure", failure()),
            ("warn", warn()),
        ] {
            for level in levels {
                let effort = effort_color(level, &[]);
                let distance = delta_e(semantic, effort);
                assert!(
                    distance > 25.0,
                    "{name} vs effort {level}: ΔE {distance:.1} (mínimo 25)"
                );
            }
        }
    }

    /// Sucesso é neutro de propósito (decisão de produto): o significado vem do
    /// ícone e do texto, não da cor. Ele só não pode encostar no neutro do
    /// esforço `off`, senão vira mingau.
    #[test]
    fn success_neutral_stays_off_the_neutral_effort() {
        for level in ["off", "minimal"] {
            let distance = delta_e(ok(), effort_color(level, &[]));
            assert!(distance > 15.0, "ok vs effort {level}: ΔE {distance:.1}");
        }
    }

    #[test]
    fn effort_colors_match_pi_dark_theme() {
        for (level, hex) in [
            ("off", 0x6c767bff),
            ("minimal", 0x68808dff),
            ("low", 0x5489a4ff),
            ("medium", 0x6185ccff),
            ("high", 0x9776e5ff),
            ("xhigh", 0xde54c1ff),
            ("max", 0xfe5462ff),
            ("unknown", 0x6c767bff),
        ] {
            let actual = effort_color(level, &[]).color;
            let expected = c(hex).color;
            assert!(
                (actual.hue.into_positive_degrees() - expected.hue.into_positive_degrees()).abs()
                    < 0.001,
                "{level}"
            );
            assert!((actual.saturation - expected.saturation).abs() < 0.00001, "{level}");
            assert!((actual.lightness - expected.lightness).abs() < 0.00001, "{level}");
        }
    }
}
