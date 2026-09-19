mod chrome;
mod git_panel;
mod overlays;
mod projects;
mod sidebar;
mod workspace;
mod worktrees;

use git_panel::{GitPanel, GitPanelEvent};

use std::{
    collections::HashMap,
};

use gpui::{
    actions, div, prelude::*, px, App, Context, Entity, ExternalPaths, FocusHandle, IntoElement, KeyBinding,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, ScrollHandle, SharedString,
    Subscription, Window,
};
use maditor_git::{
    diff::FileDiff,
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


/// Draggable dividers of the app's own layout (the git panel manages its own).
#[derive(Clone, Copy, PartialEq)]
enum Resize {
    Sidebar,
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
    sidebar_width: f32,
    dragging: Option<(Resize, f32)>,
    git_open: bool,
    git_panel: Entity<GitPanel>,
    _git_panel_events: Subscription,
}

impl Maditor {
    pub fn new(initial: Option<String>, config: Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let git_panel = cx.new(|cx| GitPanel::new(cx));
        let git_panel_events = cx.subscribe_in(&git_panel, window, |this, _, event: &GitPanelEvent, window, cx| match event {
            GitPanelEvent::PickBranch(anchor) => this.open_picker(PickerTarget::GraphBranch, *anchor, window, cx),
            GitPanelEvent::Close => {
                this.git_open = false;
                cx.notify();
            }
            GitPanelEvent::Error(message) => {
                this.error = Some(message.clone());
                cx.notify();
            }
        });
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
            sidebar_width: 280.,
            dragging: None,
            git_open: false,
            git_panel,
            _git_panel_events: git_panel_events,
        };
        match initial {
            Some(path) if std::path::Path::new(&path).is_file() => this.open_paths(&[path.into()], window, cx),
            initial => {
                if let Some(path) = initial.or_else(|| this.config.last_project.clone()) {
                    this.open_project(path, cx);
                }
            }
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
            .when(self.has_loose_files(), |d| {
                let active = self.repo.is_empty();
                d.child(
                    div()
                        .id("loose-files")
                        .size(px(32.))
                        .rounded_md()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .when(active, |d| d.bg(TEXT_STRONG()).text_color(BG()))
                        .when(!active, |d| d.border_1().border_color(BORDER()).text_color(TEXT_DIM()).hover(|d| d.bg(SELECTED())))
                        .on_click(cx.listener(|this, _, _, cx| this.show_loose_files(cx)))
                        .child("≡"),
                )
            })
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



fn empty(message: impl Into<SharedString>) -> impl IntoElement {
    div().flex_1().flex().items_center().justify_center().text_color(TEXT_DIM()).child(message.into())
}

impl Maditor {
    /// Everything right of the project rail: the sidebar and the editor area.
    fn main_area(&self, viewport_w: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        let row = || div().flex_1().min_w_0().flex();
        if self.repo.is_empty() {
            if self.has_loose_files() {
                return row().child(self.editor_area(viewport_w - 48., cx)).into_any_element();
            }
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
        let editor_width = viewport_w - 48. - self.sidebar_width - 5.;
        row()
            .child(self.sidebar(cx))
            .child(maditor_ui::resize::handle("rz-sidebar", true, cx.listener(|this, ev: &MouseDownEvent, _, _| {
                this.dragging = Some((Resize::Sidebar, f32::from(ev.position.x)));
            })))
            .child(self.editor_area(editor_width, cx))
            .into_any_element()
    }
}

impl Maditor {
    /// Window-wide mouse moves drive whichever divider is being dragged (the sidebar's here, the
    /// git panel's own in the panel).
    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if let Some((Resize::Sidebar, last)) = self.dragging {
            let x = f32::from(ev.position.x);
            self.sidebar_width = (self.sidebar_width + x - last).clamp(200., 520.);
            self.dragging = Some((Resize::Sidebar, x));
            cx.notify();
        }
        if self.git_open {
            self.git_panel.update(cx, |panel, cx| panel.drag_to(ev, cx));
        }
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        self.dragging = None;
        self.git_panel.update(cx, |panel, _| panel.end_drag());
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
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| this.open_paths(paths.paths(), window, cx)))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| this.on_mouse_move(ev, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_drag(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_drag(cx)))
            .bg(BG())
            .text_color(TEXT())
            .text_size(px(13.))
            .child(self.topbar(cx))
            .child(div().flex_1().min_h_0().flex().child(self.rail(cx)).child(self.main_area(viewport_w, cx)))
            .when(repo_ok && self.git_open, |d| d.child(self.git_panel.clone()))
            .child(self.status_bar(cx))
            .children(self.context_menu(cx))
            .children(self.confirm_modal(cx))
            .children(self.picker_overlay(window, cx))
    }
}

#[cfg(test)]
mod tests;
