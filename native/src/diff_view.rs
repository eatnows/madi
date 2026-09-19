//! Diff rows for both layouts: side-by-side (deleted/inserted lines paired into old|new columns)
//! and unified (one column, for narrow panes), plus how a row is drawn.
use std::ops::Range;

use gpui::{div, prelude::*, px, AnyElement, HighlightStyle, IntoElement, Rgba, SharedString, StyledText};
use maditor_core::diff::DiffLine;

use crate::theme::*;

pub const ROW_H: f32 = 20.0;
/// Below this pane width each half of a side-by-side diff would be too cramped to read.
pub const SPLIT_MIN_WIDTH: f32 = 760.0;

const CHAR_W: f32 = 7.3;
const GUTTER_W: f32 = 44.0;

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

pub enum UnifiedRow {
    Gap,
    Line { old: Option<usize>, new: Option<usize>, tag: &'static str, text: SharedString, emphasis: Vec<Range<usize>> },
}

/// One file's diff in both layouts, built once so switching layout on resize is free.
#[derive(Default)]
pub struct DiffData {
    split: Vec<Row>,
    unified: Vec<UnifiedRow>,
    split_width: f32,
    unified_width: f32,
}

fn text_and_emphasis(line: &DiffLine) -> (String, Vec<Range<usize>>) {
    let mut text = String::new();
    let mut emphasis = Vec::new();
    for seg in &line.segments {
        let start = text.len();
        text.push_str(&seg.text);
        if seg.emphasized {
            emphasis.push(start..text.len());
        }
    }
    (text, emphasis)
}

fn cell_of(line: &DiffLine, new_side: bool) -> Cell {
    let (text, emphasis) = text_and_emphasis(line);
    Cell {
        lineno: if new_side { line.new_lineno } else { line.old_lineno },
        tag: line.tag,
        text: text.into(),
        emphasis,
    }
}

/// Pairs delete/insert runs side by side so they render as aligned old|new columns.
fn build_split(lines: &[DiffLine]) -> Vec<Row> {
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

fn build_unified(lines: &[DiffLine]) -> Vec<UnifiedRow> {
    lines
        .iter()
        .map(|line| {
            if line.tag == "gap" {
                return UnifiedRow::Gap;
            }
            let (text, emphasis) = text_and_emphasis(line);
            UnifiedRow::Line {
                old: line.old_lineno,
                new: line.new_lineno,
                tag: line.tag,
                text: text.into(),
                emphasis,
            }
        })
        .collect()
}

/// Monospace column count, wide (CJK) glyphs as two.
fn columns(text: &str) -> usize {
    text.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

impl DiffData {
    pub fn new(lines: &[DiffLine]) -> Self {
        let split = build_split(lines);
        let unified = build_unified(lines);
        let widest_half = split
            .iter()
            .filter_map(|row| match row {
                Row::Pair(l, r) => Some(
                    [l, r].iter().filter_map(|c| c.as_ref()).map(|c| columns(&c.text)).max().unwrap_or(0),
                ),
                Row::Gap => None,
            })
            .max()
            .unwrap_or(0);
        let widest_line = unified
            .iter()
            .filter_map(|row| match row {
                UnifiedRow::Line { text, .. } => Some(columns(text)),
                UnifiedRow::Gap => None,
            })
            .max()
            .unwrap_or(0);
        Self {
            split,
            unified,
            // Width the layout needs so its longest line isn't clipped (estimated once per file).
            split_width: 2.0 * (GUTTER_W + widest_half as f32 * CHAR_W + 24.0),
            unified_width: 2.0 * GUTTER_W + 24.0 + widest_line as f32 * CHAR_W + 24.0,
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.split.is_empty()
    }

    pub fn len(&self, unified: bool) -> usize {
        if unified { self.unified.len() } else { self.split.len() }
    }

    pub fn width(&self, unified: bool) -> f32 {
        if unified { self.unified_width } else { self.split_width }
    }

    pub fn render_row(&self, unified: bool, ix: usize) -> AnyElement {
        if unified {
            render_unified_row(&self.unified[ix]).into_any_element()
        } else {
            render_split_row(&self.split[ix]).into_any_element()
        }
    }
}

fn colors_for(tag: Option<&str>) -> (Option<Rgba>, Rgba, Rgba, Rgba) {
    match tag {
        Some("insert") => (Some(ADD_BG()), ADD_FG(), ADD_STRONG_BG(), ADD_STRONG_FG()),
        Some("delete") => (Some(DEL_BG()), DEL_FG(), DEL_STRONG_BG(), DEL_STRONG_FG()),
        _ => (None, TEXT(), SELECTED(), TEXT_STRONG()),
    }
}

fn text_element(text: &SharedString, emphasis: &[Range<usize>], fg: Rgba, strong_bg: Rgba, strong_fg: Rgba) -> impl IntoElement {
    let style = HighlightStyle {
        color: Some(strong_fg.into()),
        background_color: Some(strong_bg.into()),
        ..Default::default()
    };
    let highlights: Vec<_> = emphasis.iter().map(|r| (r.clone(), style)).collect();
    div()
        .whitespace_nowrap()
        .text_color(fg)
        .child(StyledText::new(text.clone()).with_highlights(highlights))
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

#[cfg(test)]
mod tests {
    use super::*;
    use maditor_core::diff::Segment;

    fn line(tag: &'static str, old: Option<usize>, new: Option<usize>, text: &str) -> DiffLine {
        DiffLine {
            tag,
            old_lineno: old,
            new_lineno: new,
            segments: vec![Segment { text: text.into(), emphasized: false }],
            skipped: None,
        }
    }

    #[test]
    fn split_pairs_a_replaced_line_while_unified_keeps_both_in_order() {
        let lines = [
            line("equal", Some(1), Some(1), "keep"),
            line("delete", Some(2), None, "old"),
            line("insert", None, Some(2), "new"),
        ];
        let data = DiffData::new(&lines);
        assert_eq!(data.len(false), 2, "delete+insert share one split row");
        assert_eq!(data.len(true), 3, "unified lists every line");
    }

    #[test]
    fn width_grows_with_the_longest_line_and_counts_cjk_double() {
        let short = DiffData::new(&[line("equal", Some(1), Some(1), "abc")]);
        let long = DiffData::new(&[line("equal", Some(1), Some(1), &"x".repeat(200))]);
        let cjk = DiffData::new(&[line("equal", Some(1), Some(1), &"가".repeat(100))]);
        assert!(long.width(false) > short.width(false));
        assert!(cjk.width(true) > DiffData::new(&[line("equal", Some(1), Some(1), &"x".repeat(100))]).width(true));
        // Unified needs two gutters in one column; split shows two full halves.
        assert!(long.width(false) > long.width(true));
    }
}
