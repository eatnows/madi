//! Side-by-side diff rows: pairing of deleted/inserted lines and how a row is drawn.
use std::ops::Range;

use gpui::{div, prelude::*, px, HighlightStyle, IntoElement, SharedString, StyledText};
use maditor_core::diff::DiffLine;

use crate::theme::*;

pub const ROW_H: f32 = 20.0;

pub struct Cell {
    lineno: Option<usize>,
    tag: &'static str,
    text: SharedString,
    emphasis: Vec<Range<usize>>,
}

pub enum Row {
    Gap,
    Pair(Option<Cell>, Option<Cell>),
}

fn cell_of(line: &DiffLine, new_side: bool) -> Cell {
    let mut text = String::new();
    let mut emphasis = Vec::new();
    for seg in &line.segments {
        let start = text.len();
        text.push_str(&seg.text);
        if seg.emphasized {
            emphasis.push(start..text.len());
        }
    }
    Cell {
        lineno: if new_side { line.new_lineno } else { line.old_lineno },
        tag: line.tag,
        text: text.into(),
        emphasis,
    }
}

/// Pairs delete/insert runs side by side so they render as aligned old|new columns.
pub fn build_rows(lines: &[DiffLine]) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        match line.tag {
            "gap" => {
                rows.push(Row::Gap);
                i += 1;
            }
            "equal" => {
                rows.push(Row::Pair(Some(cell_of(line, false)), Some(cell_of(line, true))));
                i += 1;
            }
            _ => {
                let mut deletes = Vec::new();
                while i < lines.len() && lines[i].tag == "delete" {
                    deletes.push(&lines[i]);
                    i += 1;
                }
                let mut inserts = Vec::new();
                while i < lines.len() && lines[i].tag == "insert" {
                    inserts.push(&lines[i]);
                    i += 1;
                }
                for k in 0..deletes.len().max(inserts.len()) {
                    rows.push(Row::Pair(
                        deletes.get(k).map(|l| cell_of(l, false)),
                        inserts.get(k).map(|l| cell_of(l, true)),
                    ));
                }
            }
        }
    }
    rows
}

fn render_cell(cell: &Option<Cell>) -> impl IntoElement {
    let (bg, fg, strong_bg, strong_fg) = match cell.as_ref().map(|c| c.tag) {
        Some("insert") => (Some(ADD_BG), ADD_FG, ADD_STRONG_BG, ADD_STRONG_FG),
        Some("delete") => (Some(DEL_BG), DEL_FG, DEL_STRONG_BG, DEL_STRONG_FG),
        _ => (None, TEXT, SELECTED, TEXT_STRONG),
    };
    let lineno = cell.as_ref().and_then(|c| c.lineno).map(|n| n.to_string()).unwrap_or_default();
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .h(px(ROW_H))
        .items_center()
        .overflow_hidden()
        .when_some(bg, |d, bg| d.bg(bg))
        .child(div().w(px(44.)).flex_none().pr_2().text_right().text_color(TEXT_DIM).child(lineno))
        .when_some(cell.as_ref(), |d, c| {
            let style = HighlightStyle {
                color: Some(strong_fg.into()),
                background_color: Some(strong_bg.into()),
                ..Default::default()
            };
            let highlights = c.emphasis.iter().map(move |r| (r.clone(), style));
            d.child(
                div()
                    .whitespace_nowrap()
                    .text_color(fg)
                    .child(StyledText::new(c.text.clone()).with_highlights(highlights)),
            )
        })
}

pub fn render_row(row: &Row) -> impl IntoElement {
    let base = div().flex().w_full().h(px(ROW_H));
    match row {
        Row::Gap => base
            .justify_center()
            .items_center()
            .bg(PANEL)
            .text_color(TEXT_DIMMER)
            .child("⋯ unchanged ⋯"),
        Row::Pair(left, right) => base.child(render_cell(left)).child(
            div().flex_1().min_w_0().border_l_1().border_color(BORDER).child(render_cell(right)),
        ),
    }
}
