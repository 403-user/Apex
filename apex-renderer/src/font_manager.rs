use std::ops::Range;
use std::sync::Arc;
use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use crate::glyph_key::FontId;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug)]
pub struct FontResolvedSpan {
    pub font_id: FontId,
    pub byte_range: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct FontMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_height: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
    pub strikethrough_position: f32,
    pub strikethrough_thickness: f32,
}

pub struct FontEntry {
    pub id: FontId,
    pub font: FontArc,
    pub data: Arc<Vec<u8>>,
    pub name: String,
    pub metrics: FontMetrics,
}

fn compute_metrics(font: &FontArc, font_size: f32) -> FontMetrics {
    let scale = font.pt_to_px_scale(font_size).unwrap_or(PxScale::from(font_size));
    let scaled = font.as_scaled(scale);
    let ascent = scaled.ascent();
    let descent = scaled.descent();
    let line_height = ascent - descent;
    // Derive decoration metrics from ascender/descender in pixel space.
    // Underline sits below baseline; strikethrough sits near mid-x-height.
    let underline_position = descent * 0.4;
    let underline_thickness = (line_height * 0.04).max(1.0);
    let strikethrough_position = ascent * 0.3;
    let strikethrough_thickness = (line_height * 0.04).max(1.0);
    FontMetrics {
        ascent,
        descent,
        line_height,
        underline_position,
        underline_thickness,
        strikethrough_position,
        strikethrough_thickness,
    }
}

pub struct FontManager {
    entries: Vec<FontEntry>,
}

impl FontManager {
    pub fn new(font_size: f32) -> anyhow::Result<Self> {
        let mut entries = Vec::new();
        let mut next_id = 0u8;

        let font_paths = [
            "/usr/share/fonts/truetype/jetbrains/JetBrainsMono-Regular.ttf",
            "/usr/share/fonts/opentype/jetbrains/JetBrainsMono-Regular.ttf",
            "/usr/share/fonts/truetype/firacode/FiraCode-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/opentype/firacode/FiraCode-Regular.otf",
            "/usr/share/fonts/TTF/FiraCode-Regular.ttf",
        ];

        let loaded_primary = font_paths.iter().any(|path| {
            if let Ok(data) = std::fs::read(path) {
                if let Ok(font) = FontArc::try_from_vec(data.clone()) {
                    log::info!("[FontManager] primary: {}", path);
                    let metrics = compute_metrics(&font, font_size);
                    entries.push(FontEntry {
                        id: FontId(next_id),
                        font,
                        data: Arc::new(data),
                        name: path.rsplit('/').next().unwrap_or(path).to_string(),
                        metrics,
                    });
                    next_id += 1;
                    true
                } else { false }
            } else { false }
        });

        if !loaded_primary {
            // Try fc-match as fallback for primary font
            if let Ok(output) = std::process::Command::new("fc-match")
                .arg("-f")
                .arg("%{file}")
                .arg("monospace")
                .output()
            {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    if let Ok(data) = std::fs::read(&path) {
                        if let Ok(font) = FontArc::try_from_vec(data.clone()) {
                            log::info!("[FontManager] primary via fc-match: {}", path);
                            let metrics = compute_metrics(&font, font_size);
                            entries.push(FontEntry {
                                id: FontId(next_id),
                                font,
                                data: Arc::new(data),
                                name: path.rsplit('/').next().unwrap_or(&path).to_string(),
                                metrics,
                            });
                            next_id += 1;
                        }
                    }
                }
            }
        }

        // Try embedded fallback
        if entries.is_empty() {
            let embedded: &[u8] = include_bytes!("../fonts/NimbusMonoPS-Regular.otf");
            if let Ok(font) = FontArc::try_from_slice(embedded) {
                log::info!("[FontManager] embedded fallback: NimbusMonoPS");
                let metrics = compute_metrics(&font, font_size);
                entries.push(FontEntry {
                    id: FontId(next_id),
                    font,
                    data: Arc::new(embedded.to_vec()),
                    name: "NimbusMonoPS".into(),
                    metrics,
                });
                next_id += 1;
            }
        }

        if entries.is_empty() {
            anyhow::bail!("No usable monospace font found");
        }

