//! Window chrome: the top bar (where you are) and the status bar (git panel toggle and details of
//! what's open).
use gpui::{div, prelude::*, px, Context, IntoElement};

use super::{workspace::{TabBody, TabKey}, Maditor};
use crate::theme::*;

impl Maditor {
    /// `maditor / project / path/of/the/active/file`.
    fn breadcrumb(&self) -> String {
        if self.repo.is_empty() {
            return "maditor".into();
        }
        let mut crumb = format!("maditor / {}", Self::project_name(&self.repo));
        match self.active_tab().map(|t| (&t.key, &t.title)) {
            Some((TabKey::File(path), _)) => {
                let rel = path.strip_prefix(&self.repo).unwrap_or(path).to_string_lossy().into_owned();
                crumb.push_str(&format!(" / {rel}"));
            }
            Some((TabKey::Diff { path, .. }, _)) => crumb.push_str(&format!(" / {path} (diff)")),
            None => {}
        }
        crumb
    }

    pub(super) fn topbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(40.))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .bg(CHROME())
            .border_b_1()
            .border_color(BORDER())
            .font_family(MONO)
            .text_color(TEXT_DIM())
            .child(self.breadcrumb())
            .when_some(self.error.clone(), |d, e| d.child(div().text_color(RED()).text_xs().child(e)))
            .child(div().flex_1())
            .child(
                div()
                    .id("appearance")
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_xs()
                    .cursor_pointer()
                    .text_color(TEXT_DIM())
                    .hover(|d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                    .on_click(cx.listener(|this, _, _, cx| this.cycle_appearance(cx)))
                    .child(self.config.appearance.label()),
            )
    }

    pub(super) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let repo_ok = !self.repo.is_empty() && self.issue.is_none();
        let details = match self.active_tab().map(|t| &t.body) {
            Some(TabBody::File(editor)) => {
                let editor = editor.read(cx);
                let (line, col) = editor.cursor_display();
                format!("Ln {line}, Col {col}   {}   UTF-8", editor.line_ending())
            }
            Some(TabBody::Diff(diff)) => format!("+{}  -{}", diff.additions, diff.deletions),
            None => String::new(),
        };
        div()
            .flex_none()
            .h(px(28.))
            .flex()
            .items_center()
            .px_3()
            .gap_3()
            .bg(CHROME())
            .border_t_1()
            .border_color(BORDER())
            .text_xs()
            .when(repo_ok, |d| {
                d.child(
                    div()
                        .id("graph-tab")
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .cursor_pointer()
                        .when(self.git_open, |d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                        .when(!self.git_open, |d| d.text_color(TEXT_DIM()).hover(|d| d.bg(PANEL())))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.git_open = !this.git_open;
                            cx.notify();
                        }))
                        .child("Graph"),
                )
                .when(!self.graph_branch.is_empty(), |d| {
                    d.child(div().font_family(MONO).text_color(TEXT_DIM()).child(self.graph_branch.clone()))
                })
            })
            .child(div().flex_1())
            .child(div().font_family(MONO).text_color(TEXT_DIM()).child(details))
    }
}
