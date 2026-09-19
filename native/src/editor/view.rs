//! The editor view: a `Buffer` with cursor/selection, IME-aware text input, keyboard actions,
//! mouse selection and a virtualized, scrollable rendering. Adapted from gpui's `input` example
//! (Apache-2.0) and extended to multiple lines.
use std::{cell::RefCell, collections::HashMap, ops::Range, path::PathBuf};

use gpui::{
    actions, canvas, div, fill, point, prelude::*, px, relative, size, uniform_list, App, Bounds,
    ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Rgba, ScrollHandle, ScrollStrategy,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle,
    UniformListScrollHandle, Window,
};
use unicode_segmentation::UnicodeSegmentation;

use super::buffer::{Buffer, Pos};
use crate::theme::*;

actions!(
    editor,
    [
        Left, Right, Up, Down, SelectLeft, SelectRight, SelectUp, SelectDown, LineStart, LineEnd,
        SelectLineStart, SelectLineEnd, DocStart, DocEnd, WordLeft, WordRight, SelectWordLeft,
        SelectWordRight, Backspace, Delete, Newline, Tab, Copy, Cut, Paste, SelectAll, Undo, Redo,
        Save, PageUp, PageDown,
    ]
);

pub fn bind_keys(cx: &mut App) {
    let ctx = Some("Editor");
    let mac = cfg!(target_os = "macos");
    let (line_start, line_end, word_left, word_right) = if mac {
        ("cmd-left", "cmd-right", "alt-left", "alt-right")
    } else {
        ("home", "end", "ctrl-left", "ctrl-right")
    };
    let sel = |k: &str| format!("shift-{k}");
    cx.bind_keys([
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("up", Up, ctx),
        KeyBinding::new("down", Down, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("shift-up", SelectUp, ctx),
        KeyBinding::new("shift-down", SelectDown, ctx),
        KeyBinding::new(line_start, LineStart, ctx),
        KeyBinding::new(line_end, LineEnd, ctx),
        KeyBinding::new(&sel(line_start), SelectLineStart, ctx),
        KeyBinding::new(&sel(line_end), SelectLineEnd, ctx),
        KeyBinding::new(word_left, WordLeft, ctx),
        KeyBinding::new(word_right, WordRight, ctx),
        KeyBinding::new(&sel(word_left), SelectWordLeft, ctx),
        KeyBinding::new(&sel(word_right), SelectWordRight, ctx),
        KeyBinding::new("secondary-up", DocStart, ctx),
        KeyBinding::new("secondary-down", DocEnd, ctx),
        KeyBinding::new("home", LineStart, ctx),
        KeyBinding::new("end", LineEnd, ctx),
        KeyBinding::new("pageup", PageUp, ctx),
        KeyBinding::new("pagedown", PageDown, ctx),
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("enter", Newline, ctx),
        KeyBinding::new("tab", Tab, ctx),
        KeyBinding::new("secondary-c", Copy, ctx),
        KeyBinding::new("secondary-x", Cut, ctx),
        KeyBinding::new("secondary-v", Paste, ctx),
        KeyBinding::new("secondary-a", SelectAll, ctx),
        KeyBinding::new("secondary-z", Undo, ctx),
        KeyBinding::new("secondary-shift-z", Redo, ctx),
        KeyBinding::new("secondary-s", Save, ctx),
    ]);
}

/// Things the editor tells its owner about.
pub enum EditorEvent {
    SaveFailed(String),
}

impl gpui::EventEmitter<EditorEvent> for Editor {}

pub const ROW_H: f32 = 20.0;
const GUTTER_W: f32 = 56.0;
const CHAR_W: f32 = 7.9;
const TAB: &str = "    ";

pub struct Editor {
    focus_handle: FocusHandle,
    pub buffer: Buffer,
    /// The moving end of the selection; `anchor` is the fixed end (equal when nothing is selected).
    cursor: Pos,
    anchor: Pos,
    /// IME composition in progress (underlined until committed).
    marked: Option<(Pos, Pos)>,
    /// Column (in characters) vertical movement tries to return to.
    goal_col: Option<usize>,
    pub path: Option<PathBuf>,
    saved_revision: u64,
    crlf: bool,
    pub scroll: UniformListScrollHandle,
    hscroll: ScrollHandle,
    /// Shaped lines and their on-screen bounds from the last paint, for hit-testing and IME.
    layouts: RefCell<HashMap<usize, (ShapedLine, Bounds<Pixels>)>>,
    is_selecting: bool,
    reveal_cursor: bool,
    content_width: f32,
    width_revision: u64,
}

impl Editor {
    pub fn new(text: &str, path: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let crlf = text.contains("\r\n");
        let mut editor = Self {
            focus_handle: cx.focus_handle(),
            buffer: Buffer::from_text(text),
            cursor: Pos::default(),
            anchor: Pos::default(),
            marked: None,
            goal_col: None,
            path,
            saved_revision: 0,
            crlf,
            scroll: UniformListScrollHandle::new(),
            hscroll: ScrollHandle::new(),
            layouts: RefCell::new(HashMap::new()),
            is_selecting: false,
            reveal_cursor: false,
            content_width: 0.,
            width_revision: u64::MAX,
        };
        editor.saved_revision = editor.buffer.revision();
        editor
    }

    pub fn is_dirty(&self) -> bool {
        self.buffer.revision() != self.saved_revision
    }

    #[cfg(test)]
    pub fn text(&self) -> String {
        self.buffer.text()
    }

    #[cfg(test)]
    pub fn cursor(&self) -> Pos {
        self.cursor
    }

    pub fn selection(&self) -> (Pos, Pos) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }

    fn selected_text(&self) -> String {
        let (a, b) = self.selection();
        self.buffer.text_in(a, b)
    }

    /// Writes the buffer to its path (restoring CRLF line endings if the file had them).
    pub fn save(&mut self, cx: &mut Context<Self>) -> std::io::Result<()> {
        let Some(path) = self.path.clone() else { return Ok(()) };
        let mut text = self.buffer.text();
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        std::fs::write(path, text)?;
        self.saved_revision = self.buffer.revision();
        cx.notify();
        Ok(())
    }

    // ---- cursor & selection ---------------------------------------------------------------

    fn set_cursor(&mut self, pos: Pos, extend: bool, cx: &mut Context<Self>) {
        self.cursor = self.buffer.clamp(pos);
        if !extend {
            self.anchor = self.cursor;
        }
        self.goal_col = None;
        self.marked = None;
        self.buffer.break_undo_run();
        self.reveal_cursor = true;
        self.scroll_to_cursor();
        cx.notify();
    }

    fn scroll_to_cursor(&self) {
        self.scroll.scroll_to_item(self.cursor.row, ScrollStrategy::Center);
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

    fn word_left(&self, mut p: Pos) -> Pos {
        loop {
            let before = self.prev_boundary(p);
            if before == p {
                return p;
            }
            let ch = self.char_at(before);
            p = before;
            if ch.map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false) {
                break;
            }
        }
        while p.col > 0 {
            let before = self.prev_boundary(p);
            if !self.char_at(before).map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false) {
                break;
            }
            p = before;
        }
        p
    }

    fn word_right(&self, mut p: Pos) -> Pos {
        loop {
            let after = self.next_boundary(p);
            if after == p {
                return p;
            }
            let ch = self.char_at(p);
            p = after;
            if ch.map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false) {
                break;
            }
        }
        while p.col < self.buffer.line(p.row).len() {
            if !self.char_at(p).map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false) {
                break;
            }
            p = self.next_boundary(p);
        }
        p
    }

    fn char_at(&self, p: Pos) -> Option<char> {
        self.buffer.line(p.row).get(p.col..).and_then(|s| s.chars().next())
    }

    /// Moves vertically, keeping the remembered column across short lines.
    fn move_vertical(&mut self, delta: isize, extend: bool, cx: &mut Context<Self>) {
        let goal = self.goal_col.unwrap_or_else(|| self.buffer.line(self.cursor.row)[..self.cursor.col].chars().count());
        let row = (self.cursor.row as isize + delta).clamp(0, self.buffer.line_count() as isize - 1) as usize;
        let col = if row == self.cursor.row && delta != 0 {
            if delta < 0 { 0 } else { self.buffer.line(row).len() }
        } else {
            let line = self.buffer.line(row);
            line.char_indices().nth(goal).map(|(i, _)| i).unwrap_or(line.len())
        };
        self.set_cursor(Pos::new(row, col), extend, cx);
        self.goal_col = Some(goal);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let (a, b) = self.selection();
        let to = if a != b { a } else { self.prev_boundary(self.cursor) };
        self.set_cursor(to, false, cx);
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let (a, b) = self.selection();
        let to = if a != b { b } else { self.next_boundary(self.cursor) };
        self.set_cursor(to, false, cx);
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, false, cx);
    }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, false, cx);
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(self.prev_boundary(self.cursor), true, cx);
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(self.next_boundary(self.cursor), true, cx);
    }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, true, cx);
    }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, true, cx);
    }
    fn page(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.move_vertical(delta * 30, false, cx);
    }
    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        self.page(-1, cx);
    }
    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        self.page(1, cx);
    }

    /// First non-blank column of the line, toggling with column 0 like most editors.
    fn line_start_pos(&self) -> Pos {
        let line = self.buffer.line(self.cursor.row);
        let indent = line.len() - line.trim_start().len();
        Pos::new(self.cursor.row, if self.cursor.col == indent { 0 } else { indent })
    }
    fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(self.line_start_pos(), false, cx);
    }
    fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(Pos::new(self.cursor.row, self.buffer.line(self.cursor.row).len()), false, cx);
    }
    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(self.line_start_pos(), true, cx);
    }
    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(Pos::new(self.cursor.row, self.buffer.line(self.cursor.row).len()), true, cx);
    }
    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(Pos::default(), false, cx);
    }
    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(self.buffer.end(), false, cx);
    }
    fn word_left_action(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(self.word_left(self.cursor), false, cx);
    }
    fn word_right_action(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(self.word_right(self.cursor), false, cx);
    }
    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(self.word_left(self.cursor), true, cx);
    }
    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.set_cursor(self.word_right(self.cursor), true, cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.anchor = Pos::default();
        self.cursor = self.buffer.end();
        cx.notify();
    }

    // ---- editing ---------------------------------------------------------------------------

    fn edit(&mut self, from: Pos, to: Pos, text: &str, typing: bool, cx: &mut Context<Self>) {
        let end = self.buffer.replace(from, to, text, typing);
        self.cursor = end;
        self.anchor = end;
        self.goal_col = None;
        self.marked = None;
        self.reveal_cursor = true;
        self.scroll_to_cursor();
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        let (a, b) = self.selection();
        let from = if a != b { a } else { self.prev_boundary(self.cursor) };
        self.edit(from, b, "", false, cx);
    }
    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let (a, b) = self.selection();
        let to = if a != b { b } else { self.next_boundary(self.cursor) };
        self.edit(a, to, "", false, cx);
    }
    /// A newline that keeps the current line's indentation.
    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        let (a, b) = self.selection();
        let line = self.buffer.line(a.row);
        let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect::<String>();
        let indent = &indent[..indent.len().min(a.col)];
        self.edit(a, b, &format!("\n{indent}"), true, cx);
    }
    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        let (a, b) = self.selection();
        self.edit(a, b, TAB, false, cx);
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.selected_text();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.selected_text();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            let (a, b) = self.selection();
            self.edit(a, b, "", false, cx);
        }
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            let (a, b) = self.selection();
            self.edit(a, b, &text.replace("\r\n", "\n"), false, cx);
        }
    }
    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(cursor) = self.buffer.undo() {
            self.cursor = cursor;
            self.anchor = cursor;
            self.reveal_cursor = true;
            self.scroll_to_cursor();
            cx.notify();
        }
    }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(cursor) = self.buffer.redo() {
            self.cursor = cursor;
            self.anchor = cursor;
            self.reveal_cursor = true;
            self.scroll_to_cursor();
            cx.notify();
        }
    }
    fn save_action(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = self.save(cx) {
            let name = self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            cx.emit(EditorEvent::SaveFailed(format!("Couldn't save {name}: {e}")));
        }
    }

    // ---- mouse -----------------------------------------------------------------------------

    fn pos_for_point(&self, position: Point<Pixels>) -> Pos {
        let layouts = self.layouts.borrow();
        let hit = layouts.iter().find(|(_, (_, b))| position.y >= b.top() && position.y < b.bottom());
        match hit {
            Some((row, (line, bounds))) => {
                let col = line.closest_index_for_x(position.x - bounds.left());
                self.buffer.clamp(Pos::new(*row, col))
            }
            None => {
                // Above/below the painted rows: snap to the nearest end of the visible range.
                let first = layouts.keys().min().copied().unwrap_or(0);
                let last = layouts.keys().max().copied().unwrap_or(0);
                match layouts.get(&first) {
                    Some((_, b)) if position.y < b.top() => Pos::new(first, 0),
                    _ => Pos::new(last, self.buffer.line(last.min(self.buffer.line_count() - 1)).len()),
                }
            }
        }
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        let pos = self.pos_for_point(ev.position);
        self.set_cursor(pos, ev.modifiers.shift, cx);
        self.reveal_cursor = false;
    }
    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let pos = self.pos_for_point(ev.position);
            self.cursor = pos;
            cx.notify();
        }
    }
    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    // ---- UTF-16 offsets for the platform input handler ----------------------------------------

    fn pos_to_utf16(&self, p: Pos) -> usize {
        let mut n = 0;
        for row in 0..p.row.min(self.buffer.line_count()) {
            n += self.buffer.line(row).encode_utf16().count() + 1;
        }
        let line = self.buffer.line(p.row.min(self.buffer.line_count() - 1));
        n + line[..p.col.min(line.len())].encode_utf16().count()
    }

    fn utf16_to_pos(&self, mut offset: usize) -> Pos {
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

    fn content_width(&mut self) -> f32 {
        if self.width_revision != self.buffer.revision() {
            let cols = (0..self.buffer.line_count())
                .map(|r| self.buffer.line(r).chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum::<usize>())
                .max()
                .unwrap_or(0);
            self.content_width = GUTTER_W + cols as f32 * CHAR_W + 80.0;
            self.width_revision = self.buffer.revision();
        }
        self.content_width
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let (a, b) = (self.utf16_to_pos(range_utf16.start), self.utf16_to_pos(range_utf16.end));
        actual_range.replace(self.pos_to_utf16(a)..self.pos_to_utf16(b));
        Some(self.buffer.text_in(a, b))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let (a, b) = self.selection();
        Some(UTF16Selection { range: self.pos_to_utf16(a)..self.pos_to_utf16(b), reversed: self.cursor < self.anchor })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.map(|(a, b)| self.pos_to_utf16(a)..self.pos_to_utf16(b))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let (from, to) = match (range_utf16, self.marked) {
            (Some(r), _) => (self.utf16_to_pos(r.start), self.utf16_to_pos(r.end)),
            (None, Some((a, b))) => (a, b),
            (None, None) => self.selection(),
        };
        let typing = self.marked.is_none() && text.chars().count() == 1;
        self.edit(from, to, text, typing, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (from, to) = match (range_utf16, self.marked) {
            (Some(r), _) => (self.utf16_to_pos(r.start), self.utf16_to_pos(r.end)),
            (None, Some((a, b))) => (a, b),
            (None, None) => self.selection(),
        };
        let end = self.buffer.replace(from, to, text, false);
        self.marked = if text.is_empty() { None } else { Some((from, end)) };
        let base = self.pos_to_utf16(from);
        match new_selected_range_utf16 {
            Some(r) => {
                self.anchor = self.utf16_to_pos(base + r.start);
                self.cursor = self.utf16_to_pos(base + r.end);
            }
            None => {
                self.anchor = end;
                self.cursor = end;
            }
        }
        self.reveal_cursor = true;
        cx.notify();
    }

    fn bounds_for_range(&mut self, range_utf16: Range<usize>, _bounds: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let start = self.utf16_to_pos(range_utf16.start);
        let end = self.utf16_to_pos(range_utf16.end);
        let layouts = self.layouts.borrow();
        let (line, bounds) = layouts.get(&start.row)?;
        let end_col = if end.row == start.row { end.col } else { self.buffer.line(start.row).len() };
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(start.col), bounds.top()),
            point(bounds.left() + line.x_for_index(end_col), bounds.bottom()),
        ))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.pos_to_utf16(self.pos_for_point(p)))
    }
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// One visible line: shapes the text, paints selection and (when focused) the caret.
struct LineElement {
    editor: Entity<Editor>,
    row: usize,
}

