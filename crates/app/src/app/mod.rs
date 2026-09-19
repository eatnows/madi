mod chrome;
mod git_panel;
mod picker_view;
mod sidebar;
mod workspace;

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use gpui::{
    actions, div, prelude::*, px, App, Context, Entity, FocusHandle, Focusable, IntoElement,
    KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, PathPromptOptions, Pixels, Point,
    ScrollHandle, SharedString, Subscription, UniformListScrollHandle, Window,
};
use maditor_git::{
    branches::{self, BranchRow},
    diff::{self, FileDiff},
    diff_layout::DiffLayout,
    git_log::CommitInfo,
    graph::GraphRow,
    worktree::{self, WorktreeInfo},
};

use maditor_project::{
    config::{Appearance, Config},
    scan::{scan_repo, Issue, ScanOutcome},
    tree::TreeRow,
};

use crate::{
    scroll::{axis_locked, scroll_to_top},
    text_input::TextInput,
    theme::*,
};

actions!(maditor, [SelectPrev, SelectNext, PickerConfirm, PickerCancel, ModalCancel, TreeEnter, TreeExpand, TreeCollapse]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, Some("NavList")),
        KeyBinding::new("down", SelectNext, Some("NavList")),
        KeyBinding::new("up", SelectPrev, Some("Picker")),
        KeyBinding::new("down", SelectNext, Some("Picker")),
        KeyBinding::new("enter", PickerConfirm, Some("Picker")),
        KeyBinding::new("escape", PickerCancel, Some("Picker")),
        KeyBinding::new("escape", ModalCancel, Some("Modal")),
        KeyBinding::new("enter", TreeEnter, Some("FileTree")),
        KeyBinding::new("right", TreeExpand, Some("FileTree")),
        KeyBinding::new("left", TreeCollapse, Some("FileTree")),
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

/// What happens once the user confirms a destructive step.
#[derive(Clone)]
enum ConfirmAction {
    RemoveWorktree(String),
    CloseTab(usize),
    CloseProject(String),
}

/// The "are you sure" step before something destructive (deleting a worktree, discarding edits).
struct Confirm {
    title: String,
    message: String,
    label: String,
    action: ConfirmAction,
}

/// What the sidebar shows: the project's files, or its git worktrees and their changes.
#[derive(Clone, Copy, PartialEq)]
enum SidebarView {
    Files,
    Worktrees,
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
    Sidebar,
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
    sidebar: f32,
    git_height: f32,
    graph_pane: f32,
    commit_files: f32,
}

