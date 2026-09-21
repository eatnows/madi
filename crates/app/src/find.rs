//! Find and replace in the active file: a bar above the editor, with every match highlighted.
use gyeol::{div, text, Color, Cx, Element, Ime, Key, NamedKey};

use crate::{
    app::{Focus, Madi},
    editor,
    theme::Palette,
};

type El = Element<Madi>;

#[derive(Default)]
pub struct Find {
    pub query: String,
    pub replacement: String,
    pub replace_open: bool,
    /// Which field typing goes to.
    pub in_replace: bool,
}

impl Madi {
    /// Shows the bar (the selection, if it is on one line, becomes the query) and jumps to a match.
    pub fn open_find(&mut self, replace: bool, cx: &mut Cx) {
        let Some(ed) = self.active_editor() else { return };
        let selected = ed.doc.selected_text();
        let mut find = self.find.take().unwrap_or_default();
        if !selected.is_empty() && !selected.contains('\n') {
            find.query = selected;
        }
        find.replace_open |= replace;
        find.in_replace = false;
        self.find = Some(find);
        self.focus = Focus::Find;
        self.find_step(true, true, cx);
    }

    pub fn close_find(&mut self) {
        self.find = None;
        self.focus = Focus::Editor;
    }

    /// Selects the next/previous match of the query. `keep_current` is for a query that just changed.
    fn find_step(&mut self, forward: bool, keep_current: bool, cx: &mut Cx) {
        let Some(query) = self.find.as_ref().map(|f| f.query.clone()) else { return };
        let Some(ed) = self.active_editor_mut() else { return };
        if ed.doc.select_match(&query, forward, keep_current) {
            self.touched(cx);
        }
    }

    /// Replaces the selected match (if the selection is one) and moves on to the next.
    fn replace_current(&mut self, cx: &mut Cx) {
        let Some((query, replacement)) = self.find.as_ref().map(|f| (f.query.clone(), f.replacement.clone())) else { return };
        let Some(ed) = self.active_editor_mut() else { return };
        ed.doc.replace_match(&query, &replacement);
        ed.doc.select_match(&query, true, false);
        self.touched(cx);
    }

    fn replace_all(&mut self, cx: &mut Cx) {
        let Some((query, replacement)) = self.find.as_ref().map(|f| (f.query.clone(), f.replacement.clone())) else { return };
        let Some(ed) = self.active_editor_mut() else { return };
        ed.doc.replace_all(&query, &replacement);
        self.touched(cx);
    }

    fn find_type(&mut self, input: &str, cx: &mut Cx) {
        let Some(find) = &mut self.find else { return };
        let in_replace = find.in_replace;
        let field = if in_replace { &mut find.replacement } else { &mut find.query };
        field.push_str(input);
        if !in_replace {
            self.find_step(true, true, cx);
        }
    }

    fn find_owns_keys(&self) -> bool {
        self.focus == Focus::Find && self.find.is_some() && self.active_editor().is_some()
    }

    /// Keys while the bar has focus; shortcuts with Cmd/Ctrl fall through to the app except paste and save.
    pub fn find_key(&mut self, key: &Key, text: Option<&str>, cx: &mut Cx) -> bool {
        if !self.find_owns_keys() {
            return false;
        }
        let (cmd, shift) = (cx.modifiers.command(), cx.modifiers.shift);
        let in_replace = self.find.as_ref().is_some_and(|f| f.in_replace);
        match key {
            Key::Named(NamedKey::Escape) => self.close_find(),
            Key::Named(NamedKey::Enter) if cmd => self.replace_all(cx),
            Key::Named(NamedKey::Enter) if in_replace => self.replace_current(cx),
            Key::Named(NamedKey::Enter) => self.find_step(!shift, false, cx),
            Key::Named(NamedKey::Tab) => {
                if let Some(find) = self.find.as_mut().filter(|f| f.replace_open) {
                    find.in_replace = !find.in_replace;
                }
            }
            Key::Named(NamedKey::Backspace) => {
                if let Some(find) = &mut self.find {
                    if find.in_replace {
                        find.replacement.pop();
                    } else {
                        find.query.pop();
                        self.find_step(true, true, cx);
                    }
                }
            }
            Key::Char(c) if cmd && c.eq_ignore_ascii_case("v") => {
                if let Some(pasted) = cx.clipboard_text() {
                    self.find_type(pasted.lines().next().unwrap_or(""), cx);
                }
            }
            Key::Char(c) if cmd && c.eq_ignore_ascii_case("s") => self.save_active(cx),
            _ if cmd => return false,
            _ => {
                if let Some(input) = text {
                    self.find_type(input, cx);
                }
            }
        }
        true
    }