struct LinePrepaint {
    line: ShapedLine,
    selection: Option<PaintQuad>,
    cursor: Option<PaintQuad>,
    cursor_x: Option<Pixels>,
}

impl IntoElement for LineElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Element for LineElement {
    type RequestLayoutState = ();
    type PrepaintState = LinePrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = px(ROW_H).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> LinePrepaint {
        let editor = self.editor.read(cx);
        let text: SharedString = editor.buffer.line(self.row).to_string().into();
        let (sel_a, sel_b) = editor.selection();
        let style = window.text_style();
        let base = TextRun {
            len: text.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match editor.marked.filter(|(a, b)| a.row == self.row && b.row == self.row) {
            Some((a, b)) => vec![
                TextRun { len: a.col, ..base.clone() },
                TextRun {
                    len: b.col - a.col,
                    underline: Some(UnderlineStyle { color: Some(base.color), thickness: px(1.), wavy: false }),
                    ..base.clone()
                },
                TextRun { len: text.len() - b.col, ..base },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect(),
            None => vec![base],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window.text_system().shape_line(text.clone(), font_size, &runs, None);

        let row = self.row;
        let selection = (sel_a != sel_b && row >= sel_a.row && row <= sel_b.row).then(|| {
            let from = if row == sel_a.row { sel_a.col } else { 0 };
            let (to, extra) = if row == sel_b.row { (sel_b.col, px(0.)) } else { (text.len(), px(6.)) };
            fill(
                Bounds::from_corners(
                    point(bounds.left() + line.x_for_index(from), bounds.top()),
                    point(bounds.left() + line.x_for_index(to) + extra, bounds.bottom()),
                ),
                Rgba { a: 0.28, ..AMBER() },
            )
        });
        let cursor_x = (editor.cursor.row == row).then(|| line.x_for_index(editor.cursor.col));
        let cursor = cursor_x.filter(|_| sel_a == sel_b).map(|x| {
            fill(Bounds::new(point(bounds.left() + x, bounds.top()), size(px(2.), bounds.size.height)), TEXT_STRONG())
        });
        LinePrepaint { line, selection, cursor, cursor_x }
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), prepaint: &mut LinePrepaint, window: &mut Window, cx: &mut App) {
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line = prepaint.line.clone();
        line.paint(bounds.origin, px(ROW_H), window, cx).unwrap();

        let focused = self.editor.read(cx).focus_handle.is_focused(window);
        if focused {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }
        let row = self.row;
        let cursor_x = prepaint.cursor_x;
        self.editor.update(cx, |editor, cx| {
            editor.layouts.borrow_mut().insert(row, (line, bounds));
            // Keep the caret in view horizontally after typing/moving (once, not on scroll).
            if editor.reveal_cursor && editor.cursor.row == row {
                if let Some(x) = cursor_x {
                    let viewport = editor.hscroll.bounds();
                    let abs = bounds.left() + x;
                    let margin = px(40.);
                    let mut offset = editor.hscroll.offset();
                    if abs > viewport.right() - margin {
                        offset.x -= abs - (viewport.right() - margin);
                    } else if abs < viewport.left() + px(GUTTER_W) + margin {
                        offset.x += (viewport.left() + px(GUTTER_W) + margin) - abs;
                    }
                    offset.x = offset.x.min(px(0.));
                    editor.hscroll.set_offset(offset);
                    editor.reveal_cursor = false;
                    cx.notify();
                }
            }
        });
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Rows are re-registered every paint; drop the previous frame's so scrolled-out rows
        // don't linger for hit-testing.
        self.layouts.borrow_mut().clear();
        let entity = cx.entity();
        let focus = self.focus_handle.clone();
        let width = self.content_width();
        let input_entity = entity.clone();

        let mut hscroll = div().id("editor-hscroll").size_full().overflow_x_scroll().track_scroll(&self.hscroll);
        hscroll.style().restrict_scroll_to_axis = Some(true);
        let mut list = uniform_list(
            "editor-lines",
            self.buffer.line_count(),
            cx.processor(move |this, range: Range<usize>, _w, cx| {
                let editor = cx.entity();
                range
                    .map(|row| {
                        let _ = &this;
                        div()
                            .flex()
                            .h(px(ROW_H))
                            .child(
                                div()
                                    .w(px(GUTTER_W))
                                    .flex_none()
                                    .pr_3()
                                    .text_right()
                                    .text_color(TEXT_DIM())
                                    .child((row + 1).to_string()),
                            )
                            .child(div().flex_1().child(LineElement { editor: editor.clone(), row }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .min_w(px(width))
        .h_full();
        list.style().restrict_scroll_to_axis = Some(true);

        div()
            .key_context("Editor")
            .track_focus(&focus)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::line_start))
            .on_action(cx.listener(Self::line_end))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::word_left_action))
            .on_action(cx.listener(Self::word_right_action))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::save_action))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .relative()
            .size_full()
            .bg(BG())
            .font_family(MONO)
            .text_size(px(13.))
            .line_height(px(ROW_H))
            .text_color(TEXT())
            .child(
                // Registers the platform text-input handler once per frame for the whole editor.
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, input_entity.clone()), cx);
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(hscroll.child(list))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};

    fn setup<'a>(text: &str, cx: &'a mut TestAppContext) -> (Entity<Editor>, &'a mut VisualTestContext) {
        cx.update(|cx| bind_keys(cx));
        let text = text.to_string();
        let (editor, cx) = cx.add_window_view(move |_, cx| Editor::new(&text, None, cx));
        let handle = editor.read_with(cx, |e, _| e.focus_handle.clone());
        cx.update(|window, _| window.focus(&handle));
        cx.run_until_parked();
        (editor, cx)
    }

    fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
        editor.read_with(cx, |e, _| e.text())
    }

