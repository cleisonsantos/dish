//! Moldura client-side para compositores que não desenham a janela.
//!
//! No GNOME/Wayland o Mutter não oferece decoração server-side, então a janela
//! nasce sem bordas e sem alça de redimensionamento. Onde o compositor delega
//! a moldura ao aplicativo, esta camada fica atrás do conteúdo e reserva as
//! bordas (via `BORDER`) para o redimensionamento; o arrasto fica na região
//! livre da barra de título, separada dos controles e do conteúdo.
//!
//! X11, macOS e Windows mantêm as decorações nativas (`Decorations::Server`);
//! nestes a camada não é construída.

use gpui::prelude::*;
use gpui::*;

/// Espessura da borda invisível de redimensionamento.
pub const BORDER: f32 = 8.;

/// O compositor deixou a moldura por conta do aplicativo?
pub fn client_side(window: &Window) -> bool {
    cfg!(target_os = "linux") && matches!(window.window_decorations(), Decorations::Client { .. })
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
