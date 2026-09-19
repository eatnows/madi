//! The editor's text model: lines of text with positions, edits and undo/redo. No UI, so it is
//! unit-tested directly. Columns are UTF-8 byte offsets within a line (always on char boundaries).
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }
}

impl PartialOrd for Pos {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pos {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.row, self.col).cmp(&(other.row, other.col))
    }
}

/// One reversible change: at `at`, `removed` was replaced by `inserted`.
#[derive(Clone, Debug)]
struct Edit {
    at: Pos,
    removed: String,
    inserted: String,
}

pub struct Buffer {
    lines: Vec<String>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    /// Consecutive single-typed edits merge into one undo step until this is cleared.
    coalesce: bool,
    /// Bumped on every change, so views know when to refresh derived data.
    revision: u64,
}

fn end_of(at: Pos, text: &str) -> Pos {
    let mut lines = text.split('\n');
    let first = lines.next().unwrap_or("");
    match lines.last() {
        None => Pos::new(at.row, at.col + first.len()),
        Some(last) => Pos::new(at.row + text.matches('\n').count(), last.len()),
    }
}

impl Buffer {
    pub fn from_text(text: &str) -> Self {
        let mut lines: Vec<String> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self { lines, undo: Vec::new(), redo: Vec::new(), coalesce: false, revision: 0 }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, row: usize) -> &str {
        &self.lines[row]
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn end(&self) -> Pos {
        let row = self.lines.len() - 1;
        Pos::new(row, self.lines[row].len())
    }

    /// Clamps a position onto the text (row in range, column within the line, on a char boundary).
    pub fn clamp(&self, p: Pos) -> Pos {
        let row = p.row.min(self.lines.len() - 1);
        let line = &self.lines[row];
        let mut col = p.col.min(line.len());
        while !line.is_char_boundary(col) {
            col -= 1;
        }
        Pos::new(row, col)
    }

    pub fn text_in(&self, from: Pos, to: Pos) -> String {
        let (from, to) = (self.clamp(from.min(to)), self.clamp(from.max(to)));
        if from.row == to.row {
            return self.lines[from.row][from.col..to.col].to_string();
        }
        let mut out = self.lines[from.row][from.col..].to_string();
        for row in from.row + 1..to.row {
            out.push('\n');
            out.push_str(&self.lines[row]);
        }
        out.push('\n');
        out.push_str(&self.lines[to.row][..to.col]);
        out
    }

    /// Replaces `from..to` with `text`; returns the position just after the inserted text.
    /// `typing` marks single-character input so runs of it undo together.
    pub fn replace(&mut self, from: Pos, to: Pos, text: &str, typing: bool) -> Pos {
        let (from, to) = (self.clamp(from.min(to)), self.clamp(from.max(to)));
        let removed = self.text_in(from, to);
        let end = self.apply(from, &removed, text);

        let mergeable = typing && self.coalesce && removed.is_empty() && text != "\n";
        match (mergeable, self.undo.last_mut()) {
            (true, Some(last)) if last.removed.is_empty() && end_of(last.at, &last.inserted) == from => {
                last.inserted.push_str(text);
            }
            _ => self.undo.push(Edit { at: from, removed, inserted: text.to_string() }),
        }
        self.coalesce = typing && text != "\n";
        self.redo.clear();
        end
    }

    /// Applies the raw change (no history): replaces `removed` at `at` with `inserted`.
    fn apply(&mut self, at: Pos, removed: &str, inserted: &str) -> Pos {
        let to = end_of(at, removed);
        let tail = self.lines[to.row][to.col..].to_string();
        self.lines[at.row].truncate(at.col);
        self.lines[at.row].push_str(inserted);
        let mut new_lines = self.lines[at.row].split('\n').map(str::to_string).collect::<Vec<_>>();
        let end = Pos::new(at.row + new_lines.len() - 1, new_lines.last().map(|l| l.len()).unwrap_or(0));
        new_lines.last_mut().unwrap().push_str(&tail);
        self.lines.splice(at.row..=to.row, new_lines);
        self.revision += 1;
        end
    }

    pub fn undo(&mut self) -> Option<Pos> {
        let edit = self.undo.pop()?;
        self.coalesce = false;
        self.apply(edit.at, &edit.inserted, &edit.removed);
        let cursor = end_of(edit.at, &edit.removed);
        self.redo.push(edit);
        Some(cursor)
    }

    pub fn redo(&mut self) -> Option<Pos> {
        let edit = self.redo.pop()?;
        self.coalesce = false;
        let cursor = self.apply(edit.at, &edit.removed, &edit.inserted);
        self.undo.push(edit);
        Some(cursor)
    }

    /// Ends the current typing run so the next character starts a new undo step (after a cursor move).
    pub fn break_undo_run(&mut self) {
        self.coalesce = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(row: usize, col: usize) -> Pos {
        Pos::new(row, col)
    }

    #[test]
    fn splits_lines_and_normalizes_crlf() {
        let b = Buffer::from_text("a\r\nbb\n\nccc");
        assert_eq!((b.line_count(), b.line(0), b.line(1), b.line(2), b.line(3)), (4, "a", "bb", "", "ccc"));
        assert_eq!(Buffer::from_text("").line_count(), 1);
        assert_eq!(Buffer::from_text("x\n").line_count(), 2, "a trailing newline leaves an empty last line");
    }

    #[test]
    fn inserts_within_and_across_lines() {
        let mut b = Buffer::from_text("hello world");
        let end = b.replace(p(0, 5), p(0, 5), ",", false);
        assert_eq!((b.text().as_str(), end), ("hello, world", p(0, 6)));
        let end = b.replace(p(0, 6), p(0, 6), "\nnew\nlines", false);
        assert_eq!(b.text(), "hello,\nnew\nlines world");
        assert_eq!(end, p(2, 5));
    }

    #[test]
    fn deletes_ranges_spanning_lines() {
        let mut b = Buffer::from_text("one\ntwo\nthree");
        b.replace(p(0, 2), p(2, 2), "", false);
        assert_eq!(b.text(), "onree");
        assert_eq!(b.line_count(), 1);
    }

    #[test]
    fn text_in_reads_back_a_selection_in_either_order() {
        let b = Buffer::from_text("one\ntwo\nthree");
        assert_eq!(b.text_in(p(0, 1), p(2, 2)), "ne\ntwo\nth");
        assert_eq!(b.text_in(p(2, 2), p(0, 1)), "ne\ntwo\nth");
    }

    #[test]
    fn multibyte_text_is_clamped_to_char_boundaries() {
        let mut b = Buffer::from_text("안녕하세요");
        assert_eq!(b.clamp(p(0, 4)), p(0, 3), "byte 4 is inside the second syllable");
        let end = b.replace(p(0, 3), p(0, 3), "X", false);
        assert_eq!((b.text().as_str(), end), ("안X녕하세요", p(0, 4)));
    }

    #[test]
    fn undo_and_redo_restore_text_and_cursor() {
        let mut b = Buffer::from_text("abc\ndef");
        b.replace(p(0, 1), p(1, 1), "", false);
        assert_eq!(b.text(), "aef");
        assert_eq!(b.undo(), Some(p(1, 1)));
        assert_eq!(b.text(), "abc\ndef");
        assert_eq!(b.redo(), Some(p(0, 1)));
        assert_eq!(b.text(), "aef");
        assert_eq!(b.redo(), None);
    }

    #[test]
    fn a_typing_run_undoes_as_one_step_but_a_newline_breaks_it() {
        let mut b = Buffer::from_text("");
        let mut at = p(0, 0);
        for ch in ["h", "e", "l", "l", "o"] {
            at = b.replace(at, at, ch, true);
        }
        at = b.replace(at, at, "\n", true);
        for ch in ["h", "i"] {
            at = b.replace(at, at, ch, true);
        }
        assert_eq!(b.text(), "hello\nhi");
        b.undo();
        assert_eq!(b.text(), "hello\n", "the second line's typing undoes together");
        b.undo();
        assert_eq!(b.text(), "hello", "then the newline");
        b.undo();
        assert_eq!(b.text(), "", "then the whole first word");
    }

    #[test]
    fn a_new_edit_clears_redo_and_cursor_moves_end_a_run() {
        let mut b = Buffer::from_text("");
        let a = b.replace(p(0, 0), p(0, 0), "a", true);
        b.break_undo_run();
        b.replace(a, a, "b", true);
        assert_eq!(b.text(), "ab");
        b.undo();
        assert_eq!(b.text(), "a", "the moved-away typing was its own step");
        b.replace(p(0, 1), p(0, 1), "z", false);
        assert_eq!(b.redo(), None, "redo history is dropped by a new edit");
    }
}