    pub fn find_ime(&mut self, ime: &Ime, cx: &mut Cx) -> bool {
        if !self.find_owns_keys() {
            return false;
        }
        if let Ime::Commit(input) = ime {
            self.find_type(input, cx);
        }
        true
    }

    /// The query to highlight in the editor (empty when the bar is closed).
    pub fn find_query(&self) -> &str {
        self.find.as_ref().map_or("", |f| f.query.as_str())
    }

    fn find_count_label(&self) -> String {
        let (Some(find), Some(ed)) = (&self.find, self.active_editor()) else { return String::new() };
        if find.query.is_empty() {
            return String::new();
        }
        let matches = ed.doc.find_all(&find.query);
        if matches.is_empty() {
            return "No results".into();
        }
        let selected = ed.doc.selection();
        match matches.iter().position(|m| *m == selected) {
            Some(ix) => format!("{} of {}", ix + 1, matches.len()),
            None => format!("{} results", matches.len()),
        }
    }

    pub fn find_bar(&self, p: &Palette) -> Option<El> {
        let find = self.find.as_ref()?;
        self.active_editor()?;
        let focused = self.focus == Focus::Find;
        let field = |value: &str, placeholder: &str, active: bool, replace: bool| -> El {
            let shown = if value.is_empty() && !active { placeholder.to_string() } else if active { format!("{value}│") } else { value.to_string() };
            div().grow().px(8.).py(4.).rounded(4.).border(1., if active { p.amber } else { p.border }).bg(p.bg)
                .on_click(move |s: &mut Madi, _| {
                    s.focus = Focus::Find;
                    if let Some(find) = &mut s.find {
                        find.in_replace = replace;
                    }
                })
                .child(text(shown).text_size(12.).text_family(editor::MONO).text_color(if value.is_empty() && !active { p.text_dim } else { p.text_strong }))
        };
        let button = |label: &str, action: fn(&mut Madi, &mut Cx)| -> El {
            div().px(8.).py(4.).rounded(4.).hover_bg(p.selected).on_click(move |s: &mut Madi, cx| action(s, cx)).child(text(label.to_string()).text_size(12.).text_color(p.text_strong))
        };
        let mut bar = div().col().gap(4.).px(8.).py(6.).bg(p.chrome).child(
            div().row().items_center().gap(6.)
                .child(button(if find.replace_open { "▾" } else { "▸" }, |s, _| {
                    if let Some(find) = &mut s.find {
                        find.replace_open = !find.replace_open;
                        find.in_replace &= find.replace_open;
                    }
                }))
                .child(field(&find.query, "Find", focused && !find.in_replace, false))
                .child(text(self.find_count_label()).text_size(12.).text_color(p.text_dim))
                .child(button("↑", |s, cx| s.find_step(false, false, cx)))
                .child(button("↓", |s, cx| s.find_step(true, false, cx)))
                .child(button("×", |s, _| s.close_find())),
        );
        if find.replace_open {
            bar = bar.child(
                div().row().items_center().gap(6.)
                    .child(div().w(21.))
                    .child(field(&find.replacement, "Replace", focused && find.in_replace, true))
                    .child(button("Replace", |s, cx| s.replace_current(cx)))
                    .child(button("All", |s, cx| s.replace_all(cx))),
            );
        }
        Some(div().col().child(bar).child(div().h(1.).bg(p.border)))
    }
}

/// Fill for the matches other than the selected one.
pub fn match_color(p: &Palette) -> Color {
    p.amber.with_alpha(0.16)
}
