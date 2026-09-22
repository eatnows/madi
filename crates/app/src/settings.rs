//! Settings modal: choices persist immediately while the modal owns keyboard input.
use gyeol::{div, text, Color, Cx, Element, NamedKey};
use madi_project::config::{Appearance, FONT_SIZE_RANGE};

use crate::{app::Madi, editor::MONO, theme::Palette};

type El = Element<Madi>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsTab {
    General,
    Editor,
    Plugins,
}

impl SettingsTab {
    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Editor => "Editor",
            Self::Plugins => "Plugins",
        }
    }
}

fn rule(p: &Palette) -> El {
    div().w(1.).min_h(1.).bg(p.border)
}

fn setting_row(p: &Palette, title: &str, description: &str, control: El) -> El {
    div()
        .child(
            div()
                .row()
                .items_center()
                .justify_between()
                .gap(24.)
                .py(16.)
                .child(
                    div()
                        .gap(4.)
                        .child(text(title).text_size(14.).text_color(p.text_strong))
                        .child(text(description).text_size(12.).text_color(p.text_dim)),
                )
                .child(control),
        )
        .child(div().h(1.).bg(p.border_soft))
}

fn strip(p: &Palette, cells: Vec<El>) -> El {
    let mut row = div().row().border(1., p.border).rounded(6.);
    for (i, cell) in cells.into_iter().enumerate() {
        if i > 0 {
            row = row.child(rule(p));
        }
        row = row.child(cell);
    }
    row
}

fn cell(label: impl Into<String>, color: Color) -> El {
    div().min_w(28.).h(26.).px(12.).items_center().justify_center().child(text(label).text_size(12.).text_color(color))
}

impl Madi {
    pub fn open_settings(&mut self) {
        self.settings_open = true;
    }

    pub(crate) fn close_settings(&mut self) {
        self.settings_open = false;
    }

    fn set_appearance(&mut self, appearance: Appearance) {
        self.config.appearance = appearance;
        self.config.save();
    }

    fn set_font_size(&mut self, size: f32) {
        self.config.editor_font_size = Some(size.clamp(FONT_SIZE_RANGE.0, FONT_SIZE_RANGE.1));
        self.config.save();
    }

    fn general_settings(&self, p: &Palette) -> El {
        let choices = Appearance::ALL.into_iter().map(|appearance| {
            let active = self.config.appearance == appearance;
            cell(appearance.label(), if active { p.text_strong } else { p.text_dim })
                .bg(if active { p.selected } else { Color::TRANSPARENT })
                .hover_bg(p.selected)
                .on_click(move |s: &mut Madi, _| s.set_appearance(appearance))
        });
        setting_row(
            p,
            "Theme",
            "Follow the system, or always use light or dark.",
            strip(p, choices.collect()),
        )
    }

    fn editor_settings(&self, p: &Palette) -> El {
        let size = self.config.font_size();
        let step = |label, delta, enabled| {
            let button = cell(label, if enabled { p.text_strong } else { p.text_dimmer });
            if enabled {
                button.hover_bg(p.selected).on_click(move |s: &mut Madi, _| s.set_font_size(s.config.font_size() + delta))
            } else {
                button
            }
        };
        let control = strip(
            p,
            vec![
                step("−", -1., size > FONT_SIZE_RANGE.0),
                cell(size.to_string(), p.text_strong).w(44.).text_family(MONO),
                step("+", 1., size < FONT_SIZE_RANGE.1),
            ],
        );
        div()
            .child(setting_row(p, "Font size", "Size of the editor's text, in pixels.", control))
            .child(
                div()
                    .mt(16.)
                    .p(12.)
                    .rounded(6.)
                    .border(1., p.border)
                    .bg(p.panel)
                    .text_family(MONO)
                    .text_size(size)
                    .text_color(p.text)
                    .child(text("fn main() {"))
                    .child(text("    println!(\"Hello, madi\");"))
                    .child(text("}")),
            )
    }

