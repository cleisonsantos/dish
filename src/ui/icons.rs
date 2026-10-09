//! Ícones vetoriais da proposta — desenhados com `PathBuilder`, não glifos.
//!
//! Grade de 16×16, traço uniforme, cantos retos: a intenção é um conjunto
//! técnico e seco, que combine com uma bancada de trabalho, em vez das formas
//! arredondadas e "amigáveis" do repertório de terminal.
//!
//! Nada disto depende de fonte: se o glifo não existir no sistema, o ícone
//! continua idêntico.

use gpui::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    ChevronRight,
    ChevronDown,
    Plus,
    Search,
    Panel,
    Folder,
    Activity,
    Check,
    Alert,
    Stop,
    Send,
    Clock,
    File,
    Terminal,
    Dots,
    Minimize,
    Close,
    Trash,
}

/// Um ícone desenhado num quadrado de `size` px.
pub fn icon(icon: Icon, size: f32, color: Hsla) -> impl IntoElement {
    let stroke = (size * 0.095).clamp(1.0, 2.2);
    canvas(
        move |_bounds, _window, _cx| (),
        move |bounds, _state, window, _cx| {
            let scale = f32::from(bounds.size.width) / 16.0;
            if let Some(path) = build(icon, stroke, scale, bounds.origin) {
                window.paint_path(path, color);
            }
        },
    )
    .flex_none()
    .w(px(size))
    .h(px(size))
}

fn circle(b: &mut PathBuilder, cx: f32, cy: f32, r: f32, _filled: bool) {
    let to = |x: f32, y: f32| point(px(x), px(y));
    b.move_to(to(cx + r, cy));
    b.arc_to(point(px(r), px(r)), px(0.), false, true, to(cx, cy + r));
    b.arc_to(point(px(r), px(r)), px(0.), false, true, to(cx - r, cy));
    b.arc_to(point(px(r), px(r)), px(0.), false, true, to(cx, cy - r));
    b.arc_to(point(px(r), px(r)), px(0.), false, true, to(cx + r, cy));
    b.close();
}

fn rounded_rect(b: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, r: f32) {
    let to = |x: f32, y: f32| point(px(x), px(y));
    let arc = move |b: &mut PathBuilder, tx: f32, ty: f32| {
        b.arc_to(point(px(r), px(r)), px(0.), false, true, to(tx, ty));
    };
    b.move_to(to(x + r, y));
    b.line_to(to(x + w - r, y));
    arc(b, x + w, y + r);
    b.line_to(to(x + w, y + h - r));
    arc(b, x + w - r, y + h);
    b.line_to(to(x + r, y + h));
    arc(b, x, y + h - r);
    b.line_to(to(x, y + r));
    arc(b, x + r, y);
    b.close();
}

