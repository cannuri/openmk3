//! Framebuffer rendering helpers.
//!
//! The maschine-core framebuffer is opaque to this crate on purpose — we
//! receive a generic pixel-writing trait so tests don't need the whole
//! transport. Callers adapt `maschine_core::display::Framebuffer` via the
//! blanket impl at the bottom of this module.

use maschine_proto::Rgb;

use crate::font;

pub trait PixelSink {
    fn width(&self) -> u16;
    fn height(&self) -> u16;
    fn set(&mut self, x: u16, y: u16, c: Rgb);

    fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, c: Rgb) {
        for yy in y..y.saturating_add(h) {
            for xx in x..x.saturating_add(w) {
                self.set(xx, yy, c);
            }
        }
    }

    fn outline_rect(&mut self, x: u16, y: u16, w: u16, h: u16, c: Rgb) {
        if w == 0 || h == 0 { return; }
        for xx in x..x + w { self.set(xx, y, c); self.set(xx, y + h - 1, c); }
        for yy in y..y + h { self.set(x, yy, c); self.set(x + w - 1, yy, c); }
    }

    fn draw_text(&mut self, mut x: u16, y: u16, text: &str, color: Rgb) -> u16 {
        for ch in text.chars() {
            if x + font::CHAR_W > self.width() { break; }
            let g = font::glyph(ch);
            for (col, bits) in g.iter().enumerate() {
                for row in 0..font::CHAR_H {
                    if (bits >> row) & 1 != 0 {
                        self.set(x + col as u16, y + row as u16, color);
                    }
                }
            }
            x += font::CHAR_W;
        }
        x
    }
}
