//! Window chrome: the top bar (where you are) and the status bar (git panel toggle and details of
//! what's open).
use gpui::{div, prelude::*, px, Context, IntoElement};

use super::{workspace::{TabBody, TabKey}, Madi};
use madi_ui::theme::*;

impl Madi {
    /// The window's heading: what is open (bold) and where it lives (dim).
    pub(super) fn title_parts(&self) -> (String, String) {
        let file_name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = |p: &std::path::Path| p.parent().map(|d| d.display().to_string()).unwrap_or_default();
        let project = Self::project_name(&self.repo);
        let active = self.active_tab().map(|t| &t.key);
        if self.repo.is_empty() {
            return match active {
                Some(TabKey::File(path)) => (file_name(path), dir(path)),
                _ => ("Madi".into(), String::new()),
            };
        }
        match active {
            Some(TabKey::File(path)) => {
                let rel_dir = path.strip_prefix(&self.repo).map(dir).unwrap_or_else(|_| dir(path));
                let place = if rel_dir.is_empty() { project } else { format!("{project} · {rel_dir}") };
                (file_name(path), place)
            }
            Some(TabKey::Diff { path, .. }) => (file_name(std::path::Path::new(path)), format!("{project} · Diff")),
            None => (project, String::new()),
        }
    }

    fn icon_button<I: IntoElement>(
        &self,
        id: &'static str,
        active: bool,
        icon: fn(gpui::Rgba) -> I,
        on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    ) -> impl IntoElement {
        let color = if active { TEXT_STRONG() } else { TEXT_DIM() };
        div()
            .id(id)
            .size(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .cursor_pointer()
            .when(active, |d| d.bg(SELECTED()))
            .hover(|d| d.bg(SELECTED()))
            .on_click(on_click)
            .child(icon(color))
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
            .child(div().text_color(TEXT_STRONG()).child(self.title_parts().0))
            .child(div().text_xs().text_color(TEXT_DIM()).child(self.title_parts().1))
            .when_some(self.error.clone(), |d, e| d.child(div().text_color(RED()).text_xs().child(e)))
            .child(div().flex_1())
            .child(self.icon_button("focus-mode", self.focus_mode, madi_ui::icon::focus, cx.listener(|this, _, _, cx| this.toggle_focus_mode(cx))))
            .child(self.icon_button("settings", self.settings_open, madi_ui::icon::sliders, cx.listener(|this, _, window, cx| this.open_settings(window, cx))))
    }

    pub(super) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let repo_ok = !self.repo.is_empty() && self.issue.is_none();
        let graph_branch = self.git_panel.read(cx).branch().to_string();
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
                .when(!graph_branch.is_empty(), |d| {
                    d.child(div().font_family(MONO).text_color(TEXT_DIM()).child(graph_branch.clone()))
                })
            })
            .child(div().flex_1())
            .child(div().font_family(MONO).text_color(TEXT_DIM()).child(details))
    }
}
