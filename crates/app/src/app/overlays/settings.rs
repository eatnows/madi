//! The settings modal (Cmd+,): a tab per topic, so new settings (git, ...) only add a tab or a row.
use gpui::{div, prelude::*, px, AnyElement, Context, Focusable, IntoElement, MouseButton, SharedString, Window};
use madi_project::config::{Appearance, FONT_SIZE_RANGE};
use madi_ui::theme::*;

use crate::app::{workspace::TabBody, Madi, ModalCancel};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SettingsTab {
    General,
    Editor,
}

impl SettingsTab {
    const ALL: [SettingsTab; 2] = [SettingsTab::General, SettingsTab::Editor];

    fn label(self) -> &'static str {
        match self {
            SettingsTab::General => "General",
            SettingsTab::Editor => "Editor",
        }
    }
}

/// One setting: what it is and what it does on the left, its control on the right.
fn setting_row(title: &'static str, description: &'static str, control: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_6()
        .py_4()
        .border_b_1()
        .border_color(BORDER_SOFT())
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_color(TEXT_STRONG()).child(title))
                .child(div().text_xs().text_color(TEXT_DIM()).child(description)),
        )
        .child(control)
}

/// A bordered strip of cells; the app's one control shape for choices and steppers.
fn control_strip() -> gpui::Div {
    div().flex().flex_none().items_center().rounded_md().border_1().border_color(BORDER()).overflow_hidden()
}

fn cell(id: impl Into<gpui::ElementId>, first: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(26.))
        .min_w(px(28.))
        .px_3()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .when(!first, |d| d.border_l_1().border_color(BORDER()))
}

impl Madi {
    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.confirm.is_some() {
            return;
        }
        self.settings_open = true;
        window.focus(&self.modal_focus);
        cx.notify();
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_open = false;
        match self.active_tab().map(|t| &t.body) {
            Some(TabBody::File(editor)) => {
                let handle = editor.focus_handle(cx);
                window.focus(&handle);
            }
            _ => window.focus(&self.tree_focus),
        }
        cx.notify();
    }

    fn general_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.config.appearance;
        let choices = Appearance::ALL.into_iter().enumerate().map(|(i, a)| {
            let on = a == current;
            cell(("theme", i), i == 0)
                .cursor_pointer()
                .when(on, |d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                .when(!on, |d| d.text_color(TEXT_DIM()).hover(|d| d.bg(SELECTED())))
                .on_click(cx.listener(move |this, _, _, cx| this.set_appearance(a, cx)))
                .child(a.label())
        });
        setting_row("Theme", "Follow the system, or always use light or dark.", control_strip().children(choices)).into_any_element()
    }

    fn editor_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let size = self.config.font_size();
        let step = |id: &'static str, label: &'static str, delta: f32, enabled: bool, first: bool, cx: &mut Context<Self>| {
            cell(id, first)
                .text_color(if enabled { TEXT_STRONG() } else { TEXT_DIMMER() })
                .when(enabled, |d| d.cursor_pointer().hover(|d| d.bg(SELECTED())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if enabled {
                        this.set_font_size(this.config.font_size() + delta, cx)
                    }
                }))
                .child(label)
        };
        let control = control_strip()
            .child(step("font-dec", "−", -1., size > FONT_SIZE_RANGE.0, true, cx))
            .child(cell("font-size", false).w(px(44.)).font_family(MONO).text_color(TEXT_STRONG()).child(format!("{size}")))
            .child(step("font-inc", "+", 1., size < FONT_SIZE_RANGE.1, false, cx));

        let sample: SharedString = "fn main() {\n    println!(\"Hello, madi\");\n}".into();
        div()
            .child(setting_row("Font size", "Size of the editor's text, in pixels.", control))
            .child(
                div()
                    .mt_4()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(BORDER())
                    .bg(PANEL())
                    .font_family(MONO)
                    .text_size(px(size))
                    .line_height(px((size * 1.55).round()))
                    .text_color(TEXT())
                    .child(sample),
            )
            .into_any_element()
    }

    pub(crate) fn settings_modal(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.settings_open {
            return None;
        }
        let tab = self.settings_tab;
        let content = match tab {
            SettingsTab::General => self.general_settings(cx),
            SettingsTab::Editor => self.editor_settings(cx),
        };
        let nav = SettingsTab::ALL.into_iter().enumerate().map(|(i, t)| {
            let on = t == tab;
            div()
                .id(("settings-tab", i))
                .h(px(28.))
                .px_3()
                .flex()
                .items_center()
                .rounded_md()
                .text_xs()
                .cursor_pointer()
                .when(on, |d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                .when(!on, |d| d.text_color(TEXT_DIM()).hover(|d| d.bg(SELECTED())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings_tab = t;
                    cx.notify();
                }))
                .child(t.label())
        });

        let header = div()
            .flex_none()
            .h(px(40.))
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .bg(CHROME())
            .border_b_1()
            .border_color(BORDER())
            .child(div().text_color(TEXT_STRONG()).child("Settings"))
            .child(div().flex_1())
            .child(
                div()
                    .id("settings-close")
                    .size(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                    .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx)))
                    .child("×"),
            );

        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(div().w(px(176.)).flex_none().flex().flex_col().gap_1().p_2().bg(PANEL()).border_r_1().border_color(BORDER()).children(nav))
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .px_6()
                    .py_4()
                    .child(div().mb_2().text_size(px(15.)).text_color(TEXT_STRONG()).child(tab.label()))
                    .child(content),
            );

        Some(
            div()
                .id("settings-overlay")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::rgba(0x00000059))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| this.close_settings(window, cx)))
                .child(
                    div()
                        .id("settings")
                        .key_context("Modal")
                        .track_focus(&self.modal_focus)
                        .on_action(cx.listener(|this, _: &ModalCancel, window, cx| this.close_settings(window, cx)))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .w(px(760.))
                        .h(px(460.))
                        .max_w_full()
                        .max_h_full()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .rounded_lg()
                        .bg(BG())
                        .border_1()
                        .border_color(BORDER())
                        .shadow_lg()
                        .child(header)
                        .child(body),
                ),
        )
    }
}
