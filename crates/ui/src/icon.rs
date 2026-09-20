//! Tiny icons drawn from plain divs, so they follow the theme and never depend on a glyph font.
use gpui::{div, prelude::*, px, Rgba};

/// Four corner brackets: the "focus / expand" icon.
pub fn focus(color: Rgba) -> impl IntoElement {
    let corner = || div().absolute().size(px(5.)).border_color(color);
    div()
        .relative()
        .size(px(14.))
        .child(corner().top_0().left_0().border_t_1().border_l_1())
        .child(corner().top_0().right_0().border_t_1().border_r_1())
        .child(corner().bottom_0().left_0().border_b_1().border_l_1())
        .child(corner().bottom_0().right_0().border_b_1().border_r_1())
}

/// Three sliders: the "settings" icon.
pub fn sliders(color: Rgba) -> impl IntoElement {
    let row = |knob_at: f32| {
        div()
            .relative()
            .w(px(14.))
            .h(px(4.))
            .child(div().absolute().top(px(1.5)).w_full().h(px(1.)).bg(color))
            .child(div().absolute().left(px(knob_at)).size(px(4.)).rounded_full().bg(color))
    };
    div().flex().flex_col().justify_between().w(px(14.)).h(px(14.)).child(row(2.)).child(row(8.)).child(row(4.))
}
