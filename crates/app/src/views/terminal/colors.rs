//! Terminal colors (spec 05 §3.10): default fg/bg, cursor and selection from the theme
//! tokens, ANSI 0-15 from VS Code's palettes (light or dark), 16-255 the xterm cube and
//! gray ramp, plus whatever the running app set with OSC 4/10/11/12.

use gpui::{Hsla, Rgba};
use tern_terminal::{CellStyle, ColorScheme, TerminalSnapshot, color_rgb};

use crate::theme::color::mix_srgb;
use crate::tokens::Palette;

/// Faint text: 155/255 fg + 100/255 bg.
pub const FAINT_WEIGHT: f32 = 155. / 255.;

pub fn rgb_hsla(rgb: u32) -> Hsla {
    Rgba {
        r: ((rgb >> 16) & 0xff) as f32 / 255.,
        g: ((rgb >> 8) & 0xff) as f32 / 255.,
        b: (rgb & 0xff) as f32 / 255.,
        a: 1.,
    }
    .into()
}

fn hsla_rgb(c: Hsla) -> u32 {
    let r = Rgba::from(c);
    let byte = |v: f32| ((v.clamp(0., 1.) * 255.).round() as u32) & 0xff;
    (byte(r.r) << 16) | (byte(r.g) << 8) | byte(r.b)
}

#[derive(Clone, Debug)]
pub struct TermColors {
    pub foreground: Hsla,
    pub background: Hsla,
    pub cursor: Hsla,
    pub selection: Hsla,
    scheme: ColorScheme,
    /// OSC overrides: palette index (0-255, 256 fg, 257 bg, 258 cursor) → rgb.
    overrides: Vec<(u16, u32)>,
}

impl TermColors {
    pub fn new(tokens: &Palette, dark: bool) -> Self {
        let mut scheme = if dark {
            ColorScheme::dark()
        } else {
            ColorScheme::light()
        };
        scheme.foreground = hsla_rgb(tokens.terminal_foreground);
        scheme.background = hsla_rgb(tokens.terminal_background);
        scheme.cursor = hsla_rgb(tokens.terminal_cursor);
        Self {
            foreground: tokens.terminal_foreground,
            background: tokens.terminal_background,
            cursor: tokens.terminal_cursor,
            selection: tokens.terminal_selection_background,
            scheme,
            overrides: Vec::new(),
        }
    }

    /// The scheme the PTY answers OSC color queries with.
    pub fn scheme(&self) -> ColorScheme {
        self.scheme.clone()
    }

    /// Applies the snapshot's OSC overrides.
    pub fn with_overrides(mut self, snapshot: &TerminalSnapshot) -> Self {
        self.overrides = snapshot.color_overrides.clone();
        for &(index, value) in &snapshot.color_overrides {
            let c = rgb_hsla(value & 0x00ff_ffff);
            match index {
                256 => self.foreground = c,
                257 => self.background = c,
                258 => self.cursor = c,
                _ => {}
            }
        }
        self
    }

    pub fn indexed(&self, index: u8) -> Hsla {
        if let Some((_, v)) = self.overrides.iter().find(|(i, _)| *i == index as u16) {
            return rgb_hsla(v & 0x00ff_ffff);
        }
        rgb_hsla(self.scheme.resolve(index as usize))
    }

    fn color(&self, value: Option<u32>) -> Option<Hsla> {
        value.map(|v| match color_rgb(v) {
            Ok(index) => self.indexed(index),
            Err(rgb) => rgb_hsla(rgb),
        })
    }

    /// The (foreground, background) a cell draws with. Background is None when it is
    /// the terminal's default, so the pane fill shows through. Inverse swaps; faint
    /// blends toward the background.
    pub fn cell(&self, style: &CellStyle) -> (Hsla, Option<Hsla>) {
        let mut fg = self.color(style.fg);
        let mut bg = self.color(style.bg);
        if style.inverse {
            let new_bg = fg.unwrap_or(self.foreground);
            let new_fg = bg.unwrap_or(self.background);
            fg = Some(new_fg);
            bg = Some(new_bg);
        }
        let mut fg = fg.unwrap_or(self.foreground);
        if style.dim {
            fg = mix_srgb(fg, bg.unwrap_or(self.background), FAINT_WEIGHT);
        }
        (fg, bg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(dark: bool) -> TermColors {
        let tokens = if dark { Palette::dark() } else { Palette::light() };
        TermColors::new(&tokens, dark)
    }

    fn hex(c: Hsla) -> u32 {
        hsla_rgb(c)
    }

    #[test]
    fn ansi_palettes() {
        let d = colors(true);
        let l = colors(false);
        assert_eq!(hex(d.indexed(2)), 0x0dbc79);
        assert_eq!(hex(l.indexed(2)), 0x107c10);
        assert_eq!(hex(d.indexed(12)), 0x3b8eea);
        assert_eq!(hex(l.indexed(15)), 0xa5a5a5);
        // Cube and gray ramp.
        assert_eq!(hex(d.indexed(16)), 0x000000);
        assert_eq!(hex(d.indexed(196)), 0xff0000);
        assert_eq!(hex(d.indexed(231)), 0xffffff);
        assert_eq!(hex(d.indexed(232)), 0x080808);
        assert_eq!(hex(d.indexed(255)), 0xeeeeee);
    }

    #[test]
    fn defaults_follow_tokens() {
        let d = colors(true);
        assert_eq!(hex(d.foreground), 0xf5f5f5);
        assert_eq!(hex(d.background), 0x000000);
        assert_eq!(d.scheme().cursor, 0xb4cbff);
        let (fg, bg) = d.cell(&CellStyle::default());
        assert_eq!(hex(fg), 0xf5f5f5);
        assert_eq!(bg, None);
    }

    #[test]
    fn inverse_and_truecolor() {
        let d = colors(true);
        let inv = CellStyle {
            inverse: true,
            ..Default::default()
        };
        let (fg, bg) = d.cell(&inv);
        assert_eq!(hex(fg), 0x000000);
        assert_eq!(hex(bg.unwrap()), 0xf5f5f5);
        let tc = CellStyle {
            fg: Some(tern_terminal::TRUECOLOR_FLAG | 0x123456),
            bg: Some(1),
            ..Default::default()
        };
        let (fg, bg) = d.cell(&tc);
        assert_eq!(hex(fg), 0x123456);
        assert_eq!(hex(bg.unwrap()), 0xcd3131);
    }

    #[test]
    fn faint_blends() {
        let d = colors(true);
        let dim = CellStyle {
            dim: true,
            fg: Some(15),
            ..Default::default()
        };
        let (fg, _) = d.cell(&dim);
        // e5e5e5 * 155/255 over black ≈ 8b8b8b
        let v = hex(fg);
        let r = (v >> 16) & 0xff;
        assert!((0x8a..=0x8c).contains(&r), "{v:06x}");
    }

    #[test]
    fn osc_overrides() {
        let snapshot = TerminalSnapshot {
            color_overrides: vec![(1, 0xff_00ff00), (257, 0xff_202020)],
            ..Default::default()
        };
        let d = colors(true).with_overrides(&snapshot);
        assert_eq!(hex(d.indexed(1)), 0x00ff00);
        assert_eq!(hex(d.background), 0x202020);
    }
}
