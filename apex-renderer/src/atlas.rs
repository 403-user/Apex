use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use std::cell::Cell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use wgpu::{Device, Queue, Texture, TextureFormat, Extent3d};

use crate::glyph_key::GlyphKey;

fn save_atlas_ppm(data: &[u8], width: u32, height: u32, path: &Path) {
    use std::io::Write;
    match std::fs::File::create(path) {
        Ok(mut f) => {
            let header = format!("P6\n{} {}\n255\n", width, height);
            let _ = f.write_all(header.as_bytes());
            for y in 0..height {
                for x in 0..width {
                    let idx = ((y * width + x) * 4) as usize;
                    let r = data.get(idx).copied().unwrap_or(0);
                    let row = [r, r, r];
                    let _ = f.write_all(&row);
                }
            }
            log::info!("Saved atlas debug image: {}", path.display());
        }
        Err(e) => log::warn!("Failed to save atlas debug image: {}", e),
    }
}

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
    pub bearing_x: f32,
    pub bearing_y: f32,
    pub last_used_frame: Cell<u64>,
}

#[derive(Clone)]
pub struct RasterRequest {
    pub key: GlyphKey,
    pub font: FontArc,
    pub size: u16,
}

pub struct RasterResult {
    pub key: GlyphKey,
    pub width: u32,
    pub height: u32,
    pub bearing_x: f32,
    pub bearing_y: f32,
    pub advance: f32,
    pub pixels: Vec<u8>,
}

pub fn rasterize_glyph_pixels(font: &FontArc, glyph_id: u16, key: &GlyphKey, size: u16) -> Option<RasterResult> {
    let gid = ab_glyph::GlyphId(glyph_id);
    let scale = font.pt_to_px_scale(size as f32).unwrap_or(PxScale::from(size as f32));
    let scaled_font = font.as_scaled(scale);
    let advance = scaled_font.h_advance(gid);

    let glyph = ab_glyph::Glyph { id: gid, scale, position: ab_glyph::point(0.0, 0.0) };
    let bounds = match scaled_font.outline_glyph(glyph) {
        Some(outlined) => outlined.px_bounds(),
        None => return None,
    };

    let (width, height, bearing_x, bearing_y) = (
        bounds.width().ceil() as u32,
        bounds.height().ceil() as u32,
        bounds.min.x,
        bounds.min.y,
    );

    if width == 0 || height == 0 {
        return Some(RasterResult {
            key: key.clone(),
            width: 1, height: 1,
            advance, bearing_x: 0.0, bearing_y: 0.0,
            pixels: vec![0u8; 4],
        });
    }

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    if let Some(outline) = scaled_font.outline_glyph(
        ab_glyph::Glyph { id: gid, scale, position: ab_glyph::point(0.0, 0.0) }
    ) {
        outline.draw(|x, y, coverage| {
            if x < width && y < height {
                let idx = ((y * width + x) * 4) as usize;
                let cv = (coverage * 255.0) as u8;
                pixels[idx] = cv;
                pixels[idx + 1] = cv;
                pixels[idx + 2] = cv;
                pixels[idx + 3] = cv;
            }
        });
    }

    Some(RasterResult { key: key.clone(), width, height, bearing_x, bearing_y, advance, pixels })
}

pub struct PendingUpload {
    pub key: GlyphKey,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub bearing_x: f32,
    pub bearing_y: f32,
    pub advance: f32,
    pub pixels: Vec<u8>,
}

