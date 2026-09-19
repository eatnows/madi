mod git_panel;
mod picker_view;

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use gpui::{
    actions, div, prelude::*, px, uniform_list, App, ClickEvent, Context, Entity, FocusHandle,
    Focusable, IntoElement, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent,
    PathPromptOptions, Pixels, Point, ScrollHandle, SharedString, Subscription,
    UniformListScrollHandle, Window,
};
use maditor_core::{
    diff::{self, FileDiff},
    git_log::CommitInfo,
    worktree::{self, RepoStatus, WorktreeInfo},
};

use crate::{
    config::Config,
    diff_view::{build_rows, render_row, Row},
    graph::GraphRow,
    picker,
    text_input::TextInput,
    theme::*,
};

actions!(maditor, [SelectPrev, SelectNext, PickerConfirm, PickerCancel, ModalCancel]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, Some("NavList")),
        KeyBinding::new("down", SelectNext, Some("NavList")),
        KeyBinding::new("up", SelectPrev, Some("Picker")),
        KeyBinding::new("down", SelectNext, Some("Picker")),
        KeyBinding::new("enter", PickerConfirm, Some("Picker")),
        KeyBinding::new("escape", PickerCancel, Some("Picker")),
        KeyBinding::new("escape", ModalCancel, Some("Modal")),
    ]);
}

/// What a picker selection applies to.
#[derive(Clone)]
enum PickerTarget {
    /// The base branch pinned for the worktree at this path.
    WorktreeBase(String),
    /// The branch the git panel's graph shows (pins it, ending "follow worktree").
    GraphBranch,
}

enum MenuTarget {
    Project(String),
    Worktree(usize),
}

/// A worktree pending the "are you sure" step before it's deleted.
struct ConfirmRemove {
    path: String,
    name: String,
    branch: Option<String>,
}

struct BranchPickerState {
    target: PickerTarget,
    anchor: Point<Pixels>,
    input: Entity<TextInput>,
    collapsed: HashSet<String>,
    highlighted: usize,
    last_query: String,
    _subscription: Subscription,
}

const GRAPH_PAGE: usize = 100;

#[derive(Clone, Copy, PartialEq)]
enum Resize {
    WorktreePanel,
    FilePanel,
    GitHeight,
    GraphPane,
    CommitFiles,
}

impl Resize {
    fn horizontal(self) -> bool {
        self != Resize::GitHeight
    }
}

struct Sizes {
    worktree: f32,
    files: f32,
    git_height: f32,
    graph_pane: f32,
    commit_files: f32,
}

enum FileItem {
    Label(&'static str),
    File(usize),
}

enum ScanOutcome {
    Issue(&'static str),
    Loaded { worktrees: Vec<WorktreeInfo>, pins: HashMap<String, String>, branches: Vec<String> },
}

/// Runs off the UI thread: git work on a big repo shouldn't freeze the window.
fn scan_repo(repo: String, mut pins: HashMap<String, String>) -> Result<ScanOutcome, String> {
    match worktree::check_repo(repo.clone()) {
        RepoStatus::Ok => {}
        RepoStatus::NotARepo => return Ok(ScanOutcome::Issue("Not a git repository")),
        RepoStatus::Missing => return Ok(ScanOutcome::Issue("Folder not found")),
    }
    let branches = worktree::list_branches(repo.clone())?;
    let default = if branches.iter().any(|b| b == "main") {
        "main".to_string()
    } else {
        branches.first().cloned().unwrap_or_default()
    };
    for wt in worktree::list_worktrees(repo.clone(), HashMap::new())? {
        pins.entry(wt.path).or_insert_with(|| default.clone());
    }
    let worktrees = worktree::list_worktrees(repo, pins.clone())?;
    Ok(ScanOutcome::Loaded { worktrees, pins, branches })
}

pub struct Maditor {
    config: Config,
    repo: String,
    issue: Option<&'static str>,
    error: Option<String>,
    worktrees: Vec<WorktreeInfo>,
    branches: Vec<String>,
    pins: HashMap<String, String>,
    picker: Option<BranchPickerState>,
    selected_wt: Option<usize>,
    files: Vec<FileDiff>,
    file_items: Vec<FileItem>,
    selected_file: Option<usize>,
    rows: Vec<Row>,
    loading_diff: bool,
    /// Bumped per request so a slow, superseded scan/diff can't overwrite a newer one.
    scan_gen: u64,
    diff_gen: u64,
    menu: Option<(Point<Pixels>, MenuTarget)>,
    confirm: Option<ConfirmRemove>,
    modal_focus: FocusHandle,
    wt_focus: FocusHandle,
    file_focus: FocusHandle,
    wt_scroll: ScrollHandle,
    file_scroll: ScrollHandle,
    sizes: Sizes,
    dragging: Option<(Resize, f32)>,
    // git panel
    git_open: bool,
    graph_branch: String,
    follow_worktree: bool,
    commits: Vec<CommitInfo>,
    graph_rows: Vec<GraphRow>,
    has_more: bool,
    fetching: bool,
    graph_gen: u64,
    selected_oid: Option<String>,
    detail_collapsed: bool,
    commit_files: Vec<FileDiff>,
    commit_rows: Vec<Row>,
    selected_cfile: Option<usize>,
    commit_gen: u64,
    hovered_lane: Option<usize>,
    graph_focus: FocusHandle,
    cfile_focus: FocusHandle,
    graph_scroll: UniformListScrollHandle,
    cfile_scroll: ScrollHandle,
}

impl Maditor {
    pub fn new(initial: Option<String>, config: Config, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            config,
            repo: String::new(),
            issue: None,
            error: None,
            worktrees: Vec::new(),
            branches: Vec::new(),
            pins: HashMap::new(),
            picker: None,
            selected_wt: None,
            files: Vec::new(),
            file_items: Vec::new(),
            selected_file: None,
            rows: Vec::new(),
            loading_diff: false,
            scan_gen: 0,
            diff_gen: 0,
            menu: None,
            confirm: None,
            modal_focus: cx.focus_handle(),
            wt_focus: cx.focus_handle(),
            file_focus: cx.focus_handle(),
            wt_scroll: ScrollHandle::new(),
            file_scroll: ScrollHandle::new(),
            sizes: Sizes { worktree: 248., files: 260., git_height: 280., graph_pane: 460., commit_files: 220. },
            dragging: None,
            git_open: false,
            graph_branch: String::new(),
            follow_worktree: true,
            commits: Vec::new(),
            graph_rows: Vec::new(),
            has_more: true,
            fetching: false,
            graph_gen: 0,
            selected_oid: None,
            detail_collapsed: false,
            commit_files: Vec::new(),
            commit_rows: Vec::new(),
            selected_cfile: None,
            commit_gen: 0,
            hovered_lane: None,
            graph_focus: cx.focus_handle(),
            cfile_focus: cx.focus_handle(),
            graph_scroll: UniformListScrollHandle::new(),
            cfile_scroll: ScrollHandle::new(),
        };
        if let Some(path) = initial.or_else(|| this.config.last_project.clone()) {
            this.open_project(path, cx);
        }
        this
    }

