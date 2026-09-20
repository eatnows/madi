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

use madi_text::{Document, Pos};
use madi_ui::theme::*;

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
const PAGE_LINES: isize = 30;

pub struct Editor {
    focus_handle: FocusHandle,
    doc: Document,
    pub path: Option<PathBuf>,
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
        Self {
            focus_handle: cx.focus_handle(),
            doc: Document::new(text),
            path,
            scroll: UniformListScrollHandle::new(),
            hscroll: ScrollHandle::new(),
            layouts: RefCell::new(HashMap::new()),
            is_selecting: false,
            reveal_cursor: false,
            content_width: 0.,
            width_revision: u64::MAX,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.doc.is_dirty()
    }

    /// 1-based line and column (in characters) of the caret, for the status bar.
    pub fn cursor_display(&self) -> (usize, usize) {
        self.doc.cursor_display()
    }

    pub fn line_ending(&self) -> &'static str {
        self.doc.line_ending()
    }

    #[cfg(test)]
    pub fn text(&self) -> String {
        self.doc.text()
    }

    #[cfg(test)]
    pub fn cursor(&self) -> Pos {
        self.doc.cursor()
    }

    /// Writes the document to its path (restoring CRLF line endings if the file had them).
    pub fn save(&mut self, cx: &mut Context<Self>) -> std::io::Result<()> {
        let Some(path) = self.path.clone() else { return Ok(()) };
        std::fs::write(path, self.doc.text_for_save())?;
        self.doc.mark_saved();
        cx.notify();
        Ok(())
    }

    /// After the document changed or the caret moved: keep the caret in view and repaint.
    fn touched(&mut self, cx: &mut Context<Self>) {
        self.reveal_cursor = true;
        self.scroll.scroll_to_item(self.doc.cursor().row, ScrollStrategy::Center);
        cx.notify();
    }

    fn content_width(&mut self) -> f32 {
        if self.width_revision != self.doc.revision() {
            let cols = (0..self.doc.line_count())
                .map(|r| self.doc.line(r).chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum::<usize>())
                .max()
                .unwrap_or(0);
            self.content_width = GUTTER_W + cols as f32 * CHAR_W + 80.0;
            self.width_revision = self.doc.revision();
        }
        self.content_width
    }

    // ---- actions: each forwards to the document, then keeps the caret visible ----------------------

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_left(false);
        self.touched(cx);
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_right(false);
        self.touched(cx);
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_vertical(-1, false);
        self.touched(cx);
    }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_vertical(1, false);
        self.touched(cx);
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_left(true);
        self.touched(cx);
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_right(true);
        self.touched(cx);
    }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_vertical(-1, true);
        self.touched(cx);
    }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_vertical(1, true);
        self.touched(cx);
    }
    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_vertical(-PAGE_LINES, false);
        self.touched(cx);
    }
    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_vertical(PAGE_LINES, false);
        self.touched(cx);
    }
    fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_line_start(false);
        self.touched(cx);
    }
    fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_line_end(false);
        self.touched(cx);
    }
    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_line_start(true);
        self.touched(cx);
    }
    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_line_end(true);
        self.touched(cx);
    }
    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_doc_start();
        self.touched(cx);
    }
    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_doc_end();
        self.touched(cx);
    }
    fn word_left_action(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_word_left(false);
        self.touched(cx);
    }
    fn word_right_action(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_word_right(false);
        self.touched(cx);
    }
    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_word_left(true);
        self.touched(cx);
    }
    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.move_word_right(true);
        self.touched(cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.select_all();
        cx.notify();
    }
    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.backspace();
        self.touched(cx);
    }
    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.delete();
        self.touched(cx);
    }
    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.newline();
        self.touched(cx);
    }
    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.tab();
        self.touched(cx);
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.doc.selected_text();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.doc.cut() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.touched(cx);
        }
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            self.doc.paste(&text);
            self.touched(cx);
        }
    }
    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.undo() {
            self.touched(cx);
        }
    }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.doc.redo() {
            self.touched(cx);
        }
    }
    fn save_action(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = self.save(cx) {
            let name = self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            cx.emit(EditorEvent::SaveFailed(format!("Couldn't save {name}: {e}")));
        }
    }

    // ---- mouse -----------------------------------------------------------------------------------

    fn pos_for_point(&self, position: Point<Pixels>) -> Pos {
        let layouts = self.layouts.borrow();
        let hit = layouts.iter().find(|(_, (_, b))| position.y >= b.top() && position.y < b.bottom());
        match hit {
            Some((row, (line, bounds))) => Pos::new(*row, line.closest_index_for_x(position.x - bounds.left())),
            None => {
                // Above/below the painted rows: snap to the nearest end of the visible range.
                let first = layouts.keys().min().copied().unwrap_or(0);
                let last = layouts.keys().max().copied().unwrap_or(0);
                match layouts.get(&first) {
                    Some((_, b)) if position.y < b.top() => Pos::new(first, 0),
                    _ => Pos::new(last, usize::MAX),
                }
            }
        }
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        let pos = self.pos_for_point(ev.position);
        self.doc.set_cursor(pos, ev.modifiers.shift);
        cx.notify();
    }
    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let pos = self.pos_for_point(ev.position);
            self.doc.set_cursor(pos, true);
            cx.notify();
        }
    }
    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
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
        let (text, actual) = self.doc.text_in_utf16(range_utf16);
        actual_range.replace(actual);
        Some(text)
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let (range, reversed) = self.doc.selection_utf16();
        Some(UTF16Selection { range, reversed })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.doc.marked_utf16()
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.doc.unmark();
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.doc.replace_utf16(range_utf16, text);
        self.touched(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.doc.replace_and_mark_utf16(range_utf16, text, new_selected_range_utf16);
        self.reveal_cursor = true;
        cx.notify();
    }

    fn bounds_for_range(&mut self, range_utf16: Range<usize>, _bounds: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let start = self.doc.utf16_to_pos(range_utf16.start);
        let end = self.doc.utf16_to_pos(range_utf16.end);
        let layouts = self.layouts.borrow();
        let (line, bounds) = layouts.get(&start.row)?;
        let end_col = if end.row == start.row { end.col } else { self.doc.line(start.row).len() };
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(start.col), bounds.top()),
            point(bounds.left() + line.x_for_index(end_col), bounds.bottom()),
        ))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let clamped = self.doc.clamp(self.pos_for_point(p));
        Some(self.doc.pos_to_utf16(clamped))
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
        let text: SharedString = editor.doc.line(self.row).to_string().into();
        let (sel_a, sel_b) = editor.doc.selection();
        let style = window.text_style();
        let base = TextRun {
            len: text.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match editor.doc.marked().filter(|(a, b)| a.row == self.row && b.row == self.row) {
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
        let cursor_x = (editor.doc.cursor().row == row).then(|| line.x_for_index(editor.doc.cursor().col));
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
            if editor.reveal_cursor && editor.doc.cursor().row == row {
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
            self.doc.line_count(),
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
        assert_eq!(editor.read_with(cx, |e, _| e.doc.selected_text()), "abc\nde", "two right, then down keeps column 2");
        cx.simulate_keystrokes("left");
        assert_eq!(editor.read_with(cx, |e, _| e.cursor()), Pos::new(0, 0), "left collapses to the selection start");
    }

    #[gpui::test]
    fn undo_redo_and_save_round_trip_with_crlf_preserved(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join("madi-test-editor");
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
            assert!(editor.read_with(cx, |e, _| e.doc.marked().is_some()), "still composing");
        }
        editor.update_in(cx, |e, window, cx| e.replace_text_in_range(None, "한", window, cx));
        assert_eq!(text(&editor, cx), "한");
        assert!(editor.read_with(cx, |e, _| e.doc.marked().is_none()));
        assert_eq!(editor.read_with(cx, |e, _| e.cursor()), Pos::new(0, "한".len()));
    }

}
