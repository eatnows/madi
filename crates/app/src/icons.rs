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

/// An eye: the Markdown preview toggle.
pub fn eye<S>(color: Color) -> Element<S> {
    div()
        .size(14.)
        .items_center()
        .justify_center()
        .child(div().w(14.).h(9.).rounded(5.).border(1.4, color).items_center().justify_center().child(div().size(4.).rounded(2.).bg(color)))
}

/// Three adjustable bars with their knobs: the settings icon.
pub fn sliders<S>(color: Color) -> Element<S> {
    let line = |left: bool| {
        div()
            .w(14.)
            .h(4.)
            .items_center()
            .child(div().w(14.).h(1.).bg(color))
            .child(div().absolute().left(if left { 3. } else { 9. }).size(4.).rounded(2.).bg(color))
    };
    div().size(14.).justify_between().child(line(true)).child(line(false)).child(line(true))
}
