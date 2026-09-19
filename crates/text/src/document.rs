//! A document being edited: the text, the caret and selection, an IME composition in progress,
//! and every editing/navigation command. No UI, so all of it is tested directly; a view only
//! forwards keystrokes here and draws the result.
use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::{Buffer, Pos};

const TAB: &str = "    ";

pub struct Document {
    buffer: Buffer,
    /// The moving end of the selection; `anchor` is the fixed end (equal when nothing is selected).
    cursor: Pos,
    anchor: Pos,
    /// IME composition in progress (shown underlined until committed).
    marked: Option<(Pos, Pos)>,
    /// Column (in characters) vertical movement tries to return to across short lines.
    goal_col: Option<usize>,
    crlf: bool,
    saved_revision: u64,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Document {
    pub fn new(text: &str) -> Self {
        let buffer = Buffer::from_text(text);
        Self {
            saved_revision: buffer.revision(),
            buffer,
            cursor: Pos::default(),
            anchor: Pos::default(),
            marked: None,
            goal_col: None,
            crlf: text.contains("\r\n"),
        }
    }

    // ---- reading -------------------------------------------------------------------------------

    pub fn text(&self) -> String {
        self.buffer.text()
    }

    pub fn line_count(&self) -> usize {
        self.buffer.line_count()
    }

    pub fn line(&self, row: usize) -> &str {
        self.buffer.line(row)
    }

    pub fn end(&self) -> Pos {
        self.buffer.end()
    }

    /// `pos` moved onto the text (row in range, column within the line on a char boundary).
    pub fn clamp(&self, pos: Pos) -> Pos {
        self.buffer.clamp(pos)
    }

    pub fn revision(&self) -> u64 {
        self.buffer.revision()
    }

    pub fn cursor(&self) -> Pos {
        self.cursor
    }

    pub fn marked(&self) -> Option<(Pos, Pos)> {
        self.marked
    }

    /// The selection in document order (empty when start == end).
    pub fn selection(&self) -> (Pos, Pos) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }

    pub fn selected_text(&self) -> String {
        let (a, b) = self.selection();
        self.buffer.text_in(a, b)
    }

    pub fn text_in(&self, from: Pos, to: Pos) -> String {
        self.buffer.text_in(from, to)
    }

    /// 1-based line and column (in characters) of the caret, for a status bar.
    pub fn cursor_display(&self) -> (usize, usize) {
        let line = self.buffer.line(self.cursor.row);
        (self.cursor.row + 1, line[..self.cursor.col.min(line.len())].chars().count() + 1)
    }

    // ---- saving --------------------------------------------------------------------------------

    pub fn is_dirty(&self) -> bool {
        self.buffer.revision() != self.saved_revision
    }

    pub fn mark_saved(&mut self) {
        self.saved_revision = self.buffer.revision();
    }

