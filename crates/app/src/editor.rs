//! The text editor: a view of a `Document` with a gutter, caret, selection and input handling.
use std::{cell::Cell, path::PathBuf, rc::Rc, time::Instant};

use gyeol::{div, list_rows, text, Cx, Element, Event, Ime, Key, MouseEvent, NamedKey, Rect, Shaper, TextStyle};
use madi_text::{find_in_line, Document, Pos};

use crate::{
    app::{Focus, Madi},
    find::match_color,
    theme::Palette,
};

pub const MONO: &str = "Menlo";
pub const GUTTER_W: f32 = 56.;
const RIGHT_MARGIN: f32 = 80.;

type El = Element<Madi>;

pub struct Editor {
    pub doc: Document,
    pub path: PathBuf,
    /// The widest line in columns, cached for one document revision.
    cols: Cell<(u64, usize)>,
    /// The installed plugin's highlight/symbol rules for this file's extension, if any.
    pub language: Option<Rc<madi_plugins::Language>>,
}

impl Editor {
    pub fn new(text: &str, path: PathBuf, language: Option<Rc<madi_plugins::Language>>) -> Editor {
        Editor { doc: Document::new(text), path, cols: Cell::new((u64::MAX, 0)), language }
    }

    pub fn row_h(size: f32) -> f32 {
        (size * 1.55).round()
    }

    pub fn text_style(size: f32) -> TextStyle {
        TextStyle::new(size).family(MONO)
    }

    /// Ids of the two scroll containers: the lines (vertical) inside the wide area (horizontal).
    fn lines_id(&self) -> (&'static str, &PathBuf) {
        ("editor-lines", &self.path)
    }

    fn x_id(&self) -> (&'static str, &PathBuf) {
        ("editor-x", &self.path)
    }

    /// The widest line in columns; wide characters count double.
    fn max_cols(&self) -> usize {
        let (revision, cols) = self.cols.get();
        if revision == self.doc.revision() {
            return cols;
        }
        let cols = (0..self.doc.line_count())
            .map(|r| self.doc.line(r).chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum::<usize>())
            .max()
            .unwrap_or(0);
        self.cols.set((self.doc.revision(), cols));
        cols
    }
}

// ---- drawing ---------------------------------------------------------------------------------

pub fn view(ed: &Editor, cx: &mut Cx, p: &Palette, size: f32, caret_visible: bool, find: &str) -> El {
    let row_h = Editor::row_h(size);
    let style = Editor::text_style(size);
    let char_w = cx.shaper.width("M", style);
    let content_w = GUTTER_W + ed.max_cols() as f32 * char_w + RIGHT_MARGIN;
    let viewport_w = cx.viewport(ed.x_id()).map_or(0., |v| v.0);
    let range = cx.visible_rows(ed.lines_id(), ed.doc.line_count(), row_h);
    let rows: Vec<El> = range.clone().map(|row| line_row(ed, row, size, row_h, char_w, p, caret_visible, find, cx.shaper)).collect();

    let lines = list_rows(ed.lines_id(), ed.doc.line_count(), row_h, range, rows)
        .w(content_w.max(viewport_w))
        .h_full()
        .on_mouse_down(|s: &mut Madi, cx, e| s.editor_mouse(cx, e, true))
        .on_drag(|s: &mut Madi, cx, e| s.editor_mouse(cx, e, false));
    div().id(ed.x_id()).grow().overflow_x_scroll().bg(p.bg).text_size(size).text_family(MONO).text_color(p.text).child(lines)
}

#[allow(clippy::too_many_arguments)]
fn line_row(ed: &Editor, row: usize, size: f32, row_h: f32, char_w: f32, p: &Palette, caret_visible: bool, find: &str, shaper: &mut Shaper) -> El {
    let doc = &ed.doc;
    let line = doc.line(row);
    let style = Editor::text_style(size);
    let mut area = div().row().items_center().grow().h(row_h);

    for m in find_in_line(line, find) {
        let x0 = shaper.caret_x(line, style, m.start);
        let x1 = shaper.caret_x(line, style, m.end);
        area = area.child(div().absolute().left(x0).top(0.).w((x1 - x0).max(0.)).h(row_h).bg(match_color(p)));
    }
    // Selected part of this line (a selection that continues past the line's end also covers the newline).
    let (a, b) = doc.selection();
    if a != b && row >= a.row && row <= b.row {
        let from = if row == a.row { a.col } else { 0 };
        let to = if row == b.row { b.col } else { line.len() };
        let x0 = shaper.caret_x(line, style, from);
        let x1 = shaper.caret_x(line, style, to) + if row < b.row { char_w } else { 0. };
        area = area.child(div().absolute().left(x0).top(0.).w((x1 - x0).max(0.)).h(row_h).bg(p.amber.with_alpha(0.25)));
    }
    // The text an input method is still composing is underlined.
    if let Some((ma, mb)) = doc.marked() {
        if row >= ma.row && row <= mb.row {
            let from = if row == ma.row { ma.col } else { 0 };
            let to = if row == mb.row { mb.col } else { line.len() };
            let x0 = shaper.caret_x(line, style, from);
            let x1 = shaper.caret_x(line, style, to);
            area = area.child(div().absolute().left(x0).top(row_h - 3.).w((x1 - x0).max(0.)).h(1.5).bg(p.text));
        }
    }
    if let Some(lang) = &ed.language {
        for (range, scope) in lang.tokenize(line) {
            area = area.child(text(line[range].to_string()).text_color(scope.map_or(p.text, |s| p.syntax_color(s))));
        }
    } else {
        area = area.child(text(line.to_string()));
    }
    let caret = doc.cursor();
    if caret_visible && caret.row == row {
        let x = shaper.caret_x(line, style, caret.col);
        area = area.child(div().absolute().left(x).top(2.).w(1.5).h(row_h - 4.).bg(p.text_strong));
    }

    let gutter = div().row().w(GUTTER_W).justify_end().child(text((row + 1).to_string()).text_color(p.text_dim)).child(div().w(12.));
    div().row().child(gutter).child(area)
}