    fn mod_key() -> &'static str {
        if cfg!(target_os = "macos") { "cmd" } else { "ctrl" }
    }

    #[gpui::test]
    fn typing_including_korean_lands_in_the_buffer(cx: &mut TestAppContext) {
        let (editor, cx) = setup("", cx);
        cx.simulate_input("hello 안녕");
        assert_eq!(text(&editor, cx), "hello 안녕");
        assert_eq!(editor.read_with(cx, |e, _| e.cursor()), Pos::new(0, "hello 안녕".len()));
    }

    #[gpui::test]
    fn enter_keeps_indentation_and_backspace_removes_a_whole_character(cx: &mut TestAppContext) {
        let (editor, cx) = setup("", cx);
        cx.simulate_input("  fn");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("x");
        assert_eq!(text(&editor, cx), "  fn\n  x");
        cx.simulate_keystrokes("backspace backspace");
        assert_eq!(text(&editor, cx), "  fn\n ");

        let (editor, cx) = setup("", cx);
        cx.simulate_input("한글");
        cx.simulate_keystrokes("backspace");
        assert_eq!(text(&editor, cx), "한", "one keypress removes one syllable, not one byte");
    }

    #[gpui::test]
    fn arrows_select_and_typing_replaces_the_selection(cx: &mut TestAppContext) {
        let (editor, cx) = setup("hello world", cx);
        cx.simulate_keystrokes("secondary-a".replace("secondary", mod_key()).as_str());
        cx.simulate_input("X");
        assert_eq!(text(&editor, cx), "X");

        let (editor, cx) = setup("abc\ndef", cx);
        cx.simulate_keystrokes("shift-right shift-right shift-down");
        assert_eq!(editor.read_with(cx, |e, _| e.selected_text()), "abc\nde", "two right, then down keeps column 2");
        cx.simulate_keystrokes("left");
        assert_eq!(editor.read_with(cx, |e, _| e.cursor()), Pos::new(0, 0), "left collapses to the selection start");
    }

    #[gpui::test]
    fn vertical_movement_remembers_the_column_across_short_lines(cx: &mut TestAppContext) {
        let (editor, cx) = setup("abcdefgh\nab\nabcdefgh", cx);
        editor.update(cx, |e, cx| e.set_cursor(Pos::new(0, 6), false, cx));
        cx.simulate_keystrokes("down");
        assert_eq!(editor.read_with(cx, |e, _| e.cursor()), Pos::new(1, 2), "clamped to the short line");
        cx.simulate_keystrokes("down");
        assert_eq!(editor.read_with(cx, |e, _| e.cursor()), Pos::new(2, 6), "and back to the goal column");
    }

    #[gpui::test]
    fn undo_redo_and_save_round_trip_with_crlf_preserved(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join("maditor-native-test-editor");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.txt");
        std::fs::write(&path, "one\r\ntwo\r\n").unwrap();
        cx.update(|cx| bind_keys(cx));
        let p = path.clone();
        let (editor, cx) = cx.add_window_view(move |_, cx| {
            let text = std::fs::read_to_string(&p).unwrap();
            Editor::new(&text, Some(p), cx)
        });
        let handle = editor.read_with(cx, |e, _| e.focus_handle.clone());
        cx.update(|window, _| window.focus(&handle));
        cx.run_until_parked();

        assert!(!editor.read_with(cx, |e, _| e.is_dirty()));
        cx.simulate_input("X");
        assert!(editor.read_with(cx, |e, _| e.is_dirty()));
        cx.simulate_keystrokes(&format!("{}-z", mod_key()));
        assert_eq!(text(&editor, cx), "one\ntwo\n");
        cx.simulate_keystrokes(&format!("{}-shift-z", mod_key()));
        assert_eq!(text(&editor, cx), "Xone\ntwo\n");

        cx.simulate_keystrokes(&format!("{}-s", mod_key()));
        assert!(!editor.read_with(cx, |e, _| e.is_dirty()), "saving clears the dirty flag");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Xone\r\ntwo\r\n", "CRLF endings are kept");
    }

    #[gpui::test]
    fn ime_composition_is_replaced_by_the_committed_text(cx: &mut TestAppContext) {
        let (editor, cx) = setup("", cx);
        // Korean IME: 'ㅎ' is shown as marked text, then replaced by '하', then committed as '한'.
        for composing in ["ㅎ", "하"] {
            editor.update_in(cx, |e, window, cx| e.replace_and_mark_text_in_range(None, composing, None, window, cx));
            assert_eq!(text(&editor, cx), composing);
            assert!(editor.read_with(cx, |e, _| e.marked.is_some()), "still composing");
        }
        editor.update_in(cx, |e, window, cx| e.replace_text_in_range(None, "한", window, cx));
        assert_eq!(text(&editor, cx), "한");
        assert!(editor.read_with(cx, |e, _| e.marked.is_none()));
        assert_eq!(editor.read_with(cx, |e, _| e.cursor()), Pos::new(0, "한".len()));
    }

    #[gpui::test]
    fn utf16_offsets_round_trip_across_lines_and_wide_characters(cx: &mut TestAppContext) {
        let (editor, _cx) = setup("a😀b\n안녕\nxyz", cx);
        editor.read_with(_cx, |e, _| {
            for pos in [Pos::new(0, 0), Pos::new(0, 1), Pos::new(0, 5), Pos::new(0, 6), Pos::new(1, 3), Pos::new(2, 2)] {
                assert_eq!(e.utf16_to_pos(e.pos_to_utf16(pos)), pos, "{pos:?}");
            }
            // '😀' is two UTF-16 units, so 'b' starts at offset 3.
            assert_eq!(e.pos_to_utf16(Pos::new(0, 5)), 3);
        });
    }
}
