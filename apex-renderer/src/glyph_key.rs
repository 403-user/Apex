use bitflags::bitflags;

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub struct FontId(pub u8);

impl Default for FontId {
    fn default() -> Self {
        FontId(0)
    }
}

bitflags! {
    #[derive(Copy, Clone, Debug, Default, Hash, Eq, PartialEq)]
    pub struct GlyphStyle: u16 {
        const BOLD      = 1 << 0;
        const ITALIC    = 1 << 1;
        const DIM       = 1 << 2;
        const UNDERLINE = 1 << 3;
    }
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct GlyphKey {
    pub glyph_id: u16,
    pub font_id: FontId,
    pub font_size: u16,
    pub style: GlyphStyle,
}

impl GlyphKey {
    pub fn new(glyph_id: u16, font_id: FontId, font_size: u16, style: GlyphStyle) -> Self {
        GlyphKey { glyph_id, font_id, font_size, style }
    }
}
