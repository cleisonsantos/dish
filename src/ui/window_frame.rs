//! Moldura client-side para plataformas que não desenham a janela.
//!
//! No GNOME/Wayland o Mutter não oferece decoração server-side, então a janela
//! nasce sem bordas e sem alça de redimensionamento. Onde o compositor delega
//! a moldura ao aplicativo, esta camada fica atrás do conteúdo e reserva as
//! bordas (via `BORDER`) para o redimensionamento; o arrasto fica na região
//! livre da barra de título, separada dos controles e do conteúdo.
//!
//! X11 e macOS mantêm as decorações nativas (`Decorations::Server`). No Windows
//! o `gpui_ce_windows` reporta `Server` mesmo quando cria o HWND sem
//! `WS_CAPTION`, então o relatório não serve como prova: a legenda nativa é
//! detectada em tempo de execução, lendo o estilo real da janela.

use gpui::prelude::*;
use gpui::*;

/// Espessura da borda invisível de redimensionamento.
pub const BORDER: f32 = 8.;

/// O sistema desenha a barra de título desta janela?
///
/// `Some(true)`: o HWND tem `WS_CAPTION`; `Some(false)`: não tem (ou não deu
/// para ler o handle — nesse caso mantemos o comportamento antigo e o app
/// desenha a barra). `None`: fora do Windows não há sonda; o relatório de
/// decorações da plataforma é a fonte da verdade.
#[cfg(target_os = "windows")]
fn runtime_caption(window: &Window) -> Option<bool> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongW, GWL_STYLE, WS_CAPTION};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        // Sem HWND não há prova da legenda; mantém o comportamento antigo.
        return Some(false);
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return Some(false);
    };
    let hwnd = HWND(win32.hwnd.get() as *mut core::ffi::c_void);
    // `WS_CAPTION` é `WS_BORDER | WS_DLGFRAME`; exigir os dois bits evita
    // tratar uma janela só com borda como se tivesse legenda.
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
    Some(style & WS_CAPTION.0 == WS_CAPTION.0)
}

#[cfg(not(target_os = "windows"))]
fn runtime_caption(_window: &Window) -> Option<bool> {
    None
}

/// Decisão pura, sem janela: a moldura pertence ao aplicativo?
///
/// No Windows o estilo real do HWND manda, porque o backend pode reportar
/// `Server` e ainda assim criar (ou não) a legenda. Nas outras plataformas só
/// o relatório de decorações importa: `Client` é o compositor delegando a
/// moldura ao app.
pub(crate) fn needs_client_frame(decorations: Decorations, runtime_caption: Option<bool>) -> bool {
    match runtime_caption {
        Some(has_native_caption) => !has_native_caption,
        None => matches!(decorations, Decorations::Client { .. }),
    }
}

pub fn client_side(window: &Window) -> bool {
    needs_client_frame(window.window_decorations(), runtime_caption(window))
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
    fn client_frame_follows_decorations_and_native_caption() {
        let client = Decorations::Client {
            tiling: Tiling::default(),
        };

        // Linux/macOS: sem sonda de HWND, o relatório de decorações decide.
        assert!(needs_client_frame(client, None));
        assert!(!needs_client_frame(Decorations::Server, None));

        // Windows sem `WS_CAPTION`: nenhuma barra nativa, o app desenha.
        assert!(needs_client_frame(Decorations::Server, Some(false)));
        assert!(needs_client_frame(client, Some(false)));

        // Windows com `WS_CAPTION`: a barra é do sistema, nunca duplicar.
        assert!(!needs_client_frame(Decorations::Server, Some(true)));
        assert!(!needs_client_frame(client, Some(true)));
    }
}
