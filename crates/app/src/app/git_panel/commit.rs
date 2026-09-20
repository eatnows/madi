//! The selected commit's message box beneath the graph.
use gpui::{div, prelude::*, px, Context, IntoElement};
use madi_ui::{scroll::axis_locked, theme::*};

use super::GitPanel;

impl GitPanel {
    pub(super) fn commit_detail(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
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
}
