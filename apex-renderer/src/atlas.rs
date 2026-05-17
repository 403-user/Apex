use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use std::collections::HashMap;
use wgpu::{Device, Queue, Texture, TextureFormat, Extent3d, util::{DeviceExt, TextureDataOrder}};

pub const ATLAS_SIZE: u32 = 1024;
pub const CELL_SIZE: f32 = 14.0;
pub const CELL_HEIGHT: f32 = 20.0;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GlyphVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub fg_color: [f32; 4],
    pub bg_color: [f32; 4],
}

#[derive(Clone)]
pub struct AtlasGlyph {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub advance: f32,
}

fn load_font() -> anyhow::Result<FontArc> {
    let paths = [
        "/usr/share/fonts/truetype/firacode/FiraCode-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/opentype/firacode/FiraCode-Regular.otf",
        "/usr/share/fonts/TTF/FiraCode-Regular.ttf",
    ];
    for path in &paths {
        if let Ok(data) = std::fs::read(path) {
            if let Ok(font) = FontArc::try_from_vec(data) {
                log::info!("Loaded font: {}", path);
                return Ok(font);
            }
        }
    }
    anyhow::bail!(
        "No usable monospace font found. Install: sudo apt install fonts-firacode fonts-dejavu-core"
    );
}

pub struct GlyphAtlas {
    pub texture: Texture,
    pub width: u32,
    pub height: u32,
    pub font: FontArc,
    pub cell_width: f32,
    pub cell_height: f32,
    glyph_map: HashMap<(char, u16), AtlasGlyph>,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    size: u16,
}

impl GlyphAtlas {
    pub fn new(device: &Device, queue: &Queue) -> anyhow::Result<Self> {
        let font = load_font()?;
        let scale = font.pt_to_px_scale(CELL_SIZE).unwrap_or(PxScale::from(CELL_SIZE));
        let scaled_font = font.as_scaled(scale);
        let advance = scaled_font.h_advance(scaled_font.glyph_id('W'));

        let mut atlas_data = vec![0u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize];
        let dummy = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glyph Atlas (placeholder)"),
            size: Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let mut atlas = GlyphAtlas {
            texture: dummy,
            width: ATLAS_SIZE,
            height: ATLAS_SIZE,
            font,
            cell_width: advance.ceil().max(8.0),
            cell_height: CELL_HEIGHT,
            glyph_map: HashMap::new(),
            cursor_x: 0,
            cursor_y: 0,
            row_height: (CELL_HEIGHT.ceil() as u32) + 2,
            size: CELL_SIZE as u16,
        };

        for ch in (32u8..=126).map(|c| c as char) {
            atlas.rasterize(&mut atlas_data, ch);
        }
        for ch in ['█', '▀', '▄', '░', '▒', '▓', '│', '─', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼', '●', '◆', '■'] {
            atlas.rasterize(&mut atlas_data, ch);
        }

        atlas.texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Glyph Atlas"),
                size: Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &atlas_data,
        );

        Ok(atlas)
    }

    pub fn rasterize(&mut self, atlas_data: &mut [u8], ch: char) -> Option<AtlasGlyph> {
        let key = (ch, self.size);
        if let Some(entry) = self.glyph_map.get(&key) {
            return Some(entry.clone());
        }

        let glyph_id = self.font.glyph_id(ch);
        let scale = self.font.pt_to_px_scale(self.size as f32).unwrap_or(PxScale::from(self.size as f32));
        let scaled_font = self.font.as_scaled(scale);
        let advance = scaled_font.h_advance(glyph_id);

        let glyph = ab_glyph::Glyph {
            id: glyph_id,
            scale,
            position: ab_glyph::point(0.0, 0.0),
        };

        if ch == ' ' {
            let entry = AtlasGlyph { x: 0, y: 0, width: 1, height: 1, advance };
            self.glyph_map.insert(key, entry);
            return self.glyph_map.get(&key).cloned();
        }

        let bounds = match scaled_font.outline_glyph(glyph) {
            Some(outlined) => Some(outlined.px_bounds()),
            None => None,
        };

        let (width, height) = match bounds {
            Some(b) => (b.width().ceil() as u32, b.height().ceil() as u32),
            None => (1, 1),
        };

        if width == 0 || height == 0 {
            let entry = AtlasGlyph { x: 0, y: 0, width: 1, height: 1, advance };
            self.glyph_map.insert(key, entry);
            return self.glyph_map.get(&key).cloned();
        }

    if width.saturating_add(self.cursor_x).saturating_add(1) >= ATLAS_SIZE {
        self.cursor_x = 0;
        self.cursor_y = self.cursor_y.saturating_add(self.row_height);
    }
    if height.saturating_add(self.cursor_y).saturating_add(1) >= ATLAS_SIZE {
        log::warn!("Glyph atlas full");
        return None;
    }

    let origin_x = self.cursor_x;
    let origin_y = self.cursor_y;
    self.cursor_x = self.cursor_x.saturating_add(width).saturating_add(1);

        if let Some(outline) = scaled_font.outline_glyph(
            ab_glyph::Glyph {
                id: glyph_id,
                scale,
                position: ab_glyph::point(0.0, 0.0),
            }
        ) {
            let px_bounds = outline.px_bounds();
            let ox = px_bounds.min.x.floor() as i32;
            let oy = px_bounds.min.y.floor() as i32;
            outline.draw(|x, y, coverage| {
                let px = (x as i32 - ox) as u32;
                let py = (y as i32 - oy) as u32;
                if px < width && py < height {
                    let dst = (origin_y.saturating_add(py))
                    .saturating_mul(ATLAS_SIZE)
                    .saturating_add(origin_x)
                    .saturating_add(px) as usize;
                    let idx = dst.saturating_mul(4);
                    if idx.saturating_add(4) <= atlas_data.len() {
                        atlas_data[idx] = (coverage * 255.0) as u8;
                    }
                }
            });
        }

        let entry = AtlasGlyph { x: origin_x, y: origin_y, width, height, advance };
        self.glyph_map.insert(key, entry);
        self.glyph_map.get(&key).cloned()
    }

    pub fn get_glyph(&self, ch: char, size: u16) -> Option<&AtlasGlyph> {
        self.glyph_map.get(&(ch, size))
    }
}
