//! Browse/Load UI state machine — pure logic, no framebuffer rendering.

use maschine_proto::{DisplayId, Rgb};
use nks_index::PresetRow;

use crate::layout;
use crate::render::PixelSink;

/// The facet the user is currently drilling into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacetLevel {
    Type,
    Subtype,
    Mode,
    Vendor,
}

#[derive(Debug, Default)]
pub struct BrowseState {
    pub facet: Option<FacetLevel>,
    pub type_filter: Option<String>,
    pub subtype_filter: Option<String>,
    pub mode_filter: Option<String>,
    pub vendor_filter: Option<String>,
    pub text_query: Option<String>,
    pub cursor: usize,
    pub rows: Vec<PresetRow>,
    /// Presently highlighted facet value while the user is drilling down.
    pub facet_values: Vec<String>,
    pub facet_cursor: usize,
}

impl BrowseState {
    pub fn selected(&self) -> Option<&PresetRow> {
        self.rows.get(self.cursor)
    }

    pub fn cursor_up(&mut self) {
        if self.cursor > 0 { self.cursor -= 1; }
    }

    pub fn cursor_down(&mut self) {
        if self.cursor + 1 < self.rows.len() { self.cursor += 1; }
    }

    pub fn set_rows(&mut self, rows: Vec<PresetRow>) {
        self.rows = rows;
        self.cursor = 0;
    }

    /// Breadcrumb string rendered on the left display header.
    pub fn breadcrumb(&self) -> String {
        let mut parts = vec!["All".to_string()];
        if let Some(v) = &self.vendor_filter { parts.push(v.clone()); }
        if let Some(t) = &self.type_filter { parts.push(t.clone()); }
        if let Some(s) = &self.subtype_filter { parts.push(s.clone()); }
        if let Some(m) = &self.mode_filter { parts.push(m.clone()); }
        parts.join(" / ")
    }

    /// Render the browse UI across both displays.
    pub fn render(&self, which: DisplayId, sink: &mut dyn PixelSink) {
        match which {
            DisplayId::Left => self.render_left(sink),
            DisplayId::Right => self.render_right(sink),
        }
    }

    fn render_left(&self, sink: &mut dyn PixelSink) {
        let bg = Rgb::new(0x05, 0x08, 0x12);
        let fg = Rgb::new(0xe0, 0xe0, 0xe0);
        let accent = Rgb::new(0xff, 0x80, 0x20);

        sink.fill_rect(0, 0, sink.width(), sink.height(), bg);
        sink.fill_rect(0, 0, sink.width(), layout::BREADCRUMB_HEIGHT, Rgb::new(0x10, 0x14, 0x22));
        sink.draw_text(8, 8, &self.breadcrumb(), fg);

        // Facet list
        let first_y = layout::BREADCRUMB_HEIGHT + 8;
        let row_h = layout::ROW_HEIGHT;
        for (i, val) in self.facet_values.iter().enumerate() {
            let y = first_y + (i as u16) * row_h;
            if y + row_h > sink.height() - layout::STATUS_HEIGHT { break; }
            let is_cursor = i == self.facet_cursor;
            if is_cursor {
                sink.fill_rect(0, y - 2, sink.width(), row_h, Rgb::new(0x1c, 0x22, 0x30));
                sink.fill_rect(0, y - 2, 4, row_h, accent);
            }
            sink.draw_text(12, y + 4, val, if is_cursor { accent } else { fg });
        }

        // Status strip
        let sy = sink.height() - layout::STATUS_HEIGHT;
        sink.fill_rect(0, sy, sink.width(), layout::STATUS_HEIGHT, Rgb::new(0x10, 0x14, 0x22));
        let status = format!("{} presets", self.rows.len());
        sink.draw_text(8, sy + 8, &status, Rgb::new(0x90, 0xa0, 0xc0));
    }

    fn render_right(&self, sink: &mut dyn PixelSink) {
        let bg = Rgb::new(0x05, 0x08, 0x12);
        let fg = Rgb::new(0xe0, 0xe0, 0xe0);
        let dim = Rgb::new(0x70, 0x80, 0x90);
        let accent = Rgb::new(0x20, 0xc0, 0xff);

        sink.fill_rect(0, 0, sink.width(), sink.height(), bg);

        let row_h = layout::ROW_HEIGHT;
        let visible = ((sink.height() - layout::STATUS_HEIGHT) / row_h).saturating_sub(1) as usize;
        let start = self.cursor.saturating_sub(visible / 2);
        for (offset, row) in self.rows.iter().skip(start).take(visible).enumerate() {
            let y = 8 + (offset as u16) * row_h;
            let is_cursor = start + offset == self.cursor;
            if is_cursor {
                sink.fill_rect(0, y - 2, sink.width(), row_h, Rgb::new(0x16, 0x26, 0x34));
                sink.fill_rect(0, y - 2, 4, row_h, accent);
            }
            sink.draw_text(12, y + 2, &row.name, if is_cursor { accent } else { fg });
            sink.draw_text(12, y + 14, &row.bank_chain, dim);
        }

        let sy = sink.height() - layout::STATUS_HEIGHT;
        sink.fill_rect(0, sy, sink.width(), layout::STATUS_HEIGHT, Rgb::new(0x10, 0x14, 0x22));
        if let Some(sel) = self.selected() {
            let caption = if sel.vendor.is_empty() { sel.plugin_ref.clone() } else { sel.vendor.clone() };
            sink.draw_text(8, sy + 8, &caption, Rgb::new(0x90, 0xa0, 0xc0));
        }
    }
}
