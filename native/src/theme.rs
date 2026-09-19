//! Light and dark palettes (mirroring the webview app's CSS tokens). Colors are functions that
//! read the current mode, so a mode switch takes effect on the next frame with no plumbing.
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::Rgba;

static DARK: AtomicBool = AtomicBool::new(true);

pub fn set_dark(dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
}

pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

const fn rgb_c(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

macro_rules! palette {
    ($($name:ident: $dark:literal, $light:literal;)*) => {
        $(
            #[allow(non_snake_case)]
            pub fn $name() -> Rgba {
                if is_dark() { rgb_c($dark) } else { rgb_c($light) }
            }
        )*
    };
}

palette! {
    BG: 0x101112, 0xf7f7f6;
    CHROME: 0x151617, 0xffffff;
    PANEL: 0x131415, 0xfbfbfa;
    BORDER: 0x232425, 0xe4e4e2;
    BORDER_SOFT: 0x1c1d1e, 0xececea;
    TEXT: 0xc6c7c9, 0x3a3b3c;
    TEXT_STRONG: 0xe2e3e4, 0x1a1b1c;
    TEXT_DIM: 0x5c5d60, 0x8a8b8c;
    TEXT_DIMMER: 0x3d3e40, 0xc7c7c4;
    SELECTED: 0x1d1e1f, 0xeeeeec;
    AMBER: 0xa08256, 0x96793f;
    GREEN: 0x7fa87f, 0x4d7a4d;
    RED: 0xa87f7f, 0xa24d4d;
    ADD_BG: 0x17201a, 0xeaf3ea;
    ADD_FG: 0xa3bfa3, 0x3f6b3f;
    ADD_STRONG_BG: 0x2a4a2e, 0xcbe6cb;
    ADD_STRONG_FG: 0xcce6cc, 0x2c522c;
    DEL_BG: 0x1f1717, 0xfbeeee;
    DEL_FG: 0xb99a9a, 0x8a3f3f;
    DEL_STRONG_BG: 0x4a2a2a, 0xf2d0d0;
    DEL_STRONG_FG: 0xe8cccc, 0x6b2c2c;
}

pub const MONO: &str = "Menlo";

const LANES_DARK: [u32; 6] = [0xa08256, 0x7fa87f, 0xa87f7f, 0x8a8fbf, 0xbf8fbf, 0x8fb0bf];
const LANES_LIGHT: [u32; 6] = [0x96793f, 0x4d7a4d, 0xa24d4d, 0x5a60a0, 0xa05aa0, 0x4d80a0];

pub fn lane_color(lane: usize) -> Rgba {
    let palette = if is_dark() { &LANES_DARK } else { &LANES_LIGHT };
    rgb_c(palette[lane % palette.len()])
}
