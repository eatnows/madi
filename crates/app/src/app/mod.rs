mod chrome;
mod git_panel;
mod overlays;
mod projects;
mod sidebar;
mod workspace;
mod worktrees;

use std::{
    collections::HashMap,
    rc::Rc,
};

use gpui::{
    actions, div, prelude::*, px, App, Context, FocusHandle, IntoElement,
    KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point,
    ScrollHandle, SharedString, UniformListScrollHandle, Window,
};
use maditor_git::{
    diff::FileDiff,
    diff_layout::DiffLayout,
    git_log::CommitInfo,
    graph::GraphRow,
    worktree::WorktreeInfo,
};

use maditor_project::{
    config::{Appearance, Config},
    scan::Issue,
    tree::TreeRow,
};

use maditor_ui::theme::*;

use overlays::{BranchPickerState, Confirm, ConfirmAction, MenuTarget, PickerTarget};

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





/// What the sidebar shows: the project's files, or its git worktrees and their changes.
#[derive(Clone, Copy, PartialEq)]
enum SidebarView {
    Files,
    Worktrees,
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
        maditor_ui::theme::set_dark(self.resolve_dark(window));
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