pub fn flush_uploads(
    device: &Device,
    queue: &Queue,
    atlas: &mut GlyphAtlas,
    pending: Vec<PendingUpload>,
) {
    if pending.is_empty() {
        return;
    }

    let bpp = 4u32;

    struct UploadSlot {
        offset: u64,
        row_pitch: u32,
        width: u32,
        height: u32,
        x: u32,
        y: u32,
    }

    let mut total_size = 0u64;
    let mut slots = Vec::with_capacity(pending.len());
    let mut all_padded: Vec<Vec<u8>> = Vec::with_capacity(pending.len());

    for upload in &pending {
        if upload.width == 1 && upload.height == 1 {
            continue;
        }
        let row_pitch = ((upload.width * bpp + 255) / 256) * 256;
        let size = row_pitch as u64 * upload.height as u64;

        let mut padded = vec![0u8; size as usize];
        for row in 0..upload.height as usize {
            let src = row * (upload.width as usize * 4);
            let dst = row * row_pitch as usize;
            let end = dst + (upload.width as usize * 4);
            if end <= padded.len() && src + (upload.width as usize * 4) <= upload.pixels.len() {
                padded[dst..end].copy_from_slice(&upload.pixels[src..src + (upload.width as usize * 4)]);
            }
        }

        slots.push(UploadSlot {
            offset: total_size,
            row_pitch, width: upload.width, height: upload.height,
            x: upload.x, y: upload.y,
        });
        all_padded.push(padded);
        total_size += size;
    }

    if total_size == 0 {
        atlas.frame += 1;
        for upload in pending {
            let entry = AtlasGlyph {
                x: 0, y: 0, width: 1, height: 1,
                advance: upload.advance,
                bearing_x: 0.0, bearing_y: 0.0,
                last_used_frame: Cell::new(atlas.frame),
            };
            atlas.glyph_map.insert(upload.key, entry);
        }
        return;
    }

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Atlas Staging"),
        size: total_size,
        usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::MAP_WRITE,
        mapped_at_creation: true,
    });

    {
        let mut view = staging.slice(..).get_mapped_range_mut();
        for (i, padded) in all_padded.iter().enumerate() {
            let offset = slots[i].offset as usize;
            let end = offset + padded.len();
            if end <= view.len() {
                view[offset..end].copy_from_slice(padded);
            }
        }
    }
    staging.unmap();

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Staging Upload Encoder"),
    });

    for slot in &slots {
        encoder.copy_buffer_to_texture(
            wgpu::ImageCopyBuffer {
                buffer: &staging,
                layout: wgpu::ImageDataLayout {
                    offset: slot.offset,
                    bytes_per_row: Some(slot.row_pitch),
                    rows_per_image: Some(slot.height),
                },
            },
            wgpu::ImageCopyTexture {
                texture: &atlas.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: slot.x, y: slot.y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            Extent3d { width: slot.width, height: slot.height, depth_or_array_layers: 1 },
        );
    }

    queue.submit(Some(encoder.finish()));

    atlas.frame += 1;
    for upload in pending {
        let entry = AtlasGlyph {
            x: upload.x, y: upload.y,
            width: upload.width, height: upload.height,
            advance: upload.advance,
            bearing_x: upload.bearing_x,
            bearing_y: upload.bearing_y,
            last_used_frame: Cell::new(atlas.frame),
        };
        atlas.glyph_map.insert(upload.key, entry);
    }
}

pub fn spawn_raster_worker(
    rx: std::sync::mpsc::Receiver<RasterRequest>,
    result_tx: std::sync::mpsc::Sender<RasterResult>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("raster-worker".into())
        .spawn(move || {
            while let Ok(request) = rx.recv() {
                if let Some(result) = rasterize_glyph_pixels(&request.font, request.key.glyph_id, &request.key, request.size) {
                    let _ = result_tx.send(result);
                }
            }
            log::info!("Raster worker exiting");
        })
        .expect("Failed to spawn raster worker thread")
}

pub struct Shelf {
    pub y: u32,
    pub height: u32,
    cursor_x: u32,
}

pub struct ShelfAllocator {
    shelves: Vec<Shelf>,
    atlas_size: u32,
    gap: u32,
}

impl ShelfAllocator {
    pub fn new(atlas_size: u32) -> Self {
        ShelfAllocator {
            shelves: Vec::new(),
            atlas_size,
            gap: 1,
        }
    }

