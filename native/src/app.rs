use std::{collections::HashMap, ops::Range};

use gpui::{
    div, prelude::*, px, uniform_list, Context, HighlightStyle, IntoElement, SharedString,
    StyledText, Window,
};
use maditor_core::{
    diff::{self, DiffLine, FileDiff},
    worktree::{self, RepoStatus, WorktreeInfo},
};

use crate::theme::*;

const ROW_H: f32 = 20.0;

struct Cell {
    lineno: Option<usize>,
    tag: &'static str,
    text: SharedString,
    emphasis: Vec<Range<usize>>,
}

enum Row {
    Gap,
    Pair(Option<Cell>, Option<Cell>),
}

enum FileItem {
    Label(&'static str),
    File(usize),
}

pub struct Maditor {
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
}

fn cell_of(line: &DiffLine, new_side: bool) -> Cell {
    let mut text = String::new();
    let mut emphasis = Vec::new();
    for seg in &line.segments {
        let start = text.len();
        text.push_str(&seg.text);
        if seg.emphasized {
            emphasis.push(start..text.len());
        }
    }
    Cell {
        lineno: if new_side { line.new_lineno } else { line.old_lineno },
        tag: line.tag,
        text: text.into(),
        emphasis,
    }
}

/// Pairs delete/insert runs side by side so they render as aligned old|new columns.
fn build_rows(lines: &[DiffLine]) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        match line.tag {
            "gap" => {
                rows.push(Row::Gap);
                i += 1;
            }
            "equal" => {
                rows.push(Row::Pair(Some(cell_of(line, false)), Some(cell_of(line, true))));
                i += 1;
            }
            _ => {
                let mut deletes = Vec::new();
                while i < lines.len() && lines[i].tag == "delete" {
                    deletes.push(&lines[i]);
                    i += 1;
                }
                let mut inserts = Vec::new();
                while i < lines.len() && lines[i].tag == "insert" {
                    inserts.push(&lines[i]);
                    i += 1;
                }
                for k in 0..deletes.len().max(inserts.len()) {
                    rows.push(Row::Pair(
                        deletes.get(k).map(|l| cell_of(l, false)),
                        inserts.get(k).map(|l| cell_of(l, true)),
                    ));
                }
            }
        }
    }
    rows
}

impl Maditor {
    pub fn new(repo: String) -> Self {
        let mut this = Self {
            repo,
            issue: None,
            error: None,
            worktrees: Vec::new(),
            pins: HashMap::new(),
            selected_wt: None,
            files: Vec::new(),
            file_items: Vec::new(),
            selected_file: None,
            rows: Vec::new(),
        };
        if let Err(e) = this.load() {
            this.error = Some(e);
        }
        this
    }

    fn load(&mut self) -> Result<(), String> {
        match worktree::check_repo(self.repo.clone()) {
            RepoStatus::Ok => {}
            RepoStatus::NotARepo => {
                self.issue = Some("Not a git repository");
                return Ok(());
            }
            RepoStatus::Missing => {
                self.issue = Some("Folder not found");
                return Ok(());
            }
        }
        let branches = worktree::list_branches(self.repo.clone())?;
        let default = if branches.iter().any(|b| b == "main") {
            "main".to_string()
        } else {
            branches.first().cloned().unwrap_or_default()
        };
        for wt in worktree::list_worktrees(self.repo.clone(), HashMap::new())? {
            self.pins.insert(wt.path, default.clone());
        }
        self.worktrees = worktree::list_worktrees(self.repo.clone(), self.pins.clone())?;
        Ok(())
    }

    fn project_name(&self) -> String {
        self.repo.rsplit('/').find(|s| !s.is_empty()).unwrap_or(&self.repo).to_string()
    }

