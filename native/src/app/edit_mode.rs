//! Edit mode: a project's file tree on the left, open files as tabs on the right. Editors and tree
//! state are kept per project, so switching projects never drops unsaved work.
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use gpui::{div, prelude::*, px, ClickEvent, Context, Entity, Focusable, IntoElement, Subscription, Window};

use super::{
    axis_locked, Confirm, ConfirmAction, Maditor, Resize, SelectNext, SelectPrev,
    TreeCollapse, TreeEnter, TreeExpand,
};
use crate::{
    editor::view::{Editor, EditorEvent},
    theme::*,
};

/// Files bigger than this aren't opened (the view isn't built for huge buffers yet).
const MAX_OPEN_BYTES: u64 = 2 * 1024 * 1024;

pub(super) struct Tab {
    pub path: PathBuf,
    pub editor: Entity<Editor>,
    _subscriptions: Vec<Subscription>,
}

#[derive(Default)]
pub(super) struct Workspace {
    pub tabs: Vec<Tab>,
    pub active: Option<usize>,
    pub expanded: HashSet<PathBuf>,
    pub selected: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TreeRow {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

/// Lists a directory: folders first, then files, each alphabetical ignoring case. `.git` and
/// `.DS_Store` are noise, not something to edit.
fn read_dir_sorted(dir: &Path) -> Vec<(PathBuf, String, bool)> {
    let mut entries: Vec<(PathBuf, String, bool)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name != ".git" && name != ".DS_Store").then(|| {
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                (e.path(), name, is_dir)
            })
        })
        .collect();
    entries.sort_by_key(|(_, name, is_dir)| (!*is_dir, name.to_lowercase()));
    entries
}

/// The visible rows of the tree: a folder's children appear only while it is expanded.
pub(super) fn build_tree_rows(root: &Path, expanded: &HashSet<PathBuf>) -> Vec<TreeRow> {
    fn walk(dir: &Path, depth: usize, expanded: &HashSet<PathBuf>, out: &mut Vec<TreeRow>) {
        for (path, name, is_dir) in read_dir_sorted(dir) {
            let open = is_dir && expanded.contains(&path);
            out.push(TreeRow { path: path.clone(), name, depth, is_dir, expanded: open });
            if open {
                walk(&path, depth + 1, expanded, out);
            }
        }
    }
    let mut rows = Vec::new();
    walk(root, 0, expanded, &mut rows);
    rows
}

impl Maditor {
    fn workspace(&self) -> Option<&Workspace> {
        self.workspaces.get(&self.repo)
    }

    fn workspace_mut(&mut self) -> &mut Workspace {
        self.workspaces.entry(self.repo.clone()).or_default()
    }

    pub(super) fn refresh_tree(&mut self) {
        let root = PathBuf::from(&self.repo);
        let expanded = self.workspace().map(|w| w.expanded.clone()).unwrap_or_default();
        self.tree_rows = if self.repo.is_empty() { Vec::new() } else { build_tree_rows(&root, &expanded) };
    }

    /// How many open files in `project` have unsaved changes.
    pub(super) fn dirty_tab_count(&self, project: &str, cx: &gpui::App) -> usize {
        self.workspaces
            .get(project)
            .map(|w| w.tabs.iter().filter(|t| t.editor.read(cx).is_dirty()).count())
            .unwrap_or(0)
    }

    pub(super) fn toggle_dir(&mut self, path: &Path, cx: &mut Context<Self>) {
        let ws = self.workspace_mut();
        if !ws.expanded.remove(path) {
            ws.expanded.insert(path.to_path_buf());
        }
        self.refresh_tree();
        cx.notify();
    }