    fn project_name(path: &str) -> String {
        path.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(path).to_string()
    }

    fn open_project(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.config.projects.contains(&path) {
            self.config.projects.push(path.clone());
        }
        self.config.last_project = Some(path.clone());
        self.config.save();
        self.scan(path, cx);
    }

    fn reset_view(&mut self) {
        self.issue = None;
        self.error = None;
        self.worktrees.clear();
        self.branches.clear();
        self.picker = None;
        self.selected_wt = None;
        self.files.clear();
        self.file_items.clear();
        self.selected_file = None;
        self.rows.clear();
        self.loading_diff = false;
        self.diff_gen += 1;
        self.follow_worktree = true;
        self.graph_branch.clear();
        self.clear_graph();
    }

    fn scan(&mut self, path: String, cx: &mut Context<Self>) {
        self.reset_view();
        self.repo = path.clone();
        self.scan_gen += 1;
        let generation = self.scan_gen;
        let saved = self.config.pins.get(&path).cloned().unwrap_or_default();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let repo = path.clone();
            let result = cx.background_spawn(async move { scan_repo(repo, saved) }).await;
            this.update(cx, |this, cx| {
                if this.scan_gen != generation {
                    return;
                }
                match result {
                    Ok(ScanOutcome::Issue(issue)) => this.issue = Some(issue),
                    Ok(ScanOutcome::Loaded { worktrees, pins, branches }) => {
                        this.worktrees = worktrees;
                        this.branches = branches;
                        this.pins = pins.clone();
                        this.config.pins.insert(path, pins);
                        this.config.save();
                        this.sync_graph_branch(cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn add_project(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open a git repository".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                if let Some(path) = paths.into_iter().next() {
                    let path = path.to_string_lossy().into_owned();
                    this.update(cx, |this, cx| this.open_project(path, cx)).ok();
                }
            }
        })
        .detach();
    }

    fn close_project(&mut self, path: &str, cx: &mut Context<Self>) {
        let idx = self.config.projects.iter().position(|p| p == path);
        self.config.projects.retain(|p| p != path);
        if self.config.last_project.as_deref() == Some(path) {
            self.config.last_project = None;
        }
        self.config.save();
        if self.repo == path {
            let next = idx
                .and_then(|i| self.config.projects.get(i.min(self.config.projects.len().saturating_sub(1))))
                .cloned();
            match next {
                Some(next) => self.open_project(next, cx),
                None => {
                    self.reset_view();
                    self.repo.clear();
                }
            }
        }
        cx.notify();
    }

    fn select_worktree(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected_wt = Some(ix);
        self.follow_worktree = true;
        self.sync_graph_branch(cx);
        self.files.clear();
        self.file_items.clear();
        self.selected_file = None;
        self.rows.clear();
        self.error = None;
        self.loading_diff = true;
        self.diff_gen += 1;
        let generation = self.diff_gen;

        let path = self.worktrees[ix].path.clone();
        let base = self.pins.get(&path).cloned().unwrap_or_default();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { diff::diff_against_base(path, base) }).await;
            this.update(cx, |this, cx| {
                if this.diff_gen != generation {
                    return;
                }
                this.loading_diff = false;
                match result {
                    Ok(result) => {
                        this.files = result.files;
                        let mut last = "";
                        for (i, f) in this.files.iter().enumerate() {
                            if f.section != last {
                                last = f.section;
                                this.file_items.push(FileItem::Label(if last == "committed" {
                                    "COMMITTED"
                                } else {
                                    "UNCOMMITTED"
                                }));
                            }
                            this.file_items.push(FileItem::File(i));
                        }
                        if !this.files.is_empty() {
                            this.select_file(0, cx);
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn select_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected_file = Some(ix);
        self.rows = build_rows(&self.files[ix].lines);
        cx.notify();
    }

    fn open_picker(&mut self, target: PickerTarget, anchor: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("Search branches", cx));
        window.focus(&input.focus_handle(cx));
        let subscription = cx.observe(&input, |this, input, cx| {
            let query = input.read(cx).content().to_string();
            if let Some(p) = &mut this.picker {
                if p.last_query != query {
                    p.last_query = query;
                    p.highlighted = 0;
                }
            }
            cx.notify();
        });
        self.picker = Some(BranchPickerState {
            target,
            anchor,
            input,
            collapsed: HashSet::new(),
            highlighted: 0,
            last_query: String::new(),
            _subscription: subscription,
        });
        cx.notify();
    }

    fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.take().is_some() {
            window.focus(&self.wt_focus);
            cx.notify();
        }
    }

    fn picker_rows(&self, cx: &App) -> Vec<picker::PickerRow> {
        match &self.picker {
            Some(p) => picker::flatten(&self.branches, &p.collapsed, p.input.read(cx).content()),
            None => Vec::new(),
        }
    }

    /// Enter/click on a picker row: a branch applies to the target, a folder toggles open/closed.
    fn picker_activate(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picked) = self.picker_rows(cx).into_iter().nth(row) else { return };
        match picked {
            picker::PickerRow::Folder { full_path, .. } => {
                if let Some(p) = &mut self.picker {
                    if !p.collapsed.remove(&full_path) {
                        p.collapsed.insert(full_path);
                    }
                }
                cx.notify();
            }
            picker::PickerRow::Option { full_path, .. } => {
                let Some(target) = self.picker.as_ref().map(|p| p.target.clone()) else { return };
                self.close_picker(window, cx);
                match target {
                    PickerTarget::WorktreeBase(path) => self.change_base(path, full_path, cx),
                    PickerTarget::GraphBranch => {
                        self.follow_worktree = false;
                        if self.graph_branch != full_path {
                            self.graph_branch = full_path;
                            self.load_graph(cx);
                        }
                    }
                }
            }
        }
    }

    fn move_picker(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.picker_rows(cx).len();
        if let Some(p) = &mut self.picker {
            if len > 0 {
                p.highlighted = (p.highlighted as isize + delta).clamp(0, len as isize - 1) as usize;
                cx.notify();
            }
        }
    }

    /// Pins a new base branch for a worktree, refreshing ahead/behind and (if it's the one on
    /// screen) its diff.
    fn change_base(&mut self, worktree_path: String, branch: String, cx: &mut Context<Self>) {
        self.pins.insert(worktree_path.clone(), branch);
        self.config.pins.insert(self.repo.clone(), self.pins.clone());
        self.config.save();
        self.refresh_worktrees(Some(worktree_path), cx);
    }

    /// Reloads the worktree list keeping the selection (by path); `reload_diff_for` re-diffs that
    /// worktree if it's the selected one.
    fn refresh_worktrees(&mut self, reload_diff_for: Option<String>, cx: &mut Context<Self>) {
        let (repo, pins, generation) = (self.repo.clone(), self.pins.clone(), self.scan_gen);
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { worktree::list_worktrees(repo, pins) }).await;
            this.update(cx, |this, cx| {
                if this.scan_gen != generation {
                    return;
                }
                match result {
                    Ok(worktrees) => {
                        let selected_path = this.selected_wt.map(|i| this.worktrees[i].path.clone());
                        this.worktrees = worktrees;
                        this.selected_wt =
                            selected_path.and_then(|p| this.worktrees.iter().position(|w| w.path == p));
                        match (this.selected_wt, selected_path_matches(&this.selected_wt, &this.worktrees, reload_diff_for.as_deref())) {
                            (Some(ix), true) => this.select_worktree(ix, cx),
                            (None, _) => {
                                // The selected worktree is gone: clear its diff.
                                this.files.clear();
                                this.file_items.clear();
                                this.selected_file = None;
                                this.rows.clear();
                                this.follow_worktree = true;
                                this.sync_graph_branch(cx);
                            }
                            _ => {}
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn remove_worktree(&mut self, path: String, cx: &mut Context<Self>) {
        self.error = None;
        let repo = self.repo.clone();
        let generation = self.scan_gen;
        cx.spawn(async move |this, cx| {
            let target = path.clone();
            let result = cx
                .background_spawn(async move { worktree::remove_worktree(repo, target) })
                .await;
            this.update(cx, |this, cx| {
                if this.scan_gen != generation {
                    return;
                }
                match result {
                    Ok(()) => {
                        this.pins.remove(&path);
                        this.config.pins.insert(this.repo.clone(), this.pins.clone());
                        this.config.save();
                        this.refresh_worktrees(None, cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn move_worktree(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.worktrees.is_empty() {
            return;
        }
        let current = self.selected_wt.map(|i| i as isize).unwrap_or(-1);
        let next = (current + delta).clamp(0, self.worktrees.len() as isize - 1) as usize;
        if Some(next) != self.selected_wt {
            self.wt_scroll.scroll_to_item(next);
            self.select_worktree(next, cx);
        }
    }

    fn move_file(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.files.is_empty() {
            return;
        }
        let current = self.selected_file.map(|i| i as isize).unwrap_or(-1);
        let next = (current + delta).clamp(0, self.files.len() as isize - 1) as usize;
        if Some(next) != self.selected_file {
            if let Some(pos) = self.file_items.iter().position(|it| matches!(it, FileItem::File(i) if *i == next)) {
                self.file_scroll.scroll_to_item(pos);
            }
            self.select_file(next, cx);
        }
    }

    fn rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tiles = self.config.projects.iter().enumerate().map(|(n, path)| {
            let active = *path == self.repo;
            let letter = Self::project_name(path).chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            let click_path = path.clone();
            let menu_path = path.clone();
            div()
                .id(("project", n))
                .size(px(32.))
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(active, |d| d.bg(TEXT_STRONG).text_color(BG))
                .when(!active, |d| d.border_1().border_color(BORDER).text_color(TEXT_DIM).hover(|d| d.bg(SELECTED)))
                .on_click(cx.listener(move |this, _, _, cx| this.open_project(click_path.clone(), cx)))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        this.menu = Some((ev.position, MenuTarget::Project(menu_path.clone())));
                        cx.notify();
                    }),
                )
                .child(letter)
        });
        div()
            .w(px(48.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .pt_2()
            .bg(CHROME)
            .border_r_1()
            .border_color(BORDER)
            .children(tiles)
            .child(
                div()
                    .id("add-project")
                    .size(px(32.))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_color(TEXT_DIM)
                    .border_1()
                    .border_color(BORDER)
                    .hover(|d| d.bg(SELECTED))
                    .on_click(cx.listener(|this, _, _, cx| this.add_project(cx)))
                    .child("+"),
            )
    }

    fn worktree_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.worktrees.iter().enumerate().map(|(ix, wt)| {
            let selected = self.selected_wt == Some(ix);
            let status = match (wt.ahead, wt.behind) {
                (Some(0), Some(0)) => "up to date".to_string(),
                (Some(a), Some(b)) => format!("↑{a} ↓{b}"),
                _ => String::new(),
            };
            let base = self.pins.get(&wt.path).cloned().unwrap_or_default();
            let wt_path = wt.path.clone();
            let is_main = wt.is_main;
            div()
                .id(("wt", ix))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        if !is_main {
                            this.menu = Some((ev.position, MenuTarget::Worktree(ix)));
                            cx.notify();
                        }
                    }),
                )
                .px_2()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |d| d.bg(SELECTED))
                .hover(|d| d.bg(SELECTED))
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.wt_focus);
                    this.select_worktree(ix, cx);
                }))
                .child(div().text_color(TEXT_STRONG).child(wt.name.clone()))
                .child(div().text_xs().text_color(TEXT_DIM).child(wt.branch.clone().unwrap_or_else(|| "(detached)".into())))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .text_color(TEXT_DIM)
                        .child(
                            div()
                                .id(("base", ix))
                                .px_1()
                                .rounded_sm()
                                .cursor_pointer()
                                .hover(|d| d.bg(BORDER).text_color(TEXT_STRONG))
                                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                                    cx.stop_propagation();
                                    let anchor = ev.position();
                                    this.open_picker(PickerTarget::WorktreeBase(wt_path.clone()), anchor, window, cx);
                                }))
                                .child(format!("base: {base} ⌄")),
                        )
                        .child(status),
                )
        });
        div()
            .w(px(self.sizes.worktree))
            .flex_none()
            .flex()
            .flex_col()
            .bg(PANEL)
            .border_r_1()
            .border_color(BORDER)
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(BORDER_SOFT)
                    .child(div().text_color(TEXT_STRONG).child(Self::project_name(&self.repo)))
                    .child(div().text_xs().text_color(TEXT_DIM).child("Worktrees")),
            )
            .child(
                div()
                    .id("wt-list")
                    .key_context("NavList")
                    .track_focus(&self.wt_focus)
                    .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_worktree(-1, cx)))
                    .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_worktree(1, cx)))
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.wt_scroll)
                    .p_2()
                    .children(rows),
            )
    }

    fn file_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, subtitle) = self
            .selected_wt
            .map(|ix| {
                let wt = &self.worktrees[ix];
                (wt.name.clone(), format!("vs {}", self.pins.get(&wt.path).cloned().unwrap_or_default()))
            })
            .unwrap_or_default();
        let items = self.file_items.iter().enumerate().map(|(n, item)| match item {
            FileItem::Label(text) => {
                div().id(("label", n)).px_2().pt_3().pb_1().text_xs().text_color(TEXT_DIM).child(*text)
            }
            FileItem::File(ix) => {
                let ix = *ix;
                file_row(("file", n), &self.files[ix], self.selected_file == Some(ix)).on_click(cx.listener(
                    move |this, _, window, cx| {
                        window.focus(&this.file_focus);
                        this.select_file(ix, cx);
                    },
                ))
            }
        });
        div()
            .w(px(self.sizes.files))
            .flex_none()
            .flex()
            .flex_col()
            .bg(PANEL)
            .border_r_1()
            .border_color(BORDER)
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(BORDER_SOFT)
                    .child(div().text_color(TEXT_STRONG).child(title))
                    .child(div().text_xs().text_color(TEXT_DIM).child(subtitle)),
            )
            .child(
                div()
                    .id("file-list")
                    .key_context("NavList")
                    .track_focus(&self.file_focus)
                    .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_file(-1, cx)))
                    .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_file(1, cx)))
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.file_scroll)
                    .p_2()
                    .children(items),
            )
    }

    fn body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let row = || div().flex_1().min_w_0().flex();
        if self.repo.is_empty() {
            return row().child(empty("No project")).into_any_element();
        }
        if let Some(issue) = self.issue {
            let repo = self.repo.clone();
            return row()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(TEXT_DIM)
                .child(issue)
                .child(
                    div()
                        .id("close-broken")
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(BORDER)
                        .text_xs()
                        .text_color(TEXT)
                        .cursor_pointer()
                        .hover(|d| d.bg(SELECTED))
                        .on_click(cx.listener(move |this, _, _, cx| this.close_project(&repo, cx)))
                        .child("Close project"),
                )
                .into_any_element();
        }
        if self.worktrees.is_empty() && self.error.is_none() {
            return row().child(empty("No data")).into_any_element();
        }
        let mut body = row().child(self.worktree_panel(cx)).child(self.resize_handle(Resize::WorktreePanel, cx));
        if self.selected_wt.is_none() {
            return body.child(empty("No data")).into_any_element();
        }
        if self.loading_diff {
            return body.child(empty("Loading…")).into_any_element();
        }
        if self.files.is_empty() && self.error.is_none() {
            return body.child(empty("No changes")).into_any_element();
        }
        body = body.child(self.file_panel(cx)).child(self.resize_handle(Resize::FilePanel, cx)).child(
            div().flex_1().min_w_0().bg(BG).child(
                uniform_list(
                    "diff",
                    self.rows.len(),
                    cx.processor(|this, range: Range<usize>, _w, _cx| {
                        range.map(|ix| render_row(&this.rows[ix])).collect::<Vec<_>>()
                    }),
                )
                .size_full()
                .font_family(MONO)
                .text_size(px(12.)),
            ),
        );
        body.into_any_element()
    }

    fn context_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (pos, target) = self.menu.as_ref()?;
        let (label, danger) = match target {
            MenuTarget::Project(_) => ("Close project", false),
            MenuTarget::Worktree(_) => ("Remove worktree…", true),
        };
        let target = match target {
            MenuTarget::Project(p) => MenuTarget::Project(p.clone()),
            MenuTarget::Worktree(i) => MenuTarget::Worktree(*i),
        };
        Some(
            div()
                .id("menu-overlay")
                .absolute()
                .inset_0()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                    this.menu = None;
                    cx.notify();
                }))
                .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _, cx| {
                    this.menu = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .id("menu")
                        .absolute()
                        .left(pos.x)
                        .top(pos.y)
                        .min_w(px(170.))
                        .p_1()
                        .rounded_md()
                        .bg(CHROME)
                        .border_1()
                        .border_color(BORDER)
                        .child(
                            div()
                                .id("menu-item")
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .text_xs()
                                .text_color(if danger { RED } else { TEXT_STRONG })
                                .cursor_pointer()
                                .hover(|d| d.bg(SELECTED))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.menu = None;
                                    match &target {
                                        MenuTarget::Project(path) => this.close_project(path, cx),
                                        MenuTarget::Worktree(ix) => {
                                            if let Some(wt) = this.worktrees.get(*ix) {
                                                this.confirm = Some(ConfirmRemove {
                                                    path: wt.path.clone(),
                                                    name: wt.name.clone(),
                                                    branch: wt.branch.clone(),
                                                });
                                                window.focus(&this.modal_focus);
                                            }
                                        }
                                    }
                                    cx.notify();
                                }))
                                .child(label),
                        ),
                ),
        )
    }

    /// The second, explicit step before a worktree (and its uncommitted changes) is deleted.
    fn confirm_modal(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let c = self.confirm.as_ref()?;
        let path = c.path.clone();
        let message = format!(
            "Remove worktree \"{}\" ({})? This deletes its working directory. Uncommitted changes will be lost.",
            c.name,
            c.branch.clone().unwrap_or_else(|| "detached".into())
        );
        let close = |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            this.confirm = None;
            window.focus(&this.wt_focus);
            cx.notify();
        };
        Some(
            div()
                .id("modal-overlay")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::rgba(0x00000059))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| close(this, window, cx)))
                .child(
                    div()
                        .id("modal")
                        .key_context("Modal")
                        .track_focus(&self.modal_focus)
                        .on_action(cx.listener(move |this, _: &ModalCancel, window, cx| close(this, window, cx)))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .w(px(360.))
                        .p_4()
                        .rounded_lg()
                        .bg(CHROME)
                        .border_1()
                        .border_color(BORDER)
                        .child(div().text_size(px(13.5)).text_color(TEXT_STRONG).child("Remove worktree"))
                        .child(div().mt_2().text_xs().text_color(TEXT_DIM).child(message))
                        .child(
                            div()
                                .mt_4()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    div()
                                        .id("modal-cancel")
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(BORDER)
                                        .text_xs()
                                        .text_color(TEXT_STRONG)
                                        .cursor_pointer()
                                        .hover(|d| d.bg(SELECTED))
                                        .on_click(cx.listener(move |this, _, window, cx| close(this, window, cx)))
                                        .child("Cancel"),
                                )
                                .child(
                                    div()
                                        .id("modal-remove")
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(RED)
                                        .text_xs()
                                        .text_color(BG)
                                        .cursor_pointer()
                                        .hover(|d| d.opacity(0.9))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            close(this, window, cx);
                                            this.remove_worktree(path.clone(), cx);
                                        }))
                                        .child("Remove"),
                                ),
                        ),
                ),
        )
    }
}

