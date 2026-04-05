pub struct BitmapFont {
    pub font_w: usize,
    pub font_h: usize,
    pub glyphs: usize,
    pub bitmap: &'static [u8],
}

impl BitmapFont {
    /// Test if pixel (col, row) within glyph `ch` is set.
    /// Bit N of byte Y = pixel at (col N, row Y), with bit 0 = leftmost column.
    pub fn pixel(&self, ch: u8, col: usize, row: usize) -> bool {
        if ch as usize >= self.glyphs || col >= self.font_w || row >= self.font_h {
            return false;
        }
        let byte = self.bitmap[ch as usize * self.font_h + row];
        (byte >> col) & 1 != 0
    }
}
