//! Small icons drawn from plain elements, so they follow the theme and need no font.
use gyeol::{div, Align, Color, Element};

fn bar<S>(w: f32, h: f32, color: Color) -> Element<S> {
    div().w(w).h(h).bg(color)
}

/// One corner bracket of the focus icon: a horizontal and a vertical bar meeting at the corner.
fn corner<S>(color: Color, top: bool, left: bool) -> Element<S> {
    let side = if left { Align::Start } else { Align::End };
    let across = bar(6., 1., color);
    let down = bar(1., 5., color).align_self(side);
    let mut corner = div().w(6.).h(6.);
    corner = if top { corner.child(across).child(down) } else { corner.child(down).child(across) };
    corner
}

/// Four corner brackets: the "focus" icon.
pub fn focus<S>(color: Color) -> Element<S> {
    div()
        .size(14.)
        .justify_between()
        .child(div().row().justify_between().child(corner(color, true, true)).child(corner(color, true, false)))
        .child(div().row().justify_between().child(corner(color, false, true)).child(corner(color, false, false)))
}