/// One changed-file row (status letter, path, +/- counts); the caller adds the click handler.
fn file_row(id: impl Into<gpui::ElementId>, f: &FileDiff, selected: bool) -> gpui::Stateful<gpui::Div> {
    let (letter, color) = match f.status.as_str() {
        "added" => ("A", GREEN),
        "deleted" => ("D", RED),
        _ => ("M", AMBER),
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .when(selected, |d| d.bg(SELECTED))
        .hover(|d| d.bg(SELECTED))
        .child(div().w(px(12.)).flex_none().text_color(color).child(letter))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(TEXT)
                .child(f.path.clone()),
        )
        .child(div().text_xs().text_color(ADD_FG).child(format!("+{}", f.additions)))
        .child(div().text_xs().text_color(DEL_FG).child(format!("-{}", f.deletions)))
}

fn selected_path_matches(selected: &Option<usize>, worktrees: &[WorktreeInfo], path: Option<&str>) -> bool {
    match (selected, path) {
        (Some(i), Some(path)) => worktrees[*i].path == path,
        _ => false,
    }
}

fn empty(message: impl Into<SharedString>) -> impl IntoElement {
    div().flex_1().flex().items_center().justify_center().text_color(TEXT_DIM).child(message.into())
}

impl Render for Maditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut breadcrumb = format!("maditor / {}", Self::project_name(&self.repo));
        if let Some(ix) = self.selected_wt {
            let wt = &self.worktrees[ix];
            breadcrumb.push_str(&format!(" / {}", wt.branch.clone().unwrap_or_else(|| wt.name.clone())));
        }
        if self.repo.is_empty() {
            breadcrumb = "maditor".into();
        }

        let repo_ok = !self.repo.is_empty() && self.issue.is_none();
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| this.on_root_mouse_move(ev, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.dragging = None))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| this.dragging = None))
            .bg(BG)
            .text_color(TEXT)
            .text_size(px(13.))
            .child(
                div()
                    .h(px(40.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .bg(CHROME)
                    .border_b_1()
                    .border_color(BORDER)
                    .font_family(MONO)
                    .text_color(TEXT_DIM)
                    .child(breadcrumb)
                    .when_some(self.error.clone(), |d, e| d.child(div().text_color(RED).text_xs().child(e))),
            )
            .child(div().flex_1().min_h_0().flex().child(self.rail(cx)).child(self.body(cx)))
            .when(repo_ok && self.git_open, |d| d.child(self.git_panel(cx)))
            .when(repo_ok, |d| d.child(self.bottom_bar(cx)))
            .children(self.context_menu(cx))
            .children(self.confirm_modal(cx))
            .children(self.picker_overlay(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::{path::Path, process::Command};

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// A repo with `main` plus a linked worktree `wt` (branch feature) that edits one file.
    fn fixture(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("maditor-native-test-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t"]);
        git(&repo, &["config", "user.name", "T"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(repo.join("b.txt"), "x\n").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        let wt = root.join("wt");
        git(&repo, &["worktree", "add", "-q", "-b", "feature", wt.to_str().unwrap()]);
        std::fs::write(wt.join("a.txt"), "one\nTWO\n").unwrap();
        std::fs::write(wt.join("b.txt"), "y\n").unwrap();
        git(&wt, &["commit", "-qam", "edit"]);
        (root, repo)
    }

    fn config_in(root: &Path) -> Config {
        Config::at(Some(root.join("config.json")))
    }

    #[gpui::test]
    fn scans_a_repo_and_loads_a_worktree_diff(cx: &mut TestAppContext) {
        let (root, repo) = fixture("scan");
        let path = repo.to_string_lossy().into_owned();
        let config = config_in(&root);
        let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
        cx.run_until_parked();

        view.read_with(cx, |m, _| {
            assert_eq!(m.worktrees.len(), 2);
            assert!(m.pins.values().all(|b| b == "main"));
            assert_eq!(m.config.projects, vec![path.clone()]);
        });

        let wt_ix = view.read_with(cx, |m, _| m.worktrees.iter().position(|w| !w.is_main).unwrap());
        view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
        cx.run_until_parked();

        view.read_with(cx, |m, _| {
            assert!(!m.loading_diff);
            assert_eq!(m.files.len(), 2);
            assert_eq!(m.selected_file, Some(0));
            assert!(!m.rows.is_empty());
        });
    }

    #[gpui::test]
    fn arrow_navigation_clamps_at_both_ends(cx: &mut TestAppContext) {
        let (root, repo) = fixture("nav");
        let path = repo.to_string_lossy().into_owned();
        let config = config_in(&root);
        let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
        cx.run_until_parked();

        view.update(cx, |m, cx| m.move_worktree(1, cx));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |m, _| m.selected_wt), Some(0));
        view.update(cx, |m, cx| m.move_worktree(1, cx));
        view.update(cx, |m, cx| m.move_worktree(1, cx));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |m, _| m.selected_wt), Some(1), "clamped at the last worktree");

        let on_wt = view.read_with(cx, |m, _| !m.worktrees[m.selected_wt.unwrap()].is_main);
        if !on_wt {
            view.update(cx, |m, cx| m.move_worktree(-1, cx));
            cx.run_until_parked();
        }
        view.update(cx, |m, cx| m.move_file(1, cx));
        assert_eq!(view.read_with(cx, |m, _| m.selected_file), Some(1));
        view.update(cx, |m, cx| m.move_file(1, cx));
        assert_eq!(view.read_with(cx, |m, _| m.selected_file), Some(1), "clamped at the last file");
        view.update(cx, |m, cx| m.move_file(-5, cx));
        assert_eq!(view.read_with(cx, |m, _| m.selected_file), Some(0));
    }

    #[gpui::test]
    fn picking_a_base_branch_pins_it_and_refreshes_ahead_behind(cx: &mut TestAppContext) {
        let (root, repo) = fixture("pick");
        let path = repo.to_string_lossy().into_owned();
        let config = config_in(&root);
        let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
        cx.run_until_parked();

        let (wt_ix, wt_path) = view.read_with(cx, |m, _| {
            let ix = m.worktrees.iter().position(|w| !w.is_main).unwrap();
            (ix, m.worktrees[ix].path.clone())
        });
        assert_eq!(view.read_with(cx, |m, _| (m.worktrees[wt_ix].ahead, m.worktrees[wt_ix].behind)), (Some(1), Some(0)));

        view.update_in(cx, |m, window, cx| {
            m.open_picker(PickerTarget::WorktreeBase(wt_path.clone()), Point::default(), window, cx)
        });
        // Rows are alphabetical: feature, main. Activate "feature" (the worktree's own branch).
        let feature_row = view.read_with(cx, |m, cx| {
            m.picker_rows(cx)
                .iter()
                .position(|r| matches!(r, picker::PickerRow::Option { full_path, .. } if full_path == "feature"))
                .unwrap()
        });
        view.update_in(cx, |m, window, cx| m.picker_activate(feature_row, window, cx));
        cx.run_until_parked();

        view.read_with(cx, |m, _| {
            assert!(m.picker.is_none(), "picker closes after a pick");
            assert_eq!(m.pins.get(&wt_path).map(String::as_str), Some("feature"));
            let ix = m.worktrees.iter().position(|w| w.path == wt_path).unwrap();
            assert_eq!((m.worktrees[ix].ahead, m.worktrees[ix].behind), (Some(0), Some(0)));
        });
        assert_eq!(
            config_in(&root).pins[&path][&wt_path], "feature",
            "the pin was persisted"
        );
    }

    #[gpui::test]
    fn graph_follows_the_worktree_until_pinned(cx: &mut TestAppContext) {
        let (root, repo) = fixture("graph");
        let path = repo.to_string_lossy().into_owned();
        let config = config_in(&root);
        let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
        cx.run_until_parked();

        view.read_with(cx, |m, _| {
            assert_eq!(m.graph_branch, "main", "with only a project selected, default to main");
            assert_eq!(m.commits.len(), 1);
        });

        let wt_ix = view.read_with(cx, |m, _| m.worktrees.iter().position(|w| !w.is_main).unwrap());
        view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
        cx.run_until_parked();
        view.read_with(cx, |m, _| {
            assert_eq!(m.graph_branch, "feature");
            assert_eq!(m.commits.len(), 2);
            assert!(m.follow_worktree);
        });

        // Pin the graph to main via the panel's own picker: it stops following.
        view.update_in(cx, |m, window, cx| m.open_picker(PickerTarget::GraphBranch, Point::default(), window, cx));
        let main_row = view.read_with(cx, |m, cx| {
            m.picker_rows(cx)
                .iter()
                .position(|r| matches!(r, picker::PickerRow::Option { full_path, .. } if full_path == "main"))
                .unwrap()
        });
        view.update_in(cx, |m, window, cx| m.picker_activate(main_row, window, cx));
        cx.run_until_parked();
        view.read_with(cx, |m, _| {
            assert_eq!((m.graph_branch.as_str(), m.follow_worktree, m.commits.len()), ("main", false, 1));
        });

        // Clicking a worktree resumes following.
        view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
        cx.run_until_parked();
        view.read_with(cx, |m, _| assert_eq!((m.graph_branch.as_str(), m.follow_worktree), ("feature", true)));
    }

    #[gpui::test]
    fn selecting_a_commit_loads_its_files_and_reclicking_deselects(cx: &mut TestAppContext) {
        let (root, repo) = fixture("commit");
        let path = repo.to_string_lossy().into_owned();
        let config = config_in(&root);
        let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
        cx.run_until_parked();
        let wt_ix = view.read_with(cx, |m, _| m.worktrees.iter().position(|w| !w.is_main).unwrap());
        view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
        cx.run_until_parked();

        view.update(cx, |m, cx| m.select_commit(0, true, cx));
        cx.run_until_parked();
        view.read_with(cx, |m, _| {
            assert_eq!(m.selected_oid.as_deref(), Some(m.commits[0].oid.as_str()));
            assert_eq!(m.commit_files.len(), 2, "the tip commit edits a.txt and b.txt");
            assert_eq!(m.selected_cfile, Some(0));
            assert!(!m.commit_rows.is_empty());
        });

        view.update(cx, |m, cx| m.move_commit(1, cx));
        cx.run_until_parked();
        view.read_with(cx, |m, _| {
            assert_eq!(m.selected_oid.as_deref(), Some(m.commits[1].oid.as_str()));
            assert_eq!(m.commit_files.len(), 2, "the root commit adds both files");
        });
        view.update(cx, |m, cx| m.move_commit(1, cx));
        view.read_with(cx, |m, _| assert_eq!(m.selected_oid.as_deref(), Some(m.commits[1].oid.as_str()), "clamped"));

        view.update(cx, |m, cx| m.select_commit(1, true, cx));
        view.read_with(cx, |m, _| assert!(m.selected_oid.is_none() && m.commit_files.is_empty()));
    }

    #[gpui::test]
    fn removing_a_worktree_keeps_the_other_selection_and_drops_its_pin(cx: &mut TestAppContext) {
        let (root, repo) = fixture("remove");
        let path = repo.to_string_lossy().into_owned();
        let config = config_in(&root);
        let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
        cx.run_until_parked();

        let (main_ix, wt_path) = view.read_with(cx, |m, _| {
            (
                m.worktrees.iter().position(|w| w.is_main).unwrap(),
                m.worktrees.iter().find(|w| !w.is_main).unwrap().path.clone(),
            )
        });
        view.update(cx, |m, cx| m.select_worktree(main_ix, cx));
        cx.run_until_parked();

        view.update(cx, |m, cx| m.remove_worktree(wt_path.clone(), cx));
        cx.run_until_parked();

        view.read_with(cx, |m, _| {
            assert_eq!(m.worktrees.len(), 1);
            assert!(m.worktrees[0].is_main);
            assert_eq!(m.selected_wt, Some(0), "the still-existing selection is kept");
            assert!(!m.pins.contains_key(&wt_path));
            assert!(m.error.is_none());
        });
        assert!(!std::path::Path::new(&wt_path).exists(), "the working directory is gone");
        assert!(!config_in(&root).pins[&path].contains_key(&wt_path));
    }

    #[gpui::test]
    fn removing_the_selected_worktree_clears_its_diff(cx: &mut TestAppContext) {
        let (root, repo) = fixture("remove-selected");
        let path = repo.to_string_lossy().into_owned();
        let config = config_in(&root);
        let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path), config, cx));
        cx.run_until_parked();
        let (wt_ix, wt_path) = view.read_with(cx, |m, _| {
            let ix = m.worktrees.iter().position(|w| !w.is_main).unwrap();
            (ix, m.worktrees[ix].path.clone())
        });
        view.update(cx, |m, cx| m.select_worktree(wt_ix, cx));
        cx.run_until_parked();
        assert!(view.read_with(cx, |m, _| !m.files.is_empty()));

        view.update(cx, |m, cx| m.remove_worktree(wt_path, cx));
        cx.run_until_parked();
        view.read_with(cx, |m, _| {
            assert_eq!(m.selected_wt, None);
            assert!(m.files.is_empty() && m.rows.is_empty());
            assert_eq!(m.graph_branch, "main", "the graph falls back to the default branch");
        });
    }

    #[gpui::test]
    fn non_git_folder_reports_why_and_can_be_closed(cx: &mut TestAppContext) {
        let root = std::env::temp_dir().join("maditor-native-test-plain");
        let _ = std::fs::remove_dir_all(&root);
        let plain = root.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let path = plain.to_string_lossy().into_owned();
        let config = config_in(&root);
        let (view, cx) = cx.add_window_view(|_, cx| Maditor::new(Some(path.clone()), config, cx));
        cx.run_until_parked();

        view.read_with(cx, |m, _| {
            assert_eq!(m.issue, Some("Not a git repository"));
            assert!(m.worktrees.is_empty());
        });

        view.update(cx, |m, cx| m.close_project(&path, cx));
        view.read_with(cx, |m, _| {
            assert!(m.config.projects.is_empty());
            assert!(m.repo.is_empty());
            assert_eq!(m.config.last_project, None);
        });
        // ...and the closure was persisted.
        assert!(config_in(&root).projects.is_empty());
    }
}
