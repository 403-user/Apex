use arrayvec::ArrayString;
use crate::glyph_key::{FontId, GlyphStyle};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDirection {
    Ltr,
    Rtl,
}

#[derive(Clone, Debug)]
pub struct TextRun {
    pub row: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub text: ArrayString<256>,
    pub style: GlyphStyle,
    pub direction: TextDirection,
    pub script: unicode_script::Script,
}

/// A run positioned in visual order. `logical_index` maps back to the
/// corresponding `TextRun` in the original logical-order array.
#[derive(Clone, Debug)]
pub struct VisualRun {
    pub logical_index: usize,
    pub direction: TextDirection,
    pub col_start: usize,
    pub col_end: usize,
}

#[derive(Clone, Debug)]
pub struct ShapedGlyph {
    pub glyph_id: u16,
    pub font_id: FontId,
    pub source_col: usize,
    pub byte_offset: usize,
    pub x_advance: f32,
    pub y_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

pub struct Shaper {
    font_size: f32,
}

impl Shaper {
    pub fn new(font_size: f32) -> Self {
        Shaper { font_size }
    }

    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    /// Shape a run of text with a specific font. `font_id` is stamped on each
    /// output `ShapedGlyph` so the renderer can look up the correct atlas entry.
    /// `cluster_to_col` maps each byte offset in the input text to its grid column.
    pub fn shape_run(
        &self, font_data: &[u8], font_id: FontId,
        run: &TextRun, cluster_to_col: &[usize],
    ) -> Vec<ShapedGlyph> {
        let face = rustybuzz::Face::from_slice(font_data, 0)
            .expect("Invalid font data in shaper");
        let upem = face.units_per_em() as f32;

        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(&run.text);
        match run.direction {
            TextDirection::Ltr => buffer.set_direction(rustybuzz::Direction::LeftToRight),
            TextDirection::Rtl => buffer.set_direction(rustybuzz::Direction::RightToLeft),
        }

        let output = rustybuzz::shape(&face, &[], buffer);

        let positions = output.glyph_positions();
        let infos = output.glyph_infos();

        let scale = self.font_size / upem;
        infos.iter().zip(positions.iter()).map(|(info, pos)| {
            ShapedGlyph {
                glyph_id: info.glyph_id as u16,
                font_id,
                source_col: cluster_to_col.get(info.cluster as usize).copied().unwrap_or(run.col_start),
                byte_offset: info.cluster as usize,
                x_advance: pos.x_advance as f32 * scale,
                y_advance: pos.y_advance as f32 * scale,
                x_offset: pos.x_offset as f32 * scale,
                y_offset: pos.y_offset as f32 * scale,
            }
        }).collect()
    }

    /// Post-shape correction: detect .notdef glyphs and re-shape
    /// failing clusters with fallback fonts. Mutates `glyphs` in place.
    pub fn correct_notdef_glyphs(
        &self,
        font_manager: &crate::font_manager::FontManager,
        glyphs: &mut Vec<ShapedGlyph>,
        run: &TextRun,
        cluster_to_col: &[usize],
    ) {
        if !glyphs.iter().any(|g| g.glyph_id == 0) {
            return;
        }

        // Build cluster byte ranges for the run text
        let clusters: Vec<(usize, usize)> = run.text.grapheme_indices(true)
            .map(|(start, gc)| (start, start + gc.len()))
            .collect();

        let mut corrected: Vec<ShapedGlyph> = Vec::with_capacity(glyphs.len());
        let mut glyph_idx = 0;
        while glyph_idx < glyphs.len() {
            if glyphs[glyph_idx].glyph_id == 0 {
                let byte_off = glyphs[glyph_idx].byte_offset;

                // Find the cluster's byte range
                let &(cl_start, cl_end) = clusters.iter()
                    .find(|(s, e)| *s <= byte_off && byte_off < *e)
                    .unwrap_or(&(0, run.text.len()));

                // Collect all glyphs belonging to this cluster
                let mut cluster_end = glyph_idx;
                while cluster_end < glyphs.len()
                    && glyphs[cluster_end].byte_offset >= cl_start
                    && glyphs[cluster_end].byte_offset < cl_end
                {
                    cluster_end += 1;
                }

                // Try fallback fonts
                let cluster_text = &run.text[cl_start..cl_end];
                let mut replaced = false;

                let original_font_id = glyphs[glyph_idx].font_id;
                for fallback_id in font_manager.fallback_ids(original_font_id) {
                    if let Some(fd) = font_manager.font_data(fallback_id) {
                        let cluster_run = TextRun {
                            row: run.row,
                            col_start: glyphs[glyph_idx].source_col,
                            col_end: glyphs[cluster_end.saturating_sub(1)].source_col + 1,
                            text: {
                                let mut t = ArrayString::new();
                                let _ = t.try_push_str(cluster_text);
                                t
                            },
                            style: run.style,
                            direction: run.direction,
                            script: run.script,
                        };
                        let cluster_bytes: Vec<usize> =
                            if cl_end <= cluster_to_col.len() {
                                cluster_to_col[cl_start..cl_end].to_vec()
                            } else {
                                let safe = cl_end.min(cluster_to_col.len());
                                cluster_to_col[cl_start..safe].to_vec()
                            };

                        let replacement = self.shape_run(fd, fallback_id, &cluster_run, &cluster_bytes);
                        if replacement.iter().all(|g| g.glyph_id != 0) {
                            corrected.extend(replacement);
                            replaced = true;
                            break;
                        }
                    }
                }

                if !replaced {
                    corrected.extend(glyphs[glyph_idx..cluster_end].iter().cloned());
                }
                glyph_idx = cluster_end;
            } else {
                corrected.push(glyphs[glyph_idx].clone());
                glyph_idx += 1;
            }
        }

        *glyphs = corrected;
    }
}