    fn plugins_settings(&self, p: &Palette) -> El {
        let focused = self.plugin_install_focused;
        let url = self.plugin_install_url.clone();
        let shown = if url.is_empty() && !focused { "Plugin manifest URL (https:// or file://)".to_string() } else if focused { format!("{url}│") } else { url.clone() };
        let install_row = div().row().items_center().gap(8.)
            .child(
                div().grow().px(10.).py(7.).rounded(6.).border(1., if focused { p.amber } else { p.border }).bg(p.bg)
                    .on_click(|s: &mut Madi, _| s.focus_plugin_url())
                    .child(text(shown).text_size(12.).text_family(MONO).text_color(if url.is_empty() && !focused { p.text_dim } else { p.text_strong })),
            )
            .child(cell("Install", p.text_strong).bg(p.selected).rounded(6.).hover_bg(p.selected).on_click(|s: &mut Madi, _| s.install_plugin()));

        let list = if self.plugins.is_empty() {
            div().mt(20.).p(16.).rounded(6.).border(1., p.border_soft).bg(p.panel)
                .child(text("No plugins installed").text_size(13.).text_color(p.text_strong))
                .child(text("Paste a plugin manifest URL above to install its grammar and highlight query.").mt(6.).text_size(12.).text_color(p.text_dim))
        } else {
            let rows = self.plugins.iter().map(|installed| {
                let id = installed.manifest.id.clone();
                let extensions = installed.manifest.extensions.join(", ");
                div().col()
                    .child(
                        div().row().items_center().justify_between().py(10.)
                            .child(
                                div().gap(2.)
                                    .child(text(installed.manifest.name.clone()).text_size(13.).text_color(p.text_strong))
                                    .child(text(format!(".{extensions}  ·  v{}", installed.manifest.version)).text_size(11.).text_color(p.text_dim)),
                            )
                            .child(cell("Remove", p.red).hover_bg(p.selected).rounded(6.).on_click(move |s: &mut Madi, _| s.uninstall_plugin(id.clone()))),
                    )
                    .child(div().h(1.).bg(p.border_soft))
            });
            div().mt(20.).children(rows)
        };
        div()
            .child(setting_row(p, "Language plugins", "Install a language's syntax grammar and highlight query, by URL.", div()))
            .child(install_row)
            .child(list)
    }

    fn nav_item(&self, p: &Palette, tab: SettingsTab) -> El {
        let active = self.settings_tab == tab;
        div()
            .h(28.)
            .px(12.)
            .items_center()
            .rounded(6.)
            .bg(if active { p.selected } else { Color::TRANSPARENT })
            .hover_bg(p.selected)
            .on_click(move |s: &mut Madi, _| s.settings_tab = tab)
            .child(text(tab.label()).text_size(12.).text_color(if active { p.text_strong } else { p.text_dim }))
    }

    pub fn settings_modal(&self, p: &Palette) -> Option<El> {
        if !self.settings_open {
            return None;
        }
        let tab = self.settings_tab;
        let content = match tab {
            SettingsTab::General => self.general_settings(p),
            SettingsTab::Editor => self.editor_settings(p),
            SettingsTab::Plugins => self.plugins_settings(p),
        };
        let modal = div()
            .w(760.)
            .h(460.)
            .rounded(10.)
            .border(1., p.border)
            .bg(p.bg)
            .overflow_hidden()
            // This no-op consumes clicks inside the dialog before they can reach the backdrop.
            .on_click(|_: &mut Madi, _| {})
            .child(
                div()
                    .row()
                    .items_center()
                    .justify_between()
                    .h(40.)
                    .px(16.)
                    .bg(p.chrome)
                    .child(text("Settings").text_size(14.).text_color(p.text_strong))
                    .child(
                        div()
                            .size(24.)
                            .items_center()
                            .justify_center()
                            .rounded(6.)
                            .hover_bg(p.selected)
                            .on_click(|s: &mut Madi, _| s.close_settings())
                            .child(text("×").text_size(14.).text_color(p.text_dim)),
                    ),
            )
            .child(div().h(1.).bg(p.border))
            .child(
                div()
                    .row()
                    .grow()
                    .child(
                        div()
                            .w(176.)
                            .p(8.)
                            .gap(4.)
                            .bg(p.panel)
                            .child(self.nav_item(p, SettingsTab::General))
                            .child(self.nav_item(p, SettingsTab::Editor))
                            .child(self.nav_item(p, SettingsTab::Plugins)),
                    )
                    .child(rule(p))
                    .child(
                        div()
                            .grow()
                            .px(24.)
                            .py(16.)
                            .child(text(tab.label()).text_size(15.).text_color(p.text_strong).mb(8.))
                            .child(content),
                    ),
            );
        Some(
            div()
                .inset(0.)
                .items_center()
                .justify_center()
                .bg(Color::hex(0x000000).with_alpha(0.35))
                .on_click(|s: &mut Madi, _| s.close_settings())
                .child(modal),
        )
    }

    pub fn settings_key(&mut self, key: &gyeol::Key, cx: &Cx) -> bool {
        if !self.settings_open {
            return false;
        }
        if matches!(key, gyeol::Key::Named(NamedKey::Escape)) {
            self.close_settings();
        } else if matches!(key, gyeol::Key::Char(c) if c == ",") && cx.modifiers.command() {
            self.close_settings();
        }
        true
    }
}
