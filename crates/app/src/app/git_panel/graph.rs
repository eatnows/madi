//! Drawing the commit graph: the lane lines, curves and nodes of each row, and the row itself.
use gpui::{
    canvas, div, fill, point, prelude::*, px, size, Bounds, ClickEvent, Context, Corners, Hsla,
    IntoElement, PathBuilder, Window,
};
use madi_git::{graph::GraphRow, time::relative as relative_time};
use madi_ui::theme::*;

use super::{GitPanel, GRAPH_MIN_W};

const ROW_H: f32 = 36.0;
const LANE_W: f32 = 16.0;
const LANE_X0: f32 = 10.0;

fn lane_x(lane: usize) -> f32 {
    LANE_X0 + lane as f32 * LANE_W
}

impl GitPanel {
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

    pub(super) fn graph_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let commit = &self.commits[ix];
        let row = &self.rows[ix];
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
                    .child(relative_time(now, commit.timestamp)),
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

}
