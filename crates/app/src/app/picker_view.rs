use gpui::{div, prelude::*, px, Context, IntoElement, MouseButton, Window};

use super::{Maditor, PickerCancel, PickerConfirm, PickerTarget, SelectNext, SelectPrev};
use maditor_ui::theme::*;
use maditor_git::branches::BranchRow;

const WIDTH: f32 = 260.0;
const LIST_MAX_H: f32 = 300.0;

impl Maditor {
    pub(super) fn picker_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let p = self.picker.as_ref()?;
        let current = match &p.target {
            PickerTarget::WorktreeBase(path) => self.pins.get(path).cloned().unwrap_or_default(),
            PickerTarget::GraphBranch => self.graph_branch.clone(),
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
                            super::axis_locked(div().id("picker-list"))
                                .max_h(px(LIST_MAX_H))
                                .overflow_y_scroll()
                                .p_1()
                                .children(rows),
                        ),
                ),
        )
    }
}