// ---- input -----------------------------------------------------------------------------------

/// The word around byte `col` of `line` (letters, digits and `_`), as byte offsets.
pub(crate) fn word_bounds(line: &str, col: usize) -> (usize, usize) {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let col = col.min(line.len());
    let mut start = col;
    while let Some(c) = line[..start].chars().next_back().filter(|c| is_word(*c)) {
        start -= c.len_utf8();
    }
    let mut end = col;
    while let Some(c) = line[end..].chars().next().filter(|c| is_word(*c)) {
        end += c.len_utf8();
    }
    (start, end)
}

impl Madi {
    /// A mouse press (`down`) or drag on the text: places the caret, or extends the selection.
    pub fn editor_mouse(&mut self, cx: &mut Cx, e: MouseEvent, down: bool) {
        self.focus = Focus::Editor;
        let size = self.config.font_size();
        let (row_h, style) = (Editor::row_h(size), Editor::text_style(size));
        let Some(ed) = self.active_editor_mut() else { return };
        let scroll_y = cx.scroll_offset(ed.lines_id()).1;
        let row = (((e.local.1 + scroll_y) / row_h).floor().max(0.) as usize).min(ed.doc.line_count() - 1);
        let col = cx.shaper.hit(ed.doc.line(row), style, e.local.0 - GUTTER_W);
        let pos = Pos::new(row, col);
        if !down {
            ed.doc.set_cursor(pos, true);
        } else {
            match e.click_count {
                1 => ed.doc.set_cursor(pos, e.modifiers.shift),
                2 => {
                    let (a, b) = word_bounds(ed.doc.line(row), col);
                    ed.doc.set_cursor(Pos::new(row, a), false);
                    ed.doc.set_cursor(Pos::new(row, b), true);
                }
                _ => {
                    let end = ed.doc.line(row).len();
                    ed.doc.set_cursor(Pos::new(row, 0), false);
                    ed.doc.set_cursor(Pos::new(row, end), true);
                }
            }
        }
        self.touched(cx);
    }

    /// After the document changed or the caret moved: restart the blink and keep the caret in view.
    pub fn touched(&mut self, cx: &mut Cx) {
        self.blink_epoch = Instant::now();
        // Editing a preview tab makes it a regular one.
        if let Some(ws) = self.workspaces.get_mut(&self.repo) {
            if let Some(tab) = ws.active.and_then(|i| ws.tabs.get_mut(i)) {
                if tab.editor.doc.is_dirty() {
                    tab.preview = false;
                }
            }
        }
        let size = self.config.font_size();
        let (row_h, style) = (Editor::row_h(size), Editor::text_style(size));
        let Some(ed) = self.active_editor() else { return };
        let caret = ed.doc.cursor();
        let x = cx.shaper.caret_x(ed.doc.line(caret.row), style, caret.col);
        cx.scroll_to_reveal(ed.lines_id(), Rect::new(0., caret.row as f32 * row_h, 1., row_h));
        cx.scroll_to_reveal(ed.x_id(), Rect::new((GUTTER_W + x - 24.).max(0.), 0., 72., 1.));
    }

    /// Input-method events: composition shows provisional text, a commit inserts the final text.
    pub fn editor_ime(&mut self, ime: &Ime, cx: &mut Cx) {
        let Some(ed) = self.active_editor_mut() else { return };
        match ime {
            Ime::Preedit { text, .. } => ed.doc.replace_and_mark_utf16(None, text, None),
            Ime::Commit(text) => {
                ed.doc.replace_utf16(None, text);
                ed.doc.unmark();
            }
        }
        self.touched(cx);
    }

