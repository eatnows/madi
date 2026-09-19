//! A project's workspace: its file tree and open tabs. A tab is either a file in the editor or a
//! read-only diff, so reviewing a worktree's changes happens in the same place as editing. Editors
//! and tree state are kept per project, so switching projects never drops unsaved work.
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{
    div, prelude::*, px, ClickEvent, Context, Entity, Focusable, IntoElement, ScrollHandle,
    Subscription, UniformListScrollHandle, Window,
};

use maditor_git::diff_layout::DiffLayout;
use maditor_project::tree::{build_tree_rows, TreeRow};

use super::{
    Confirm, ConfirmAction, Maditor, SelectNext, SelectPrev, TreeCollapse, TreeEnter, TreeExpand,
};
use maditor_ui::{diff_view::diff_view, scroll::axis_locked, theme::*};

use crate::editor::view::{Editor, EditorEvent};

/// Files bigger than this aren't opened (the view isn't built for huge buffers yet).
const MAX_OPEN_BYTES: u64 = 2 * 1024 * 1024;

/// Identity of a tab, so opening the same thing again focuses it instead of duplicating it.
#[derive(Clone, PartialEq)]
pub(super) enum TabKey {
    File(PathBuf),
    Diff { worktree: String, path: String, section: &'static str },
}

pub(super) struct DiffTab {
    pub data: Rc<DiffLayout>,
    pub vscroll: UniformListScrollHandle,
    pub hscroll: ScrollHandle,
    pub subtitle: String,
    pub additions: usize,
    pub deletions: usize,
}

pub(super) enum TabBody {
    File(Entity<Editor>),
    Diff(DiffTab),
}

pub(super) struct Tab {
    pub key: TabKey,
    pub title: String,
    /// A preview tab (italic) is replaced by the next thing opened in preview; editing it, or a
    /// double-click, makes it a regular tab. Keeps arrow-key browsing from littering the tab bar.
    pub preview: bool,
    pub body: TabBody,
    _subscriptions: Vec<Subscription>,
}

#[derive(Default)]
pub(super) struct Workspace {
    pub tabs: Vec<Tab>,
    pub active: Option<usize>,
    pub expanded: HashSet<PathBuf>,
    pub selected: Option<PathBuf>,
}

impl Maditor {
    pub(super) fn workspace(&self) -> Option<&Workspace> {
        self.workspaces.get(&self.repo)
    }

    pub(super) fn workspace_mut(&mut self) -> &mut Workspace {
        self.workspaces.entry(self.repo.clone()).or_default()
    }

    pub(super) fn active_tab(&self) -> Option<&Tab> {
        let ws = self.workspace()?;
        ws.tabs.get(ws.active?)
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
            .map(|w| w.tabs.iter().filter(|t| matches!(&t.body, TabBody::File(e) if e.read(cx).is_dirty())).count())
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

    // ---- tabs ----------------------------------------------------------------------------------

    /// Adds `tab` (or reuses the tab with the same key) and returns its index. A preview open
    /// replaces the current preview tab; a regular open pins whatever it lands on.
    fn install_tab(&mut self, mut tab: Tab, preview: bool) -> usize {
        let ws = self.workspace_mut();
        if let Some(ix) = ws.tabs.iter().position(|t| t.key == tab.key) {
            if !preview {
                ws.tabs[ix].preview = false;
            }
            return ix;
        }
        tab.preview = preview;
        if preview {
            if let Some(ix) = ws.tabs.iter().position(|t| t.preview) {
                ws.tabs[ix] = tab;
                return ix;
            }
        }
        ws.tabs.push(tab);
        ws.tabs.len() - 1
    }

    pub(super) fn open_file(&mut self, path: PathBuf, preview: bool, window: &mut Window, cx: &mut Context<Self>) {
        let key = TabKey::File(path.clone());
        if let Some(ix) = self.workspace().and_then(|w| w.tabs.iter().position(|t| t.key == key)) {
            if !preview {
                self.workspace_mut().tabs[ix].preview = false;
            }
            self.activate_tab(ix, Some(window), cx);
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
        // Editing turns a preview tab into a regular one; either way the tab bar repaints.
        let observed = editor.clone();
        let repaint = cx.observe(&editor, move |this, _, cx| {
            if observed.read(cx).is_dirty() {
                for ws in this.workspaces.values_mut() {
                    for tab in &mut ws.tabs {
                        if matches!(&tab.body, TabBody::File(e) if e == &observed) {
                            tab.preview = false;
                        }
                    }
                }
            }
            cx.notify();
        });
        let failures = cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
            let EditorEvent::SaveFailed(message) = event;
            this.error = Some(message.clone());
            cx.notify();
        });
        let ix = self.install_tab(
            Tab { key, title: name, preview, body: TabBody::File(editor), _subscriptions: vec![repaint, failures] },
            preview,
        );
        self.activate_tab(ix, Some(window), cx);
    }

