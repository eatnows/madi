//! A file's diff shaped for display in both layouts: side-by-side (deleted/inserted lines paired
//! into old|new columns) and unified (one column, for narrow panes). Built once per file so
//! switching layout on resize is free. Pure data; drawing it is the UI's job.
use std::{ops::Range, sync::Arc};

use crate::diff::DiffLine;

pub struct Cell {
    pub lineno: Option<usize>,
    /// "equal" | "delete" | "insert"
    pub tag: &'static str,
    pub text: Arc<str>,
    /// Byte ranges of `text` to emphasize (the words that changed).
    pub emphasis: Vec<Range<usize>>,
}

pub enum Row {
    Gap,
    Pair(Option<Cell>, Option<Cell>),
}

pub enum UnifiedRow {
    Gap,
    Line { old: Option<usize>, new: Option<usize>, tag: &'static str, text: Arc<str>, emphasis: Vec<Range<usize>> },
}

#[derive(Default)]
pub struct DiffLayout {
    pub split: Vec<Row>,
    pub unified: Vec<UnifiedRow>,
    widest_split: usize,
    widest_unified: usize,
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
            UnifiedRow::Line { old: line.old_lineno, new: line.new_lineno, tag: line.tag, text: text.into(), emphasis }
        })
        .collect()
}

/// Monospace column count, wide (CJK) glyphs as two.
fn columns(text: &str) -> usize {
    text.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

impl DiffLayout {
    pub fn new(lines: &[DiffLine]) -> Self {
        let split = build_split(lines);
        let unified = build_unified(lines);
        let widest_split = split
            .iter()
            .filter_map(|row| match row {
                Row::Pair(l, r) => Some([l, r].iter().filter_map(|c| c.as_ref()).map(|c| columns(&c.text)).max().unwrap_or(0)),
                Row::Gap => None,
            })
            .max()
            .unwrap_or(0);
        let widest_unified = unified
            .iter()
            .filter_map(|row| match row {
                UnifiedRow::Line { text, .. } => Some(columns(text)),
                UnifiedRow::Gap => None,
            })
            .max()
            .unwrap_or(0);
        Self { split, unified, widest_split, widest_unified }
    }

    pub fn is_empty(&self) -> bool {
        self.split.is_empty()
    }

    pub fn len(&self, unified: bool) -> usize {
        if unified { self.unified.len() } else { self.split.len() }
    }

    /// The longest line, in monospace columns, that the layout has to fit.
    pub fn widest_line(&self, unified: bool) -> usize {
        if unified { self.widest_unified } else { self.widest_split }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::Segment;

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
        let layout = DiffLayout::new(&lines);
        assert_eq!(layout.len(false), 2, "delete+insert share one split row");
        assert_eq!(layout.len(true), 3, "unified lists every line");
    }

    #[test]
    fn widest_line_counts_cjk_as_two_columns() {
        let ascii = DiffLayout::new(&[line("equal", Some(1), Some(1), &"x".repeat(100))]);
        let cjk = DiffLayout::new(&[line("equal", Some(1), Some(1), &"가".repeat(100))]);
        assert_eq!((ascii.widest_line(false), cjk.widest_line(false)), (100, 200));
        assert_eq!(cjk.widest_line(true), 200);
    }

    #[test]
    fn emphasis_ranges_point_at_the_changed_words() {
        let mut l = line("insert", None, Some(1), "");
        l.segments = vec![
            Segment { text: "keep ".into(), emphasized: false },
            Segment { text: "new".into(), emphasized: true },
        ];
        let layout = DiffLayout::new(&[l]);
        match &layout.unified[0] {
            UnifiedRow::Line { text, emphasis, .. } => assert_eq!(&text[emphasis[0].clone()], "new"),
            UnifiedRow::Gap => panic!("expected a line"),
        }
    }
}