fn build(icon: Icon, stroke: f32, scale: f32, origin: Point<Pixels>) -> Option<Path<Pixels>> {
    let to = |x: f32, y: f32| point(px(x), px(y));
    let filled = matches!(icon, Icon::Stop | Icon::Dots);
    let mut b = if filled {
        PathBuilder::fill()
    } else {
        PathBuilder::stroke(px(stroke))
    };

    match icon {
        Icon::ChevronRight => {
            b.move_to(to(6., 3.5));
            b.line_to(to(11., 8.));
            b.line_to(to(6., 12.5));
        }
        Icon::ChevronDown => {
            b.move_to(to(3.5, 6.));
            b.line_to(to(8., 10.5));
            b.line_to(to(12.5, 6.));
        }
        Icon::Plus => {
            b.move_to(to(8., 3.5));
            b.line_to(to(8., 12.5));
            b.move_to(to(3.5, 8.));
            b.line_to(to(12.5, 8.));
        }
        Icon::Search => {
            circle(&mut b, 7., 7., 4.2, false);
            b.move_to(to(10.2, 10.2));
            b.line_to(to(13.5, 13.5));
        }
        Icon::Panel => {
            rounded_rect(&mut b, 2., 3., 12., 10., 1.5);
            b.move_to(to(10.2, 3.));
            b.line_to(to(10.2, 13.));
        }
        Icon::Folder => {
            b.move_to(to(2., 12.2));
            b.line_to(to(2., 4.8));
            b.arc_to(point(px(1.), px(1.)), px(0.), false, true, to(3., 3.8));
            b.line_to(to(6.2, 3.8));
            b.line_to(to(7.6, 5.8));
            b.line_to(to(13., 5.8));
            b.arc_to(point(px(1.), px(1.)), px(0.), false, true, to(14., 6.8));
            b.line_to(to(14., 12.2));
            b.arc_to(point(px(1.), px(1.)), px(0.), false, true, to(13., 13.2));
            b.line_to(to(3., 13.2));
            b.arc_to(point(px(1.), px(1.)), px(0.), false, true, to(2., 12.2));
            b.close();
        }
        Icon::Activity => {
            b.move_to(to(1.8, 8.5));
            b.line_to(to(4.6, 8.5));
            b.line_to(to(6.2, 4.));
            b.line_to(to(9.4, 12.4));
            b.line_to(to(11., 8.5));
            b.line_to(to(14.2, 8.5));
        }
        Icon::Check => {
            b.move_to(to(3.5, 8.4));
            b.line_to(to(6.6, 11.5));
            b.line_to(to(12.5, 4.5));
        }
        Icon::Alert => {
            b.move_to(to(8., 2.6));
            b.line_to(to(14.4, 13.2));
            b.line_to(to(1.6, 13.2));
            b.close();
            b.move_to(to(8., 6.6));
            b.line_to(to(8., 9.6));
            b.move_to(to(8., 11.5));
            b.line_to(to(8., 11.6));
        }
        Icon::Stop => {
            rounded_rect(&mut b, 4., 4., 8., 8., 1.);
        }
        Icon::Send => {
            b.move_to(to(3., 8.));
            b.line_to(to(12.4, 8.));
            b.move_to(to(8.6, 4.2));
            b.line_to(to(12.6, 8.));
            b.line_to(to(8.6, 11.8));
        }
        Icon::Clock => {
            circle(&mut b, 8., 8., 5.6, false);
            b.move_to(to(8., 4.8));
            b.line_to(to(8., 8.2));
            b.line_to(to(10.6, 9.6));
        }
        Icon::File => {
            b.move_to(to(4., 2.6));
            b.line_to(to(9.6, 2.6));
            b.line_to(to(12.4, 5.4));
            b.line_to(to(12.4, 13.4));
            b.line_to(to(4., 13.4));
            b.close();
            b.move_to(to(9.4, 2.6));
            b.line_to(to(9.4, 5.6));
            b.line_to(to(12.4, 5.6));
        }
        Icon::Terminal => {
            rounded_rect(&mut b, 2., 3.5, 12., 9., 1.5);
            b.move_to(to(5., 7.));
            b.line_to(to(7., 9.));
            b.line_to(to(5., 11.));
            b.move_to(to(8.8, 11.));
            b.line_to(to(11.6, 11.));
        }
        Icon::Dots => {
            circle(&mut b, 4., 8., 1.1, true);
            circle(&mut b, 8., 8., 1.1, true);
            circle(&mut b, 12., 8., 1.1, true);
        }
        Icon::Minimize => {
            b.move_to(to(3., 8.));
            b.line_to(to(13., 8.));
        }
        Icon::Close => {
            b.move_to(to(4.2, 4.2));
            b.line_to(to(11.8, 11.8));
            b.move_to(to(11.8, 4.2));
            b.line_to(to(4.2, 11.8));
        }
        Icon::Trash => {
            b.move_to(to(2.8, 4.4));
            b.line_to(to(13.2, 4.4));
            b.move_to(to(6.2, 4.4));
            b.line_to(to(6.2, 2.8));
            b.line_to(to(9.8, 2.8));
            b.line_to(to(9.8, 4.4));
            b.move_to(to(4.4, 4.4));
            b.line_to(to(5.2, 13.2));
            b.line_to(to(10.8, 13.2));
            b.line_to(to(11.6, 4.4));
            b.move_to(to(6.8, 6.8));
            b.line_to(to(7.0, 10.8));
            b.move_to(to(9.2, 6.8));
            b.line_to(to(9.0, 10.8));
        }
    }

    b.scale(scale);
    b.translate(origin);
    b.build().ok()
}
