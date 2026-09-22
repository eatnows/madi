//! Madi's colors, one set for light and one for dark, passed around as a value (no global switch).
use gyeol::Color;

// Not every color is used yet: the rest come into play as more of the app is ported.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color,
    pub chrome: Color,
    pub panel: Color,
    pub border: Color,
    pub border_soft: Color,
    pub text: Color,
    pub text_strong: Color,
    pub text_dim: Color,
    pub text_dimmer: Color,
    pub selected: Color,
    pub amber: Color,
    pub green: Color,
    pub red: Color,
    pub add_bg: Color,
    pub add_fg: Color,
    pub add_strong_bg: Color,
    pub add_strong_fg: Color,
    pub del_bg: Color,
    pub del_fg: Color,
    pub del_strong_bg: Color,
    pub del_strong_fg: Color,
    pub syntax_keyword: Color,
    pub syntax_string: Color,
    pub syntax_number: Color,
    pub syntax_type: Color,
}

impl Palette {
    pub fn new(dark: bool) -> Palette {
        let c = |dark_hex: u32, light_hex: u32| Color::hex(if dark { dark_hex } else { light_hex });
        Palette {
            bg: c(0x101112, 0xf7f7f6),
            chrome: c(0x151617, 0xffffff),
            panel: c(0x131415, 0xfbfbfa),
            border: c(0x232425, 0xe4e4e2),
            border_soft: c(0x1c1d1e, 0xececea),
            text: c(0xc6c7c9, 0x3a3b3c),
            text_strong: c(0xe2e3e4, 0x1a1b1c),
            text_dim: c(0x5c5d60, 0x8a8b8c),
            text_dimmer: c(0x3d3e40, 0xc7c7c4),
            selected: c(0x1d1e1f, 0xeeeeec),
            amber: c(0xa08256, 0x96793f),
            green: c(0x7fa87f, 0x4d7a4d),
            red: c(0xa87f7f, 0xa24d4d),
            add_bg: c(0x17201a, 0xeaf3ea),
            add_fg: c(0xa3bfa3, 0x3f6b3f),
            add_strong_bg: c(0x2a4a2e, 0xcbe6cb),
            add_strong_fg: c(0xcce6cc, 0x2c522c),
            del_bg: c(0x1f1717, 0xfbeeee),
            del_fg: c(0xb99a9a, 0x8a3f3f),
            del_strong_bg: c(0x4a2a2a, 0xf2d0d0),
            del_strong_fg: c(0xe8cccc, 0x6b2c2c),
            syntax_keyword: c(0x7aa2f7, 0x2952cc),
            syntax_string: c(0x9ece6a, 0x3f7d20),
            syntax_number: c(0xd19a66, 0xa15c00),
            syntax_type: c(0x7dcfff, 0x0f7b8a),
        }
    }

    /// The color for a highlight scope name a plugin's rules declared; an unrecognized scope (or
    /// a plugin-free file) falls back to the plain text color.
    pub fn syntax_color(&self, scope: &str) -> Color {
        match scope {
            "keyword" => self.syntax_keyword,
            "string" => self.syntax_string,
            "number" => self.syntax_number,
            "type" => self.syntax_type,
            "comment" => self.text_dim,
            _ => self.text,
        }
    }
}
