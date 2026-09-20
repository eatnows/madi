//! Drawing a diff: side-by-side or unified rows, responsive to the room available.
use std::{ops::Range, rc::Rc};

use gpui::{
    div, prelude::*, px, uniform_list, AnyElement, ElementId, HighlightStyle, IntoElement, Rgba,
    ScrollHandle, SharedString, StyledText, UniformListScrollHandle,
};
use madi_git::diff_layout::{Cell, DiffLayout, Row, UnifiedRow};

use crate::{scroll::axis_locked, theme::*};

pub const ROW_H: f32 = 20.0;
/// Below this pane width each half of a side-by-side diff would be too cramped to read.
pub const SPLIT_MIN_WIDTH: f32 = 760.0;

const CHAR_W: f32 = 7.3;
const GUTTER_W: f32 = 44.0;

/// Pixel width the diff needs so its longest line isn't clipped (a monospace estimate).
pub fn width(layout: &DiffLayout, unified: bool) -> f32 {
    let text = layout.widest_line(unified) as f32 * CHAR_W;
    if unified { 2.0 * GUTTER_W + 24.0 + text + 24.0 } else { 2.0 * (GUTTER_W + text + 24.0) }
}

fn render_row(layout: &DiffLayout, unified: bool, ix: usize) -> AnyElement {
    if unified {
        render_unified_row(&layout.unified[ix]).into_any_element()
    } else {
        render_split_row(&layout.split[ix]).into_any_element()
    }
}

/// A file's diff, responsive to the room it has: side-by-side when there is space, otherwise a
/// single unified column (each half of a split would be unreadably narrow). Scrolls sideways for
/// lines longer than the pane, vertically through a virtualized list.
pub fn diff_view(
    id: impl Into<ElementId>,
    data: Rc<DiffLayout>,
    vscroll: &UniformListScrollHandle,
    hscroll: &ScrollHandle,
    available_width: f32,
) -> impl IntoElement {
    let id: ElementId = id.into();
    let unified = available_width < SPLIT_MIN_WIDTH;
    let (count, px_width) = (data.len(unified), width(&data, unified));
    axis_locked(div().id(id.clone()).flex_1().min_w_0().bg(BG()).overflow_x_scroll())
        .track_scroll(hscroll)
        .child(
            axis_locked(uniform_list(id, count, move |range: Range<usize>, _, _| {
                range.map(|ix| render_row(&data, unified, ix)).collect::<Vec<_>>()
            }))
            .track_scroll(vscroll.clone())
            .min_w(px(px_width))
            .h_full()
            .font_family(MONO)
            .text_size(px(12.)),
        )
}

fn colors_for(tag: Option<&str>) -> (Option<Rgba>, Rgba, Rgba, Rgba) {
    match tag {
        Some("insert") => (Some(ADD_BG()), ADD_FG(), ADD_STRONG_BG(), ADD_STRONG_FG()),
        Some("delete") => (Some(DEL_BG()), DEL_FG(), DEL_STRONG_BG(), DEL_STRONG_FG()),
        _ => (None, TEXT(), SELECTED(), TEXT_STRONG()),
    }
}

fn text_element(text: &std::sync::Arc<str>, emphasis: &[Range<usize>], fg: Rgba, strong_bg: Rgba, strong_fg: Rgba) -> impl IntoElement {
    let style = HighlightStyle {
        color: Some(strong_fg.into()),
        background_color: Some(strong_bg.into()),
        ..Default::default()
    };
    let highlights: Vec<_> = emphasis.iter().map(|r| (r.clone(), style)).collect();
    div()
        .whitespace_nowrap()
        .text_color(fg)
        .child(StyledText::new(SharedString::from(text.clone())).with_highlights(highlights))
}

fn gutter(n: Option<usize>) -> impl IntoElement {
    div()
        .w(px(GUTTER_W))
        .flex_none()
        .pr_2()
        .text_right()
        .text_color(TEXT_DIM())
        .child(n.map(|n| n.to_string()).unwrap_or_default())
}

fn render_cell(cell: &Option<Cell>) -> impl IntoElement {
    let (bg, fg, strong_bg, strong_fg) = colors_for(cell.as_ref().map(|c| c.tag));
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .h(px(ROW_H))
        .items_center()
        .overflow_hidden()
        .when_some(bg, |d, bg| d.bg(bg))
        .child(gutter(cell.as_ref().and_then(|c| c.lineno)))
        .when_some(cell.as_ref(), |d, c| d.child(text_element(&c.text, &c.emphasis, fg, strong_bg, strong_fg)))
}

fn render_split_row(row: &Row) -> impl IntoElement {
    let base = div().flex().w_full().h(px(ROW_H));
    match row {
        Row::Gap => base
            .justify_center()
            .items_center()
            .bg(PANEL())
            .text_color(TEXT_DIMMER())
            .child("⋯ unchanged ⋯"),
        Row::Pair(left, right) => base.child(render_cell(left)).child(
            div().flex_1().min_w_0().border_l_1().border_color(BORDER()).child(render_cell(right)),
        ),
    }
}

fn render_unified_row(row: &UnifiedRow) -> impl IntoElement {
    let base = div().flex().w_full().h(px(ROW_H)).items_center();
    match row {
        UnifiedRow::Gap => base
            .justify_center()
            .bg(PANEL())
            .text_color(TEXT_DIMMER())
            .child("⋯ unchanged ⋯"),
        UnifiedRow::Line { old, new, tag, text, emphasis } => {
            let (bg, fg, strong_bg, strong_fg) = colors_for(Some(tag));
            let marker = match *tag {
                "insert" => "+",
                "delete" => "-",
                _ => " ",
            };
            base.when_some(bg, |d, bg| d.bg(bg))
                .child(gutter(*old))
                .child(gutter(*new))
                .child(div().w(px(16.)).flex_none().text_color(fg).child(marker))
                .child(text_element(text, emphasis, fg, strong_bg, strong_fg))
        }
    }
}