    pub(super) fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.workspace().and_then(|w| w.tabs.iter().position(|t| t.path == path)) {
            self.activate_tab(ix, window, cx);
            return;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let opened = std::fs::metadata(&path)
            .map_err(|e| e.to_string())
            .and_then(|m| if m.len() > MAX_OPEN_BYTES { Err("file is too large to open".to_string()) } else { Ok(()) })
            .and_then(|_| std::fs::read(&path).map_err(|e| e.to_string()))
            .and_then(|bytes| String::from_utf8(bytes).map_err(|_| "not a UTF-8 text file".to_string()));
        let text = match opened {
            Ok(text) => text,
            Err(reason) => {
                self.error = Some(format!("Can't open {name}: {reason}"));
                cx.notify();
                return;
            }
        };
        self.error = None;

        let editor = cx.new(|cx| Editor::new(&text, Some(path.clone()), cx));
        let repaint = cx.observe(&editor, |_, _, cx| cx.notify());
        let failures = cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
            let EditorEvent::SaveFailed(message) = event;
            this.error = Some(message.clone());
            cx.notify();
        });
        let ws = self.workspace_mut();
        ws.tabs.push(Tab { path, editor, _subscriptions: vec![repaint, failures] });
        let ix = ws.tabs.len() - 1;
        self.activate_tab(ix, window, cx);
    }

    fn activate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let ws = self.workspace_mut();
        ws.active = Some(ix);
        ws.selected = Some(ws.tabs[ix].path.clone());
        let handle = ws.tabs[ix].editor.focus_handle(cx);
        window.focus(&handle);
        cx.notify();
    }

    /// Closing a tab with unsaved changes asks first; otherwise it just closes.
    pub(super) fn request_close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        let dirty = self.workspace().and_then(|w| w.tabs.get(ix)).map(|t| t.editor.read(cx).is_dirty()).unwrap_or(false);
        if !dirty {
            self.close_tab(ix, cx);
            return;
        }
        let name = self.workspace().map(|w| w.tabs[ix].path.file_name().unwrap_or_default().to_string_lossy().into_owned()).unwrap_or_default();
        self.confirm = Some(Confirm {
            title: "Unsaved changes".into(),
            message: format!("\"{name}\" has unsaved changes. Close it and discard them?"),
            label: "Discard".into(),
            action: ConfirmAction::CloseTab(ix),
        });
        cx.notify();
    }

    pub(super) fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        let ws = self.workspace_mut();
        if ix >= ws.tabs.len() {
            return;
        }
        ws.tabs.remove(ix);
        ws.active = match ws.active {
            _ if ws.tabs.is_empty() => None,
            Some(a) if a > ix => Some(a - 1),
            Some(a) if a == ix => Some(ix.min(ws.tabs.len() - 1)),
            other => other,
        };
        cx.notify();
    }

    fn move_tree_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.tree_rows.is_empty() {
            return;
        }
        let current = self
            .workspace()
            .and_then(|w| w.selected.as_ref())
            .and_then(|p| self.tree_rows.iter().position(|r| &r.path == p))
            .map(|i| i as isize)
            .unwrap_or(-1);
        let next = (current + delta).clamp(0, self.tree_rows.len() as isize - 1) as usize;
        let path = self.tree_rows[next].path.clone();
        self.tree_scroll.scroll_to_item(next);
        self.workspace_mut().selected = Some(path);
        cx.notify();
    }

    fn selected_row(&self) -> Option<TreeRow> {
        let selected = self.workspace()?.selected.as_ref()?;
        self.tree_rows.iter().find(|r| &r.path == selected).cloned()
    }

    fn tree_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected_row() {
            Some(row) if row.is_dir => self.toggle_dir(&row.path, cx),
            Some(row) => self.open_file(row.path, window, cx),
            None => {}
        }
    }

    fn tree_set_expanded(&mut self, expand: bool, cx: &mut Context<Self>) {
        if let Some(row) = self.selected_row() {
            if row.is_dir && row.expanded != expand {
                self.toggle_dir(&row.path, cx);
            }
        }
    }

    pub(super) fn tree_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.workspace().and_then(|w| w.selected.clone());
        let rows = self.tree_rows.iter().enumerate().map(|(i, row)| {
            let is_selected = selected.as_ref() == Some(&row.path);
            let path = row.path.clone();
            let is_dir = row.is_dir;
            div()
                .id(("tree", i))
                .flex()
                .items_center()
                .gap_1()
                .h(px(24.))
                .pl(px(8. + row.depth as f32 * 14.))
                .pr_2()
                .rounded_md()
                .text_xs()
                .cursor_pointer()
                .text_color(if is_dir { TEXT_STRONG() } else { TEXT() })
                .when(is_selected, |d| d.bg(SELECTED()))
                .hover(|d| d.bg(SELECTED()))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    window.focus(&this.tree_focus);
                    this.workspace_mut().selected = Some(path.clone());
                    if is_dir {
                        this.toggle_dir(&path, cx);
                    } else {
                        this.open_file(path.clone(), window, cx);
                    }
                }))
                .child(div().w(px(10.)).flex_none().text_color(TEXT_DIM()).child(if !row.is_dir {
                    ""
                } else if row.expanded {
                    "▾"
                } else {
                    "▸"
                }))
                .child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().child(row.name.clone()))
        });
        div()
            .w(px(self.sizes.tree))
            .flex_none()
            .flex()
            .flex_col()
            .bg(PANEL())
            .border_r_1()
            .border_color(BORDER())
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(BORDER_SOFT())
                    .child(div().text_color(TEXT_STRONG()).child(Self::project_name(&self.repo)))
                    .child(div().text_xs().text_color(TEXT_DIM()).child("Files")),
            )
            .child(
                axis_locked(div().id("tree-list"))
                    .key_context("NavList FileTree")
                    .track_focus(&self.tree_focus)
                    .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_tree_selection(-1, cx)))
                    .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_tree_selection(1, cx)))
                    .on_action(cx.listener(|this, _: &TreeEnter, window, cx| this.tree_enter(window, cx)))
                    .on_action(cx.listener(|this, _: &TreeExpand, _, cx| this.tree_set_expanded(true, cx)))
                    .on_action(cx.listener(|this, _: &TreeCollapse, _, cx| this.tree_set_expanded(false, cx)))
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.tree_scroll)
                    .p_2()
                    .children(rows),
            )
    }

    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace();
        let active = ws.and_then(|w| w.active);
        let tabs = ws.into_iter().flat_map(|w| w.tabs.iter().enumerate()).map(|(i, tab)| {
            let name = tab.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let dirty = tab.editor.read(cx).is_dirty();
            let is_active = active == Some(i);
            div()
                .id(("tab", i))
                .flex()
                .items_center()
                .gap_2()
                .h_full()
                .px_3()
                .border_r_1()
                .border_color(BORDER())
                .text_xs()
                .cursor_pointer()
                .when(is_active, |d| d.bg(BG()).text_color(TEXT_STRONG()))
                .when(!is_active, |d| d.text_color(TEXT_DIM()).hover(|d| d.bg(SELECTED())))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.activate_tab(i, window, cx)))
                .child(name)
                .child(
                    div()
                        .id(("tab-close", i))
                        .w(px(14.))
                        .flex()
                        .justify_center()
                        .rounded_sm()
                        .text_color(if dirty { AMBER() } else { TEXT_DIM() })
                        .hover(|d| d.bg(BORDER()).text_color(TEXT_STRONG()))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.request_close_tab(i, cx);
                        }))
                        .child(if dirty { "●" } else { "×" }),
                )
        });
        div()
            .flex_none()
            .h(px(34.))
            .flex()
            .bg(CHROME())
            .border_b_1()
            .border_color(BORDER())
            .children(tabs)
    }

    pub(super) fn edit_body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let active = self.workspace().and_then(|w| w.active.and_then(|i| w.tabs.get(i)));
        let area = div().flex_1().min_w_0().flex().flex_col().child(self.tab_bar(cx)).child(match active {
            Some(tab) => div().flex_1().min_h_0().child(tab.editor.clone()).into_any_element(),
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(TEXT_DIM())
                .child("Open a file from the tree")
                .into_any_element(),
        });
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .child(self.tree_panel(cx))
            .child(self.resize_handle(Resize::Tree, cx))
            .child(area)
            .into_any_element()
    }
}