    /// Opens the diff of one changed file of the selected worktree as a tab.
    pub(super) fn open_diff(&mut self, file_ix: usize, preview: bool, cx: &mut Context<Self>) {
        let (Some(wt_ix), Some(file)) = (self.selected_wt, self.files.get(file_ix)) else { return };
        let wt = &self.worktrees[wt_ix];
        let base = self.pins.get(&wt.path).cloned().unwrap_or_default();
        let key = TabKey::Diff { worktree: wt.path.clone(), path: file.path.clone(), section: file.section };
        let title = Path::new(&file.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| file.path.clone());
        let section = if file.section == "committed" { "committed" } else { "uncommitted" };
        let body = TabBody::Diff(DiffTab {
            data: Rc::new(DiffLayout::new(&file.lines)),
            vscroll: UniformListScrollHandle::new(),
            hscroll: ScrollHandle::new(),
            subtitle: format!("{} · {section} · vs {base}", wt.name),
            additions: file.additions,
            deletions: file.deletions,
        });
        let path = file.path.clone();
        let ix = self.install_tab(Tab { key, title: format!("{title}"), preview, body, _subscriptions: Vec::new() }, preview);
        let _ = path;
        self.activate_tab(ix, None, cx);
    }

    /// Makes a tab current; file tabs also take keyboard focus when a window is given.
    pub(super) fn activate_tab(&mut self, ix: usize, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let ws = self.workspace_mut();
        if ix >= ws.tabs.len() {
            return;
        }
        ws.active = Some(ix);
        if let TabKey::File(path) = &ws.tabs[ix].key {
            ws.selected = Some(path.clone());
        }
        if let (Some(window), TabBody::File(editor)) = (window, &ws.tabs[ix].body) {
            let handle = editor.focus_handle(cx);
            window.focus(&handle);
        }
        cx.notify();
    }

    /// Closing a file with unsaved changes asks first; anything else just closes.
    pub(super) fn request_close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.workspace().and_then(|w| w.tabs.get(ix)) else { return };
        let dirty = matches!(&tab.body, TabBody::File(e) if e.read(cx).is_dirty());
        if !dirty {
            self.close_tab(ix, cx);
            return;
        }
        self.confirm = Some(Confirm {
            title: "Unsaved changes".into(),
            message: format!("\"{}\" has unsaved changes. Close it and discard them?", tab.title),
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

    // ---- file tree navigation --------------------------------------------------------------------

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
            Some(row) => self.open_file(row.path, false, window, cx),
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

    // ---- rendering -----------------------------------------------------------------------------

    /// The explorer's scrolling list of files and folders.
    pub(super) fn tree_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    window.focus(&this.tree_focus);
                    this.workspace_mut().selected = Some(path.clone());
                    if is_dir {
                        this.toggle_dir(&path, cx);
                    } else {
                        // Single click previews; double click keeps the tab.
                        this.open_file(path.clone(), ev.click_count() < 2, window, cx);
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
            .children(rows)
    }

    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace();
        let active = ws.and_then(|w| w.active);
        let tabs = ws.into_iter().flat_map(|w| w.tabs.iter().enumerate()).map(|(i, tab)| {
            let dirty = matches!(&tab.body, TabBody::File(e) if e.read(cx).is_dirty());
            let is_diff = matches!(tab.body, TabBody::Diff(_));
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
                .when(tab.preview, |d| d.italic())
                .when(is_active, |d| d.bg(BG()).text_color(TEXT_STRONG()))
                .when(!is_active, |d| d.text_color(TEXT_DIM()).hover(|d| d.bg(SELECTED())))
                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    if ev.click_count() >= 2 {
                        this.workspace_mut().tabs[i].preview = false;
                    }
                    this.activate_tab(i, Some(window), cx);
                }))
                .when(is_diff, |d| d.child(div().text_color(AMBER()).child("±")))
                .child(tab.title.clone())
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
        div().flex_none().h(px(34.)).flex().bg(CHROME()).border_b_1().border_color(BORDER()).children(tabs)
    }

    /// The main area: tabs on top, the active file or diff below, or a hint when nothing's open.
    pub(super) fn editor_area(&self, available_width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let active_ix = self.workspace().and_then(|w| w.active).unwrap_or(0);
        let content = match self.active_tab() {
            Some(Tab { body: TabBody::File(editor), .. }) => div().flex_1().min_h_0().child(editor.clone()).into_any_element(),
            Some(Tab { body: TabBody::Diff(diff), title, .. }) => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_3()
                        .h(px(30.))
                        .px_4()
                        .border_b_1()
                        .border_color(BORDER_SOFT())
                        .text_xs()
                        .child(div().text_color(TEXT_STRONG()).child(title.clone()))
                        .child(div().text_color(TEXT_DIM()).child(diff.subtitle.clone()))
                        .child(div().flex_1())
                        .child(div().text_color(ADD_FG()).child(format!("+{}", diff.additions)))
                        .child(div().text_color(DEL_FG()).child(format!("-{}", diff.deletions))),
                )
                .child(diff_view(("diff-tab", active_ix), diff.data.clone(), &diff.vscroll, &diff.hscroll, available_width))
                .into_any_element(),
            None => div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(TEXT_DIM())
                .child(div().text_size(px(15.)).text_color(TEXT()).child("Nothing open"))
                .child(div().text_xs().child("Pick a file in Files to edit it"))
                .child(div().text_xs().child("or a worktree in Worktrees to review its changes"))
                .into_any_element(),
        };
        div().flex_1().min_w_0().flex().flex_col().child(self.tab_bar(cx)).child(content)
    }
}
