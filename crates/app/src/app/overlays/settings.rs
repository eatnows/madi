//! The settings modal (Cmd+,): a tab per topic, so new settings (git, ...) only add a tab or a row.
use gpui::{div, prelude::*, px, Context, Focusable, IntoElement, MouseButton, Window};
use madi_project::config::{Appearance, FONT_SIZE_RANGE};
use madi_ui::theme::*;

use crate::app::{Madi, ModalCancel};

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
            Some(super::super::workspace::TabBody::File(editor)) => {
                let handle = editor.focus_handle(cx);
                window.focus(&handle);
            }
            _ => window.focus(&self.tree_focus),
        }
        cx.notify();
    }

    fn setting_row(label: &'static str) -> gpui::Div {
        div().flex().items_center().justify_between().h(px(32.)).child(div().text_xs().text_color(TEXT_DIM()).child(label))
    }

    fn general_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.config.appearance;
        let themes = Appearance::ALL.into_iter().map(|a| {
            let on = a == current;
            div()
                .id(("theme", a as usize))
                .px_3()
                .py_1()
                .rounded_md()
                .text_xs()
                .cursor_pointer()
                .when(on, |d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                .when(!on, |d| d.text_color(TEXT_DIM()).hover(|d| d.bg(SELECTED())))
                .on_click(cx.listener(move |this, _, _, cx| this.set_appearance(a, cx)))
                .child(a.label())
        })
        .collect::<Vec<_>>();

        Self::setting_row("Theme").child(div().flex().gap_1().children(themes))
    }

    fn editor_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let size = self.config.font_size();
        let stepper = |id: &'static str, label: &'static str, delta: f32, enabled: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .border_1()
                .border_color(BORDER())
                .text_color(if enabled { TEXT_STRONG() } else { TEXT_DIMMER() })
                .when(enabled, |d| d.cursor_pointer().hover(|d| d.bg(SELECTED())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if enabled {
                        this.set_font_size(this.config.font_size() + delta, cx)
                    }
                }))
                .child(label)
        };
        let dec = stepper("font-dec", "−", -1., size > FONT_SIZE_RANGE.0, cx);
        let inc = stepper("font-inc", "+", 1., size < FONT_SIZE_RANGE.1, cx);

        Self::setting_row("Font size").child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(dec)
                .child(div().w(px(24.)).text_center().text_xs().text_color(TEXT_STRONG()).child(format!("{size}")))
                .child(inc),
        )
    }

    pub(crate) fn settings_modal(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.settings_open {
            return None;
        }
        let content = match self.settings_tab {
            SettingsTab::General => self.general_settings(cx).into_any_element(),
            SettingsTab::Editor => self.editor_settings(cx).into_any_element(),
        };
        let tabs = SettingsTab::ALL.into_iter().enumerate().map(|(i, tab)| {
            let on = tab == self.settings_tab;
            div()
                .id(("settings-tab", i))
                .px_3()
                .py_1()
                .rounded_md()
                .text_xs()
                .cursor_pointer()
                .when(on, |d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                .when(!on, |d| d.text_color(TEXT_DIM()).hover(|d| d.bg(SELECTED())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings_tab = tab;
                    cx.notify();
                }))
                .child(tab.label())
        });

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
                        .w(px(520.))
                        .p_4()
                        .rounded_lg()
                        .bg(CHROME())
                        .border_1()
                        .border_color(BORDER())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(div().text_size(px(13.5)).text_color(TEXT_STRONG()).child("Settings"))
                                .child(
                                    div()
                                        .id("settings-close")
                                        .px_2()
                                        .rounded_md()
                                        .text_color(TEXT_DIM())
                                        .cursor_pointer()
                                        .hover(|d| d.bg(SELECTED()).text_color(TEXT_STRONG()))
                                        .on_click(cx.listener(|this, _, window, cx| this.close_settings(window, cx)))
                                        .child("×"),
                                ),
                        )
                        .child(
                            div()
                                .mt_3()
                                .flex()
                                .gap_4()
                                .child(div().w(px(110.)).flex_none().flex().flex_col().gap_1().children(tabs))
                                .child(div().flex_1().min_w_0().child(content)),
                        ),
                ),
        )
    }
}
