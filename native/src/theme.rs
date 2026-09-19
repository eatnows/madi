//! Dark palette, mirrored from the webview app's CSS tokens so both frontends look the same.
use gpui::Rgba;

pub const BG: Rgba = rgb_c(0x101112);
pub const CHROME: Rgba = rgb_c(0x151617);
pub const PANEL: Rgba = rgb_c(0x131415);
pub const BORDER: Rgba = rgb_c(0x232425);
pub const BORDER_SOFT: Rgba = rgb_c(0x1c1d1e);
pub const TEXT: Rgba = rgb_c(0xc6c7c9);
pub const TEXT_STRONG: Rgba = rgb_c(0xe2e3e4);
pub const TEXT_DIM: Rgba = rgb_c(0x5c5d60);
pub const TEXT_DIMMER: Rgba = rgb_c(0x3d3e40);
pub const SELECTED: Rgba = rgb_c(0x1d1e1f);
pub const AMBER: Rgba = rgb_c(0xa08256);
pub const GREEN: Rgba = rgb_c(0x7fa87f);
pub const RED: Rgba = rgb_c(0xa87f7f);
pub const ADD_BG: Rgba = rgb_c(0x17201a);
pub const ADD_FG: Rgba = rgb_c(0xa3bfa3);
pub const ADD_STRONG_BG: Rgba = rgb_c(0x2a4a2e);
pub const ADD_STRONG_FG: Rgba = rgb_c(0xcce6cc);
pub const DEL_BG: Rgba = rgb_c(0x1f1717);
pub const DEL_FG: Rgba = rgb_c(0xb99a9a);
pub const DEL_STRONG_BG: Rgba = rgb_c(0x4a2a2a);
pub const DEL_STRONG_FG: Rgba = rgb_c(0xe8cccc);

pub const MONO: &str = "Menlo";

// `gpui::rgb` isn't const, so build the value by hand for use in `const` items.
const fn rgb_c(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}