    pub fn line_ending(&self) -> &'static str {
        if self.crlf { "CRLF" } else { "LF" }
    }

    /// The text to write to disk: the file's own line endings are kept.
    pub fn text_for_save(&self) -> String {
        let text = self.buffer.text();
        if self.crlf { text.replace('\n', "\r\n") } else { text }
    }

    // ---- caret & selection ---------------------------------------------------------------------

    /// Puts the caret at `pos`; with `extend` the anchor stays, growing the selection.
    pub fn set_cursor(&mut self, pos: Pos, extend: bool) {
        self.cursor = self.buffer.clamp(pos);
        if !extend {
            self.anchor = self.cursor;
        }
        self.goal_col = None;
        self.marked = None;
        self.buffer.break_undo_run();
    }

    pub fn select_all(&mut self) {
        self.anchor = Pos::default();
        self.cursor = self.buffer.end();
    }

    fn prev_boundary(&self, p: Pos) -> Pos {
        if p.col == 0 {
            return if p.row == 0 { p } else { Pos::new(p.row - 1, self.buffer.line(p.row - 1).len()) };
        }
        let line = self.buffer.line(p.row);
        let col = line.grapheme_indices(true).rev().find_map(|(i, _)| (i < p.col).then_some(i)).unwrap_or(0);
        Pos::new(p.row, col)
    }

    fn next_boundary(&self, p: Pos) -> Pos {
        let line = self.buffer.line(p.row);
        if p.col >= line.len() {
            return if p.row + 1 < self.buffer.line_count() { Pos::new(p.row + 1, 0) } else { p };
        }
        let col = line.grapheme_indices(true).find_map(|(i, _)| (i > p.col).then_some(i)).unwrap_or(line.len());
        Pos::new(p.row, col)
    }

    fn char_at(&self, p: Pos) -> Option<char> {
        self.buffer.line(p.row).get(p.col..).and_then(|s| s.chars().next())
    }

    fn word_left_of(&self, mut p: Pos) -> Pos {
        loop {
            let before = self.prev_boundary(p);
            if before == p {
                return p;
            }
            let ch = self.char_at(before);
            p = before;
            if ch.map(is_word).unwrap_or(false) {
                break;
            }
        }
        while p.col > 0 {
            let before = self.prev_boundary(p);
            if !self.char_at(before).map(is_word).unwrap_or(false) {
                break;
            }
            p = before;
        }
        p
    }

    fn word_right_of(&self, mut p: Pos) -> Pos {
        loop {
            let after = self.next_boundary(p);
            if after == p {
                return p;
            }
            let ch = self.char_at(p);
            p = after;
            if ch.map(is_word).unwrap_or(false) {
                break;
            }
        }
        while p.col < self.buffer.line(p.row).len() {
            if !self.char_at(p).map(is_word).unwrap_or(false) {
                break;
            }
            p = self.next_boundary(p);
        }
        p
    }

    /// Left: collapses a selection to its start, otherwise steps one character back.
    pub fn move_left(&mut self, extend: bool) {
        let (a, b) = self.selection();
        let to = if !extend && a != b { a } else { self.prev_boundary(self.cursor) };
        self.set_cursor(to, extend);
    }

    pub fn move_right(&mut self, extend: bool) {
        let (a, b) = self.selection();
        let to = if !extend && a != b { b } else { self.next_boundary(self.cursor) };
        self.set_cursor(to, extend);
    }

    /// Moves by lines, keeping the remembered column across short lines. Past the first/last line
    /// it goes to the start/end of that line.
    pub fn move_vertical(&mut self, delta: isize, extend: bool) {
        let goal = self.goal_col.unwrap_or_else(|| self.buffer.line(self.cursor.row)[..self.cursor.col].chars().count());
        let row = (self.cursor.row as isize + delta).clamp(0, self.buffer.line_count() as isize - 1) as usize;
        let col = if row == self.cursor.row && delta != 0 {
            if delta < 0 { 0 } else { self.buffer.line(row).len() }
        } else {
            let line = self.buffer.line(row);
            line.char_indices().nth(goal).map(|(i, _)| i).unwrap_or(line.len())
        };
        self.set_cursor(Pos::new(row, col), extend);
        self.goal_col = Some(goal);
    }

    /// First non-blank column, toggling with column 0 like most editors.
    pub fn move_line_start(&mut self, extend: bool) {
        let line = self.buffer.line(self.cursor.row);
        let indent = line.len() - line.trim_start().len();
        let col = if self.cursor.col == indent { 0 } else { indent };
        self.set_cursor(Pos::new(self.cursor.row, col), extend);
    }

    pub fn move_line_end(&mut self, extend: bool) {
        self.set_cursor(Pos::new(self.cursor.row, self.buffer.line(self.cursor.row).len()), extend);
    }

    pub fn move_doc_start(&mut self) {
        self.set_cursor(Pos::default(), false);
    }

    pub fn move_doc_end(&mut self) {
        self.set_cursor(self.buffer.end(), false);
    }

    pub fn move_word_left(&mut self, extend: bool) {
        self.set_cursor(self.word_left_of(self.cursor), extend);
    }

    pub fn move_word_right(&mut self, extend: bool) {
        self.set_cursor(self.word_right_of(self.cursor), extend);
    }

    // ---- editing -------------------------------------------------------------------------------

    fn edit(&mut self, from: Pos, to: Pos, text: &str, typing: bool) {
        let end = self.buffer.replace(from, to, text, typing);
        self.cursor = end;
        self.anchor = end;
        self.goal_col = None;
        self.marked = None;
    }

    /// Replaces the selection (or inserts at the caret). `typing` groups single characters into one
    /// undo step.
    pub fn insert(&mut self, text: &str, typing: bool) {
        let (a, b) = self.selection();
        self.edit(a, b, text, typing);
    }

    pub fn backspace(&mut self) {
        let (a, b) = self.selection();
        let from = if a != b { a } else { self.prev_boundary(self.cursor) };
        self.edit(from, b, "", false);
    }

    pub fn delete(&mut self) {
        let (a, b) = self.selection();
        let to = if a != b { b } else { self.next_boundary(self.cursor) };
        self.edit(a, to, "", false);
    }

    /// A newline that keeps the current line's indentation.
    pub fn newline(&mut self) {
        let (a, b) = self.selection();
        let indent: String = self.buffer.line(a.row).chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let indent = &indent[..indent.len().min(a.col)];
        self.edit(a, b, &format!("\n{indent}"), true);
    }

    pub fn tab(&mut self) {
        let (a, b) = self.selection();
        self.edit(a, b, TAB, false);
    }

    /// Removes the selection and returns it (nothing selected: `None`).
    pub fn cut(&mut self) -> Option<String> {
        let text = self.selected_text();
        if text.is_empty() {
            return None;
        }
        let (a, b) = self.selection();
        self.edit(a, b, "", false);
        Some(text)
    }

    pub fn paste(&mut self, text: &str) {
        self.insert(&text.replace("\r\n", "\n"), false);
    }

    pub fn undo(&mut self) -> bool {
        match self.buffer.undo() {
            Some(cursor) => {
                self.cursor = cursor;
                self.anchor = cursor;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.buffer.redo() {
            Some(cursor) => {
                self.cursor = cursor;
                self.anchor = cursor;
                true
            }
            None => false,
        }
    }

    // ---- platform text input (UTF-16 offsets over the whole document, '\n' between lines) ------

    pub fn pos_to_utf16(&self, p: Pos) -> usize {
        let mut n = 0;
        for row in 0..p.row.min(self.buffer.line_count()) {
            n += self.buffer.line(row).encode_utf16().count() + 1;
        }
        let line = self.buffer.line(p.row.min(self.buffer.line_count() - 1));
        n + line[..p.col.min(line.len())].encode_utf16().count()
    }

    pub fn utf16_to_pos(&self, mut offset: usize) -> Pos {
        for row in 0..self.buffer.line_count() {
            let line = self.buffer.line(row);
            let len = line.encode_utf16().count();
            if offset <= len {
                let mut units = 0;
                for (i, ch) in line.char_indices() {
                    if units >= offset {
                        return Pos::new(row, i);
                    }
                    units += ch.len_utf16();
                }
                return Pos::new(row, line.len());
            }
            offset -= len + 1;
        }
        self.buffer.end()
    }

    /// The text of a UTF-16 range and the range it actually covers after clamping.
    pub fn text_in_utf16(&self, range: Range<usize>) -> (String, Range<usize>) {
        let (a, b) = (self.utf16_to_pos(range.start), self.utf16_to_pos(range.end));
        (self.buffer.text_in(a, b), self.pos_to_utf16(a)..self.pos_to_utf16(b))
    }

    /// The selection as UTF-16 offsets, and whether the caret is at its start.
    pub fn selection_utf16(&self) -> (Range<usize>, bool) {
        let (a, b) = self.selection();
        (self.pos_to_utf16(a)..self.pos_to_utf16(b), self.cursor < self.anchor)
    }

    pub fn marked_utf16(&self) -> Option<Range<usize>> {
        self.marked.map(|(a, b)| self.pos_to_utf16(a)..self.pos_to_utf16(b))
    }

    pub fn unmark(&mut self) {
        self.marked = None;
    }

    /// The range an input-method edit applies to: the explicit one, else the composition in
    /// progress, else the selection.
    fn edit_range(&self, range_utf16: Option<Range<usize>>) -> (Pos, Pos) {
        match (range_utf16, self.marked) {
            (Some(r), _) => (self.utf16_to_pos(r.start), self.utf16_to_pos(r.end)),
            (None, Some(marked)) => marked,
            (None, None) => self.selection(),
        }
    }

    /// Commits `text` over the target range (ending any composition).
    pub fn replace_utf16(&mut self, range_utf16: Option<Range<usize>>, text: &str) {
        let (from, to) = self.edit_range(range_utf16);
        let typing = self.marked.is_none() && text.chars().count() == 1;
        self.edit(from, to, text, typing);
    }

    /// Shows `text` as in-progress composition; `new_selection_utf16` is relative to its start.
    pub fn replace_and_mark_utf16(&mut self, range_utf16: Option<Range<usize>>, text: &str, new_selection_utf16: Option<Range<usize>>) {
        let (from, to) = self.edit_range(range_utf16);
        let end = self.buffer.replace(from, to, text, false);
        self.marked = if text.is_empty() { None } else { Some((from, end)) };
        let base = self.pos_to_utf16(from);
        match new_selection_utf16 {
            Some(r) => {
                self.anchor = self.utf16_to_pos(base + r.start);
                self.cursor = self.utf16_to_pos(base + r.end);
            }
            None => {
                self.anchor = end;
                self.cursor = end;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        Document::new(text)
    }

    fn type_str(d: &mut Document, s: &str) {
        for ch in s.chars() {
            d.insert(&ch.to_string(), true);
        }
    }

    #[test]
    fn typing_including_korean_lands_at_the_caret() {
        let mut d = doc("");
        type_str(&mut d, "hello 안녕");
        assert_eq!(d.text(), "hello 안녕");
        assert_eq!(d.cursor(), Pos::new(0, "hello 안녕".len()));
    }

    #[test]
    fn newline_keeps_indentation_and_backspace_removes_a_whole_character() {
        let mut d = doc("");
        type_str(&mut d, "  fn");
        d.newline();
        type_str(&mut d, "x");
        assert_eq!(d.text(), "  fn\n  x");
        d.backspace();
        d.backspace();
        assert_eq!(d.text(), "  fn\n ");

        let mut d = doc("");
        type_str(&mut d, "한글");
        d.backspace();
        assert_eq!(d.text(), "한", "one press removes one syllable, not one byte");
    }

    #[test]
    fn selecting_all_then_typing_replaces_everything() {
        let mut d = doc("hello world");
        d.select_all();
        d.insert("X", true);
        assert_eq!(d.text(), "X");
    }

    #[test]
    fn extended_moves_select_and_a_plain_left_collapses_to_the_start() {
        let mut d = doc("abc\ndef");
        d.move_right(true);
        d.move_right(true);
        d.move_vertical(1, true);
        assert_eq!(d.selected_text(), "abc\nde");
        d.move_left(false);
        assert_eq!(d.cursor(), Pos::new(0, 0));
        assert_eq!(d.selected_text(), "");
    }

    #[test]
    fn vertical_movement_remembers_the_column_across_short_lines() {
        let mut d = doc("abcdefgh\nab\nabcdefgh");
        d.set_cursor(Pos::new(0, 6), false);
        d.move_vertical(1, false);
        assert_eq!(d.cursor(), Pos::new(1, 2), "clamped to the short line");
        d.move_vertical(1, false);
        assert_eq!(d.cursor(), Pos::new(2, 6), "and back to the goal column");
        d.move_vertical(1, false);
        assert_eq!(d.cursor(), Pos::new(2, 8), "past the last line goes to its end");
    }

    #[test]
    fn line_start_toggles_between_indent_and_column_zero() {
        let mut d = doc("    code");
        d.set_cursor(Pos::new(0, 8), false);
        d.move_line_start(false);
        assert_eq!(d.cursor().col, 4);
        d.move_line_start(false);
        assert_eq!(d.cursor().col, 0);
    }

    #[test]
    fn words_are_skipped_by_alphanumeric_runs() {
        let mut d = doc("foo_bar  baz.qux");
        d.set_cursor(Pos::new(0, 0), false);
        d.move_word_right(false);
        assert_eq!(d.cursor().col, 7, "past foo_bar");
        d.move_word_right(false);
        assert_eq!(d.cursor().col, 12, "past baz");
        d.move_word_left(false);
        assert_eq!(d.cursor().col, 9, "back to the start of baz");
    }

    #[test]
    fn cut_paste_undo_redo_and_dirty_tracking() {
        let mut d = doc("one two");
        assert!(!d.is_dirty());
        d.set_cursor(Pos::new(0, 0), false);
        d.move_word_right(true);
        assert_eq!(d.cut().as_deref(), Some("one"));
        assert_eq!(d.text(), " two");
        assert!(d.is_dirty());
        d.paste("X\r\nY");
        assert_eq!(d.text(), "X\nY two", "pasted CRLF is normalized");
        assert!(d.undo() && d.undo());
        assert_eq!(d.text(), "one two");
        assert!(!d.is_dirty() || d.text() == "one two");
        assert!(d.redo());
        assert_eq!(d.text(), " two");
        d.mark_saved();
        assert!(!d.is_dirty());
        assert_eq!(d.cut(), None, "nothing selected");
    }

    #[test]
    fn save_text_keeps_the_files_own_line_endings() {
        let mut crlf = doc("a\r\nb\r\n");
        crlf.insert("X", false);
        assert_eq!((crlf.line_ending(), crlf.text_for_save().as_str()), ("CRLF", "Xa\r\nb\r\n"));
        let lf = doc("a\nb\n");
        assert_eq!((lf.line_ending(), lf.text_for_save().as_str()), ("LF", "a\nb\n"));
    }

    #[test]
    fn ime_composition_is_replaced_by_the_committed_text() {
        let mut d = doc("");
        // Korean IME: 'ㅎ' shown as marked text, replaced by '하', committed as '한'.
        for composing in ["ㅎ", "하"] {
            d.replace_and_mark_utf16(None, composing, None);
            assert_eq!(d.text(), composing);
            assert!(d.marked().is_some(), "still composing");
        }
        d.replace_utf16(None, "한");
        assert_eq!(d.text(), "한");
        assert!(d.marked().is_none());
        assert_eq!(d.cursor(), Pos::new(0, "한".len()));
    }

    #[test]
    fn utf16_offsets_round_trip_across_lines_and_wide_characters() {
        let d = doc("a😀b\n안녕\nxyz");
        for pos in [Pos::new(0, 0), Pos::new(0, 1), Pos::new(0, 5), Pos::new(0, 6), Pos::new(1, 3), Pos::new(2, 2)] {
            assert_eq!(d.utf16_to_pos(d.pos_to_utf16(pos)), pos, "{pos:?}");
        }
        assert_eq!(d.pos_to_utf16(Pos::new(0, 5)), 3, "'😀' is two UTF-16 units, so 'b' starts at 3");
        assert_eq!(d.text_in_utf16(0..3).0, "a😀");
    }

    #[test]
    fn cursor_display_counts_characters_not_bytes() {
        let mut d = doc("가나다\nx");
        d.set_cursor(Pos::new(0, "가나".len()), false);
        assert_eq!(d.cursor_display(), (1, 3));
    }
}
