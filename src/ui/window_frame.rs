//! Moldura client-side para plataformas que não desenham a janela.
//!
//! No GNOME/Wayland o Mutter não oferece decoração server-side, então a janela
//! nasce sem bordas e sem alça de redimensionamento. Onde o compositor delega
//! a moldura ao aplicativo, esta camada fica atrás do conteúdo e reserva as
//! bordas (via `BORDER`) para o redimensionamento; o arrasto fica na região
//! livre da barra de título, separada dos controles e do conteúdo.
//!
//! X11 e macOS mantêm as decorações nativas (`Decorations::Server`). O backend
//! do Windows também reporta `Server`, mas cria um HWND sem `WS_CAPTION` e não
//! desenha barra nenhuma; até o `gpui_ce_windows` corrigir isso, o Dish assume
//! a moldura lá também.

use gpui::prelude::*;
use gpui::*;

/// Espessura da borda invisível de redimensionamento.
pub const BORDER: f32 = 8.;

/// O compositor deixou a moldura por conta do aplicativo?
///
/// O backend do Windows cria a janela sem legenda mas continua reportando
/// `Decorations::Server`. Enquanto isso não muda no `gpui_ce_windows`, a
/// moldura do app é a única barra de título que existe lá. Quando o backend
/// passar a desenhar (ou reportar corretamente) a legenda nativa, basta
/// desligar `WINDOWS_DRAWS_NO_NATIVE_TITLEBAR`.
const WINDOWS_DRAWS_NO_NATIVE_TITLEBAR: bool = cfg!(target_os = "windows");

pub fn client_side(window: &Window) -> bool {
    needs_client_frame(window.window_decorations(), WINDOWS_DRAWS_NO_NATIVE_TITLEBAR)
}

fn needs_client_frame(decorations: Decorations, windows_without_native_bar: bool) -> bool {
    matches!(decorations, Decorations::Client { .. }) || windows_without_native_bar
}

/// Camada atrás do conteúdo. `None` quando a plataforma desenha a moldura.
pub fn layer(window: &Window) -> Option<Div> {
    if !client_side(window) {
        return None;
    }
    let border = px(BORDER);
    let handle = |cursor: CursorStyle, edge: ResizeEdge| {
        div()
            .absolute()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, move |_event, window, _cx| {
                window.start_window_resize(edge);
            })
    };

    Some(
        div()
            .absolute()
            .size_full()
            // Bordas.
            .child(
                handle(CursorStyle::ResizeUpDown, ResizeEdge::Top)
                    .top(px(0.))
                    .left(border)
                    .right(border)
                    .h(border),
            )
            .child(
                handle(CursorStyle::ResizeUpDown, ResizeEdge::Bottom)
                    .bottom(px(0.))
                    .left(border)
                    .right(border)
                    .h(border),
            )
            .child(
                handle(CursorStyle::ResizeLeftRight, ResizeEdge::Left)
                    .left(px(0.))
                    .top(border)
                    .bottom(border)
                    .w(border),
            )
            .child(
                handle(CursorStyle::ResizeLeftRight, ResizeEdge::Right)
                    .right(px(0.))
                    .top(border)
                    .bottom(border)
                    .w(border),
            )
            // Cantos.
            .child(
                handle(CursorStyle::ResizeUpLeftDownRight, ResizeEdge::TopLeft)
                    .top(px(0.))
                    .left(px(0.))
                    .size(border),
            )
            .child(
                handle(CursorStyle::ResizeUpRightDownLeft, ResizeEdge::TopRight)
                    .top(px(0.))
                    .right(px(0.))
                    .size(border),
            )
            .child(
                handle(CursorStyle::ResizeUpRightDownLeft, ResizeEdge::BottomLeft)
                    .bottom(px(0.))
                    .left(px(0.))
                    .size(border),
            )
            .child(
                handle(CursorStyle::ResizeUpLeftDownRight, ResizeEdge::BottomRight)
                    .bottom(px(0.))
                    .right(px(0.))
                    .size(border),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::needs_client_frame;
    use gpui::{Decorations, Tiling};

    #[test]
    fn client_frame_follows_decorations_and_windows_workaround() {
        let client = Decorations::Client {
            tiling: Tiling::default(),
        };
        assert!(needs_client_frame(client, false));
        assert!(needs_client_frame(client, true));
        assert!(!needs_client_frame(Decorations::Server, false));
        assert!(needs_client_frame(Decorations::Server, true));
    }
}
