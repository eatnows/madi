//! The bottom git panel, a view of its own: a commit graph on the left, the selected commit's
//! message, files and diff on the right. It owns all of its state and tells the app about the few
//! things it can't do itself (choose a branch, close, report an error) through events.
mod commit;
mod graph;

use std::{ops::Range, rc::Rc};

use gpui::{
    div, prelude::*, px, uniform_list, ClickEvent, Context, EventEmitter, FocusHandle, IntoElement,
    MouseMoveEvent, Pixels, Point, ScrollHandle, ScrollStrategy, UniformListScrollHandle, Window,
};
use madi_git::{
    diff::{self, FileDiff},
    diff_layout::DiffLayout,
    git_log::{self, CommitInfo},
    graph::{compute_rows, GraphRow},
};
use madi_ui::{
    diff_view::diff_view,
    file_row::file_row,
    resize,
    scroll::{axis_locked, list_scroll_to_top, scroll_to_top},
    theme::*,
};

use crate::app::{SelectNext, SelectPrev};

/// Commits fetched per page as the graph is scrolled.
const GRAPH_PAGE: usize = 100;
/// Rows never get narrower than this; a narrower pane scrolls sideways instead of squeezing them.
const GRAPH_MIN_W: f32 = 820.0;

pub enum GitPanelEvent {
    /// The branch button was pressed; the app shows its branch picker at this point.
    PickBranch(Point<Pixels>),
    Close,
    Error(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Handle {
    Height,
    GraphWidth,
    FilesWidth,
}

pub struct GitPanel {
    repo: String,
    branch: String,
    /// Following the selected worktree's branch (true) or pinned to one picked here (false).
    follow: bool,
    pub(crate) commits: Vec<CommitInfo>,
    rows: Vec<GraphRow>,
    has_more: bool,
    fetching: bool,
    /// Bumped per request so a slow, superseded response can't overwrite a newer one.
    generation: u64,
    pub(crate) selected_oid: Option<String>,
    detail_collapsed: bool,
    pub(crate) files: Vec<FileDiff>,
    pub(crate) diff: Rc<DiffLayout>,
    pub(crate) selected_file: Option<usize>,
    commit_generation: u64,
    hovered_lane: Option<usize>,
    graph_focus: FocusHandle,
    files_focus: FocusHandle,
    pub(crate) graph_scroll: UniformListScrollHandle,
    pub(crate) graph_hscroll: ScrollHandle,
    files_scroll: ScrollHandle,
    diff_scroll: UniformListScrollHandle,
    diff_hscroll: ScrollHandle,
    height: f32,
    graph_width: f32,
    files_width: f32,
    dragging: Option<(Handle, f32)>,
}

impl EventEmitter<GitPanelEvent> for GitPanel {}

impl GitPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            repo: String::new(),
            branch: String::new(),
            follow: true,
            commits: Vec::new(),
            rows: Vec::new(),
            has_more: true,
            fetching: false,
            generation: 0,
            selected_oid: None,
            detail_collapsed: false,
            files: Vec::new(),
            diff: Rc::default(),
            selected_file: None,
            commit_generation: 0,
            hovered_lane: None,
            graph_focus: cx.focus_handle(),
            files_focus: cx.focus_handle(),
            graph_scroll: UniformListScrollHandle::new(),
            graph_hscroll: ScrollHandle::new(),
            files_scroll: ScrollHandle::new(),
            diff_scroll: UniformListScrollHandle::new(),
            diff_hscroll: ScrollHandle::new(),
            height: 280.,
            graph_width: 460.,
            files_width: 220.,
            dragging: None,
        }
    }

    // ---- what the app can ask of it ------------------------------------------------------------

    pub fn branch(&self) -> &str {
        &self.branch
    }

    #[cfg(test)]
    pub fn is_following(&self) -> bool {
        self.follow
    }

    /// A different project: forget everything and follow again.
    pub fn reset(&mut self, repo: String, cx: &mut Context<Self>) {
        self.repo = repo;
        self.follow = true;
        self.branch.clear();
        self.clear(cx);
    }

    pub fn resume_following(&mut self) {
        self.follow = true;
    }

    /// While following, the graph tracks `target` (the selected worktree's branch, or the default
    /// one when only a project is selected); a pinned graph ignores it.
    pub fn follow(&mut self, target: Option<String>, cx: &mut Context<Self>) {
        if !self.follow {
            return;
        }
        if let Some(target) = target {
            if target != self.branch {
                self.branch = target;
                self.load(cx);
            }
        }
    }

    /// Shows `branch` regardless of the selected worktree from now on.
    pub fn pin(&mut self, branch: String, cx: &mut Context<Self>) {
        self.follow = false;
        if self.branch != branch {
            self.branch = branch;
            self.load(cx);
        }
    }

    // ---- data flow -----------------------------------------------------------------------------

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.commits.clear();
        self.rows.clear();
        self.has_more = true;
        self.fetching = false;
        list_scroll_to_top(&self.graph_scroll);
        scroll_to_top(&self.graph_hscroll);
        self.close_commit();
        cx.notify();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.clear(cx);
        if self.branch.is_empty() {
            return;
        }
        self.fetching = true;
        let generation = self.generation;
        let (repo, branch) = (self.repo.clone(), self.branch.clone());
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { git_log::git_log(repo, branch, 0, GRAPH_PAGE) }).await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.fetching = false;
                match result {
                    Ok(commits) => {
                        this.has_more = commits.len() == GRAPH_PAGE;
                        this.commits = commits;
                        this.rows = compute_rows(&this.commits);
                    }
                    Err(e) => cx.emit(GitPanelEvent::Error(e)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Infinite scroll: fetches the next page once the list nears its end.
    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.fetching || !self.has_more || self.branch.is_empty() {
            return;
        }
        self.fetching = true;
        let generation = self.generation;
        let (repo, branch, skip) = (self.repo.clone(), self.branch.clone(), self.commits.len());
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { git_log::git_log(repo, branch, skip, GRAPH_PAGE) }).await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.fetching = false;
                match result {
                    Ok(more) => {
                        this.has_more = more.len() == GRAPH_PAGE;
                        this.commits.extend(more);
                        this.rows = compute_rows(&this.commits);
                    }
                    Err(e) => cx.emit(GitPanelEvent::Error(e)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn close_commit(&mut self) {
        self.commit_generation += 1;
        self.selected_oid = None;
        self.files.clear();
        self.diff = Rc::default();
        self.selected_file = None;
        scroll_to_top(&self.files_scroll);
        self.reset_diff_scroll();
    }

    fn reset_diff_scroll(&self) {
        list_scroll_to_top(&self.diff_scroll);
        scroll_to_top(&self.diff_hscroll);
    }

    /// `toggle`: clicking the already-selected commit deselects it (arrow keys never do).
    pub(crate) fn select_commit(&mut self, ix: usize, toggle: bool, cx: &mut Context<Self>) {
        let oid = self.commits[ix].oid.clone();
        if toggle && self.selected_oid.as_deref() == Some(oid.as_str()) {
            self.close_commit();
            cx.notify();
            return;
        }
        self.close_commit();
        self.selected_oid = Some(oid.clone());
        self.detail_collapsed = false;
        let generation = self.commit_generation;
        let repo = self.repo.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { diff::diff_commit(repo, oid) }).await;
            this.update(cx, |this, cx| {
                if this.commit_generation != generation {
                    return;
                }
                match result {
                    Ok(files) => {
                        this.files = files;
                        if !this.files.is_empty() {
                            this.select_file(0, cx);
                        }
                    }
                    Err(e) => cx.emit(GitPanelEvent::Error(e)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn select_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected_file = Some(ix);
        self.diff = Rc::new(DiffLayout::new(&self.files[ix].lines));
        self.reset_diff_scroll();
        cx.notify();
    }

    pub(crate) fn move_commit(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.commits.is_empty() {
            return;
        }
        let current = self
            .selected_oid
            .as_ref()
            .and_then(|o| self.commits.iter().position(|c| &c.oid == o))
            .map(|i| i as isize)
            .unwrap_or(-1);
        let next = (current + delta).clamp(0, self.commits.len() as isize - 1) as usize;
        if current != next as isize {
            // Non-strict: only scrolls (by the minimum) when the row isn't already visible.
            let strategy = if delta > 0 { ScrollStrategy::Bottom } else { ScrollStrategy::Top };
            self.graph_scroll.scroll_to_item(next, strategy);
            self.select_commit(next, false, cx);
        }
    }

    fn move_file(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.files.is_empty() {
            return;
        }
        let current = self.selected_file.map(|i| i as isize).unwrap_or(-1);
        let next = (current + delta).clamp(0, self.files.len() as isize - 1) as usize;
        if current != next as isize {
            self.files_scroll.scroll_to_item(next);
            self.select_file(next, cx);
        }
    }

    // ---- resizing (the app forwards window-wide mouse moves here) ---------------------------------

    pub fn drag_to(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((handle, last)) = self.dragging else { return };
        let current = f32::from(if handle == Handle::Height { ev.position.y } else { ev.position.x });
        let delta = current - last;
        match handle {
            // The handle sits on the panel's top edge, so dragging up (negative) grows it.
            Handle::Height => self.height = (self.height - delta).clamp(160., 640.),
            Handle::GraphWidth => self.graph_width = (self.graph_width + delta).clamp(300., 800.),
            Handle::FilesWidth => self.files_width = (self.files_width + delta).clamp(160., 400.),
        }
        self.dragging = Some((handle, current));
        cx.notify();
    }

    pub fn end_drag(&mut self) {
        self.dragging = None;
    }

    fn handle(&self, id: &'static str, handle: Handle, cx: &mut Context<Self>) -> impl IntoElement {
        resize::handle(id, handle != Handle::Height, cx.listener(move |this, ev: &gpui::MouseDownEvent, _, _| {
            let pos = if handle == Handle::Height { ev.position.y } else { ev.position.x };
            this.dragging = Some((handle, f32::from(pos)));
        }))
    }

    // ---- rendering -----------------------------------------------------------------------------

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let following = self.follow;
        div()
            .flex_none()
            .h(px(40.))
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .border_b_1()
            .border_color(BORDER_SOFT())
            .child(div().text_size(px(12.5)).text_color(TEXT_STRONG()).child("Git"))
            .child(
                div()
                    .id("graph-branch")
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .font_family(MONO)
                    .text_size(px(11.))
                    .text_color(TEXT_STRONG())
                    .cursor_pointer()
                    .hover(|d| d.bg(SELECTED()))
                    .on_click(cx.listener(|_, ev: &ClickEvent, _, cx| cx.emit(GitPanelEvent::PickBranch(ev.position()))))
                    .child(format!("{} ⌄", self.branch)),
            )
            .child(
                div()
                    .px_2()
                    .rounded_full()
                    .text_size(px(10.5))
                    .when(following, |d| d.text_color(GREEN()).bg(ADD_BG()))
                    .when(!following, |d| d.text_color(AMBER()).bg(SELECTED()))
                    .child(if following { "following worktree" } else { "pinned" }),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("git-close")
                    .px_2()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(TEXT_DIM())
                    .hover(|d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(GitPanelEvent::Close)))
                    .child("×"),
            )
    }

    fn graph_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let list = axis_locked(div().id("graph-scroll").flex_1().min_h_0().overflow_x_scroll())
            .track_scroll(&self.graph_hscroll)
            .child(
                axis_locked(uniform_list(
                    "graph",
                    self.commits.len(),
                    cx.processor(|this, range: Range<usize>, _w, cx| {
                        if range.end + 30 >= this.commits.len() {
                            this.load_more(cx);
                        }
                        range.map(|ix| this.graph_row(ix, cx)).collect::<Vec<_>>()
                    }),
                ))
                .track_scroll(self.graph_scroll.clone())
                .min_w(px(GRAPH_MIN_W))
                .h_full(),
            );
        div()
            .id("graph-pane")
            .key_context("NavList")
            .track_focus(&self.graph_focus)
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_commit(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_commit(1, cx)))
            .w(px(self.graph_width))
            .flex_none()
            .flex()
            .flex_col()
            .min_h_0()
            .border_r_1()
            .border_color(BORDER())
            .child(list)
            .children(self.commit_detail(cx))
    }

    fn changes_pane(&self, viewport_w: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.selected_oid.is_none() {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(TEXT_DIM())
                .child("Select a commit to see its changed files")
                .into_any_element();
        }
        let items = self.files.iter().enumerate().map(|(ix, f)| {
            file_row(("cfile", ix), f, self.selected_file == Some(ix)).on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.files_focus);
                this.select_file(ix, cx);
            }))
        });
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .child(
                axis_locked(div().id("cfile-list"))
                    .key_context("NavList")
                    .track_focus(&self.files_focus)
                    .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_file(-1, cx)))
                    .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_file(1, cx)))
                    .w(px(self.files_width))
                    .flex_none()
                    .overflow_y_scroll()
                    .track_scroll(&self.files_scroll)
                    .p_2()
                    .border_r_1()
                    .border_color(BORDER())
                    .children(items),
            )
            .child(self.handle("rz-cfiles", Handle::FilesWidth, cx))
            .child(diff_view(
                "cdiff",
                self.diff.clone(),
                &self.diff_scroll,
                &self.diff_hscroll,
                viewport_w - self.graph_width - self.files_width - 11.,
            ))
            .into_any_element()
    }
}

impl Render for GitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport_w = f32::from(window.viewport_size().width);
        div()
            .relative()
            .flex_none()
            .h(px(self.height))
            .flex()
            .flex_col()
            .bg(PANEL())
            .border_t_1()
            .border_color(BORDER())
            .child(self.handle("rz-git", Handle::Height, cx))
            .child(self.header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.graph_pane(cx))
                    .child(self.handle("rz-graph", Handle::GraphWidth, cx))
                    .child(self.changes_pane(viewport_w, cx)),
            )
    }
}