        // Load fallback fonts
        let fallback_paths = [
            "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
        ];

        for path in &fallback_paths {
            if let Ok(data) = std::fs::read(path) {
                if let Ok(font) = FontArc::try_from_vec(data.clone()) {
                    log::info!("[FontManager] fallback: {}", path);
                    let metrics = compute_metrics(&font, font_size);
                    entries.push(FontEntry {
                        id: FontId(next_id),
                        font,
                        data: Arc::new(data),
                        name: path.rsplit('/').next().unwrap_or(path).to_string(),
                        metrics,
                    });
                    next_id += 1;
                }
            }
        }

        Ok(FontManager { entries })
    }

    pub fn primary_font_id(&self) -> FontId {
        self.entries.first().map(|e| e.id).unwrap_or(FontId(0))
    }

    pub fn entry(&self, id: FontId) -> Option<&FontEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn font(&self, id: FontId) -> Option<&FontArc> {
        self.entry(id).map(|e| &e.font)
    }

    pub fn font_data(&self, id: FontId) -> Option<&Arc<Vec<u8>>> {
        self.entry(id).map(|e| &e.data)
    }

    pub fn metrics(&self, id: FontId) -> Option<&FontMetrics> {
        self.entry(id).map(|e| &e.metrics)
    }

    pub fn primary_ascent(&self) -> f32 {
        self.entries.first().map(|e| e.metrics.ascent).unwrap_or(0.0)
    }

    pub fn primary_underline_position(&self) -> f32 {
        self.entries.first().map(|e| e.metrics.underline_position).unwrap_or(2.0)
    }

    pub fn primary_underline_thickness(&self) -> f32 {
        self.entries.first().map(|e| e.metrics.underline_thickness).unwrap_or(1.5)
    }

    pub fn primary_strikethrough_position(&self) -> f32 {
        self.entries.first().map(|e| e.metrics.strikethrough_position).unwrap_or(0.0)
    }

    pub fn primary_strikethrough_thickness(&self) -> f32 {
        self.entries.first().map(|e| e.metrics.strikethrough_thickness).unwrap_or(1.5)
    }

    pub fn fallback_ids(&self, exclude: FontId) -> Vec<FontId> {
        self.entries.iter()
            .filter(|e| e.id != exclude)
            .map(|e| e.id)
            .collect()
    }

    pub fn entries(&self) -> &[FontEntry] {
        &self.entries
    }

    pub fn resolve_run(&self, text: &str) -> Vec<FontResolvedSpan> {
        if text.is_empty() || self.entries.is_empty() {
            return Vec::new();
        }

        let clusters: Vec<&str> = text.graphemes(true).collect();
        if clusters.is_empty() {
            return Vec::new();
        }

        let mut spans = Vec::new();
        let mut cur_font: Option<FontId> = None;
        let mut cur_byte_start = 0usize;
        let mut byte_offset = 0usize;

        for cluster in &clusters {
            let font = self.find_font_for_cluster(cluster);
            let cluster_bytes = cluster.len();

            if Some(font) != cur_font {
                if let Some(f) = cur_font {
                    spans.push(FontResolvedSpan {
                        font_id: f,
                        byte_range: cur_byte_start..byte_offset,
                    });
                }
                cur_font = Some(font);
                cur_byte_start = byte_offset;
            }
            byte_offset += cluster_bytes;
        }

        // Flush last span
        if let Some(f) = cur_font {
            spans.push(FontResolvedSpan {
                font_id: f,
                byte_range: cur_byte_start..text.len(),
            });
        }

        spans
    }

    fn find_font_for_cluster(&self, cluster: &str) -> FontId {
        // Try primary first
        if let Some(primary) = self.entries.first() {
            if cluster.chars().all(|ch| primary.font.glyph_id(ch).0 != 0) {
                return primary.id;
            }
        }
        // Try fallbacks in insertion order
        for entry in &self.entries[1..] {
            if cluster.chars().all(|ch| entry.font.glyph_id(ch).0 != 0) {
                return entry.id;
            }
        }
        // Last resort: primary even for missing glyphs
        self.entries[0].id
    }
}
