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

/// Occurrences of `query` in `line` as byte ranges, ignoring ASCII case (other characters match exactly).
pub fn find_in_line(line: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let needle = query.to_ascii_lowercase();
    line.to_ascii_lowercase().match_indices(&needle).map(|(at, m)| at..at + m.len()).collect()
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

    // ---- find and replace ----------------------------------------------------------------------

    /// Every match of `query`, in document order. Matches never span lines.
    pub fn find_all(&self, query: &str) -> Vec<(Pos, Pos)> {
        (0..self.line_count())
            .flat_map(|row| find_in_line(self.line(row), query).into_iter().map(move |r| (Pos::new(row, r.start), Pos::new(row, r.end))))
            .collect()
    }

    /// Selects the next (or previous) match, wrapping around the document. `keep_current` lets a match
    /// that starts at the selection's start stay selected, so typing more of the query doesn't skip it.
    pub fn select_match(&mut self, query: &str, forward: bool, keep_current: bool) -> bool {
        let matches = self.find_all(query);
        let (a, b) = self.selection();
        let pick = if forward {
            let from = if keep_current { a } else { b };
            matches.iter().find(|m| m.0 >= from).or(matches.first())
        } else {
            matches.iter().rev().find(|m| m.1 <= a).or(matches.last())
        };
        let Some(&(start, end)) = pick else { return false };
        self.set_cursor(start, false);
        self.set_cursor(end, true);
        true
    }

    /// Replaces the selection when it is itself a match of `query`.
    pub fn replace_match(&mut self, query: &str, replacement: &str) -> bool {
        let selected = self.selected_text();
        if selected.is_empty() || !selected.eq_ignore_ascii_case(query) {
            return false;
        }
        self.insert(replacement, false);
        true
    }

    /// Replaces every match as a single undo step; returns how many there were.
    pub fn replace_all(&mut self, query: &str, replacement: &str) -> usize {
        let matches = self.find_all(query);
        let Some(&(first, _)) = matches.first() else { return 0 };
        let mut out = String::new();
        for row in 0..self.line_count() {
            let line = self.line(row);
            let mut done = 0;
            for r in find_in_line(line, query) {
                out.push_str(&line[done..r.start]);
                out.push_str(replacement);
                done = r.end;
            }
            out.push_str(&line[done..]);
            if row + 1 < self.line_count() {
                out.push('\n');
            }
        }
        self.select_all();
        self.insert(&out, false);
        self.set_cursor(first, false);
        matches.len()
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

    /// Inserts matching delimiters, leaving the caret inside an empty pair or wrapping a selection.
    pub fn insert_pair(&mut self, open: char, close: char) {
        let (a, b) = self.selection();
        let selected = self.buffer.text_in(a, b);
        self.edit(a, b, &format!("{open}{selected}{close}"), true);
        if a == b {
            self.move_left(false);
        }
    }

    /// Types a closer, advancing over an already-present matching closer when possible.
    pub fn insert_closer(&mut self, close: char) {
        if self.selection().0 == self.selection().1 && self.char_at(self.cursor) == Some(close) {
            self.move_right(false);
        } else {
            self.insert(&close.to_string(), true);
        }
    }

    pub fn insert_quote(&mut self, quote: char) {
        if self.selection().0 == self.selection().1 && self.char_at(self.cursor) == Some(quote) {
            self.move_right(false);
        } else {
            self.insert_pair(quote, quote);
        }
    }

    pub fn backspace(&mut self) {
        let (a, b) = self.selection();
        let from = if a != b { a } else { self.prev_boundary(self.cursor) };
        self.edit(from, b, "", false);
    }

    /// Cmd+Backspace: deletes from the caret back to the start of the line (the selection, if any;
    /// at column 0 it joins with the previous line like a plain backspace).
    pub fn delete_to_line_start(&mut self) {
        let (a, b) = self.selection();
        if a == b && b.col > 0 {
            self.edit(Pos::new(b.row, 0), b, "", false);
        } else {
            self.backspace();
        }
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
    fn pairs_delimiters_and_skips_an_existing_closer() {
        let mut d = doc("");
        d.insert_pair('(', ')');
        assert_eq!(d.text(), "()" );
        assert_eq!(d.cursor(), Pos::new(0, 1));
        d.insert("x", true);
        d.insert_closer(')');
        assert_eq!(d.text(), "(x)");
        assert_eq!(d.cursor(), Pos::new(0, 3));
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
    fn find_matches_ignoring_ascii_case_and_steps_through_them_with_wrapping() {
        let mut d = doc("Foo bar foo\nxFOOx\n한글 foo");
        assert_eq!(d.find_all("foo").len(), 4);
        assert!(d.find_all("").is_empty());

        assert!(d.select_match("foo", true, true));
        assert_eq!((d.selection().0, d.selected_text().as_str()), (Pos::new(0, 0), "Foo"));
        assert!(d.select_match("foo", true, false));
        assert_eq!(d.selection().0, Pos::new(0, 8));
        d.select_match("foo", true, false);
        d.select_match("foo", true, false);
        d.select_match("foo", true, false);
        assert_eq!(d.selection().0, Pos::new(0, 0), "wrapped past the last match");
        assert!(d.select_match("foo", false, false));
        assert_eq!(d.selection().0, Pos::new(2, "한글 ".len()), "backwards wraps to the last");
        assert!(!d.select_match("zzz", true, false));
    }

    #[test]
    fn replace_one_and_replace_all_as_a_single_undo_step() {
        let mut d = doc("foo Foo\nbar foo");
        d.select_match("foo", true, true);
        assert!(d.replace_match("foo", "x"));
        assert_eq!(d.text(), "x Foo\nbar foo");
        assert!(!d.replace_match("foo", "y"), "a caret that isn't on a match replaces nothing");

        assert_eq!(d.replace_all("foo", "한"), 2);
        assert_eq!(d.text(), "x 한\nbar 한");
        assert!(d.undo());
        assert_eq!(d.text(), "x Foo\nbar foo", "undo restores every replacement at once");
    }

    #[test]
    fn delete_to_line_start_removes_everything_left_of_the_caret() {
        let mut d = doc("keep\n  let x = 1;\nend");
        d.set_cursor(Pos::new(1, 6), false);
        d.delete_to_line_start();
        assert_eq!(d.text(), "keep\nx = 1;\nend");
        assert_eq!(d.cursor(), Pos::new(1, 0));
        d.delete_to_line_start();
        assert_eq!(d.text(), "keepx = 1;\nend", "at column 0 it joins the previous line");

        d.set_cursor(Pos::new(0, 1), false);
        d.set_cursor(Pos::new(0, 3), true);
        d.delete_to_line_start();
        assert_eq!(d.text(), "kpx = 1;\nend", "a selection is deleted as it is");
        assert!(d.undo());
        assert_eq!(d.text(), "keepx = 1;\nend");
    }

    #[test]
    fn cursor_display_counts_characters_not_bytes() {
        let mut d = doc("가나다\nx");
        d.set_cursor(Pos::new(0, "가나".len()), false);
        assert_eq!(d.cursor_display(), (1, 3));
    }
}
