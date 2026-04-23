//! CPU-side framebuffer backing one Mk3 display.

use maschine_proto::{DisplayId, Rgb};

use super::dirty::DirtyTracker;

const W: usize = DisplayId::WIDTH as usize;
const H: usize = DisplayId::HEIGHT as usize;

/// RGB565 framebuffer in native endian. Converted to big-endian only during
/// encoding.
pub struct Framebuffer {
    pub(crate) pixels: Vec<u16>,
    pub(crate) dirty: DirtyTracker,
}

impl Framebuffer {
    pub fn new() -> Self {
        Self {
            pixels: vec![0; W * H],
            dirty: DirtyTracker::new(),
        }
    }

    pub fn width(&self) -> usize { W }
    pub fn height(&self) -> usize { H }
    pub fn pixels(&self) -> &[u16] { &self.pixels }
    pub fn pixels_mut(&mut self) -> &mut [u16] { &mut self.pixels }
    pub fn is_dirty(&self) -> bool { !self.dirty.is_empty() }

    pub fn clear(&mut self, color: Rgb) {
        let v = color.to_rgb565();
        self.pixels.iter_mut().for_each(|p| *p = v);
        self.dirty.mark_full();
    }

    pub fn set_pixel(&mut self, x: u16, y: u16, c: Rgb) {
        if (x as usize) >= W || (y as usize) >= H { return; }
        self.pixels[y as usize * W + x as usize] = c.to_rgb565();
        self.dirty.mark_pixel(x, y);
    }

    /// Mark a user-known rectangle dirty, e.g. after a bulk draw.
    pub fn touch(&mut self, x: u16, y: u16, w: u16, h: u16) {
        self.dirty.mark_rect(x, y, w, h);
    }

    /// Fill a rectangle with a solid color. Clips to the framebuffer bounds.
    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, c: Rgb) {
        let x0 = (x as usize).min(W);
        let y0 = (y as usize).min(H);
        let x1 = (x0 + w as usize).min(W);
        let y1 = (y0 + h as usize).min(H);
        if x0 == x1 || y0 == y1 { return; }
        let v = c.to_rgb565();
        for yy in y0..y1 {
            let row = &mut self.pixels[yy * W + x0 .. yy * W + x1];
            for p in row { *p = v; }
        }
        self.dirty.mark_rect(x0 as u16, y0 as u16, (x1 - x0) as u16, (y1 - y0) as u16);
    }
}

impl Default for Framebuffer {
    fn default() -> Self { Self::new() }
}
