//! The branch picker: a searchable popup listing branches as "/"-grouped folders, used to choose a
//! worktree's base branch or the branch the git graph shows.
use std::collections::HashSet;

use gpui::{
    div, prelude::*, px, App, Context, Entity, Focusable, IntoElement, MouseButton, Pixels, Point,
    Subscription, Window,
};
use madi_git::branches::{self, BranchRow};
use madi_ui::{scroll::axis_locked, text_input::TextInput, theme::*};

use crate::app::{Madi, PickerCancel, PickerConfirm, SelectNext, SelectPrev};

const WIDTH: f32 = 260.0;
const LIST_MAX_H: f32 = 300.0;

/// What a picker selection applies to.
#[derive(Clone)]
pub(crate) enum PickerTarget {
    /// The base branch pinned for the worktree at this path.
    WorktreeBase(String),
    /// The branch the git panel's graph shows (pins it, ending "follow worktree").
    GraphBranch,
}

pub(crate) struct BranchPickerState {
    target: PickerTarget,
    anchor: Point<Pixels>,
    input: Entity<TextInput>,
    collapsed: HashSet<String>,
    highlighted: usize,
    last_query: String,
    _subscription: Subscription,
}

impl Madi {
    pub(crate) fn open_picker(&mut self, target: PickerTarget, anchor: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("Search branches", cx));
        window.focus(&input.focus_handle(cx));
        let subscription = cx.observe(&input, |this, input, cx| {
            let query = input.read(cx).content().to_string();
            if let Some(p) = &mut this.picker {
                if p.last_query != query {
                    p.last_query = query;
                    p.highlighted = 0;
                }
            }
            cx.notify();
        });
        self.picker = Some(BranchPickerState {
            target,
            anchor,
            input,
            collapsed: HashSet::new(),
            highlighted: 0,
            last_query: String::new(),
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(crate) fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.take().is_some() {
            window.focus(&self.wt_focus);
            cx.notify();
        }
    }

    pub(crate) fn picker_rows(&self, cx: &App) -> Vec<BranchRow> {
        match &self.picker {
            Some(p) => branches::flatten(&self.branches, &p.collapsed, p.input.read(cx).content()),
            None => Vec::new(),
        }
    }

    /// Enter/click on a picker row: a branch applies to the target, a folder toggles open/closed.
    pub(crate) fn picker_activate(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picked) = self.picker_rows(cx).into_iter().nth(row) else { return };
        match picked {
            BranchRow::Folder { full_path, .. } => {
                if let Some(p) = &mut self.picker {
                    if !p.collapsed.remove(&full_path) {
                        p.collapsed.insert(full_path);
                    }
                }
                cx.notify();
            }
            BranchRow::Branch { full_path, .. } => {
                let Some(target) = self.picker.as_ref().map(|p| p.target.clone()) else { return };
                self.close_picker(window, cx);
                match target {
                    PickerTarget::WorktreeBase(path) => self.change_base(path, full_path, cx),
                    PickerTarget::GraphBranch => {
                        self.git_panel.update(cx, |panel, cx| panel.pin(full_path, cx));
                    }
                }
            }
        }
    }

    pub(crate) fn move_picker(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.picker_rows(cx).len();
        if let Some(p) = &mut self.picker {
            if len > 0 {
                p.highlighted = (p.highlighted as isize + delta).clamp(0, len as isize - 1) as usize;
                cx.notify();
            }
        }
    }

    pub(crate) fn picker_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let p = self.picker.as_ref()?;
        let current = match &p.target {
            PickerTarget::WorktreeBase(path) => self.pins.get(path).cloned().unwrap_or_default(),
            PickerTarget::GraphBranch => self.git_panel.read(cx).branch().to_string(),
        };
        let viewport = window.viewport_size();
        let left = p.anchor.x.min(viewport.width - px(WIDTH + 8.)).max(px(8.));
        let top = (p.anchor.y + px(10.)).min(viewport.height - px(LIST_MAX_H + 60.)).max(px(8.));

        let rows = self.picker_rows(cx).into_iter().enumerate().map(|(i, row)| {
            let highlighted = i == p.highlighted;
            let base = div()
                .id(("picker-row", i))
                .flex()
                .items_center()
                .gap_1()
                .h(px(26.))
                .pr_2()
                .rounded_md()
                .text_xs()
                .font_family(MONO)
                .cursor_pointer()
                .when(highlighted, |d| d.bg(SELECTED()))
                .hover(|d| d.bg(SELECTED()))
                .on_click(cx.listener(move |this, _, window, cx| this.picker_activate(i, window, cx)));
            match row {
                BranchRow::Folder { segment, depth, full_path } => {
                    let open = !p.collapsed.contains(&full_path) || !p.last_query.is_empty();
                    base.pl(px(8. + depth as f32 * 14.))
                        .text_color(TEXT_DIM())
                        .child(if open { "▾" } else { "▸" })
                        .child(segment)
                }
                BranchRow::Branch { full_path, depth } => {
                    let label = full_path.rsplit('/').next().unwrap_or(&full_path).to_string();
                    let selected = full_path == current;
                    base.pl(px(8. + depth as f32 * 14.))
                        .justify_between()
                        .text_color(TEXT_STRONG())
                        .child(label)
                        .when(selected, |d| d.child(div().text_color(GREEN()).child("✓")))
                }
            }
        });

        Some(
            div()
                .id("picker-overlay")
                .absolute()
                .inset_0()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.close_picker(window, cx)))
                .on_mouse_down(MouseButton::Right, cx.listener(|this, _, window, cx| this.close_picker(window, cx)))
                .child(
                    div()
                        .id("picker")
                        .key_context("Picker")
                        .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_picker(-1, cx)))
                        .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_picker(1, cx)))
                        .on_action(cx.listener(|this, _: &PickerConfirm, window, cx| {
                            let row = this.picker.as_ref().map(|p| p.highlighted).unwrap_or(0);
                            this.picker_activate(row, window, cx);
                        }))
                        .on_action(cx.listener(|this, _: &PickerCancel, window, cx| this.close_picker(window, cx)))
                        // Clicks inside the popup must not reach the dismissing overlay.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .absolute()
                        .left(left)
                        .top(top)
                        .w(px(WIDTH))
                        .rounded_md()
                        .bg(CHROME())
                        .border_1()
                        .border_color(BORDER())
                        .child(div().border_b_1().border_color(BORDER_SOFT()).text_color(TEXT_STRONG()).child(p.input.clone()))
                        .child(
                            axis_locked(div().id("picker-list"))
                                .max_h(px(LIST_MAX_H))
                                .overflow_y_scroll()
                                .p_1()
                                .children(rows),
                        ),
                ),
        )
    }
}
