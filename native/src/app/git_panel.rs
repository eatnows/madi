//! The bottom git panel: commit graph on the left, the selected commit's files and diff on the
//! right, plus the panel toggle bar and the drag-to-resize handles shared with the main panels.
use std::ops::Range;

use gpui::{
    canvas, div, fill, point, prelude::*, px, size, uniform_list, Bounds, ClickEvent, Context,
    CursorStyle, Corners, Hsla, IntoElement, MouseButton, MouseMoveEvent, PathBuilder, Pixels,
    Point, ScrollStrategy, Window,
};
use maditor_core::{diff, git_log};

use super::{
    axis_locked, list_scroll_to_top, DiffWhich, scroll_to_top, Maditor, PickerTarget, Resize, SelectNext,
    SelectPrev, GRAPH_PAGE,
};
use crate::{
    diff_view::DiffData,
    graph::{self, GraphRow},
    theme::*,
};

const ROW_H: f32 = 36.0;
const LANE_W: f32 = 16.0;
const LANE_X0: f32 = 10.0;
const GRAPH_MIN_W: f32 = 820.0;

fn lane_x(lane: usize) -> f32 {
    LANE_X0 + lane as f32 * LANE_W
}

impl Maditor {
    // ---- data flow -----------------------------------------------------------------------

    pub(super) fn clear_graph(&mut self) {
        self.graph_gen += 1;
        self.commits.clear();
        self.graph_rows.clear();
        self.has_more = true;
        self.fetching = false;
        list_scroll_to_top(&self.graph_scroll);
        scroll_to_top(&self.graph_hscroll);
        self.close_commit();
    }

    /// While following, the graph tracks the selected worktree's branch (or main/first branch when
    /// only a project is selected); a manual pick in the panel pins it instead.
    pub(super) fn sync_graph_branch(&mut self, cx: &mut Context<Self>) {
        if !self.follow_worktree {
            return;
        }
        let target = self
            .selected_wt
            .and_then(|i| self.worktrees.get(i))
            .and_then(|w| w.branch.clone())
            .or_else(|| {
                if self.branches.iter().any(|b| b == "main") {
                    Some("main".to_string())
                } else {
                    self.branches.first().cloned()
                }
            });
        if let Some(target) = target {
            if target != self.graph_branch {
                self.graph_branch = target;
                self.load_graph(cx);
            }
        }
    }

