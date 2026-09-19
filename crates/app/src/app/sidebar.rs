//! The sidebar: the project's files, or its git worktrees with the changes of the selected one.
use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, IntoElement, MouseButton, MouseDownEvent};

use super::{FileItem, Maditor, MenuTarget, PickerTarget, SelectNext, SelectPrev, SidebarView};
use maditor_project::scan::Issue;

use maditor_ui::{file_row::file_row, scroll::axis_locked, theme::*};

impl Maditor {
    pub(super) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let seg = |id: &'static str, label: &'static str, view: SidebarView, cx: &mut Context<Self>| {
            let active = self.sidebar_view == view;
            div()
                .id(id)
                .px_2()
                .py_1()
                .rounded_md()
                .text_xs()
                .cursor_pointer()
                .when(active, |d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                .when(!active, |d| d.text_color(TEXT_DIM()).hover(|d| d.text_color(TEXT_STRONG())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.sidebar_view = view;
                    cx.notify();
                }))
                .child(label)
        };
        let view = match self.sidebar_view {
            SidebarView::Files => div().flex_1().min_h_0().flex().flex_col().child(self.tree_view(cx)).into_any_element(),
            SidebarView::Worktrees => self.worktrees_view(cx),
        };
        div()
            .w(px(self.sidebar_width))
            .flex_none()
            .flex()
            .flex_col()
            .bg(PANEL())
            .border_r_1()
            .border_color(BORDER())
            .child(
                div()
                    .flex_none()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .border_b_1()
                    .border_color(BORDER_SOFT())
                    .child(div().px_1().text_color(TEXT_STRONG()).child(Self::project_name(&self.repo)))
                    .child(
                        div()
                            .mt_2()
                            .flex()
                            .gap_1()
                            .child(seg("view-files", "Files", SidebarView::Files, cx))
                            .child(seg("view-worktrees", "Worktrees", SidebarView::Worktrees, cx)),
                    ),
            )
            .child(view)
    }

    fn worktrees_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let note = |text: &'static str| {
            div().flex_1().flex().items_center().justify_center().text_xs().text_color(TEXT_DIM()).child(text).into_any_element()
        };
        if self.issue == Some(Issue::NotARepo) {
            return note("Not a git repository");
        }
        if self.worktrees.is_empty() {
            return note("No worktrees");
        }

        let rows = self.worktrees.iter().enumerate().map(|(ix, wt)| {
            let selected = self.selected_wt == Some(ix);
            let status = match (wt.ahead, wt.behind) {
                (Some(0), Some(0)) => "up to date".to_string(),
                (Some(a), Some(b)) => format!("↑{a} ↓{b}"),
                _ => String::new(),
            };
            let base = self.pins.get(&wt.path).cloned().unwrap_or_default();
            let wt_path = wt.path.clone();
            let is_main = wt.is_main;
            div()
                .id(("wt", ix))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        if !is_main {
                            this.menu = Some((ev.position, MenuTarget::Worktree(ix)));
                            cx.notify();
                        }
                    }),
                )
                .px_2()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |d| d.bg(SELECTED()))
                .hover(|d| d.bg(SELECTED()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.wt_focus);
                    this.select_worktree(ix, cx);
                }))
                .child(div().text_color(TEXT_STRONG()).child(wt.name.clone()))
                .child(div().text_xs().text_color(TEXT_DIM()).child(wt.branch.clone().unwrap_or_else(|| "(detached)".into())))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .text_color(TEXT_DIM())
                        .child(
                            div()
                                .id(("base", ix))
                                .px_1()
                                .rounded_sm()
                                .cursor_pointer()
                                .hover(|d| d.bg(BORDER()).text_color(TEXT_STRONG()))
                                .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                                    cx.stop_propagation();
                                    let anchor = ev.position();
                                    this.open_picker(PickerTarget::WorktreeBase(wt_path.clone()), anchor, window, cx);
                                }))
                                .child(format!("base: {base} ⌄")),
                        )
                        .child(status),
                )
        });

        let changes_title = match self.selected_wt {
            Some(ix) => format!("CHANGES · vs {}", self.pins.get(&self.worktrees[ix].path).cloned().unwrap_or_default()),
            None => "CHANGES".to_string(),
        };
        let items = self.file_items.iter().enumerate().map(|(n, item)| match item {
            FileItem::Label(text) => div().id(("label", n)).px_2().pt_3().pb_1().text_xs().text_color(TEXT_DIM()).child(*text),
            FileItem::File(ix) => {
                let ix = *ix;
                file_row(("file", n), &self.files[ix], self.selected_file == Some(ix)).on_click(cx.listener(
                    move |this, ev: &ClickEvent, window, cx| {
                        window.focus(&this.file_focus);
                        this.select_file(ix, ev.click_count() < 2, cx);
                    },
                ))
            }
        });

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                axis_locked(div().id("wt-list"))
                    .key_context("NavList")
                    .track_focus(&self.wt_focus)
                    .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_worktree(-1, cx)))
                    .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_worktree(1, cx)))
                    .flex_none()
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .track_scroll(&self.wt_scroll)
                    .p_2()
                    .children(rows),
            )
            .child(
                div()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_b_1()
                    .border_color(BORDER_SOFT())
                    .text_xs()
                    .text_color(TEXT_DIM())
                    .child(changes_title),
            )
            .child(
                axis_locked(div().id("file-list"))
                    .key_context("NavList")
                    .track_focus(&self.file_focus)
                    .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_file(-1, cx)))
                    .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_file(1, cx)))
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.file_scroll)
                    .p_2()
                    .children(items)
                    .when(self.selected_wt.is_none(), |d| {
                        d.child(div().px_2().pt_2().text_xs().text_color(TEXT_DIM()).child("Select a worktree to see its changes"))
                    })
                    .when(self.selected_wt.is_some() && self.files.is_empty() && !self.loading_diff, |d| {
                        d.child(div().px_2().pt_2().text_xs().text_color(TEXT_DIM()).child("No changes"))
                    }),
            )
            .into_any_element()
    }
}
