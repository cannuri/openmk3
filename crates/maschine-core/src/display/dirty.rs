//! Dirty-tile tracker for the 480×272 display.
//!
//! Screen is divided into 16×16 tiles. 480/16 = 30 tiles wide,
//! 272/16 = 17 tiles tall → 510 bits, packed into 8 × u64.

pub const TILE: usize = 16;
pub const COLS: usize = 30;
pub const ROWS: usize = 17;
const BITS: usize = COLS * ROWS; // 510

pub struct DirtyTracker {
    bits: [u64; 8],
}

impl DirtyTracker {
    pub const fn new() -> Self { Self { bits: [0; 8] } }

    pub fn clear(&mut self) { self.bits = [0; 8]; }

    pub fn mark_full(&mut self) {
        for (i, b) in self.bits.iter_mut().enumerate() {
            let base = i * 64;
            if base >= BITS { *b = 0; continue; }
            let end = (base + 64).min(BITS);
            let n = end - base;
            *b = if n == 64 { !0u64 } else { (1u64 << n) - 1 };
        }
    }

    pub fn is_empty(&self) -> bool { self.bits.iter().all(|&b| b == 0) }

    pub fn mark_tile(&mut self, col: usize, row: usize) {
        if col >= COLS || row >= ROWS { return; }
        let idx = row * COLS + col;
        self.bits[idx / 64] |= 1u64 << (idx % 64);
    }

    pub fn is_tile_dirty(&self, col: usize, row: usize) -> bool {
        if col >= COLS || row >= ROWS { return false; }
        let idx = row * COLS + col;
        (self.bits[idx / 64] >> (idx % 64)) & 1 != 0
    }

    pub fn mark_pixel(&mut self, x: u16, y: u16) {
        self.mark_tile(x as usize / TILE, y as usize / TILE);
    }

    pub fn mark_rect(&mut self, x: u16, y: u16, w: u16, h: u16) {
        if w == 0 || h == 0 { return; }
        let c0 = x as usize / TILE;
        let r0 = y as usize / TILE;
        let c1 = (x as usize + w as usize - 1) / TILE;
        let r1 = (y as usize + h as usize - 1) / TILE;
        for r in r0..=r1.min(ROWS - 1) {
            for c in c0..=c1.min(COLS - 1) {
                self.mark_tile(c, r);
            }
        }
    }

    /// Greedy horizontal runs of dirty tiles for each row. Returns
    /// (row, col_start, col_end_inclusive).
    pub fn runs(&self) -> Vec<(usize, usize, usize)> {
        let mut runs = Vec::new();
        for r in 0..ROWS {
            let mut c = 0;
            while c < COLS {
                if self.is_tile_dirty(c, r) {
                    let start = c;
                    while c < COLS && self.is_tile_dirty(c, r) { c += 1; }
                    runs.push((r, start, c - 1));
                } else {
                    c += 1;
                }
            }
        }
        runs
    }
}

impl Default for DirtyTracker {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_covers_expected_tiles() {
        let mut d = DirtyTracker::new();
        d.mark_rect(15, 15, 2, 2);
        assert!(d.is_tile_dirty(0, 0));
        assert!(d.is_tile_dirty(1, 0));
        assert!(d.is_tile_dirty(0, 1));
        assert!(d.is_tile_dirty(1, 1));
        assert!(!d.is_tile_dirty(2, 2));
    }

    #[test]
    fn runs_coalesce_horizontal() {
        let mut d = DirtyTracker::new();
        d.mark_tile(2, 0);
        d.mark_tile(3, 0);
        d.mark_tile(4, 0);
        d.mark_tile(10, 0);
        d.mark_tile(5, 3);
        let runs = d.runs();
        assert_eq!(runs, vec![(0, 2, 4), (0, 10, 10), (3, 5, 5)]);
    }

    #[test]
    fn mark_full_sets_all_and_nothing_extra() {
        let mut d = DirtyTracker::new();
        d.mark_full();
        for r in 0..ROWS { for c in 0..COLS { assert!(d.is_tile_dirty(c, r)); } }
        // Bits past position 509 must not be set.
        assert_eq!(d.bits[7] >> (510 - 7 * 64), 0);
    }
}