enum FileItem {
    Label(&'static str),
    File(usize),
}

pub struct Maditor {
    config: Config,
    repo: String,
    issue: Option<Issue>,
    error: Option<String>,
    worktrees: Vec<WorktreeInfo>,
    branches: Vec<String>,
    pins: HashMap<String, String>,
    picker: Option<BranchPickerState>,
    selected_wt: Option<usize>,
    files: Vec<FileDiff>,
    file_items: Vec<FileItem>,
    selected_file: Option<usize>,
    loading_diff: bool,
    /// Bumped per request so a slow, superseded scan/diff can't overwrite a newer one.
    scan_gen: u64,
    diff_gen: u64,
    menu: Option<(Point<Pixels>, MenuTarget)>,
    confirm: Option<Confirm>,
    sidebar_view: SidebarView,
    workspaces: HashMap<String, workspace::Workspace>,
    tree_rows: Vec<TreeRow>,
    tree_focus: FocusHandle,
    tree_scroll: ScrollHandle,
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
    commit_diff: Rc<DiffLayout>,
    cdiff_scroll: UniformListScrollHandle,
    cdiff_hscroll: ScrollHandle,
    selected_cfile: Option<usize>,
    commit_gen: u64,
    hovered_lane: Option<usize>,
    graph_focus: FocusHandle,
    cfile_focus: FocusHandle,
    graph_scroll: UniformListScrollHandle,
    graph_hscroll: ScrollHandle,
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
            loading_diff: false,
            scan_gen: 0,
            diff_gen: 0,
            menu: None,
            confirm: None,
            sidebar_view: SidebarView::Files,
            workspaces: HashMap::new(),
            tree_rows: Vec::new(),
            tree_focus: cx.focus_handle(),
            tree_scroll: ScrollHandle::new(),
            modal_focus: cx.focus_handle(),
            wt_focus: cx.focus_handle(),
            file_focus: cx.focus_handle(),
            wt_scroll: ScrollHandle::new(),
            file_scroll: ScrollHandle::new(),
            sizes: Sizes { sidebar: 280., git_height: 280., graph_pane: 460., commit_files: 220. },
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
            commit_diff: Rc::default(),
            cdiff_scroll: UniformListScrollHandle::new(),
            cdiff_hscroll: ScrollHandle::new(),
            selected_cfile: None,
            commit_gen: 0,
            hovered_lane: None,
            graph_focus: cx.focus_handle(),
            cfile_focus: cx.focus_handle(),
            graph_scroll: UniformListScrollHandle::new(),
            graph_hscroll: ScrollHandle::new(),
            cfile_scroll: ScrollHandle::new(),
        };
        if let Some(path) = initial.or_else(|| this.config.last_project.clone()) {
            this.open_project(path, cx);
        }
        this
    }

    fn cycle_appearance(&mut self, cx: &mut Context<Self>) {
        self.config.appearance = self.config.appearance.next();
        self.config.save();
        cx.notify();
    }

    fn resolve_dark(&self, window: &Window) -> bool {
        match self.config.appearance {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::System => {
                matches!(window.appearance(), gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark)
            }
        }
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
        self.refresh_tree();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let repo = path.clone();
            let result = cx.background_spawn(async move { scan_repo(repo, saved) }).await;
            this.update(cx, |this, cx| {
                if this.scan_gen != generation {
                    return;
                }
                match result {
                    Ok(ScanOutcome::Issue(issue)) => {
                        this.issue = Some(issue);
                        // Without git there are no worktrees to show, but the files can still be edited.
                        this.sidebar_view = SidebarView::Files;
                    }
                    Ok(ScanOutcome::Loaded(scan)) => {
                        this.worktrees = scan.worktrees;
                        this.branches = scan.branches;
                        this.pins = scan.pins.clone();
                        this.config.pins.insert(path, scan.pins);
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

    /// Closing a project drops its open files, so unsaved changes get a confirmation first.
    fn request_close_project(&mut self, path: &str, cx: &mut Context<Self>) {
        let dirty = self.dirty_tab_count(path, cx);
        if dirty == 0 {
            self.close_project(path, cx);
            return;
        }
        self.confirm = Some(Confirm {
            title: "Unsaved changes".into(),
            message: format!("{dirty} open file(s) in this project have unsaved changes. Close the project and discard them?"),
            label: "Discard".into(),
            action: ConfirmAction::CloseProject(path.to_string()),
        });
        cx.notify();
    }

    fn close_project(&mut self, path: &str, cx: &mut Context<Self>) {
        self.workspaces.remove(path);
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
        scroll_to_top(&self.file_scroll);
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
                        // The changes are listed, but no tab opens until one is picked: selecting a
                        // worktree shouldn't take over what the editor is showing.
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Picks a changed file of the selected worktree and opens its diff as a tab (a preview tab
    /// unless `preview` is false, so browsing with the arrow keys doesn't pile up tabs).
    fn select_file(&mut self, ix: usize, preview: bool, cx: &mut Context<Self>) {
        self.selected_file = Some(ix);
        self.open_diff(ix, preview, cx);
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

    fn picker_rows(&self, cx: &App) -> Vec<BranchRow> {
        match &self.picker {
            Some(p) => branches::flatten(&self.branches, &p.collapsed, p.input.read(cx).content()),
            None => Vec::new(),
        }
    }

    /// Enter/click on a picker row: a branch applies to the target, a folder toggles open/closed.
    fn picker_activate(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picked) = self.picker_rows(cx).into_iter().nth(row) else { return };
        match picked {
            BranchRow::Folder { full_path, .. } => {
                if let Some(p) = &mut self.picker {
                    if !p.collapsed.remove(&full_path) {
                        p.collapsed.insert(full_path);
                    }
                }
                cx.notify();
            }
            BranchRow::Branch { full_path, .. } => {
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
            self.select_file(next, true, cx);
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
                .when(active, |d| d.bg(TEXT_STRONG()).text_color(BG()))
                .when(!active, |d| d.border_1().border_color(BORDER()).text_color(TEXT_DIM()).hover(|d| d.bg(SELECTED())))
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
            .bg(CHROME())
            .border_r_1()
            .border_color(BORDER())
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
                    .text_color(TEXT_DIM())
                    .border_1()
                    .border_color(BORDER())
                    .hover(|d| d.bg(SELECTED()))
                    .on_click(cx.listener(|this, _, _, cx| this.add_project(cx)))
                    .child("+"),
            )
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
                        .bg(CHROME())
                        .border_1()
                        .border_color(BORDER())
                        .child(
                            div()
                                .id("menu-item")
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .text_xs()
                                .text_color(if danger { RED() } else { TEXT_STRONG() })
                                .cursor_pointer()
                                .hover(|d| d.bg(SELECTED()))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.menu = None;
                                    match &target {
                                        MenuTarget::Project(path) => this.request_close_project(path, cx),
                                        MenuTarget::Worktree(ix) => {
                                            if let Some(wt) = this.worktrees.get(*ix) {
                                                this.confirm = Some(Confirm {
                                                    title: "Remove worktree".into(),
                                                    message: format!(
                                                        "Remove worktree \"{}\" ({})? This deletes its working directory. Uncommitted changes will be lost.",
                                                        wt.name,
                                                        wt.branch.clone().unwrap_or_else(|| "detached".into())
                                                    ),
                                                    label: "Remove".into(),
                                                    action: ConfirmAction::RemoveWorktree(wt.path.clone()),
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

    fn run_confirmed(&mut self, action: ConfirmAction, cx: &mut Context<Self>) {
        match action {
            ConfirmAction::RemoveWorktree(path) => self.remove_worktree(path, cx),
            ConfirmAction::CloseTab(ix) => self.close_tab(ix, cx),
            ConfirmAction::CloseProject(path) => self.close_project(&path, cx),
        }
    }

    /// The second, explicit step before a worktree (and its uncommitted changes) is deleted.
    fn confirm_modal(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let c = self.confirm.as_ref()?;
        let (title, message, label, action) = (c.title.clone(), c.message.clone(), c.label.clone(), c.action.clone());
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
                        .bg(CHROME())
                        .border_1()
                        .border_color(BORDER())
                        .child(div().text_size(px(13.5)).text_color(TEXT_STRONG()).child(title))
                        .child(div().mt_2().text_xs().text_color(TEXT_DIM()).child(message))
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
                                        .border_color(BORDER())
                                        .text_xs()
                                        .text_color(TEXT_STRONG())
                                        .cursor_pointer()
                                        .hover(|d| d.bg(SELECTED()))
                                        .on_click(cx.listener(move |this, _, window, cx| close(this, window, cx)))
                                        .child("Cancel"),
                                )
                                .child(
                                    div()
                                        .id("modal-remove")
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(RED())
                                        .text_xs()
                                        .text_color(BG())
                                        .cursor_pointer()
                                        .hover(|d| d.opacity(0.9))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            close(this, window, cx);
                                            this.run_confirmed(action.clone(), cx);
                                        }))
                                        .child(label),
                                ),
                        ),
                ),
        )
    }
}

/// One changed-file row (status letter, path, +/- counts); the caller adds the click handler.
fn file_row(id: impl Into<gpui::ElementId>, f: &FileDiff, selected: bool) -> gpui::Stateful<gpui::Div> {
    let (letter, color) = match f.status.as_str() {
        "added" => ("A", GREEN()),
        "deleted" => ("D", RED()),
        _ => ("M", AMBER()),
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
        .when(selected, |d| d.bg(SELECTED()))
        .hover(|d| d.bg(SELECTED()))
        .child(div().w(px(12.)).flex_none().text_color(color).child(letter))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(TEXT())
                .child(f.path.clone()),
        )
        .child(div().text_xs().text_color(ADD_FG()).child(format!("+{}", f.additions)))
        .child(div().text_xs().text_color(DEL_FG()).child(format!("-{}", f.deletions)))
}

fn selected_path_matches(selected: &Option<usize>, worktrees: &[WorktreeInfo], path: Option<&str>) -> bool {
    match (selected, path) {
        (Some(i), Some(path)) => worktrees[*i].path == path,
        _ => false,
    }
}

fn empty(message: impl Into<SharedString>) -> impl IntoElement {
    div().flex_1().flex().items_center().justify_center().text_color(TEXT_DIM()).child(message.into())
}

impl Maditor {
    /// Everything right of the project rail: the sidebar and the editor area.
    fn main_area(&self, viewport_w: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        let row = || div().flex_1().min_w_0().flex();
        if self.repo.is_empty() {
            return row().child(empty("No project")).into_any_element();
        }
        if self.issue == Some(Issue::Missing) {
            let repo = self.repo.clone();
            return row()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(TEXT_DIM())
                .child("Folder not found")
                .child(
                    div()
                        .id("close-broken")
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(BORDER())
                        .text_xs()
                        .text_color(TEXT())
                        .cursor_pointer()
                        .hover(|d| d.bg(SELECTED()))
                        .on_click(cx.listener(move |this, _, _, cx| this.request_close_project(&repo, cx)))
                        .child("Close project"),
                )
                .into_any_element();
        }
        // Room for a diff tab: the window minus the rail, the sidebar and its drag handle.
        let editor_width = viewport_w - 48. - self.sizes.sidebar - 5.;
        row()
            .child(self.sidebar(cx))
            .child(self.resize_handle(Resize::Sidebar, cx))
            .child(self.editor_area(editor_width, cx))
            .into_any_element()
    }
}

impl Render for Maditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::set_dark(self.resolve_dark(window));
        let repo_ok = !self.repo.is_empty() && self.issue.is_none();
        let viewport_w = f32::from(window.viewport_size().width);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| this.on_root_mouse_move(ev, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.dragging = None))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| this.dragging = None))
            .bg(BG())
            .text_color(TEXT())
            .text_size(px(13.))
            .child(self.topbar(cx))
            .child(div().flex_1().min_h_0().flex().child(self.rail(cx)).child(self.main_area(viewport_w, cx)))
            .when(repo_ok && self.git_open, |d| d.child(self.git_panel(viewport_w, cx)))
            .child(self.status_bar(cx))
            .children(self.context_menu(cx))
            .children(self.confirm_modal(cx))
            .children(self.picker_overlay(window, cx))
    }
}

#[cfg(test)]
mod tests;
