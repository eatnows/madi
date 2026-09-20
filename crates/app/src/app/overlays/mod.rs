//! Modal layers over the app: the right-click context menu and the confirmation dialog that guards
//! anything destructive (removing a worktree, discarding unsaved edits).
mod branch_picker;
mod settings;

pub(crate) use branch_picker::{BranchPickerState, PickerTarget};
pub(crate) use settings::SettingsTab;

use gpui::{div, prelude::*, px, Context, IntoElement, MouseButton, Window};
use madi_ui::theme::*;

use crate::app::{Madi, ModalCancel};

pub(crate) enum MenuTarget {
    Project(String),
    Worktree(usize),
}

/// What happens once the user confirms a destructive step.
#[derive(Clone)]
pub(crate) enum ConfirmAction {
    RemoveWorktree(String),
    CloseTab(usize),
    CloseProject(String),
}

/// The "are you sure" step before something destructive (deleting a worktree, discarding edits).
pub(crate) struct Confirm {
    pub title: String,
    pub message: String,
    pub label: String,
    pub action: ConfirmAction,
}

impl Madi {
    pub(crate) fn context_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let (pos, target) = self.menu.as_ref()?;
        let (label, danger) = match target {
            MenuTarget::Project(_) => ("Close project", false),
            MenuTarget::Worktree(_) => ("Remove worktree…", true),
        };
        let target = match target {
            MenuTarget::Project(p) => MenuTarget::Project(p.clone()),
            MenuTarget::Worktree(i) => MenuTarget::Worktree(*i),
        };
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
                        .bg(CHROME())
                        .border_1()
                        .border_color(BORDER())
                        .child(
                            div()
                                .id("menu-item")
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .text_xs()
                                .text_color(if danger { RED() } else { TEXT_STRONG() })
                                .cursor_pointer()
                                .hover(|d| d.bg(SELECTED()))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.menu = None;
                                    match &target {
                                        MenuTarget::Project(path) => this.request_close_project(path, cx),
                                        MenuTarget::Worktree(ix) => {
                                            if let Some(wt) = this.worktrees.get(*ix) {
                                                this.confirm = Some(Confirm {
                                                    title: "Remove worktree".into(),
                                                    message: format!(
                                                        "Remove worktree \"{}\" ({})? This deletes its working directory. Uncommitted changes will be lost.",
                                                        wt.name,
                                                        wt.branch.clone().unwrap_or_else(|| "detached".into())
                                                    ),
                                                    label: "Remove".into(),
                                                    action: ConfirmAction::RemoveWorktree(wt.path.clone()),
                                                });
                                                window.focus(&this.modal_focus);
                                            }
                                        }
                                    }
                                    cx.notify();
                                }))
                                .child(label),
                        ),
                ),
        )
    }

    pub(crate) fn run_confirmed(&mut self, action: ConfirmAction, cx: &mut Context<Self>) {
        match action {
            ConfirmAction::RemoveWorktree(path) => self.remove_worktree(path, cx),
            ConfirmAction::CloseTab(ix) => self.close_tab(ix, cx),
            ConfirmAction::CloseProject(path) => self.close_project(&path, cx),
        }
    }

    /// The second, explicit step before a worktree (and its uncommitted changes) is deleted.
    pub(crate) fn confirm_modal(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let c = self.confirm.as_ref()?;
        let (title, message, label, action) = (c.title.clone(), c.message.clone(), c.label.clone(), c.action.clone());
        let close = |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            this.confirm = None;
            window.focus(&this.wt_focus);
            cx.notify();
        };
        Some(
            div()
                .id("modal-overlay")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::rgba(0x00000059))
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| close(this, window, cx)))
                .child(
                    div()
                        .id("modal")
                        .key_context("Modal")
                        .track_focus(&self.modal_focus)
                        .on_action(cx.listener(move |this, _: &ModalCancel, window, cx| close(this, window, cx)))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .w(px(360.))
                        .p_4()
                        .rounded_lg()
                        .bg(CHROME())
                        .border_1()
                        .border_color(BORDER())
                        .child(div().text_size(px(13.5)).text_color(TEXT_STRONG()).child(title))
                        .child(div().mt_2().text_xs().text_color(TEXT_DIM()).child(message))
                        .child(
                            div()
                                .mt_4()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    div()
                                        .id("modal-cancel")
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(BORDER())
                                        .text_xs()
                                        .text_color(TEXT_STRONG())
                                        .cursor_pointer()
                                        .hover(|d| d.bg(SELECTED()))
                                        .on_click(cx.listener(move |this, _, window, cx| close(this, window, cx)))
                                        .child("Cancel"),
                                )
                                .child(
                                    div()
                                        .id("modal-remove")
                                        .px_3()
                                        .py_1()
                                        .rounded_md()
                                        .bg(RED())
                                        .text_xs()
                                        .text_color(BG())
                                        .cursor_pointer()
                                        .hover(|d| d.opacity(0.9))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            close(this, window, cx);
                                            this.run_confirmed(action.clone(), cx);
                                        }))
                                        .child(label),
                                ),
                        ),
                ),
        )
    }
}