    /// Keyboard editing, with the editor's shortcuts. While an input method composes, it owns the keys.
    pub fn editor_key(&mut self, key: &Key, text: Option<&str>, cx: &mut Cx) {
        let size = self.config.font_size();
        let row_h = Editor::row_h(size);
        let m = cx.modifiers;
        let (cmd, shift) = (m.command(), m.shift);
        let mac = cfg!(target_os = "macos");
        let word = if mac { m.alt } else { m.ctrl };
        let Some(ed) = self.active_editor_mut() else { return };
        if ed.doc.marked().is_some() {
            return;
        }
        let page = cx.viewport(ed.lines_id()).map_or(20, |v| (v.1 / row_h).floor().max(1.) as isize);
        let doc = &mut ed.doc;
        match key {
            Key::Named(NamedKey::Backspace) if cmd && mac => doc.delete_to_line_start(),
            Key::Named(NamedKey::Backspace) => doc.backspace(),
            Key::Named(NamedKey::Delete) => doc.delete(),
            Key::Named(NamedKey::Enter) => doc.newline(),
            Key::Named(NamedKey::Tab) => doc.tab(),
            Key::Named(NamedKey::ArrowLeft) if word => doc.move_word_left(shift),
            Key::Named(NamedKey::ArrowRight) if word => doc.move_word_right(shift),
            Key::Named(NamedKey::ArrowLeft) if cmd && mac => doc.move_line_start(shift),
            Key::Named(NamedKey::ArrowRight) if cmd && mac => doc.move_line_end(shift),
            Key::Named(NamedKey::ArrowLeft) => doc.move_left(shift),
            Key::Named(NamedKey::ArrowRight) => doc.move_right(shift),
            Key::Named(NamedKey::ArrowUp) if cmd => doc.move_doc_start(),
            Key::Named(NamedKey::ArrowDown) if cmd => doc.move_doc_end(),
            Key::Named(NamedKey::ArrowUp) => doc.move_vertical(-1, shift),
            Key::Named(NamedKey::ArrowDown) => doc.move_vertical(1, shift),
            Key::Named(NamedKey::Home) => doc.move_line_start(shift),
            Key::Named(NamedKey::End) => doc.move_line_end(shift),
            Key::Named(NamedKey::PageUp) => doc.move_vertical(-page, shift),
            Key::Named(NamedKey::PageDown) => doc.move_vertical(page, shift),
            Key::Char(c) if cmd => match c.to_ascii_lowercase().as_str() {
                "a" => doc.select_all(),
                "c" => cx.set_clipboard_text(&doc.selected_text()),
                "x" => {
                    if let Some(cut) = doc.cut() {
                        cx.set_clipboard_text(&cut);
                    }
                }
                "v" => {
                    if let Some(pasted) = cx.clipboard_text() {
                        doc.paste(&pasted);
                    }
                }
                "z" if shift => {
                    doc.redo();
                }
                "z" => {
                    doc.undo();
                }
                "s" => return self.save_active(cx),
                _ => return,
            },
            _ if cmd => return,
            _ => match text {
                Some("(") => doc.insert_pair('(', ')'),
                Some("[") => doc.insert_pair('[', ']'),
                Some("{") => doc.insert_pair('{', '}'),
                Some("\"") => doc.insert_quote('"'),
                Some("'") => doc.insert_quote('\''),
                Some(")") => doc.insert_closer(')'),
                Some("]") => doc.insert_closer(']'),
                Some("}") => doc.insert_closer('}'),
                Some(t) => doc.insert(t, true),
                None => return,
            },
        }
        self.touched(cx);
    }

    /// Writes the active file to disk (keeping its line endings).
    pub fn save_active(&mut self, cx: &mut Cx) {
        let Some(ed) = self.active_editor_mut() else { return };
        let name = ed.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::write(&ed.path, ed.doc.text_for_save()) {
            Ok(()) => {
                ed.doc.mark_saved();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Can't save {name}: {e}")),
        }
        self.touched(cx);
    }

    /// Routes an input event to the editor when it has the keyboard.
    pub fn editor_event(&mut self, event: &Event, cx: &mut Cx) {
        if self.focus != Focus::Editor {
            return;
        }
        match event {
            Event::KeyDown { key, text, .. } => self.editor_key(key, text.as_deref(), cx),
            Event::Ime(ime) => self.editor_ime(ime, cx),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_found_around_the_caret_including_korean() {
        assert_eq!(word_bounds("let foo_bar = 1;", 6), (4, 11));
        assert_eq!(word_bounds("hello 한글 world", "hello 한".len()), (6, "hello 한글".len()));
        assert_eq!(word_bounds("a  b", 2), (2, 2), "between spaces there is no word");
        assert_eq!(word_bounds("", 0), (0, 0));
    }
}