    pub(super) fn load_graph(&mut self, cx: &mut Context<Self>) {
        self.clear_graph();
        if self.graph_branch.is_empty() {
            return;
        }
        self.fetching = true;
        let generation = self.graph_gen;
        let (repo, branch) = (self.repo.clone(), self.graph_branch.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { git_log::git_log(repo, branch, 0, GRAPH_PAGE) })
                .await;
            this.update(cx, |this, cx| {
                if this.graph_gen != generation {
                    return;
                }
                this.fetching = false;
                match result {
                    Ok(commits) => {
                        this.has_more = commits.len() == GRAPH_PAGE;
                        this.commits = commits;
                        this.graph_rows = graph::compute_rows(&this.commits);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Infinite scroll: fetches the next page once the list nears its end.
    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.fetching || !self.has_more || self.graph_branch.is_empty() {
            return;
        }
        self.fetching = true;
        let generation = self.graph_gen;
        let (repo, branch, skip) = (self.repo.clone(), self.graph_branch.clone(), self.commits.len());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { git_log::git_log(repo, branch, skip, GRAPH_PAGE) })
                .await;
            this.update(cx, |this, cx| {
                if this.graph_gen != generation {
                    return;
                }
                this.fetching = false;
                match result {
                    Ok(more) => {
                        this.has_more = more.len() == GRAPH_PAGE;
                        this.commits.extend(more);
                        this.graph_rows = graph::compute_rows(&this.commits);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn close_commit(&mut self) {
        self.commit_gen += 1;
        self.selected_oid = None;
        self.commit_files.clear();
        self.commit_diff.clear();
        self.selected_cfile = None;
        scroll_to_top(&self.cfile_scroll);
        self.reset_cdiff_scroll();
    }

    fn reset_cdiff_scroll(&self) {
        list_scroll_to_top(&self.cdiff_scroll);
        scroll_to_top(&self.cdiff_hscroll);
    }

    /// `toggle`: clicking the already-selected commit deselects it (arrow keys never do).
    pub(super) fn select_commit(&mut self, ix: usize, toggle: bool, cx: &mut Context<Self>) {
        let oid = self.commits[ix].oid.clone();
        if toggle && self.selected_oid.as_deref() == Some(oid.as_str()) {
            self.close_commit();
            cx.notify();
            return;
        }
        self.close_commit();
        self.selected_oid = Some(oid.clone());
        self.detail_collapsed = false;
        let generation = self.commit_gen;
        let repo = self.repo.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { diff::diff_commit(repo, oid) }).await;
            this.update(cx, |this, cx| {
                if this.commit_gen != generation {
                    return;
                }
                match result {
                    Ok(files) => {
                        this.commit_files = files;
                        if !this.commit_files.is_empty() {
                            this.select_cfile(0, cx);
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

    fn select_cfile(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected_cfile = Some(ix);
        self.commit_diff = DiffData::new(&self.commit_files[ix].lines);
        self.reset_cdiff_scroll();
        cx.notify();
    }

    pub(super) fn move_commit(&mut self, delta: isize, cx: &mut Context<Self>) {
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

    pub(super) fn move_cfile(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.commit_files.is_empty() {
            return;
        }
        let current = self.selected_cfile.map(|i| i as isize).unwrap_or(-1);
        let next = (current + delta).clamp(0, self.commit_files.len() as isize - 1) as usize;
        if current != next as isize {
            self.cfile_scroll.scroll_to_item(next);
            self.select_cfile(next, cx);
        }
    }

    pub(super) fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some((kind, last)) = self.dragging else { return };
        let current = f32::from(if kind.horizontal() { position.x } else { position.y });
        let delta = current - last;
        let s = &mut self.sizes;
        match kind {
            Resize::WorktreePanel => s.worktree = (s.worktree + delta).clamp(180., 420.),
            Resize::FilePanel => s.files = (s.files + delta).clamp(180., 480.),
            // The handle sits on the panel's top edge, so dragging up (negative) grows it.
            Resize::GitHeight => s.git_height = (s.git_height - delta).clamp(160., 640.),
            Resize::GraphPane => s.graph_pane = (s.graph_pane + delta).clamp(300., 800.),
            Resize::CommitFiles => s.commit_files = (s.commit_files + delta).clamp(160., 400.),
            Resize::Tree => s.tree = (s.tree + delta).clamp(160., 480.),
        }
        self.dragging = Some((kind, current));
        cx.notify();
    }

    // ---- rendering -----------------------------------------------------------------------

    pub(super) fn resize_handle(&self, kind: Resize, cx: &mut Context<Self>) -> impl IntoElement {
        let id = match kind {
            Resize::WorktreePanel => "rz-wt",
            Resize::FilePanel => "rz-file",
            Resize::GitHeight => "rz-git",
            Resize::GraphPane => "rz-graph",
            Resize::CommitFiles => "rz-cfiles",
            Resize::Tree => "rz-tree",
        };
        let base = div()
            .id(id)
            .flex_none()
            .hover(|d| d.bg(BORDER()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _, _| {
                    let pos = if kind.horizontal() { ev.position.x } else { ev.position.y };
                    this.dragging = Some((kind, f32::from(pos)));
                }),
            );
        if kind.horizontal() {
            base.w(px(4.)).h_full().cursor(CursorStyle::ResizeLeftRight)
        } else {
            base.h(px(4.)).w_full().cursor(CursorStyle::ResizeUpDown)
        }
    }

    pub(super) fn on_root_mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        self.drag_to(ev.position, cx);
    }

    pub(super) fn bottom_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_none()
            .h(px(34.))
            .flex()
            .items_center()
            .px_3()
            .bg(CHROME())
            .border_t_1()
            .border_color(BORDER())
            .child(
                div()
                    .id("graph-tab")
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_xs()
                    .cursor_pointer()
                    .when(self.git_open, |d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                    .when(!self.git_open, |d| d.text_color(TEXT_DIM()).hover(|d| d.bg(PANEL())))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.git_open = !this.git_open;
                        cx.notify();
                    }))
                    .child("Graph"),
            )
    }

    fn lane_element(&self, ix: usize, row: &GraphRow, cx: &mut Context<Self>) -> impl IntoElement {
        let width = (row.max_lane + 1) as f32 * LANE_W + LANE_X0;
        let (lane, pass, conv, div_to, continues) =
            (row.lane, row.pass_through.clone(), row.converge_from.clone(), row.diverge_to.clone(), row.continues);
        let hovered = self.hovered_lane;

        let paint = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let at = |x: f32, y: f32| point(bounds.origin.x + px(x), bounds.origin.y + px(y));
                let tint = |l: usize| -> Hsla {
                    let c: Hsla = lane_color(l).into();
                    if hovered.is_none() || hovered == Some(l) { c } else { c.opacity(0.22) }
                };
                let stroke = |window: &mut Window, color: Hsla, build: &dyn Fn(&mut PathBuilder)| {
                    let mut b = PathBuilder::stroke(px(2.));
                    build(&mut b);
                    if let Ok(path) = b.build() {
                        window.paint_path(path, color);
                    }
                };
                for &l in &pass {
                    stroke(window, tint(l), &|b| {
                        b.move_to(at(lane_x(l), 0.));
                        b.line_to(at(lane_x(l), ROW_H));
                    });
                }
                stroke(window, tint(lane), &|b| {
                    b.move_to(at(lane_x(lane), 0.));
                    b.line_to(at(lane_x(lane), ROW_H / 2.));
                });
                if continues {
                    stroke(window, tint(lane), &|b| {
                        b.move_to(at(lane_x(lane), ROW_H / 2.));
                        b.line_to(at(lane_x(lane), ROW_H));
                    });
                }
                for &l in &conv {
                    stroke(window, tint(l), &|b| {
                        b.move_to(at(lane_x(l), 0.));
                        b.cubic_bezier_to(
                            at(lane_x(lane), ROW_H / 2.),
                            at(lane_x(l), 10.),
                            at(lane_x(lane), 8.),
                        );
                    });
                }
                for &l in &div_to {
                    stroke(window, tint(l), &|b| {
                        b.move_to(at(lane_x(lane), ROW_H / 2.));
                        b.cubic_bezier_to(
                            at(lane_x(l), ROW_H),
                            at(lane_x(lane), 28.),
                            at(lane_x(l), 26.),
                        );
                    });
                }
                let node = Bounds::new(at(lane_x(lane) - 5., ROW_H / 2. - 5.), size(px(10.), px(10.)));
                let mut quad = fill(node, tint(lane));
                quad.corner_radii = Corners::all(px(5.));
                window.paint_quad(quad);
            },
        )
        .absolute()
        .size_full();

        // Invisible hit regions: hovering exactly a line (not the whole row) highlights that lane.
        let hit = |kind: usize, l: usize, y0: f32, h: f32| {
            div()
                .id(("lane-hit", ix * 1024 + l * 4 + kind))
                .absolute()
                .left(px(lane_x(l) - 5.))
                .top(px(y0))
                .w(px(10.))
                .h(px(h))
                .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                    if *over {
                        this.hovered_lane = Some(l);
                    } else if this.hovered_lane == Some(l) {
                        this.hovered_lane = None;
                    }
                    cx.notify();
                }))
        };
        let mut regions = vec![hit(0, lane, 0., ROW_H)];
        regions.extend(row.pass_through.iter().map(|&l| hit(0, l, 0., ROW_H)));
        regions.extend(row.converge_from.iter().map(|&l| hit(1, l, 0., ROW_H / 2.)));
        regions.extend(row.diverge_to.iter().map(|&l| hit(2, l, ROW_H / 2., ROW_H / 2.)));

        div().relative().flex_none().w(px(width)).h(px(ROW_H)).child(paint).children(regions)
    }

    fn graph_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let commit = &self.commits[ix];
        let row = &self.graph_rows[ix];
        let selected = self.selected_oid.as_deref() == Some(commit.oid.as_str());
        let now = chrono::Utc::now().timestamp();
        div()
            .id(("commit", ix))
            .flex()
            .items_center()
            .gap_2()
            .h(px(ROW_H))
            .min_w(px(GRAPH_MIN_W))
            .px_4()
            .cursor_pointer()
            .when(selected, |d| d.bg(SELECTED()).border_l_2().border_color(AMBER()))
            .hover(|d| d.bg(SELECTED()))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                window.focus(&this.graph_focus);
                this.select_commit(ix, true, cx);
            }))
            .child(self.lane_element(ix, row, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(280.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(12.5))
                    .text_color(TEXT_STRONG())
                    .child(commit.summary.clone()),
            )
            .child(
                div()
                    .w(px(140.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(11.5))
                    .text_color(TEXT_DIM())
                    .child(div().size(px(16.)).flex_none().rounded_full().bg(lane_color(row.lane)))
                    .child(commit.author_name.clone()),
            )
            .child(
                div()
                    .w(px(96.))
                    .flex_none()
                    .text_size(px(11.))
                    .text_color(TEXT_DIM())
                    .child(graph::relative_time(now, commit.timestamp)),
            )
            .child(
                div()
                    .w(px(76.))
                    .flex_none()
                    .text_right()
                    .font_family(MONO)
                    .text_size(px(11.))
                    .text_color(TEXT_DIM())
                    .child(commit.short_oid.clone()),
            )
    }

    fn commit_detail(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.detail_collapsed {
            return None;
        }
        let oid = self.selected_oid.as_ref()?;
        let commit = self.commits.iter().find(|c| &c.oid == oid)?;
        let when = chrono::DateTime::from_timestamp(commit.timestamp, 0)
            .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        Some(
            div()
                .relative()
                .flex_none()
                .max_h(px(140.))
                .px_4()
                .py_2()
                .border_t_1()
                .border_color(BORDER_SOFT())
                .child(
                    div()
                        .id("commit-detail-close")
                        .absolute()
                        .top(px(6.))
                        .right(px(6.))
                        .px_2()
                        .rounded_md()
                        .cursor_pointer()
                        .text_color(TEXT_DIM())
                        .hover(|d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.detail_collapsed = true;
                            cx.notify();
                        }))
                        .child("×"),
                )
                .child(div().pr_6().text_size(px(12.5)).text_color(TEXT_STRONG()).child(commit.summary.clone()))
                .when(!commit.body.trim().is_empty(), |d| {
                    d.child(
                        axis_locked(div().id("commit-body"))
                            .mt_1()
                            .max_h(px(64.))
                            .overflow_y_scroll()
                            .font_family(MONO)
                            .text_size(px(11.5))
                            .text_color(TEXT_DIM())
                            .child(commit.body.trim().to_string()),
                    )
                })
                .child(
                    div()
                        .mt_1()
                        .text_size(px(10.5))
                        .text_color(TEXT_DIM())
                        .child(format!("{} <{}> · {} · {}", commit.author_name, commit.author_email, when, commit.oid)),
                ),
        )
    }

