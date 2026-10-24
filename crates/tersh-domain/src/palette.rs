//! Colours as programs see them, for answering colour queries (OSC 4/10/11/12),
//! e.g. Neovim picking `background=dark`.

use crate::config::parse_hex;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
}

impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        let rgb = |s: &str, fallback: Rgb| parse_hex(s).map(|(r, g, b)| Rgb { r, g, b }).unwrap_or(fallback);
        let defaults = Theme::default();
        let mut ansi = [Rgb::default(); 16];
        for (i, slot) in ansi.iter_mut().enumerate() {
            let fallback = rgb(&defaults.palette[i], Rgb::default());
            *slot = theme.palette.get(i).map_or(fallback, |c| rgb(c, fallback));
        }
        let white = Rgb { r: 0xff, g: 0xff, b: 0xff };
        Self {
            foreground: rgb(&theme.foreground, white),
            background: rgb(&theme.background, Rgb::default()),
            cursor: rgb(&theme.cursor, white),
            ansi,
        }
    }

    /// Colour at a palette index: 0-255 xterm colours, 256 foreground,
    /// 257 background, 258 cursor.
    pub fn get(&self, index: usize) -> Rgb {
        match index {
            0..=15 => self.ansi[index],
            16..=231 => {
                let i = index - 16;
                let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
                Rgb { r: level(i / 36), g: level(i / 6 % 6), b: level(i % 6) }
            },
            232..=255 => {
                let v = (8 + (index - 232) * 10) as u8;
                Rgb { r: v, g: v, b: v }
            },
            257 => self.background,
            258 => self.cursor,
            _ => self.foreground,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indices() {
        let palette = Palette::from_theme(&crate::theme::builtin("nord").unwrap());
        assert_eq!(palette.get(257), Rgb { r: 0x2e, g: 0x34, b: 0x40 });
        assert_eq!(palette.get(16), Rgb::default());
        assert_eq!(palette.get(231), Rgb { r: 255, g: 255, b: 255 });
        assert_eq!(palette.get(232), Rgb { r: 8, g: 8, b: 8 });
    }
}
