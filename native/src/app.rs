use std::{collections::HashMap, ops::Range};

use gpui::{
    actions, div, prelude::*, px, uniform_list, App, Context, FocusHandle, IntoElement, KeyBinding,
    MouseButton, MouseDownEvent, PathPromptOptions, Pixels, Point, ScrollHandle, SharedString,
    Window,
};
use maditor_core::{
    diff::{self, FileDiff},
    worktree::{self, RepoStatus, WorktreeInfo},
};

use crate::{
    config::Config,
    diff_view::{build_rows, render_row, Row},
    theme::*,
};

actions!(maditor, [SelectPrev, SelectNext]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, Some("NavList")),
        KeyBinding::new("down", SelectNext, Some("NavList")),
    ]);
}

enum FileItem {
    Label(&'static str),
    File(usize),
}

enum ScanOutcome {
    Issue(&'static str),
    Loaded { worktrees: Vec<WorktreeInfo>, pins: HashMap<String, String> },
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
    Ok(ScanOutcome::Loaded { worktrees, pins })
}

pub struct Maditor {
    config: Config,
    repo: String,
    issue: Option<&'static str>,
    error: Option<String>,
    worktrees: Vec<WorktreeInfo>,
    pins: HashMap<String, String>,
    selected_wt: Option<usize>,
    files: Vec<FileDiff>,
    file_items: Vec<FileItem>,
    selected_file: Option<usize>,
    rows: Vec<Row>,
    loading_diff: bool,
    /// Bumped per request so a slow, superseded scan/diff can't overwrite a newer one.
    scan_gen: u64,
    diff_gen: u64,
    menu: Option<(Point<Pixels>, String)>,
    wt_focus: FocusHandle,
    file_focus: FocusHandle,
    wt_scroll: ScrollHandle,
    file_scroll: ScrollHandle,
}

impl Maditor {
    pub fn new(initial: Option<String>, config: Config, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            config,
            repo: String::new(),
            issue: None,
            error: None,
            worktrees: Vec::new(),
            pins: HashMap::new(),
            selected_wt: None,
            files: Vec::new(),
            file_items: Vec::new(),
            selected_file: None,
            rows: Vec::new(),
            loading_diff: false,
            scan_gen: 0,
            diff_gen: 0,
            menu: None,
            wt_focus: cx.focus_handle(),
            file_focus: cx.focus_handle(),
            wt_scroll: ScrollHandle::new(),
            file_scroll: ScrollHandle::new(),
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
        self.selected_wt = None;
        self.files.clear();
        self.file_items.clear();
        self.selected_file = None;
        self.rows.clear();
        self.loading_diff = false;
        self.diff_gen += 1;
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
                    Ok(ScanOutcome::Loaded { worktrees, pins }) => {
                        this.worktrees = worktrees;
                        this.pins = pins.clone();
                        this.config.pins.insert(path, pins);
                        this.config.save();
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
                        this.menu = Some((ev.position, menu_path.clone()));
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
            div()
                .id(("wt", ix))
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
                        .text_xs()
                        .text_color(TEXT_DIM)
                        .child(format!("base: {base}"))
                        .child(status),
                )
        });
        div()
            .w(px(248.))
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
                let f = &self.files[ix];
                let (letter, color) = match f.status.as_str() {
                    "added" => ("A", GREEN),
                    "deleted" => ("D", RED),
                    _ => ("M", AMBER),
                };
                div()
                    .id(("file", n))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .when(self.selected_file == Some(ix), |d| d.bg(SELECTED))
                    .hover(|d| d.bg(SELECTED))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.file_focus);
                        this.select_file(ix, cx);
                    }))
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
        });
        div()
            .w(px(260.))
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
        let mut body = row().child(self.worktree_panel(cx));
        if self.selected_wt.is_none() {
            return body.child(empty("No data")).into_any_element();
        }
        if self.loading_diff {
            return body.child(empty("Loading…")).into_any_element();
        }
        if self.files.is_empty() && self.error.is_none() {
            return body.child(empty("No changes")).into_any_element();
        }
        body = body.child(self.file_panel(cx)).child(
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
        let (pos, path) = self.menu.clone()?;
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
                                .id("menu-close-project")
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .text_xs()
                                .text_color(TEXT_STRONG)
                                .cursor_pointer()
                                .hover(|d| d.bg(SELECTED))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.menu = None;
                                    this.close_project(&path, cx);
                                }))
                                .child("Close project"),
                        ),
                ),
        )
    }
}

fn empty(message: impl Into<SharedString>) -> impl IntoElement {
    div().flex_1().flex().items_center().justify_center().text_color(TEXT_DIM).child(message.into())
}

impl Render for Maditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut breadcrumb = format!("maditor / {}", Self::project_name(&self.repo));
        if let Some(ix) = self.selected_wt {
            let wt = &self.worktrees[ix];
            breadcrumb.push_str(&format!(" / {}", wt.branch.clone().unwrap_or_else(|| wt.name.clone())));
        }
        if self.repo.is_empty() {
            breadcrumb = "maditor".into();
        }

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
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
            .children(self.context_menu(cx))
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