    pub fn allocate(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        for shelf in &mut self.shelves {
            if shelf.height >= height && shelf.cursor_x + width <= self.atlas_size {
                let x = shelf.cursor_x;
                shelf.cursor_x += width + self.gap;
                return Some((x, shelf.y));
            }
        }
        let y = self.shelves.last()
            .map(|s| s.y + s.height + self.gap)
            .unwrap_or(0);
        if y + height > self.atlas_size {
            return None;
        }
        let x = 0;
        self.shelves.push(Shelf { y, height, cursor_x: width + self.gap });
        Some((x, y))
    }

}

fn load_font() -> anyhow::Result<(FontArc, Arc<Vec<u8>>)> {
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
            if let Ok(font) = FontArc::try_from_vec(data.clone()) {
                log::info!("Loaded font: {}", path);
                return Ok((font, Arc::new(data)));
            }
        }
    }
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
                    log::info!("Loaded font via fc-match: {}", path);
                    return Ok((font, Arc::new(data)));
                }
            }
        }
    }

    log::info!("Trying embedded fallback font...");
    let embedded: &[u8] = include_bytes!("../fonts/NimbusMonoPS-Regular.otf");
    if let Ok(font) = FontArc::try_from_slice(embedded) {
        log::info!("Loaded embedded fallback font (NimbusMonoPS)");
        return Ok((font, Arc::new(embedded.to_vec())));
    }

    anyhow::bail!(
        "No usable monospace font found. Install: sudo apt install fonts-firacode fonts-dejavu-core"
    );
}

pub struct GlyphAtlas {
    pub texture: Texture,
    pub width: u32,
    pub height: u32,
    pub font: Option<FontArc>,
    pub cell_width: f32,
    pub cell_height: f32,
    glyph_map: HashMap<GlyphKey, AtlasGlyph>,
    allocator: ShelfAllocator,
    pub size: u16,
    frame: u64,
    max_glyphs: usize,
}

impl GlyphAtlas {
    pub fn new_with_font(
        device: &Device, queue: &Queue,
        font: FontArc,
        atlas_dump_path: Option<&Path>,
    ) -> anyhow::Result<Self> {
        let scale = font.pt_to_px_scale(CELL_SIZE).unwrap_or(PxScale::from(CELL_SIZE));
        let scaled_font = font.as_scaled(scale);
        let advance = scaled_font.h_advance(scaled_font.glyph_id('W'));

        let mut pre_rasterize = Vec::new();
        for ch in (32u8..=126).map(|c| c as char) {
            pre_rasterize.push(font.glyph_id(ch).0);
        }
        for ch in ['█', '▀', '▄', '░', '▒', '▓', '│', '─', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼', '●', '◆', '■'] {
            pre_rasterize.push(font.glyph_id(ch).0);
        }

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
            font: Some(font),
            cell_width: advance.ceil().max(8.0),
            cell_height: CELL_HEIGHT,
            glyph_map: HashMap::new(),
            allocator: ShelfAllocator::new(ATLAS_SIZE),
            size: CELL_SIZE as u16,
            frame: 0,
            max_glyphs: 512,
        };

        for gid in pre_rasterize {
            let key = GlyphKey::new(gid, crate::glyph_key::FontId::default(), CELL_SIZE as u16, Default::default());
            atlas.rasterize(&mut atlas_data, &key);
        }

        if let Some(dump_path) = atlas_dump_path {
            save_atlas_ppm(&atlas_data, ATLAS_SIZE, ATLAS_SIZE, dump_path);
        }