    fn select_worktree(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected_wt = Some(ix);
        self.files.clear();
        self.file_items.clear();
        self.selected_file = None;
        self.rows.clear();
        self.error = None;

        let path = self.worktrees[ix].path.clone();
        let base = self.pins.get(&path).cloned().unwrap_or_default();
        match diff::diff_against_base(path, base) {
            Ok(result) => {
                self.files = result.files;
                let mut last = "";
                for (i, f) in self.files.iter().enumerate() {
                    if f.section != last {
                        last = f.section;
                        self.file_items.push(FileItem::Label(if last == "committed" {
                            "COMMITTED"
                        } else {
                            "UNCOMMITTED"
                        }));
                    }
                    self.file_items.push(FileItem::File(i));
                }
                if !self.files.is_empty() {
                    self.select_file(0, cx);
                }
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    fn select_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected_file = Some(ix);
        self.rows = build_rows(&self.files[ix].lines);
        cx.notify();
    }

    fn render_cell(cell: &Option<Cell>) -> impl IntoElement {
        let (bg, fg, strong_bg, strong_fg) = match cell.as_ref().map(|c| c.tag) {
            Some("insert") => (Some(ADD_BG), ADD_FG, ADD_STRONG_BG, ADD_STRONG_FG),
            Some("delete") => (Some(DEL_BG), DEL_FG, DEL_STRONG_BG, DEL_STRONG_FG),
            _ => (None, TEXT, SELECTED, TEXT_STRONG),
        };
        let lineno = cell
            .as_ref()
            .and_then(|c| c.lineno)
            .map(|n| n.to_string())
            .unwrap_or_default();
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .h(px(ROW_H))
            .items_center()
            .overflow_hidden()
            .when_some(bg, |d, bg| d.bg(bg))
            .child(
                div()
                    .w(px(44.))
                    .flex_none()
                    .pr_2()
                    .text_right()
                    .text_color(TEXT_DIM)
                    .child(lineno),
            )
            .when_some(cell.as_ref(), |d, c| {
                let style = HighlightStyle {
                    color: Some(strong_fg.into()),
                    background_color: Some(strong_bg.into()),
                    ..Default::default()
                };
                let highlights = c.emphasis.iter().map(move |r| (r.clone(), style));
                d.child(
                    div()
                        .whitespace_nowrap()
                        .text_color(fg)
                        .child(StyledText::new(c.text.clone()).with_highlights(highlights)),
                )
            })
    }

    fn render_row(&self, ix: usize) -> impl IntoElement {
        let row = div().flex().w_full().h(px(ROW_H));
        match &self.rows[ix] {
            Row::Gap => row
                .justify_center()
                .items_center()
                .bg(PANEL)
                .text_color(TEXT_DIMMER)
                .child("⋯ unchanged ⋯"),
            Row::Pair(left, right) => row
                .child(Self::render_cell(left))
                .child(div().flex_1().min_w_0().border_l_1().border_color(BORDER).child(Self::render_cell(right))),
        }
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
                .on_click(cx.listener(move |this, _, _, cx| this.select_worktree(ix, cx)))
                .child(div().text_color(TEXT_STRONG).child(wt.name.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(TEXT_DIM)
                        .child(wt.branch.clone().unwrap_or_else(|| "(detached)".into())),
                )
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
                    .child(div().text_color(TEXT_STRONG).child(self.project_name()))
                    .child(div().text_xs().text_color(TEXT_DIM).child("Worktrees")),
            )
            .child(div().id("wt-list").flex_1().overflow_y_scroll().p_2().children(rows))
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
            FileItem::Label(text) => div()
                .id(("label", n))
                .px_2()
                .pt_3()
                .pb_1()
                .text_xs()
                .text_color(TEXT_DIM)
                .child(*text),
            FileItem::File(ix) => {
                let ix = *ix;
                let f = &self.files[ix];
                let (letter, color) = match f.status.as_str() {
                    "added" => ("A", GREEN),
                    "deleted" => ("D", RED),
                    "renamed" => ("R", AMBER),
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
                    .on_click(cx.listener(move |this, _, _, cx| this.select_file(ix, cx)))
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
            .child(div().id("file-list").flex_1().overflow_y_scroll().p_2().children(items))
    }
}

fn empty(message: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_color(TEXT_DIM)
        .child(message.into())
}

impl Render for Maditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let breadcrumb = {
            let mut s = format!("maditor / {}", self.project_name());
            if let Some(ix) = self.selected_wt {
                let wt = &self.worktrees[ix];
                s.push_str(&format!(" / {}", wt.branch.clone().unwrap_or_else(|| wt.name.clone())));
            }
            s
        };

        let body = if let Some(issue) = self.issue {
            div().flex_1().flex().child(empty(issue))
        } else if self.selected_wt.is_none() {
            div()
                .flex_1()
                .flex()
                .child(self.worktree_panel(cx))
                .child(empty("No data"))
        } else if self.files.is_empty() && self.error.is_none() {
            div()
                .flex_1()
                .flex()
                .child(self.worktree_panel(cx))
                .child(empty("No changes"))
        } else {
            div()
                .flex_1()
                .flex()
                .min_w_0()
                .child(self.worktree_panel(cx))
                .child(self.file_panel(cx))
                .child(
                    div().flex_1().min_w_0().bg(BG).child(
                        uniform_list(
                            "diff",
                            self.rows.len(),
                            cx.processor(|this, range: Range<usize>, _w, _cx| {
                                range.map(|ix| this.render_row(ix)).collect::<Vec<_>>()
                            }),
                        )
                        .size_full()
                        .font_family(MONO)
                        .text_size(px(12.)),
                    ),
                )
        };

        div()
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
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .w(px(48.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .items_center()
                            .pt_2()
                            .bg(CHROME)
                            .border_r_1()
                            .border_color(BORDER)
                            .child(
                                div()
                                    .size(px(32.))
                                    .rounded_md()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(TEXT_STRONG)
                                    .text_color(BG)
                                    .child(self.project_name().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()),
                            ),
                    )
                    .child(body),
            )
    }
}
