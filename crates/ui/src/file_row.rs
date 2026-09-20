//! One changed-file row (status letter, path, +/- counts); the caller adds the click handler.
use gpui::{div, prelude::*, px};
use madi_git::diff::FileDiff;

use crate::theme::*;

pub fn file_row(id: impl Into<gpui::ElementId>, f: &FileDiff, selected: bool) -> gpui::Stateful<gpui::Div> {
    let (letter, color) = match f.status.as_str() {
        "added" => ("A", GREEN()),
        "deleted" => ("D", RED()),
        _ => ("M", AMBER()),
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .when(selected, |d| d.bg(SELECTED()))
        .hover(|d| d.bg(SELECTED()))
        .child(div().w(px(12.)).flex_none().text_color(color).child(letter))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(TEXT())
                .child(f.path.clone()),
        )
        .child(div().text_xs().text_color(ADD_FG()).child(format!("+{}", f.additions)))
        .child(div().text_xs().text_color(DEL_FG()).child(format!("-{}", f.deletions)))
}