    pub(super) fn git_panel(&self, viewport_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let following = self.follow_worktree;
        let header = div()
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
                    .on_click(cx.listener(|this, ev: &ClickEvent, window, cx| {
                        this.open_picker(PickerTarget::GraphBranch, ev.position(), window, cx)
                    }))
                    .child(format!("{} ⌄", self.graph_branch)),
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
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.git_open = false;
                        cx.notify();
                    }))
                    .child("×"),
            );

        let graph_list = axis_locked(div().id("graph-scroll").flex_1().min_h_0().overflow_x_scroll())
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

        let graph_pane = div()
            .id("graph-pane")
            .key_context("NavList")
            .track_focus(&self.graph_focus)
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_commit(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_commit(1, cx)))
            .w(px(self.sizes.graph_pane))
            .flex_none()
            .flex()
            .flex_col()
            .min_h_0()
            .border_r_1()
            .border_color(BORDER())
            .child(graph_list)
            .children(self.commit_detail(cx));

        let right: gpui::AnyElement = if self.selected_oid.is_none() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(TEXT_DIM())
                .child("Select a commit to see its changed files")
                .into_any_element()
        } else {
            let items = self.commit_files.iter().enumerate().map(|(ix, f)| {
                super::file_row(("cfile", ix), f, self.selected_cfile == Some(ix)).on_click(cx.listener(
                    move |this, _, window, cx| {
                        window.focus(&this.cfile_focus);
                        this.select_cfile(ix, cx);
                    },
                ))
            });
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .child(
                    axis_locked(div().id("cfile-list"))
                        .key_context("NavList")
                        .track_focus(&self.cfile_focus)
                        .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_cfile(-1, cx)))
                        .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_cfile(1, cx)))
                        .w(px(self.sizes.commit_files))
                        .flex_none()
                        .overflow_y_scroll()
                        .track_scroll(&self.cfile_scroll)
                        .p_2()
                        .border_r_1()
                        .border_color(BORDER())
                        .children(items),
                )
                .child(self.resize_handle(Resize::CommitFiles, cx))
                .child(self.diff_pane(
                    DiffWhich::Commit,
                    viewport_w - self.sizes.graph_pane - self.sizes.commit_files - 11.,
                    cx,
                ))
                .into_any_element()
        };

        div()
            .relative()
            .flex_none()
            .h(px(self.sizes.git_height))
            .flex()
            .flex_col()
            .bg(PANEL())
            .border_t_1()
            .border_color(BORDER())
            .child(self.resize_handle(Resize::GitHeight, cx))
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(graph_pane)
                    .child(self.resize_handle(Resize::GraphPane, cx))
                    .child(right),
            )
    }
}