        let tex_desc = wgpu::TextureDescriptor {
            label: Some("Glyph Atlas"),
            size: Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };
        atlas.texture = device.create_texture(&tex_desc);
        let bytes_per_row = ATLAS_SIZE as u32 * 4;
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &atlas.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas_data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(ATLAS_SIZE),
            },
            tex_desc.size,
        );

        Ok(atlas)
    }

    pub fn new_with_dump(device: &Device, queue: &Queue, atlas_dump_path: Option<&Path>) -> anyhow::Result<Self> {
        match load_font() {
            Ok((f, _font_data)) => {
                Self::new_with_font(device, queue, f, atlas_dump_path)
            }
            Err(e) => {
                log::warn!("{e} — creating empty glyph atlas (text will not render)");
                let dummy = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Glyph Atlas (empty)"),
                    size: Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                Ok(GlyphAtlas {
                    texture: dummy,
                    width: 1,
                    height: 1,
                    font: None,
                    cell_width: 8.0,
                    cell_height: 20.0,
                    glyph_map: HashMap::new(),
                    allocator: ShelfAllocator::new(1),
                    size: 14,
                    frame: 0,
                    max_glyphs: 512,
                })
            }
        }
    }

    pub fn rasterize(&mut self, atlas_data: &mut [u8], key: &GlyphKey) -> Option<AtlasGlyph> {
        if self.glyph_map.len() >= self.max_glyphs && !self.glyph_map.contains_key(key) {
            let lru = self.glyph_map.iter()
                .min_by_key(|(_, g)| g.last_used_frame.get())
                .map(|(k, _)| k.clone());
            if let Some(lru_key) = lru {
                log::debug!("Evicting glyph (cache full): {:?}", lru_key);
                self.glyph_map.remove(&lru_key);
            }
        }

        let font = self.font.as_ref()?;
        if let Some(g) = self.glyph_map.get(key) {
            self.frame += 1;
            g.last_used_frame.set(self.frame);
            return Some(g.clone());
        }

        let result = rasterize_glyph_pixels(font, key.glyph_id, key, self.size)?;

        let (w, h) = if result.width == 1 && result.height == 1 {
            (1u32, 1u32)
        } else {
            (result.width, result.height)
        };

        let (ox, oy) = self.allocator.allocate(w, h)?;

        if w > 0 && h > 0 && !(result.width == 1 && result.height == 1) {
            for y in 0..h {
                for x in 0..w {
                    let src = ((y * w + x) * 4) as usize;
                    let dst = ((oy + y) * ATLAS_SIZE + (ox + x)) as usize * 4;
                    if dst + 4 <= atlas_data.len() && src + 4 <= result.pixels.len() {
                        atlas_data[dst..dst + 4].copy_from_slice(&result.pixels[src..src + 4]);
                    }
                }
            }
        }

        self.frame += 1;
        let entry = AtlasGlyph {
            x: ox, y: oy,
            width: w, height: h,
            advance: result.advance,
            bearing_x: result.bearing_x,
            bearing_y: result.bearing_y,
            last_used_frame: Cell::new(self.frame),
        };
        self.glyph_map.insert(key.clone(), entry);
        self.glyph_map.get(key).cloned()
    }

    pub fn stage_rasterized(&mut self, result: RasterResult) -> Option<PendingUpload> {
        if self.glyph_map.contains_key(&result.key) {
            return None;
        }

        if result.width == 1 && result.height == 1 {
            let entry = AtlasGlyph {
                x: 0, y: 0, width: 1, height: 1,
                advance: result.advance,
                bearing_x: 0.0, bearing_y: 0.0,
                last_used_frame: Cell::new(self.frame),
            };
            self.glyph_map.insert(result.key, entry);
            return None;
        }

        let (ox, oy) = self.allocator.allocate(result.width, result.height)?;

        Some(PendingUpload {
            key: result.key,
            x: ox, y: oy,
            width: result.width, height: result.height,
            bearing_x: result.bearing_x,
            bearing_y: result.bearing_y,
            advance: result.advance,
            pixels: result.pixels,
        })
    }

    pub fn get_glyph(&self, key: &GlyphKey) -> Option<&AtlasGlyph> {
        let g = self.glyph_map.get(key)?;
        // Update usage without &mut self via Cell
        g.last_used_frame.set(self.frame + 1);
        Some(g)
    }

}
