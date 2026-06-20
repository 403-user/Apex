use apex_config::theme::Theme;
use anyhow::Result;
use wgpu::{
    Device, Queue, Surface, SurfaceConfiguration, Instance, InstanceDescriptor,
    Backends, ShaderModuleDescriptor, ShaderSource, RenderPipeline,
    PipelineLayoutDescriptor, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingType, TextureSampleType, ShaderStages, SamplerBindingType,
    BindGroupDescriptor, BindGroupEntry, TextureViewDimension, SamplerDescriptor,
    AddressMode, FilterMode, VertexState, FragmentState, VertexBufferLayout,
    VertexStepMode, RenderPipelineDescriptor, ColorTargetState, ColorWrites,
    BlendState, Operations, LoadOp, StoreOp,
    RenderPassDescriptor, RenderPassColorAttachment, CommandEncoderDescriptor,
    BufferUsages, BufferDescriptor,
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::EventLoop,
    keyboard::{Key, NamedKey},
    window::Window,
};
use std::collections::HashSet;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use directories::ProjectDirs;
use notify::Watcher;
use std::time::Instant;
use std::os::fd::AsRawFd;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use arrayvec::ArrayString;
use crate::atlas::{flush_uploads, GlyphAtlas, GlyphVertex, PendingUpload, RasterRequest, RasterResult, spawn_raster_worker, ATLAS_SIZE, CELL_SIZE};
use crate::shaper::{Shaper, TextRun, TextDirection, ShapedGlyph, VisualRun};
use crate::font_manager::FontManager;
use unicode_script::UnicodeScript;
use apex_config::ApexConfig;
use vte_core::grid::CellFlags;
use apex_pty::PtyInstance;
use vte_core::parser::VteProcessor;

fn locate_theme_path(name: &str) -> Option<std::path::PathBuf> {
    // Built-in themes have no external file
    if matches!(name, "kali-dark" | "default" | "backtrack") {
        return None;
    }
    let mut paths = vec![
        std::path::PathBuf::from(format!("/etc/apex/themes/{}.toml", name)),
        std::path::PathBuf::from(format!("themes/{}.toml", name)),
    ];
    if let Some(proj_dirs) = ProjectDirs::from("com", "apex", "apex") {
        paths.push(proj_dirs.config_dir().join("themes").join(format!("{}.toml", name)));
    }
    for p in paths {
        if p.exists() {
            return Some(p);
        }
    }
    None
}

const MIN_FRAME_TIME: std::time::Duration = std::time::Duration::from_millis(16); // ~60fps

pub struct CachedRowMesh {
    pub bg_vertices: Vec<GlyphVertex>,
    pub fg_vertices: Vec<GlyphVertex>,
    pub dec_vertices: Vec<GlyphVertex>,
}

impl CachedRowMesh {
    pub fn new() -> Self {
        CachedRowMesh {
            bg_vertices: Vec::new(),
            fg_vertices: Vec::new(),
            dec_vertices: Vec::new(),
        }
    }
}

pub struct SelectionRange {
    pub active: bool,
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

impl SelectionRange {
    pub fn new() -> Self {
        SelectionRange {
            active: false,
            start_row: 0,
            start_col: 0,
            end_row: 0,
            end_col: 0,
        }
    }
}

#[derive(Clone)]
pub struct SearchMatch {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

pub struct SearchHighlight {
    pub query: String,
    pub matches: Vec<SearchMatch>,
}

impl SearchHighlight {
    pub fn new() -> Self {
        SearchHighlight {
            query: String::new(),
            matches: Vec::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        !self.query.is_empty() && !self.matches.is_empty()
    }
}

struct ApexApp {
    window: Option<Arc<Window>>,
    surface: Option<Surface<'static>>,
    device: Option<Device>,
    queue: Option<Queue>,
    surface_config: Option<SurfaceConfiguration>,
    pipeline: Option<RenderPipeline>,
    fg_pipeline: Option<RenderPipeline>,
    bind_group: Option<wgpu::BindGroup>,
    sampler: Option<wgpu::Sampler>,
    compute_pipeline: Option<wgpu::ComputePipeline>,
    compute_bind_group: Option<wgpu::BindGroup>,
    glyph_input_buf: Option<wgpu::Buffer>,
    glyph_output_buf: Option<wgpu::Buffer>,
    glyph_uniform_buf: Option<wgpu::Buffer>,
    graphics_overlay_texture: Option<wgpu::Texture>,
    graphics_overlay_view: Option<wgpu::TextureView>,
    graphics_overlay_bind_group: Option<wgpu::BindGroup>,
    overlay_pipeline: Option<wgpu::RenderPipeline>,
    overlay_bgl: Option<wgpu::BindGroupLayout>,
    overlay_sampler: Option<wgpu::Sampler>,
    overlay_quad_vb: Option<wgpu::Buffer>,
    overlay_quad_ib: Option<wgpu::Buffer>,
    vertex_buf: Option<wgpu::Buffer>,
    index_buf: Option<wgpu::Buffer>,
    fg_vertex_buf: Option<wgpu::Buffer>,
    fg_index_buf: Option<wgpu::Buffer>,
    glyph_atlas: Option<GlyphAtlas>,
    processor: VteProcessor,
    last_render: Instant,
    needs_redraw: bool,
    cell_w: f32,
    cell_h: f32,
    scale_factor: f64,
    pty: Option<PtyInstance>,
    input_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
    output_rx: Option<mpsc::Receiver<Vec<u8>>>,
    pty_reader: Option<JoinHandle<()>>,
    atlas_dump: Option<PathBuf>,
    row_cache: Vec<CachedRowMesh>,
    render_epoch: u64,
    raster_tx: Option<std::sync::mpsc::Sender<RasterRequest>>,
    raster_result_rx: Option<std::sync::mpsc::Receiver<RasterResult>>,
    pending_raster: HashSet<crate::glyph_key::GlyphKey>,
    pending_uploads: Vec<PendingUpload>,
    raster_worker: Option<std::thread::JoinHandle<()>>,
    shaper: Option<Shaper>,
    font_manager: Option<FontManager>,
    primary_ascent: f32,
    primary_underline_pos: f32,
    primary_underline_thickness: f32,
    primary_strikethrough_pos: f32,
    primary_strikethrough_thickness: f32,
    debug_show_overlay: bool,
    debug_grid_vertices: Vec<GlyphVertex>,
    debug_grid_indices: Vec<u32>,
    debug_vb: Option<wgpu::Buffer>,
    debug_ib: Option<wgpu::Buffer>,
    theme: Theme,
    theme_file_path: Option<PathBuf>,
    theme_reload_rx: Option<std::sync::mpsc::Receiver<()>>,
    dec_vb: Option<wgpu::Buffer>,
    dec_ib: Option<wgpu::Buffer>,
    shaped_row_cache: HashMap<(u64, u16), (Vec<ShapedGlyph>, Vec<usize>, Vec<TextDirection>, Vec<usize>, u64)>,
    shaped_cache_gen: u64,
    cursor_vertices: Vec<GlyphVertex>,
    cursor_indices: Vec<u32>,
    cursor_vb: Option<wgpu::Buffer>,
    cursor_ib: Option<wgpu::Buffer>,
    cursor_blink_visible: bool,
    cursor_blink_accum: std::time::Duration,
    selection: SelectionRange,
    selection_vertices: Vec<GlyphVertex>,
    selection_indices: Vec<u32>,
    selection_vb: Option<wgpu::Buffer>,
    selection_ib: Option<wgpu::Buffer>,
    search: SearchHighlight,
    search_vertices: Vec<GlyphVertex>,
    search_indices: Vec<u32>,
    search_vb: Option<wgpu::Buffer>,
    search_ib: Option<wgpu::Buffer>,
    mouse_down_row: usize,
    mouse_down_col: usize,
    last_mouse_x: f64,
    last_mouse_y: f64,
}

impl ApexApp {
    fn new(scrollback_lines: u32, theme: Theme, atlas_dump: Option<PathBuf>) -> Self {
        let scrollback = scrollback_lines as usize;
        ApexApp {
            window: None,
            surface: None,
            device: None,
            queue: None,
            surface_config: None,
            pipeline: None,
            fg_pipeline: None,
            bind_group: None,
            sampler: None,
            compute_pipeline: None,
            compute_bind_group: None,
            glyph_input_buf: None,
            glyph_output_buf: None,
            glyph_uniform_buf: None,
            graphics_overlay_texture: None,
            graphics_overlay_view: None,
            graphics_overlay_bind_group: None,
            overlay_pipeline: None,
            overlay_bgl: None,
            overlay_sampler: None,
            overlay_quad_vb: None,
            overlay_quad_ib: None,
            vertex_buf: None,
            index_buf: None,
            fg_vertex_buf: None,
            fg_index_buf: None,
            glyph_atlas: None,
            processor: VteProcessor::new(24, 80, scrollback),
            last_render: Instant::now() - MIN_FRAME_TIME,
            needs_redraw: true,
            cell_w: 0.0,
            cell_h: 0.0,
            scale_factor: 1.0,
            pty: None,
            input_tx: None,
            output_rx: None,
            pty_reader: None,
                    atlas_dump,
                    theme,
                    theme_file_path: None,
                    theme_reload_rx: None,
            row_cache: Vec::new(),
            render_epoch: 0,
            raster_tx: None,
            raster_result_rx: None,
            pending_raster: HashSet::new(),
            pending_uploads: Vec::new(),
            raster_worker: None,
            shaper: None,
            font_manager: None,
            primary_ascent: 14.0,
            primary_underline_pos: 2.0,
            primary_underline_thickness: 1.5,
            primary_strikethrough_pos: -14.0 * 0.3,
            primary_strikethrough_thickness: 1.5,
            debug_show_overlay: false,
            debug_grid_vertices: Vec::new(),
            debug_grid_indices: Vec::new(),
            debug_vb: None,
            debug_ib: None,
            dec_vb: None,
            dec_ib: None,
            shaped_row_cache: HashMap::new(),
            shaped_cache_gen: 0,
            cursor_vertices: Vec::new(),
            cursor_indices: Vec::new(),
            cursor_vb: None,
            cursor_ib: None,
            cursor_blink_visible: true,
            cursor_blink_accum: std::time::Duration::ZERO,
            selection: SelectionRange::new(),
            selection_vertices: Vec::new(),
            selection_indices: Vec::new(),
            selection_vb: None,
            selection_ib: None,
            search: SearchHighlight::new(),
            search_vertices: Vec::new(),
            search_indices: Vec::new(),
            search_vb: None,
            search_ib: None,
            mouse_down_row: 0,
            mouse_down_col: 0,
            last_mouse_x: 0.0,
            last_mouse_y: 0.0,
        }
    }

    fn build_pipelines(device: &Device, config: &SurfaceConfiguration, texture: &wgpu::Texture) -> (RenderPipeline, RenderPipeline, wgpu::BindGroup, wgpu::Sampler, wgpu::BindGroupLayout) {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Terminal Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/terminal.wgsl").into()),
        });

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("Glyph Sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Glyph Atlas Layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Glyph Atlas Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Terminal Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let vtx_layout = VertexBufferLayout {
            array_stride: std::mem::size_of::<GlyphVertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2,  // position
                1 => Float32x2,  // uv
                2 => Float32x4,  // fg_color
                3 => Float32x4,  // bg_color
            ],
        };

        let bg_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Terminal BG Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[vtx_layout.clone()],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: config.format,
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let fg_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Terminal FG Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[vtx_layout.clone()],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: config.format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        (bg_pipeline, fg_pipeline, bind_group, sampler, bind_group_layout)
    }

    fn build_overlay_pipeline(device: &Device, config: &SurfaceConfiguration, atlas_bgl: &wgpu::BindGroupLayout) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::Sampler) {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Terminal Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/terminal.wgsl").into()),
        });
        let overlay_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("Overlay Sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Nearest,
            ..Default::default()
        });
        let overlay_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Overlay Texture Layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Overlay Pipeline Layout"),
            bind_group_layouts: &[atlas_bgl, &overlay_bgl],
            push_constant_ranges: &[],
        });
        let vtx_layout = VertexBufferLayout {
            array_stride: std::mem::size_of::<GlyphVertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2,
                1 => Float32x2,
                2 => Float32x4,
                3 => Float32x4,
            ],
        };
        let overlay_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Overlay Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[vtx_layout],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: "fs_overlay",
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: config.format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        (overlay_pipeline, overlay_bgl, overlay_sampler)
    }

    fn build_compute_pipeline(device: &Device) -> (wgpu::ComputePipeline, wgpu::BindGroupLayout) {
        let compute_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Compute Shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/compute.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Compute Bind Group Layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Compute Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Glyph Compute Pipeline"),
            layout: Some(&pipeline_layout),
            module: &compute_shader,
            entry_point: "cs_main",
            compilation_options: Default::default(),
            cache: None,
        });

        (pipeline, bind_group_layout)
    }

    fn row_is_ascii_simple(&self, row: usize) -> bool {
        let cols = self.processor.grid.cols;
        if row >= self.processor.grid.rows.len() {
            return true;
        }
        let cells = &self.processor.grid.rows[row].cells;
        for col in 0..cols {
            if !cells[col].content.chars().all(|c| c.is_ascii()) {
                return false;
            }
        }
        true
    }

    /// Compute a content fingerprint for a row for the shaped row cache.
    fn row_fingerprint(&self, row: usize) -> u64 {
        let cols = self.processor.grid.cols;
        if row >= self.processor.grid.rows.len() {
            return 0;
        }
        let cells = &self.processor.grid.rows[row].cells;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for col in 0..cols {
            cells[col].content.hash(&mut hasher);
            cells[col].flags.bits().hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Resolve scripts for a grid row, inheriting Common/Inherited
    /// from the nearest resolved script (left-to-right).
    fn resolve_row_scripts(&self, row: usize) -> Vec<unicode_script::Script> {
        let cols = self.processor.grid.cols;
        if row >= self.processor.grid.rows.len() || cols == 0 {
            return Vec::new();
        }
        let cells = &self.processor.grid.rows[row].cells;

        let raw: Vec<unicode_script::Script> = (0..cols).map(|col| {
            let ch = cells[col].content.chars().next().unwrap_or(' ');
            ch.script()
        }).collect();

        let initial = raw.iter().copied()
            .find(|s| *s != unicode_script::Script::Common
                && *s != unicode_script::Script::Inherited)
            .unwrap_or(unicode_script::Script::Latin);

        let mut resolved = raw;
        let mut prev = initial;
        for s in &mut resolved {
            if *s == unicode_script::Script::Common
                || *s == unicode_script::Script::Inherited
            {
                *s = prev;
            } else {
                prev = *s;
            }
        }
        resolved
    }

    /// Break a grid row into contiguous TextRuns grouped by style and script.
    fn break_row_into_runs(&self, row: usize) -> Vec<TextRun> {
        let cols = self.processor.grid.cols;
        if row >= self.processor.grid.rows.len() || cols == 0 {
            return Vec::new();
        }
        let cells = &self.processor.grid.rows[row].cells;
        let scripts = self.resolve_row_scripts(row);

        let cell_glyph_style = |flags: CellFlags| {
            let mut s = crate::glyph_key::GlyphStyle::empty();
            if flags.contains(CellFlags::BOLD) { s.insert(crate::glyph_key::GlyphStyle::BOLD); }
            if flags.contains(CellFlags::ITALIC) { s.insert(crate::glyph_key::GlyphStyle::ITALIC); }
            if flags.contains(CellFlags::DIM) { s.insert(crate::glyph_key::GlyphStyle::DIM); }
            if flags.contains(CellFlags::UNDERLINE) { s.insert(crate::glyph_key::GlyphStyle::UNDERLINE); }
            s
        };

        let mut runs: Vec<TextRun> = Vec::new();
        let mut cur_start = 0usize;
        let mut cur_style = cell_glyph_style(cells[0].flags);
        let mut cur_script = scripts[0];
        for col in 1..=cols {
            let style_here = if col < cols { cell_glyph_style(cells[col].flags) } else { crate::glyph_key::GlyphStyle::empty() };
            let script_here = if col < cols { scripts[col] } else { unicode_script::Script::Common };
            if col == cols || style_here != cur_style || script_here != cur_script {
                let mut text = ArrayString::new();
                for c in cur_start..col {
                    let _ = text.try_push_str(&cells[c].content);
                }
                let direction = match cur_script {
                    sc if sc == unicode_script::Script::Arabic
                        || sc == unicode_script::Script::Hebrew
                        || sc == unicode_script::Script::Syriac
                        || sc == unicode_script::Script::Thaana
                        || sc == unicode_script::Script::Nko => TextDirection::Rtl,
                    _ => TextDirection::Ltr,
                };
                runs.push(TextRun {
                    row,
                    col_start: cur_start,
                    col_end: col,
                    text,
                    style: cur_style,
                    direction,
                    script: cur_script,
                });
                cur_start = col;
                if col < cols {
                    cur_style = style_here;
                    cur_script = script_here;
                }
            }
        }
        runs
    }

    /// Project a logical column to its visual column using run direction data.
    /// For LTR runs the mapping is identity; for RTL runs the column order is
    /// reversed within the run so that the rightmost logical column maps to the
    /// leftmost visual position (and vice versa).
    fn logical_to_visual(&self, row: usize, logical_col: usize) -> usize {
        let runs = self.break_row_into_runs(row);
        for run in &runs {
            if logical_col >= run.col_start && logical_col < run.col_end {
                return match run.direction {
                    TextDirection::Rtl => {
                        run.col_start + (run.col_end - 1 - logical_col)
                    }
                    TextDirection::Ltr => logical_col,
                };
            }
        }
        logical_col
    }

    /// Reorder runs from logical order into visual rendering order using
    /// Unicode BiDi (UAX#9) level resolution. Uses per-cell first-char text
    /// as input to `unicode_bidi::BidiInfo`, resolves embedding levels, then
    /// reorders by L2 (via `reorder_visual`) to produce VisualRun entries with
    /// visual-column ranges.
    fn reorder_runs_visually(
        &self, row: usize,
        col_starts: &[usize],
        col_ends: &[usize],
        directions: &[TextDirection],
    ) -> Vec<VisualRun> {
        let n = col_starts.len();
        if n <= 1 {
            return col_starts.iter().enumerate().map(|(i, &cs)| VisualRun {
                logical_index: i, direction: directions[i],
                col_start: cs, col_end: col_ends[i],
            }).collect();
        }

        let cols = self.processor.grid.cols;
        let cells = &self.processor.grid.rows[row].cells;

        // Build simplified row text: first char of each cell
        let row_text: String = (0..cols).map(|c| {
            cells[c].content.chars().next().unwrap_or(' ')
        }).collect();

        let bidi_info = unicode_bidi::BidiInfo::new(&row_text, Some(unicode_bidi::Level::ltr()));
        if bidi_info.levels.len() != cols {
            // Fallback: identity order
            return col_starts.iter().enumerate().map(|(i, &cs)| VisualRun {
                logical_index: i, direction: directions[i],
                col_start: cs, col_end: col_ends[i],
            }).collect();
        }

        // Get visual order of character indices
        // visual_order[vis_i] = logical index of the character at visual position vis_i
        let visual_order = unicode_bidi::BidiInfo::reorder_visual(&bidi_info.levels);

        // Group consecutive visual positions by their logical run to produce
        // VisualRun entries with visual-column ranges
        let mut visual_runs: Vec<VisualRun> = Vec::new();
        let mut vis_start = 0usize;
        let mut current_run_idx: Option<usize> = None;

        for vis_i in 0..cols {
            let logical_col = visual_order[vis_i];
            // Find which logical run contains this column
            let run_idx = (0..n).find(|&i| {
                logical_col >= col_starts[i] && logical_col < col_ends[i]
            }).unwrap_or(0);

            match current_run_idx {
                Some(r) if r == run_idx => {}
                _ => {
                    if let Some(prev) = current_run_idx {
                        visual_runs.push(VisualRun {
                            logical_index: prev,
                            direction: directions[prev],
                            col_start: vis_start,
                            col_end: vis_i,
                        });
                    }
                    current_run_idx = Some(run_idx);
                    vis_start = vis_i;
                }
            }
        }
        if let Some(prev) = current_run_idx {
            visual_runs.push(VisualRun {
                logical_index: prev,
                direction: directions[prev],
                col_start: vis_start,
                col_end: cols,
            });
        }

        visual_runs
    }

    /// Old per-cell ASCII path — loops cells individually without shaping.
    fn render_ascii_row(
        &self, row: usize, cw: f32, ch: f32, atlas: Option<&GlyphAtlas>,
        px_to_ndc: &impl Fn(f32, f32) -> [f32; 2],
    ) -> (Vec<GlyphVertex>, Vec<GlyphVertex>, Vec<crate::glyph_key::GlyphKey>)
    {
        let cols = self.processor.grid.cols;
        if row >= self.processor.grid.rows.len() {
            return (Vec::new(), Vec::new(), Vec::new());
        }
        let mut bg = Vec::with_capacity(cols * 6);
        let mut fg = Vec::with_capacity(cols * 6);
        let mut missing = Vec::new();
        let global_reverse = self.processor.mode.contains(vte_core::state::TerminalMode::REVERSE_VIDEO);
        for col in 0..cols {
            let cell = &self.processor.grid.rows[row].cells[col];
            // Skip spacer cells from wide characters
            if cell.width == 0 { continue; }
            let reverse = cell.flags.contains(CellFlags::REVERSE) ^ global_reverse;
            let dim = if cell.flags.contains(CellFlags::DIM) { 0.5 } else { 1.0 };
            let (fg_r, fg_g, fg_b) = color_to_f32(cell.fg_color, &self.theme, reverse);
            let (bg_r, bg_g, bg_b) = color_to_f32(cell.bg_color, &self.theme, !reverse);
            let fg_color = [fg_r * dim, fg_g * dim, fg_b * dim, 1.0];
            let bg_color = [bg_r, bg_g, bg_b, 1.0];

            let x = col as f32 * cw;
            let y = row as f32 * ch;
            let tl = px_to_ndc(x, y);
            let tr = px_to_ndc(x + cw, y);
            let bl = px_to_ndc(x, y + ch);
            let br = px_to_ndc(x + cw, y + ch);

            bg.extend_from_slice(&[
                GlyphVertex { position: tl, uv: [0.0, 0.0], fg_color, bg_color },
                GlyphVertex { position: tr, uv: [0.0, 0.0], fg_color, bg_color },
                GlyphVertex { position: bl, uv: [0.0, 0.0], fg_color, bg_color },
                GlyphVertex { position: br, uv: [0.0, 0.0], fg_color, bg_color },
                GlyphVertex { position: tr, uv: [0.0, 0.0], fg_color, bg_color },
                GlyphVertex { position: bl, uv: [0.0, 0.0], fg_color, bg_color },
            ]);

            let first_char = cell.content.chars().next().unwrap_or(' ');
            if first_char != ' ' {
                let key = {
                    use ab_glyph::Font;
                    let font = atlas.and_then(|a| a.font.as_ref());
                    let gid = font.map(|f| f.glyph_id(first_char).0).unwrap_or(0);
                    let font_size = atlas.map(|a| a.size).unwrap_or(14);
                    let mut style = crate::glyph_key::GlyphStyle::empty();
                    if cell.flags.contains(CellFlags::BOLD) { style.insert(crate::glyph_key::GlyphStyle::BOLD); }
                    if cell.flags.contains(CellFlags::ITALIC) { style.insert(crate::glyph_key::GlyphStyle::ITALIC); }
                    if cell.flags.contains(CellFlags::DIM) { style.insert(crate::glyph_key::GlyphStyle::DIM); }
                    if cell.flags.contains(CellFlags::UNDERLINE) { style.insert(crate::glyph_key::GlyphStyle::UNDERLINE); }
                    crate::glyph_key::GlyphKey::new(gid, Default::default(), font_size, style)
                };
                if let Some(g) = atlas.and_then(|a| a.get_glyph(&key)) {
                    let inv = 1.0 / ATLAS_SIZE as f32;
                    let u0 = (g.x as f32 + 0.5) * inv;
                    let v0 = (g.y as f32 + 0.5) * inv;
                    let u1 = (g.x as f32 + g.width as f32 - 0.5) * inv;
                    let v1 = (g.y as f32 + g.height as f32 - 0.5) * inv;

                    let baseline_y = row as f32 * ch + self.primary_ascent;
                    let gx = col as f32 * cw + g.bearing_x;
                    let gy = baseline_y + g.bearing_y;
                    let gw = g.width as f32;
                    let gh = g.height as f32;

                    let ftl = px_to_ndc(gx, gy);
                    let ftr = px_to_ndc(gx + gw, gy);
                    let fbl = px_to_ndc(gx, gy + gh);
                    let fbr = px_to_ndc(gx + gw, gy + gh);

                    fg.extend_from_slice(&[
                        GlyphVertex { position: ftl, uv: [u0, v0], fg_color, bg_color },
                        GlyphVertex { position: ftr, uv: [u1, v0], fg_color, bg_color },
                        GlyphVertex { position: fbl, uv: [u0, v1], fg_color, bg_color },
                        GlyphVertex { position: fbr, uv: [u1, v1], fg_color, bg_color },
                        GlyphVertex { position: ftr, uv: [u1, v0], fg_color, bg_color },
                        GlyphVertex { position: fbl, uv: [u0, v1], fg_color, bg_color },
                    ]);
                } else {
                    missing.push(key);
                }
            }
        }
        (bg, fg, missing)
    }

    /// Main row vertex builder — dispatches between ASCII fast path and
    /// shaped text path.
    fn build_row_vertices(&mut self, row: usize, cw: f32, ch: f32, px_to_ndc: &impl Fn(f32, f32) -> [f32; 2]) -> (Vec<GlyphVertex>, Vec<GlyphVertex>, Vec<crate::glyph_key::GlyphKey>) {
        // Clear decoration vertices first to avoid stale data on early return
        self.row_cache[row].dec_vertices.clear();
        if row >= self.processor.grid.rows.len() {
            return (Vec::new(), Vec::new(), Vec::new());
        }

        let atlas = self.glyph_atlas.as_ref();

        let result;

        // ASCII fast path — skip shaping for simple rows
        if self.row_is_ascii_simple(row) {
            result = self.render_ascii_row(row, cw, ch, atlas, px_to_ndc);
        } else {

        // Complex text: break into runs, shape with pen positioning
        let cols = self.processor.grid.cols;
        let cells = &self.processor.grid.rows[row].cells;

        // Background quads — one per grid column (grid remains authoritative)
        let mut bg = Vec::with_capacity(cols * 6);
        let global_reverse = self.processor.mode.contains(vte_core::state::TerminalMode::REVERSE_VIDEO);
        for col in 0..cols {
            let cell = &cells[col];
            if cell.width == 0 { continue; }
            let reverse = cell.flags.contains(CellFlags::REVERSE) ^ global_reverse;
            let dim = if cell.flags.contains(CellFlags::DIM) { 0.5 } else { 1.0 };
            let (fg_r, fg_g, fg_b) = color_to_f32(cell.fg_color, &self.theme, reverse);
            let (bg_r, bg_g, bg_b) = color_to_f32(cell.bg_color, &self.theme, !reverse);
            let _fg_color = [fg_r * dim, fg_g * dim, fg_b * dim, 1.0];
            let bg_color = [bg_r, bg_g, bg_b, 1.0];
            let x = col as f32 * cw;
            let y = row as f32 * ch;
            let tl = px_to_ndc(x, y);
            let tr = px_to_ndc(x + cw, y);
            let bl = px_to_ndc(x, y + ch);
            let br = px_to_ndc(x + cw, y + ch);
            bg.extend_from_slice(&[
                GlyphVertex { position: tl, uv: [0.0; 2], fg_color: bg_color, bg_color },
                GlyphVertex { position: tr, uv: [0.0; 2], fg_color: bg_color, bg_color },
                GlyphVertex { position: bl, uv: [0.0; 2], fg_color: bg_color, bg_color },
                GlyphVertex { position: br, uv: [0.0; 2], fg_color: bg_color, bg_color },
                GlyphVertex { position: tr, uv: [0.0; 2], fg_color: bg_color, bg_color },
                GlyphVertex { position: bl, uv: [0.0; 2], fg_color: bg_color, bg_color },
            ]);
        }

        // Check the shaped row cache before shaping
        let atlas_size = atlas.map(|a| a.size).unwrap_or(14);
        let fingerprint = self.row_fingerprint(row);
        let cache_key = (fingerprint, atlas_size);

        let (shaped_glyphs, run_col_starts, run_directions, run_col_ends) = if let Some(entry) = self.shaped_row_cache.get_mut(&cache_key) {
            entry.4 = self.shaped_cache_gen;
            self.shaped_cache_gen += 1;
            (entry.0.clone(), entry.1.clone(), entry.2.clone(), entry.3.clone())
        } else {
            let runs = self.break_row_into_runs(row);
            let shaper = match self.shaper.as_ref() {
                Some(s) => s,
                None => return self.render_ascii_row(row, cw, ch, atlas, px_to_ndc),
            };
            let font_manager = match self.font_manager.as_ref() {
                Some(fm) => fm,
                None => return self.render_ascii_row(row, cw, ch, atlas, px_to_ndc),
            };

            let mut all_shaped: Vec<ShapedGlyph> = Vec::new();
            let mut col_starts: Vec<usize> = Vec::with_capacity(runs.len());
            let mut run_directions: Vec<TextDirection> = Vec::with_capacity(runs.len());
            let mut col_ends: Vec<usize> = Vec::with_capacity(runs.len());

            for run in &runs {
                let mut cluster_map = Vec::with_capacity(run.text.len());
                for c in run.col_start..run.col_end {
                    if c >= cols { break; }
                    let cell_bytes = cells[c].content.len().max(1);
                    for _ in 0..cell_bytes {
                        cluster_map.push(c);
                    }
                }

                col_starts.push(run.col_start);
                run_directions.push(run.direction);
                col_ends.push(run.col_end);

                // Resolve run into font-specific spans and shape each
                let spans = font_manager.resolve_run(&run.text);
                if spans.is_empty() {
                    continue;
                }
                for span in &spans {
                    let span_text = &run.text[span.byte_range.clone()];
                    // Build per-span cluster map
                    let span_cluster: Vec<usize> = cluster_map[span.byte_range.clone()].to_vec();
                    // Build a sub-run for this span
                    let sub_run = TextRun {
                        row: run.row,
                        col_start: cluster_map[span.byte_range.start],
                        col_end: cluster_map.get(span.byte_range.end.saturating_sub(1))
                            .copied().unwrap_or(run.col_end).saturating_add(1).min(cols),
                        text: {
                            let mut t = ArrayString::new();
                            let _ = t.try_push_str(span_text);
                            t
                        },
                        style: run.style,
                        direction: run.direction,
                        script: run.script,
                    };
                    if let Some(fd) = font_manager.font_data(span.font_id) {
                        let mut shaped = shaper.shape_run(fd, span.font_id, &sub_run, &span_cluster);
                        shaper.correct_notdef_glyphs(font_manager, &mut shaped, &sub_run, &span_cluster);
                        all_shaped.extend(shaped);
                    }
                }
            }

            // Evict LRU entry if cache is full
            if self.shaped_row_cache.len() >= 256 {
                if let Some(evict) = self.shaped_row_cache.iter().min_by_key(|(_, v)| v.4) {
                    let evict_key = evict.0.clone();
                    self.shaped_row_cache.remove(&evict_key);
                }
            }

            let gen = self.shaped_cache_gen;
            self.shaped_cache_gen += 1;
            self.shaped_row_cache.insert(cache_key,
                (all_shaped.clone(), col_starts.clone(), run_directions.clone(), col_ends.clone(), gen));
            (all_shaped, col_starts, run_directions, col_ends)
        };

        if self.debug_show_overlay {
            log::info!("[SHAPE] row={} cache_len={} glyphs={}",
                row, self.shaped_row_cache.len(), shaped_glyphs.len());
            for sg in &shaped_glyphs {
                log::info!("  glyph_id={:5} font_id={} source_col={:3} x_adv={:8.3} y_adv={:8.3} x_off={:8.3} y_off={:8.3}",
                    sg.glyph_id, sg.font_id.0, sg.source_col, sg.x_advance, sg.y_advance, sg.x_offset, sg.y_offset);
            }
        }

        // Build visual run order from logical run metadata using BiDi
        let visual_runs = self.reorder_runs_visually(row, &run_col_starts, &run_col_ends, &run_directions);

        // Render foreground from shaped glyphs with direction-aware pen tracking
        let mut fg = Vec::new();
        let mut missing = Vec::new();
        let mut run_idx = 0usize;
        let mut pen_x = match visual_runs.first() {
            Some(vr) => match vr.direction {
                TextDirection::Rtl => vr.col_end as f32 * cw,
                TextDirection::Ltr => vr.col_start as f32 * cw,
            },
            None => 0.0,
        };

        for sg in &shaped_glyphs {
            // Detect run boundary and reset pen with direction-aware start
            while run_idx + 1 < run_col_starts.len() && sg.source_col >= run_col_starts[run_idx + 1] {
                run_idx += 1;
                let vr = &visual_runs[run_idx];
                pen_x = match vr.direction {
                    TextDirection::Rtl => vr.col_end as f32 * cw,
                    TextDirection::Ltr => vr.col_start as f32 * cw,
                };
            }

            if sg.source_col >= cols { continue; }
            let cell = &cells[sg.source_col];
            if cell.content.chars().next().unwrap_or(' ') == ' ' {
                match visual_runs[run_idx].direction {
                    TextDirection::Ltr => pen_x += sg.x_advance,
                    TextDirection::Rtl => pen_x -= sg.x_advance,
                }
                continue;
            }

            let global_reverse = self.processor.mode.contains(vte_core::state::TerminalMode::REVERSE_VIDEO);
            let cell_reverse = cell.flags.contains(CellFlags::REVERSE) ^ global_reverse;
            let dim = if cell.flags.contains(CellFlags::DIM) { 0.5 } else { 1.0 };
            let (fg_r, fg_g, fg_b) = color_to_f32(cell.fg_color, &self.theme, cell_reverse);
            let (bg_r, bg_g, bg_b) = color_to_f32(cell.bg_color, &self.theme, !cell_reverse);
            let fg_color = [fg_r * dim, fg_g * dim, fg_b * dim, 1.0];
            let bg_color = [bg_r, bg_g, bg_b, 1.0];

            let mut style = crate::glyph_key::GlyphStyle::empty();
            if cell.flags.contains(CellFlags::BOLD) { style.insert(crate::glyph_key::GlyphStyle::BOLD); }
            if cell.flags.contains(CellFlags::ITALIC) { style.insert(crate::glyph_key::GlyphStyle::ITALIC); }
            if cell.flags.contains(CellFlags::DIM) { style.insert(crate::glyph_key::GlyphStyle::DIM); }
            if cell.flags.contains(CellFlags::UNDERLINE) { style.insert(crate::glyph_key::GlyphStyle::UNDERLINE); }

            let key = crate::glyph_key::GlyphKey::new(sg.glyph_id, sg.font_id, atlas_size, style);
            if let Some(atlas_g) = atlas.and_then(|a| a.get_glyph(&key)) {
                let inv = 1.0 / ATLAS_SIZE as f32;
                let u0 = (atlas_g.x as f32 + 0.5) * inv;
                let v0 = (atlas_g.y as f32 + 0.5) * inv;
                let u1 = (atlas_g.x as f32 + atlas_g.width as f32 - 0.5) * inv;
                let v1 = (atlas_g.y as f32 + atlas_g.height as f32 - 0.5) * inv;

                let baseline_y = row as f32 * ch + self.primary_ascent;
                let gx = pen_x + sg.x_offset + atlas_g.bearing_x;
                let gy = baseline_y + atlas_g.bearing_y + sg.y_offset;
                let gw = atlas_g.width as f32;
                let gh = atlas_g.height as f32;

                let ftl = px_to_ndc(gx, gy);
                let ftr = px_to_ndc(gx + gw, gy);
                let fbl = px_to_ndc(gx, gy + gh);
                let fbr = px_to_ndc(gx + gw, gy + gh);

                fg.extend_from_slice(&[
                    GlyphVertex { position: ftl, uv: [u0, v0], fg_color, bg_color },
                    GlyphVertex { position: ftr, uv: [u1, v0], fg_color, bg_color },
                    GlyphVertex { position: fbl, uv: [u0, v1], fg_color, bg_color },
                    GlyphVertex { position: fbr, uv: [u1, v1], fg_color, bg_color },
                    GlyphVertex { position: ftr, uv: [u1, v0], fg_color, bg_color },
                    GlyphVertex { position: fbl, uv: [u0, v1], fg_color, bg_color },
                ]);
            } else {
                missing.push(key);
            }

            // Direction-aware pen advance
            match visual_runs[run_idx].direction {
                TextDirection::Ltr => pen_x += sg.x_advance,
                TextDirection::Rtl => pen_x -= sg.x_advance,
            }
        }

        result = (bg, fg, missing);
        }

        // Generate decoration overlay quads (underline/strikethrough) — common to both paths
        let cols = self.processor.grid.cols;
        let cells = &self.processor.grid.rows[row].cells;
        let baseline_y = row as f32 * ch + self.primary_ascent;
        let underline_y = baseline_y + self.primary_underline_pos;
        let underline_h = self.primary_underline_thickness;
        let strikethrough_y = baseline_y + self.primary_strikethrough_pos;
        let strikethrough_h = self.primary_strikethrough_thickness;

        let mut dec = Vec::new();
        for col in 0..cols {
            if col >= cells.len() { break; }
            let cell = &cells[col];

            let style = if cell.flags.contains(CellFlags::STRIKETHROUGH) {
                Some((strikethrough_y, strikethrough_h))
            } else if cell.flags.contains(CellFlags::UNDERLINE) {
                Some((underline_y, underline_h))
            } else {
                None
            };

            if let Some((dec_y, dec_h)) = style {
                let global_reverse = self.processor.mode.contains(vte_core::state::TerminalMode::REVERSE_VIDEO);
                let cell_reverse = cell.flags.contains(CellFlags::REVERSE) ^ global_reverse;
                let dim = if cell.flags.contains(CellFlags::DIM) { 0.5 } else { 1.0 };
                let (r, g, b) = color_to_f32(cell.fg_color, &self.theme, cell_reverse);
                let color = [r * dim, g * dim, b * dim, 1.0];

                let x = col as f32 * cw;
                let tl = px_to_ndc(x, dec_y);
                let tr = px_to_ndc(x + cw, dec_y);
                let bl = px_to_ndc(x, dec_y + dec_h);
                let br = px_to_ndc(x + cw, dec_y + dec_h);

                dec.extend_from_slice(&[
                    GlyphVertex { position: tl, uv: [0.0; 2], fg_color: color, bg_color: [0.0; 4] },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: color, bg_color: [0.0; 4] },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: color, bg_color: [0.0; 4] },
                    GlyphVertex { position: br, uv: [0.0; 2], fg_color: color, bg_color: [0.0; 4] },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: color, bg_color: [0.0; 4] },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: color, bg_color: [0.0; 4] },
                ]);
            }
        }
        self.row_cache[row].dec_vertices = dec;

        result
    }

    fn build_vertex_data(&mut self) -> (Vec<GlyphVertex>, Vec<GlyphVertex>, Vec<u32>, Vec<u32>, Vec<GlyphVertex>, Vec<u32>) {
        let rows = self.processor.grid.rows_visible;
        let cw = self.cell_w.max(8.0);
        let ch = self.cell_h.max(20.0);
        let (win_w, win_h) = self.surface_config
            .as_ref()
            .map(|c| (c.width as f32, c.height as f32))
            .unwrap_or((960.0, 540.0));
        let sx = 2.0 / win_w;
        let sy = -2.0 / win_h;
        let ox = -1.0;
        let oy = 1.0;

        let px_to_ndc = |px: f32, py: f32| -> [f32; 2] {
            [px * sx + ox, py * sy + oy]
        };

        // Resize cache to match grid
        self.row_cache.resize_with(rows, CachedRowMesh::new);

        // Copy damage state to avoid borrow conflict with mutable build_row_vertices
        let full_redraw = self.processor.grid.damage.full_redraw;
        let dirty_rows = self.processor.grid.damage.dirty_rows.clone();

        // Rebuild only dirty rows
        let mut missing_set = HashSet::new();
        for row in 0..rows {
            if full_redraw || dirty_rows.get(row).copied().unwrap_or(true) {
                let (bg, fg, missing) = self.build_row_vertices(row, cw, ch, &px_to_ndc);
                self.row_cache[row].bg_vertices = bg;
                self.row_cache[row].fg_vertices = fg;
                for key in missing {
                    missing_set.insert(key);
                }
            }
        }

        // Send raster requests for missing glyphs that aren't already pending
        for key in missing_set {
            if self.pending_raster.insert(key.clone()) {
                let font = self.glyph_atlas.as_ref().and_then(|a| a.font.clone());
                let size = self.glyph_atlas.as_ref().map(|a| a.size).unwrap_or(14);
                if let (Some(f), Some(ref tx)) = (font, self.raster_tx.as_ref()) {
                    let _ = tx.send(RasterRequest { key, font: f, size });
                }
            }
        }

        // Concatenate all cached rows into bg, fg, and decoration lists — single pass
        let (bg_total, fg_total, dec_total) = self.row_cache.iter().fold(
            (0usize, 0usize, 0usize),
            |(b, f, d), r| (b + r.bg_vertices.len(), f + r.fg_vertices.len(), d + r.dec_vertices.len()),
        );
        let mut bg_vertices = Vec::with_capacity(bg_total);
        let mut fg_vertices = Vec::with_capacity(fg_total);
        let mut dec_vertices = Vec::with_capacity(dec_total);
        for cached in &self.row_cache {
            bg_vertices.extend_from_slice(&cached.bg_vertices);
            fg_vertices.extend_from_slice(&cached.fg_vertices);
            dec_vertices.extend_from_slice(&cached.dec_vertices);
        }

        let bg_indices: Vec<u32> = (0..bg_vertices.len() as u32).collect();
        let fg_indices: Vec<u32> = (0..fg_vertices.len() as u32).collect();
        let dec_indices: Vec<u32> = (0..dec_vertices.len() as u32).collect();

        // Debug grid overlay — semi-transparent cell boundary lines + baseline
        self.debug_grid_vertices.clear();
        self.debug_grid_indices.clear();
        if self.debug_show_overlay {
            let cols = self.processor.grid.cols;
            let fg_color = [1.0, 0.3, 0.1, 0.25];
            let bg_color = [0.0; 4];
            let baseline_color = [0.3, 0.6, 1.0, 0.35];
            let half_px = 0.5;

            // Horizontal lines (cell boundaries)
            for r in 0..=rows {
                let y = r as f32 * ch;
                let x0 = 0.0;
                let x1 = cols as f32 * cw;
                let tl = px_to_ndc(x0, y - half_px);
                let tr = px_to_ndc(x1, y - half_px);
                let bl = px_to_ndc(x0, y + half_px);
                let br = px_to_ndc(x1, y + half_px);
                let base = self.debug_grid_vertices.len() as u32;
                self.debug_grid_vertices.extend_from_slice(&[
                    GlyphVertex { position: tl, uv: [0.0; 2], fg_color, bg_color },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color, bg_color },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color, bg_color },
                    GlyphVertex { position: br, uv: [0.0; 2], fg_color, bg_color },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color, bg_color },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color, bg_color },
                ]);
                self.debug_grid_indices.extend_from_slice(&[base, base+1, base+2, base+3, base+1, base+2]);
            }

            // Baseline lines (one per row, blue highlight)
            for r in 0..rows {
                let y = r as f32 * ch + self.primary_ascent;
                let x0 = 0.0;
                let x1 = cols as f32 * cw;
                let tl = px_to_ndc(x0, y - half_px);
                let tr = px_to_ndc(x1, y - half_px);
                let bl = px_to_ndc(x0, y + half_px);
                let br = px_to_ndc(x1, y + half_px);
                let base = self.debug_grid_vertices.len() as u32;
                self.debug_grid_vertices.extend_from_slice(&[
                    GlyphVertex { position: tl, uv: [0.0; 2], fg_color: baseline_color, bg_color },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: baseline_color, bg_color },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: baseline_color, bg_color },
                    GlyphVertex { position: br, uv: [0.0; 2], fg_color: baseline_color, bg_color },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: baseline_color, bg_color },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: baseline_color, bg_color },
                ]);
                self.debug_grid_indices.extend_from_slice(&[base, base+1, base+2, base+3, base+1, base+2]);
            }

            // Vertical lines
            for c in 0..=cols {
                let x = c as f32 * cw;
                let y0 = 0.0;
                let y1 = rows as f32 * ch;
                let tl = px_to_ndc(x - half_px, y0);
                let tr = px_to_ndc(x + half_px, y0);
                let bl = px_to_ndc(x - half_px, y1);
                let br = px_to_ndc(x + half_px, y1);
                let base = self.debug_grid_vertices.len() as u32;
                self.debug_grid_vertices.extend_from_slice(&[
                    GlyphVertex { position: tl, uv: [0.0; 2], fg_color, bg_color },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color, bg_color },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color, bg_color },
                    GlyphVertex { position: br, uv: [0.0; 2], fg_color, bg_color },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color, bg_color },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color, bg_color },
                ]);
                self.debug_grid_indices.extend_from_slice(&[base, base+1, base+2, base+3, base+1, base+2]);
            }
        }

        (bg_vertices, fg_vertices, bg_indices, fg_indices, dec_vertices, dec_indices)
    }

    /// Build cursor overlay geometry — pure overlay, no row/shape cache invalidation.
    fn build_cursor_overlay(&mut self, cw: f32, ch: f32) {
        self.cursor_vertices.clear();
        self.cursor_indices.clear();

        let cursor = &self.processor.cursor;
        let is_visible = self.processor.mode.contains(vte_core::state::TerminalMode::CURSOR_VISIBLE);
        if !is_visible { return; }

        // Determine blink state
        let is_blinking = match cursor.style {
            vte_core::state::CursorStyle::BlinkingBlock
            | vte_core::state::CursorStyle::BlinkingUnderline
            | vte_core::state::CursorStyle::BlinkingBeam => true,
            _ => self.processor.mode.contains(vte_core::state::TerminalMode::CURSOR_BLINK),
        };
        if is_blinking && !self.cursor_blink_visible { return; }

        // Map cursor row to visible row index
        let rows = self.processor.grid.rows.len();
        let visible = self.processor.grid.rows_visible;
        let scrollback = rows.saturating_sub(visible);
        let vis_row = cursor.row.saturating_sub(scrollback);
        let logical_col = cursor.col;

        if vis_row >= visible || logical_col >= self.processor.grid.cols { return; }

        // Project cursor from logical column to visual column
        let col = self.logical_to_visual(cursor.row, logical_col);

        let (win_w, win_h) = self.surface_config
            .as_ref()
            .map(|c| (c.width as f32, c.height as f32))
            .unwrap_or((960.0, 540.0));
        let sx = 2.0 / win_w;
        let sy = -2.0 / win_h;
        let ox = -1.0;
        let oy = 1.0;
        let px_to_ndc = |px: f32, py: f32| -> [f32; 2] {
            [px * sx + ox, py * sy + oy]
        };

        let x = col as f32 * cw;
        let y = vis_row as f32 * ch;

        // Cursor color from theme
        let (cr, cg, cb) = self.theme.cursor_rgb();
        let cursor_color = [cr, cg, cb, 1.0];

        match cursor.style {
            vte_core::state::CursorStyle::Block
            | vte_core::state::CursorStyle::BlinkingBlock => {
                let tl = px_to_ndc(x, y);
                let tr = px_to_ndc(x + cw, y);
                let bl = px_to_ndc(x, y + ch);
                let br = px_to_ndc(x + cw, y + ch);
                self.cursor_vertices.extend_from_slice(&[
                    GlyphVertex { position: tl, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: br, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                ]);
                self.cursor_indices.extend(0..6);
            }
            vte_core::state::CursorStyle::Underline
            | vte_core::state::CursorStyle::BlinkingUnderline => {
                let ul_y = y + ch - 3.0;
                let ul_h = 2.0f32;
                let tl = px_to_ndc(x, ul_y);
                let tr = px_to_ndc(x + cw, ul_y);
                let bl = px_to_ndc(x, ul_y + ul_h);
                let br = px_to_ndc(x + cw, ul_y + ul_h);
                self.cursor_vertices.extend_from_slice(&[
                    GlyphVertex { position: tl, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: br, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                ]);
                self.cursor_indices.extend(0..6);
            }
            vte_core::state::CursorStyle::Beam
            | vte_core::state::CursorStyle::BlinkingBeam => {
                let beam_w = 2.0f32;
                let tl = px_to_ndc(x, y);
                let tr = px_to_ndc(x + beam_w, y);
                let bl = px_to_ndc(x, y + ch);
                let br = px_to_ndc(x + beam_w, y + ch);
                self.cursor_vertices.extend_from_slice(&[
                    GlyphVertex { position: tl, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: br, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: cursor_color, bg_color: [0.0; 4] },
                ]);
                self.cursor_indices.extend(0..6);
            }
        }
    }

    /// Build selection overlay geometry — full-cell quads for selected range.
    fn build_selection_overlay(&mut self, cw: f32, ch: f32) {
        self.selection_vertices.clear();
        self.selection_indices.clear();

        if !self.selection.active { return; }

        let rows = self.processor.grid.rows.len();
        let visible = self.processor.grid.rows_visible;
        let cols = self.processor.grid.cols;
        let scrollback = rows.saturating_sub(visible);

        let mut start_row = self.selection.start_row;
        let mut start_col = self.selection.start_col;
        let mut end_row = self.selection.end_row;
        let mut end_col = self.selection.end_col;

        // Clamp to grid
        if start_row >= rows { start_row = rows.saturating_sub(1); }
        if end_row >= rows { end_row = rows.saturating_sub(1); }
        if start_col >= cols { start_col = cols.saturating_sub(1); }
        if end_col >= cols { end_col = cols.saturating_sub(1); }

        let (win_w, win_h) = self.surface_config
            .as_ref()
            .map(|c| (c.width as f32, c.height as f32))
            .unwrap_or((960.0, 540.0));
        let sx = 2.0 / win_w;
        let sy = -2.0 / win_h;
        let ox = -1.0;
        let oy = 1.0;
        let px_to_ndc = |px: f32, py: f32| -> [f32; 2] {
            [px * sx + ox, py * sy + oy]
        };

        // Selection highlight color from theme
        let (sr, sg, sb) = self.theme.selection_rgb();
        let sel_color = [sr, sg, sb, 0.4];

        for row in start_row..=end_row {
            let vis_row = row.saturating_sub(scrollback);
            if vis_row >= visible { continue; }

            let is_first_row = row == start_row;
            let is_last_row = row == end_row;
            let r_start_c = if is_first_row { start_col } else { 0 };
            let r_end_c = if is_last_row { end_col } else { cols.saturating_sub(1) };

            for logical_col in r_start_c..=r_end_c {
                // Project logical column to visual column for RTL-aware positioning
                let col = self.logical_to_visual(row, logical_col);
                let x = col as f32 * cw;
                let y = vis_row as f32 * ch;
                let tl = px_to_ndc(x, y);
                let tr = px_to_ndc(x + cw, y);
                let bl = px_to_ndc(x, y + ch);
                let br = px_to_ndc(x + cw, y + ch);
                self.selection_vertices.extend_from_slice(&[
                    GlyphVertex { position: tl, uv: [0.0; 2], fg_color: sel_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: sel_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: sel_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: br, uv: [0.0; 2], fg_color: sel_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: tr, uv: [0.0; 2], fg_color: sel_color, bg_color: [0.0; 4] },
                    GlyphVertex { position: bl, uv: [0.0; 2], fg_color: sel_color, bg_color: [0.0; 4] },
                ]);
                self.selection_indices.extend(0..6);
            }
        }
    }

    fn screen_to_grid(&self, px: f64, py: f64) -> Option<(usize, usize)> {
        let rows = self.processor.grid.rows.len();
        let cols = self.processor.grid.cols;
        let visible = self.processor.grid.rows_visible;
        let scrollback = rows.saturating_sub(visible);
        if self.cell_w <= 0.0 || self.cell_h <= 0.0 { return None; }
        let col = (px as f32 / self.cell_w) as usize;
        let vis_row = (py as f32 / self.cell_h) as usize;
        if col >= cols || vis_row >= visible { return None; }
        let row = scrollback + vis_row;
        if row >= rows { return None; }
        Some((row, col))
    }

    fn last_mouse_grid_pos(&self) -> Option<(usize, usize)> {
        self.screen_to_grid(self.last_mouse_x, self.last_mouse_y)
    }

    #[allow(dead_code)]
    fn search_text(&mut self, query: &str) {
        self.search.query = query.to_string();
        self.search.matches.clear();
        if query.is_empty() { return; }

        let rows = self.processor.grid.rows.len();
        let cols = self.processor.grid.cols;
        let query_lower = query.to_lowercase();

        for row in 0..rows {
            let mut col = 0;
            while col < cols {
                // Build the cell's visible text (respect width 0 spacer skip)
                let cell = &self.processor.grid.rows[row].cells[col];
                let cell_text: String = cell.content.chars().collect();
                let cell_lower = cell_text.to_lowercase();

                if cell_lower.starts_with(&query_lower[..1]) || query.is_empty() {
                    // Try to match the full query starting here
                    let mut match_len = 0;
                    let mut query_idx = 0;
                    let match_col = col;
                    let mut matched = true;
                    let query_chars: Vec<char> = query_lower.chars().collect();

                    while query_idx < query_chars.len() && col + match_len < cols {
                        let c = &self.processor.grid.rows[row].cells[col + match_len];
                        let t: String = c.content.chars().collect();
                        let t_lower = t.to_lowercase();
                        let remaining: String = query_chars[query_idx..].iter().collect();

                        if t_lower.starts_with(&remaining) {
                            match_len += remaining.len().saturating_sub(1).max(1);
                            query_idx = query_chars.len();
                            break;
                        }

                        if t_lower.starts_with(&query_chars[query_idx..query_idx.saturating_add(1)].iter().collect::<String>()) {
                            query_idx += 1;
                            match_len += 1;
                        } else {
                            matched = false;
                            break;
                        }
                    }

                    if matched && query_idx >= query_chars.len() {
                        self.search.matches.push(SearchMatch {
                            start_row: row,
                            start_col: match_col,
                            end_row: row,
                            end_col: match_col + match_len.saturating_sub(1),
                        });
                    }
                }
                col += 1;
            }
        }
    }

    #[allow(dead_code)]
    fn search_next(&mut self) -> Option<&SearchMatch> {
        if self.search.matches.is_empty() { return None; }
        // Find the first match at or after the cursor position
        let cursor_row = self.processor.cursor.row;
        let cursor_col = self.processor.cursor.col;
        for m in &self.search.matches {
            if m.start_row > cursor_row || (m.start_row == cursor_row && m.start_col >= cursor_col) {
                return Some(m);
            }
        }
        self.search.matches.first()
    }

    #[allow(dead_code)]
    fn search_prev(&mut self) -> Option<&SearchMatch> {
        if self.search.matches.is_empty() { return None; }
        let cursor_row = self.processor.cursor.row;
        let cursor_col = self.processor.cursor.col;
        for m in self.search.matches.iter().rev() {
            if m.end_row < cursor_row || (m.end_row == cursor_row && m.end_col <= cursor_col) {
                return Some(m);
            }
        }
        self.search.matches.last()
    }

    fn build_search_overlay(&mut self, cw: f32, ch: f32) {
        self.search_vertices.clear();
        self.search_indices.clear();

        if !self.search.is_active() { return; }

        let rows = self.processor.grid.rows.len();
        let visible = self.processor.grid.rows_visible;
        let cols = self.processor.grid.cols;
        let scrollback = rows.saturating_sub(visible);

        let (win_w, win_h) = self.surface_config
            .as_ref()
            .map(|c| (c.width as f32, c.height as f32))
            .unwrap_or((960.0, 540.0));
        let sx = 2.0 / win_w;
        let sy = -2.0 / win_h;
        let ox = -1.0;
        let oy = 1.0;
        let px_to_ndc = |px: f32, py: f32| -> [f32; 2] {
            [px * sx + ox, py * sy + oy]
        };

        // Search highlight color — theme-derived selection color with alpha
        let (sr, sg, sb) = self.theme.selection_rgb();
        let search_color = [sr, sg, sb, 0.4];

        for m in &self.search.matches {
            for row in m.start_row..=m.end_row {
                let vis_row = row.saturating_sub(scrollback);
                if vis_row >= visible { continue; }

                let r_start_c = if row == m.start_row { m.start_col } else { 0 };
                let r_end_c = if row == m.end_row { m.end_col } else { cols.saturating_sub(1) };

                for logical_col in r_start_c..=r_end_c {
                    let col = self.logical_to_visual(row, logical_col);
                    let x = col as f32 * cw;
                    let y = vis_row as f32 * ch;
                    let tl = px_to_ndc(x, y);
                    let tr = px_to_ndc(x + cw, y);
                    let bl = px_to_ndc(x, y + ch);
                    let br = px_to_ndc(x + cw, y + ch);
                    self.search_vertices.extend_from_slice(&[
                        GlyphVertex { position: tl, uv: [0.0; 2], fg_color: search_color, bg_color: [0.0; 4] },
                        GlyphVertex { position: tr, uv: [0.0; 2], fg_color: search_color, bg_color: [0.0; 4] },
                        GlyphVertex { position: bl, uv: [0.0; 2], fg_color: search_color, bg_color: [0.0; 4] },
                        GlyphVertex { position: br, uv: [0.0; 2], fg_color: search_color, bg_color: [0.0; 4] },
                        GlyphVertex { position: tr, uv: [0.0; 2], fg_color: search_color, bg_color: [0.0; 4] },
                        GlyphVertex { position: bl, uv: [0.0; 2], fg_color: search_color, bg_color: [0.0; 4] },
                    ]);
                    self.search_indices.extend(0..6);
                }
            }
        }
    }

    fn render(&mut self) {
        // Flush staged uploads before building vertex data
        if !self.pending_uploads.is_empty() {
            let pending = std::mem::take(&mut self.pending_uploads);
            if let (Some(ref device), Some(ref queue)) = (&self.device, &self.queue) {
                if let Some(ref mut atlas) = self.glyph_atlas {
                    flush_uploads(device, queue, atlas, pending);
                }
            }
        }

        if !self.needs_redraw {
            return;
        }

        // Skip if nothing actually changed in the grid
        if !self.processor.grid.damage.any_dirty() {
            self.needs_redraw = false;
            return;
        }

        let now = Instant::now();
        if now.duration_since(self.last_render) < MIN_FRAME_TIME {
            self.needs_redraw = true; // wait for next cycle
            return;
        }
        self.last_render = now;

        // Build vertex data BEFORE borrowing self fields
        let (bg_vertices, fg_vertices, bg_indices, fg_indices, dec_vertices, dec_indices) = self.build_vertex_data();
        self.build_cursor_overlay(self.cell_w, self.cell_h);
        self.build_selection_overlay(self.cell_w, self.cell_h);
        self.build_search_overlay(self.cell_w, self.cell_h);
        if bg_vertices.is_empty() && fg_vertices.is_empty() && dec_vertices.is_empty()
            && self.cursor_vertices.is_empty() && self.selection_indices.is_empty() {
            log::warn!("No vertices to render");
            self.needs_redraw = false;
            self.processor.grid.damage.clear();
            return;
        }

        // Update graphics overlay (Sixel/Kitty) if new data arrives
        if let Some(graphics_image) = self.processor.graphics_image.take() {
            if let (Some(device), Some(queue)) = (self.device.as_ref(), self.queue.as_ref()) {
                let width = graphics_image.width.max(1);
                let height = graphics_image.height.max(1);

                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Graphics Overlay Texture"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

                let data = if graphics_image.data.len() >= (width * height * 4) as usize {
                    &graphics_image.data
                } else {
                    let mut padded = vec![0u8; (width * height * 4) as usize];
                    padded[..graphics_image.data.len()].copy_from_slice(&graphics_image.data);
                    queue.write_texture(
                        wgpu::ImageCopyTexture { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                        &padded,
                        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
                        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    );
                    self.graphics_overlay_texture = Some(texture);
                    self.graphics_overlay_view = Some(view);
                    return;
                };
                queue.write_texture(
                    wgpu::ImageCopyTexture { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    data,
                    wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
                    wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                );

                if let (Some(overlay_bgl), Some(overlay_sampler)) = (self.overlay_bgl.as_ref(), self.overlay_sampler.as_ref()) {
                    let bind_group = device.create_bind_group(&BindGroupDescriptor {
                        label: Some("Graphics Overlay Bind Group"),
                        layout: overlay_bgl,
                        entries: &[
                            BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                            BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(overlay_sampler) },
                        ],
                    });
                    self.graphics_overlay_bind_group = Some(bind_group);
                }

                let overlay_vertices = vec![
                    GlyphVertex { position: [-1.0, -1.0], uv: [0.0, 1.0], fg_color: [1.0; 4], bg_color: [0.0; 4] },
                    GlyphVertex { position: [1.0, -1.0], uv: [1.0, 1.0], fg_color: [1.0; 4], bg_color: [0.0; 4] },
                    GlyphVertex { position: [-1.0, 1.0], uv: [0.0, 0.0], fg_color: [1.0; 4], bg_color: [0.0; 4] },
                    GlyphVertex { position: [1.0, 1.0], uv: [1.0, 0.0], fg_color: [1.0; 4], bg_color: [0.0; 4] },
                ];
                let overlay_indices: Vec<u32> = vec![0, 1, 2, 1, 3, 2];

                let vert_bytes = bytemuck::cast_slice(&overlay_vertices);
                let idx_bytes = bytemuck::cast_slice(&overlay_indices);

                self.overlay_quad_vb = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Overlay Quad Verts"),
                    size: vert_bytes.len() as u64,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
                self.overlay_quad_ib = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Overlay Quad Idx"),
                    size: idx_bytes.len() as u64,
                    usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));

                if let (Some(vb), Some(ib)) = (self.overlay_quad_vb.as_ref(), self.overlay_quad_ib.as_ref()) {
                    queue.write_buffer(vb, 0, vert_bytes);
                    queue.write_buffer(ib, 0, idx_bytes);
                }

                self.graphics_overlay_texture = Some(texture);
                self.graphics_overlay_view = Some(view);
            }
        }

        let (surface, device, queue, config, bg_pipeline, fg_pipeline, bind_group) = match (
            &self.surface, &self.device, &self.queue, &self.surface_config,
            &self.pipeline, &self.fg_pipeline, &self.bind_group,
        ) {
            (Some(s), Some(d), Some(q), Some(c), Some(bp), Some(fp), Some(bg)) => (s, d, q, c, bp, fp, bg),
            _ => { self.needs_redraw = true; return; }
        };

        // Upload background vertex data
        let bg_vert_bytes = bytemuck::cast_slice(&bg_vertices);
        let bg_idx_bytes = bytemuck::cast_slice(&bg_indices);

        if self.vertex_buf.as_ref().map_or(true, |vb| vb.size() < bg_vert_bytes.len() as u64) {
            self.vertex_buf = Some(device.create_buffer(&BufferDescriptor {
                label: Some("Terminal BG Verts"),
                size: bg_vert_bytes.len().max(1024) as u64,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(bg_vb) = self.vertex_buf.as_ref() else {
            self.needs_redraw = true;
            return;
        };
        queue.write_buffer(bg_vb, 0, bg_vert_bytes);

        if self.index_buf.as_ref().map_or(true, |ib| ib.size() < bg_idx_bytes.len() as u64) {
            self.index_buf = Some(device.create_buffer(&BufferDescriptor {
                label: Some("Terminal BG Idx"),
                size: bg_idx_bytes.len().max(1024) as u64,
                usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(bg_ib) = self.index_buf.as_ref() else {
            self.needs_redraw = true;
            return;
        };
        queue.write_buffer(bg_ib, 0, bg_idx_bytes);

        // Upload foreground vertex data
        let fg_vert_bytes = bytemuck::cast_slice(&fg_vertices);
        let fg_idx_bytes = bytemuck::cast_slice(&fg_indices);

        if self.fg_vertex_buf.as_ref().map_or(true, |vb| vb.size() < fg_vert_bytes.len() as u64) {
            self.fg_vertex_buf = Some(device.create_buffer(&BufferDescriptor {
                label: Some("Terminal FG Verts"),
                size: fg_vert_bytes.len().max(1024) as u64,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(fg_vb) = self.fg_vertex_buf.as_ref() else {
            self.needs_redraw = true;
            return;
        };
        queue.write_buffer(fg_vb, 0, fg_vert_bytes);

        if self.fg_index_buf.as_ref().map_or(true, |ib| ib.size() < fg_idx_bytes.len() as u64) {
            self.fg_index_buf = Some(device.create_buffer(&BufferDescriptor {
                label: Some("Terminal FG Idx"),
                size: fg_idx_bytes.len().max(1024) as u64,
                usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(fg_ib) = self.fg_index_buf.as_ref() else {
            self.needs_redraw = true;
            return;
        };
        queue.write_buffer(fg_ib, 0, fg_idx_bytes);

        // Upload decoration overlay vertex data
        if !dec_indices.is_empty() {
            let dec_vert_bytes = bytemuck::cast_slice(&dec_vertices);
            let dec_idx_bytes = bytemuck::cast_slice(&dec_indices);
            if self.dec_vb.as_ref().map_or(true, |vb| vb.size() < dec_vert_bytes.len() as u64) {
                self.dec_vb = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Terminal Decoration Verts"),
                    size: dec_vert_bytes.len().max(1024) as u64,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if self.dec_ib.as_ref().map_or(true, |ib| ib.size() < dec_idx_bytes.len() as u64) {
                self.dec_ib = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Terminal Decoration Idx"),
                    size: dec_idx_bytes.len().max(1024) as u64,
                    usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if let (Some(ref d_vb), Some(ref d_ib)) = (&self.dec_vb, &self.dec_ib) {
                queue.write_buffer(d_vb, 0, dec_vert_bytes);
                queue.write_buffer(d_ib, 0, dec_idx_bytes);
            }
        }

        // Upload cursor overlay vertex data
        if !self.cursor_indices.is_empty() {
            let cur_vert_bytes = bytemuck::cast_slice(&self.cursor_vertices);
            let cur_idx_bytes = bytemuck::cast_slice(&self.cursor_indices);
            if self.cursor_vb.as_ref().map_or(true, |vb| vb.size() < cur_vert_bytes.len() as u64) {
                self.cursor_vb = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Cursor Verts"),
                    size: cur_vert_bytes.len().max(64) as u64,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if self.cursor_ib.as_ref().map_or(true, |ib| ib.size() < cur_idx_bytes.len() as u64) {
                self.cursor_ib = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Cursor Idx"),
                    size: cur_idx_bytes.len().max(64) as u64,
                    usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if let (Some(ref cvb), Some(ref cib)) = (&self.cursor_vb, &self.cursor_ib) {
                queue.write_buffer(cvb, 0, cur_vert_bytes);
                queue.write_buffer(cib, 0, cur_idx_bytes);
            }
        }

        // Upload selection overlay vertex data
        if !self.selection_indices.is_empty() {
            let sel_vert_bytes = bytemuck::cast_slice(&self.selection_vertices);
            let sel_idx_bytes = bytemuck::cast_slice(&self.selection_indices);
            if self.selection_vb.as_ref().map_or(true, |vb| vb.size() < sel_vert_bytes.len() as u64) {
                self.selection_vb = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Selection Verts"),
                    size: sel_vert_bytes.len().max(64) as u64,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if self.selection_ib.as_ref().map_or(true, |ib| ib.size() < sel_idx_bytes.len() as u64) {
                self.selection_ib = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Selection Idx"),
                    size: sel_idx_bytes.len().max(64) as u64,
                    usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if let (Some(ref svb), Some(ref sib)) = (&self.selection_vb, &self.selection_ib) {
                queue.write_buffer(svb, 0, sel_vert_bytes);
                queue.write_buffer(sib, 0, sel_idx_bytes);
            }
        }

        // Upload search highlight overlay vertex data
        if !self.search_indices.is_empty() {
            let sh_vert_bytes = bytemuck::cast_slice(&self.search_vertices);
            let sh_idx_bytes = bytemuck::cast_slice(&self.search_indices);
            if self.search_vb.as_ref().map_or(true, |vb| vb.size() < sh_vert_bytes.len() as u64) {
                self.search_vb = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Search Verts"),
                    size: sh_vert_bytes.len().max(64) as u64,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if self.search_ib.as_ref().map_or(true, |ib| ib.size() < sh_idx_bytes.len() as u64) {
                self.search_ib = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Search Idx"),
                    size: sh_idx_bytes.len().max(64) as u64,
                    usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if let (Some(ref svb), Some(ref sib)) = (&self.search_vb, &self.search_ib) {
                queue.write_buffer(svb, 0, sh_vert_bytes);
                queue.write_buffer(sib, 0, sh_idx_bytes);
            }
        }

        // Upload debug grid overlay
        if self.debug_show_overlay && !self.debug_grid_indices.is_empty() {
            let dbg_vert_bytes = bytemuck::cast_slice(&self.debug_grid_vertices);
            let dbg_idx_bytes = bytemuck::cast_slice(&self.debug_grid_indices);
            if self.debug_vb.as_ref().map_or(true, |vb| vb.size() < dbg_vert_bytes.len() as u64) {
                self.debug_vb = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Debug Grid Verts"),
                    size: dbg_vert_bytes.len().max(1024) as u64,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if self.debug_ib.as_ref().map_or(true, |ib| ib.size() < dbg_idx_bytes.len() as u64) {
                self.debug_ib = Some(device.create_buffer(&BufferDescriptor {
                    label: Some("Debug Grid Idx"),
                    size: dbg_idx_bytes.len().max(1024) as u64,
                    usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if let (Some(ref dbg_vb), Some(ref dbg_ib)) = (&self.debug_vb, &self.debug_ib) {
                queue.write_buffer(dbg_vb, 0, dbg_vert_bytes);
                queue.write_buffer(dbg_ib, 0, dbg_idx_bytes);
            }
        }

        let frame = match surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost) => {
                surface.configure(device, config);
                self.needs_redraw = true;
                return;
            }
            Err(e) => {
                log::warn!("Surface error: {:?}", e);
                self.needs_redraw = true;
                return;
            }
        };

        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("Frame Encoder"),
        });

        // Dispatch compute pass for glyph processing
        if let (Some(ref cp), Some(ref cbg)) = (&self.compute_pipeline, &self.compute_bind_group) {
            let pass_label = Some("Glyph Compute Pass");
            let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: pass_label,
                timestamp_writes: None,
            });
            compute_pass.set_pipeline(cp);
            compute_pass.set_bind_group(0, cbg, &[]);
            // Dispatch with 1 workgroup for now (no glyphs yet)
            compute_pass.dispatch_workgroups(1, 1, 1);
        }

        {
            let mut rp = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Terminal Render Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color { r: 0.05, g: 0.05, b: 0.07, a: 1.0 }),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Background layer — solid fills
            if !bg_indices.is_empty() {
                rp.set_pipeline(bg_pipeline);
                rp.set_bind_group(0, bind_group, &[]);
                rp.set_vertex_buffer(0, bg_vb.slice(..));
                rp.set_index_buffer(bg_ib.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..bg_indices.len() as u32, 0, 0..1);
            }

            // Decoration overlay — underline/strikethrough between BG and FG
            if !dec_indices.is_empty() {
                if let (Some(ref d_vb), Some(ref d_ib)) = (&self.dec_vb, &self.dec_ib) {
                    rp.set_pipeline(bg_pipeline);
                    rp.set_bind_group(0, bind_group, &[]);
                    rp.set_vertex_buffer(0, d_vb.slice(..));
                    rp.set_index_buffer(d_ib.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..dec_indices.len() as u32, 0, 0..1);
                }
            }

            // Selection overlay — rendered between decoration and block cursor so selection
            // highlight sits behind text but overrides cell backgrounds
            if !self.selection_indices.is_empty() {
                if let (Some(ref svb), Some(ref sib)) = (&self.selection_vb, &self.selection_ib) {
                    rp.set_pipeline(bg_pipeline);
                    rp.set_bind_group(0, bind_group, &[]);
                    rp.set_vertex_buffer(0, svb.slice(..));
                    rp.set_index_buffer(sib.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..self.selection_indices.len() as u32, 0, 0..1);
                }
            }

            // Search highlight overlay — rendered after selection, before block cursor
            if !self.search_indices.is_empty() {
                if let (Some(ref svb), Some(ref sib)) = (&self.search_vb, &self.search_ib) {
                    rp.set_pipeline(bg_pipeline);
                    rp.set_bind_group(0, bind_group, &[]);
                    rp.set_vertex_buffer(0, svb.slice(..));
                    rp.set_index_buffer(sib.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..self.search_indices.len() as u32, 0, 0..1);
                }
            }

            // Block cursor — rendered BEFORE FG so text shows on top
            if !self.cursor_indices.is_empty() {
                let is_block = matches!(self.processor.cursor.style,
                    vte_core::state::CursorStyle::Block
                    | vte_core::state::CursorStyle::BlinkingBlock);
                if is_block {
                    if let (Some(ref cvb), Some(ref cib)) = (&self.cursor_vb, &self.cursor_ib) {
                        rp.set_pipeline(bg_pipeline);
                        rp.set_bind_group(0, bind_group, &[]);
                        rp.set_vertex_buffer(0, cvb.slice(..));
                        rp.set_index_buffer(cib.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..self.cursor_indices.len() as u32, 0, 0..1);
                    }
                }
            }

            // Foreground layer — glyphs with alpha blending
            if !fg_indices.is_empty() {
                rp.set_pipeline(fg_pipeline);
                rp.set_bind_group(0, bind_group, &[]);
                rp.set_vertex_buffer(0, fg_vb.slice(..));
                rp.set_index_buffer(fg_ib.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..fg_indices.len() as u32, 0, 0..1);
            }

            // Beam/Underline cursor — rendered AFTER FG so lines show on top
            if !self.cursor_indices.is_empty() {
                let is_line_style = matches!(self.processor.cursor.style,
                    vte_core::state::CursorStyle::Underline
                    | vte_core::state::CursorStyle::BlinkingUnderline
                    | vte_core::state::CursorStyle::Beam
                    | vte_core::state::CursorStyle::BlinkingBeam);
                if is_line_style {
                    if let (Some(ref cvb), Some(ref cib)) = (&self.cursor_vb, &self.cursor_ib) {
                        rp.set_pipeline(bg_pipeline);
                        rp.set_bind_group(0, bind_group, &[]);
                        rp.set_vertex_buffer(0, cvb.slice(..));
                        rp.set_index_buffer(cib.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..self.cursor_indices.len() as u32, 0, 0..1);
                    }
                }
            }

            // Debug overlay — semi-transparent cell boundary grid
            if self.debug_show_overlay {
                if let (Some(ref dbg_vb), Some(ref dbg_ib)) = (&self.debug_vb, &self.debug_ib) {
                    if !self.debug_grid_indices.is_empty() {
                        rp.set_pipeline(bg_pipeline);
                        rp.set_bind_group(0, bind_group, &[]);
                        rp.set_vertex_buffer(0, dbg_vb.slice(..));
                        rp.set_index_buffer(dbg_ib.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..self.debug_grid_indices.len() as u32, 0, 0..1);
                    }
                }
            }

            // Graphics overlay (Sixel/Kitty) — rendered on top of everything
            if let Some(ref overlay_bg) = self.graphics_overlay_bind_group {
                if let (Some(ref ovb), Some(ref oib)) = (&self.overlay_quad_vb, &self.overlay_quad_ib) {
                    if let Some(ref op) = self.overlay_pipeline {
                        rp.set_pipeline(op);
                        rp.set_bind_group(0, bind_group, &[]);
                        rp.set_bind_group(1, overlay_bg, &[]);
                        rp.set_vertex_buffer(0, ovb.slice(..));
                        rp.set_index_buffer(oib.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..6, 0, 0..1);
                    }
                }
            }
        }

        queue.submit(Some(encoder.finish()));
        frame.present();
        self.needs_redraw = false;
        self.processor.grid.damage.clear();
        self.render_epoch += 1;
    }
}

fn color_to_f32(color: vte_core::grid::Color, theme: &Theme, is_bg: bool) -> (f32, f32, f32) {
    use vte_core::grid::Color;
    match color {
        Color::Default => {
            if is_bg { theme.background_rgb() } else { theme.foreground_rgb() }
        }
        Color::Black => theme.color_rgb(0, false),
        Color::Red => theme.color_rgb(1, false),
        Color::Green => theme.color_rgb(2, false),
        Color::Yellow => theme.color_rgb(3, false),
        Color::Blue => theme.color_rgb(4, false),
        Color::Magenta => theme.color_rgb(5, false),
        Color::Cyan => theme.color_rgb(6, false),
        Color::White => theme.color_rgb(7, false),
        Color::BrightBlack => theme.color_rgb(0, true),
        Color::BrightRed => theme.color_rgb(1, true),
        Color::BrightGreen => theme.color_rgb(2, true),
        Color::BrightYellow => theme.color_rgb(3, true),
        Color::BrightBlue => theme.color_rgb(4, true),
        Color::BrightMagenta => theme.color_rgb(5, true),
        Color::BrightCyan => theme.color_rgb(6, true),
        Color::BrightWhite => theme.color_rgb(7, true),
        Color::Indexed(i) => {
            if i < 16 {
                theme.color_rgb(i % 8, i >= 8)
            } else if i <= 231 {
                let idx = i - 16;
                let r = (idx / 36) as f32 / 5.0;
                let g = ((idx / 6) % 6) as f32 / 5.0;
                let b = (idx % 6) as f32 / 5.0;
                (r, g, b)
            } else {
                let gray = (i - 232) as f32 / 23.0;
                (gray, gray, gray)
            }
        }
        Color::Rgb(r, g, b) => (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0),
    }
}

impl ApexApp {
    fn destroy_pty(&mut self) {
        self.input_tx.take();
        if let Some(handle) = self.pty_reader.take() {
            handle.abort();
        }
        self.output_rx.take();
        self.pty.take();
    }

    /// Reload the current theme from disk, trigger redraw
    fn reload_theme(&mut self) {
        // Theme path should be set during initialization
        if let Some(ref theme_path) = self.theme_file_path {
            if let Ok(metadata) = std::fs::metadata(theme_path) {
                if let Ok(_mtime) = metadata.modified() {
                    // Try to load theme from file
                    // Try to load the new theme
                    let new_theme = Theme::from_name(&self.theme.name);
                    if new_theme != self.theme {
                        self.theme = new_theme;
                        self.needs_redraw = true;
                        if let Some(ref window) = self.window {
                            window.request_redraw();
                        }
                        log::info!("Theme reloaded from {:?}", theme_path);
                    }
                }
            }
        }
    }
}

impl Drop for ApexApp {
    fn drop(&mut self) {
        self.destroy_pty();
        self.raster_tx = None;
        if let Some(handle) = self.raster_worker.take() {
            let _ = handle.join();
        }
    }
}

impl ApplicationHandler for ApexApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("Apex Terminal")
                .with_inner_size(winit::dpi::LogicalSize::new(960.0, 540.0))
        ) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("Failed to create window: {e}");
                return;
            }
        };

        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::PRIMARY,
            ..Default::default()
        });
        let surface = match instance.create_surface(window.clone()) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to create wgpu surface: {e}");
                return;
            }
        };
        let adapter = match pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }
        )) {
            Some(a) => a,
            None => {
                log::error!("No compatible GPU adapter found");
                return;
            }
        };
        let (device, queue) = match pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("Apex Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        )) {
            Ok(dq) => dq,
            Err(e) => {
                log::error!("Failed to create wgpu device: {e}");
                return;
            }
        };

        device.on_uncaptured_error(Box::new(|error| {
            log::error!("wgpu error: {:?}", error);
        }));

        let caps = surface.get_capabilities(&adapter);
        if caps.formats.is_empty() || caps.alpha_modes.is_empty() {
            log::error!("Surface has no compatible formats or alpha modes");
            return;
        }
        let config = SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: caps.formats[0],
            width: window.inner_size().width,
            height: window.inner_size().height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let initial_vert_size = 1024 * std::mem::size_of::<GlyphVertex>() as u64;
        let vertex_buf = device.create_buffer(&BufferDescriptor {
            label: Some("Terminal BG Verts"),
            size: initial_vert_size,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buf = device.create_buffer(&BufferDescriptor {
            label: Some("Terminal BG Idx"),
            size: 1024 * 6 * 2,
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let fg_vertex_buf = device.create_buffer(&BufferDescriptor {
            label: Some("Terminal FG Verts"),
            size: initial_vert_size,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let fg_index_buf = device.create_buffer(&BufferDescriptor {
            label: Some("Terminal FG Idx"),
            size: 1024 * 6 * 2,
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let dec_vb = device.create_buffer(&BufferDescriptor {
            label: Some("Terminal Decoration Verts"),
            size: initial_vert_size,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dec_ib = device.create_buffer(&BufferDescriptor {
            label: Some("Terminal Decoration Idx"),
            size: 1024 * 6 * 2,
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Initialize FontManager, then atlas and shaper from it
        let font_manager = match FontManager::new(CELL_SIZE) {
            Ok(fm) => fm,
            Err(e) => {
                log::error!("Failed to initialize font system: {e}");
                return;
            }
        };
        let primary_id = font_manager.primary_font_id();
        let primary_font = match font_manager.font(primary_id) {
            Some(f) => f,
            None => {
                log::error!("Primary font not found");
                return;
            }
        };

        let glyph_atlas = match GlyphAtlas::new_with_font(
            &device, &queue, primary_font.clone(), self.atlas_dump.as_deref(),
        ) {
            Ok(a) => a,
            Err(e) => {
                log::error!("Failed to create glyph atlas: {e}");
                return;
            }
        };

        self.primary_ascent = font_manager.primary_ascent();
        self.primary_underline_pos = font_manager.primary_underline_position();
        self.primary_underline_thickness = font_manager.primary_underline_thickness().max(1.0);
        self.primary_strikethrough_pos = font_manager.primary_strikethrough_position();
        self.primary_strikethrough_thickness = font_manager.primary_strikethrough_thickness().max(1.0);
        self.shaper = Some(Shaper::new(CELL_SIZE));
        self.font_manager = Some(font_manager);

        let (pipeline, fg_pipeline, bind_group, sampler, atlas_bgl) = Self::build_pipelines(&device, &config, &glyph_atlas.texture);

        // Build overlay pipeline for graphics (Sixel/Kitty) overlay
        let (overlay_pipeline, overlay_bgl, overlay_sampler) = Self::build_overlay_pipeline(&device, &config, &atlas_bgl);

        // Build compute pipeline for GPU glyph processing
        let (compute_pipeline, compute_bgl) = Self::build_compute_pipeline(&device);
        let initial_glyph_buf_size = 4096u64;
        let glyph_input_buf = device.create_buffer(&BufferDescriptor {
            label: Some("Glyph Input Storage"),
            size: initial_glyph_buf_size,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let glyph_output_buf = device.create_buffer(&BufferDescriptor {
            label: Some("Glyph Output Storage"),
            size: initial_glyph_buf_size,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let glyph_uniform_buf = device.create_buffer(&BufferDescriptor {
            label: Some("Glyph Uniform"),
            size: std::mem::size_of::<[f32; 2]>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Create a dummy bind group for now; will be rebuilt when actual glyph data is submitted
        let dummy_input = device.create_buffer(&BufferDescriptor {
            label: Some("Dummy Glyph Input"),
            size: 64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dummy_output = device.create_buffer(&BufferDescriptor {
            label: Some("Dummy Glyph Output"),
            size: 64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let compute_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("Glyph Compute Bind Group"),
            layout: &compute_bgl,
            entries: &[
                BindGroupEntry { binding: 0, resource: dummy_input.as_entire_binding() },
                BindGroupEntry { binding: 1, resource: dummy_output.as_entire_binding() },
                BindGroupEntry { binding: 2, resource: glyph_uniform_buf.as_entire_binding() },
            ],
        });
        self.compute_pipeline = Some(compute_pipeline);
        self.compute_bind_group = Some(compute_bind_group);
        self.glyph_input_buf = Some(glyph_input_buf);
        self.glyph_output_buf = Some(glyph_output_buf);
        self.glyph_uniform_buf = Some(glyph_uniform_buf);

        let win_w = config.width;
        let win_h = config.height;
        self.scale_factor = window.scale_factor();
        self.cell_w = glyph_atlas.cell_width;
        self.cell_h = glyph_atlas.cell_height;
        self.window = Some(window);
        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface_config = Some(config);
        self.pipeline = Some(pipeline);
        self.fg_pipeline = Some(fg_pipeline);
        self.bind_group = Some(bind_group);
        self.sampler = Some(sampler);
        self.overlay_pipeline = Some(overlay_pipeline);
        self.overlay_bgl = Some(overlay_bgl);
        self.overlay_sampler = Some(overlay_sampler);
        self.vertex_buf = Some(vertex_buf);
        self.index_buf = Some(index_buf);
        self.fg_vertex_buf = Some(fg_vertex_buf);
        self.fg_index_buf = Some(fg_index_buf);
        self.dec_vb = Some(dec_vb);
        self.dec_ib = Some(dec_ib);
        self.glyph_atlas = Some(glyph_atlas);
        self.needs_redraw = true;

        // Spawn raster worker
        let (raster_tx, raster_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let worker = spawn_raster_worker(raster_rx, result_tx);
        self.raster_tx = Some(raster_tx);
        self.raster_result_rx = Some(result_rx);
        self.raster_worker = Some(worker);

        let cw = self.cell_w;
        let ch = self.cell_h;
        log::info!("GPU ready: grid {}x{}, atlas cell {:.1}x{:.1}, win {}x{}, scale_factor={:.2}",
            self.processor.grid.cols, self.processor.grid.rows_visible,
            cw, ch, win_w, win_h, self.scale_factor);

        if win_w > 0 && win_h > 0 && cw > 0.0 && ch > 0.0 {
            let cols = (win_w as f32 / cw).max(1.0) as usize;
            let rows = (win_h as f32 / ch).max(1.0) as usize;
            self.processor.resize(rows, cols);
            log::info!("Grid resized to {}x{} for window {}x{}", cols, rows, win_w, win_h);
        }

        // Start PTY shell
        let rows = self.processor.grid.rows_visible.max(1);
        let cols = self.processor.grid.cols.max(1);
        let (pty_rows, pty_cols) = (rows.min(u16::MAX as usize) as u16, cols.min(u16::MAX as usize) as u16);
        match PtyInstance::new(pty_rows, pty_cols) {
            Ok(pty) => {
                // Set master_fd non-blocking for async reads
                let flags = unsafe { libc::fcntl(pty.master_fd, libc::F_GETFL, 0) };
                if flags >= 0 {
                    let ret = unsafe { libc::fcntl(pty.master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
                    if ret < 0 {
                        log::error!("Failed to set PTY master_fd to O_NONBLOCK: {}", std::io::Error::last_os_error());
                    }
                }

                let reader_fd = match unsafe { libc::dup(pty.master_fd) } {
                    fd if fd >= 0 => fd,
                    _ => {
                        log::error!("Failed to dup PTY master fd");
                        self.pty = Some(pty);
                        self.needs_redraw = true;
                        if let Some(ref window) = self.window {
                            window.request_redraw();
                        }
                        return;
                    }
                };
                let (input_tx, mut input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
                let (output_tx, output_rx) = mpsc::channel::<Vec<u8>>(4096);

                let pty_reader = tokio::spawn(async move {
                    use tokio::io::unix::AsyncFd;
                    let async_fd = match AsyncFd::new(reader_fd) {
                        Ok(fd) => fd,
                        Err(_) => {
                            unsafe { libc::close(reader_fd); }
                            return;
                        },
                    };
                    let mut buf = [0u8; 4096];
                    let result: Result<(), ()> = loop {
                        tokio::select! {
                            result = async_fd.readable() => {
                                let mut guard = match result {
                                    Ok(g) => g,
                                    Err(_) => break Err(()),
                                };
                                // Retry on EINTR, treat EAGAIN as 0 bytes
                                let read_result = loop {
                                    match guard.try_io(|fd| unsafe {
                                        let ret = libc::read(fd.as_raw_fd(), buf.as_mut_ptr() as *mut libc::c_void, buf.len());
                                        if ret >= 0 { Ok(ret as usize) } else { Err(std::io::Error::last_os_error()) }
                                    }) {
                                        Ok(Ok(n)) => break Some(n),
                                        Ok(Err(e)) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                                        Ok(Err(_)) => break None,
                                        Err(_) => break None,
                                    }
                                };
                                match read_result {
                                    Some(0) | None => break Ok(()),
                                    Some(n) => {
                                        let _ = output_tx.send(buf[..n].to_vec()).await;
                                    }
                                }
                            }
                            Some(data) = input_rx.recv() => {
                                let len = data.len();
                                let mut written = 0usize;
                                while written < len {
                                    let ret = unsafe {
                                        let ptr = data.as_ptr().add(written) as *const libc::c_void;
                                        libc::write(async_fd.as_raw_fd(), ptr, len - written)
                                    };
                                    if ret < 0 {
                                        let err = std::io::Error::last_os_error();
                                        if err.kind() == std::io::ErrorKind::Interrupted {
                                            continue;
                                        }
                                        log::warn!("PTY write error: {}", err);
                                        break;
                                    }
                                    written = len.min(written.saturating_add(ret as usize));
                                }
                            }
                            else => break Ok(()),
                        }
                    };
                    unsafe { libc::close(reader_fd); }
                    let _ = result;
                });

                self.pty = Some(pty);
                self.input_tx = Some(input_tx);
                self.output_rx = Some(output_rx);
                self.pty_reader = Some(pty_reader);

                log::info!("PTY started: {}x{}", cols, rows);

                const BANNER: &str = "\
\x1b[38;2;0;255;170m\
   #####   ######  ####### ##   ##\n\
   ##  ##  ##   ## ##      ## ##\n\
   #####   ######  #####    ###\n\
   ##  ##  ##      ##      ## ##\n\
   ##  ##  ##      ####### ##   ##\n\
\x1b[0m\n\
\x1b[38;2;0;255;170m       GPU-Accelerated Offensive Terminal\x1b[0m  \x1b[38;2;74;158;255mv0.1.0\x1b[0m\n\
\n\
\x1b[38;2;0;255;170m   C2\x1b[0m  \x1b[38;2;74;158;255m- Sliver | Havoc | Mythic | Empire\x1b[0m\n\
\x1b[38;2;0;255;170m   AI\x1b[0m  \x1b[38;2;74;158;255m- Ollama | Llama.cpp\x1b[0m\n\
\x1b[38;2;0;255;170m   MUX\x1b[0m \x1b[38;2;74;158;255m- Apex Native Multiplexer\x1b[0m\n\
\n";
                self.processor.advance(BANNER.as_bytes());
                self.needs_redraw = true;
            }
            Err(e) => {
                log::error!("Failed to start PTY: {e}");
                let welcome = "\r\n\x1b[1;32mWelcome to Apex Terminal v0.1.0\x1b[0m\r\n\x1b[33mNext-Gen Kali Linux Offensive Environment\x1b[0m\r\n\r\n\x1b[1;34mshell\x1b[0m@\x1b[1;32mapex\x1b[0m:~$ ";
                self.processor.advance(welcome.as_bytes());
                self.needs_redraw = true;
            }
        }

        // Force initial redraw so the first frame is rendered
        self.needs_redraw = true;
        if let Some(ref window) = self.window {
            window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.destroy_pty();
                event_loop.exit();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor;
                if let Some(ref atlas) = self.glyph_atlas {
                    self.cell_w = atlas.cell_width;
                    self.cell_h = atlas.cell_height;
                }
                if let Some(ref config) = self.surface_config {
                    let w = config.width.max(1);
                    let h = config.height.max(1);
                    if self.cell_w > 0.0 && self.cell_h > 0.0 {
                        let cols = (w as f32 / self.cell_w).max(1.0) as usize;
                        let rows = (h as f32 / self.cell_h).max(1.0) as usize;
                        self.processor.resize(rows, cols);
                        if let Some(ref mut pty) = self.pty {
                            let _ = pty.resize(rows.min(u16::MAX as usize) as u16, cols.min(u16::MAX as usize) as u16);
                        }
                    }
                }
                self.needs_redraw = true;
            }
            WindowEvent::Resized(size) => {
                let w = size.width.max(1);
                let h = size.height.max(1);
                if let (Some(ref mut config), Some(ref device), Some(ref surface)) =
                    (&mut self.surface_config, &self.device, &self.surface)
                {
                    config.width = w;
                    config.height = h;
                    surface.configure(device, config);
                }
                if self.cell_w > 0.0 && self.cell_h > 0.0 {
                    let cols = (w as f32 / self.cell_w).max(1.0) as usize;
                    let rows = (h as f32 / self.cell_h).max(1.0) as usize;
                    self.processor.resize(rows, cols);
                    if let Some(ref mut pty) = self.pty {
                        let _ = pty.resize(rows.min(u16::MAX as usize) as u16, cols.min(u16::MAX as usize) as u16);
                    }
                }
                self.needs_redraw = true;
            }
            WindowEvent::RedrawRequested => {
                self.render();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.last_mouse_x = position.x;
                self.last_mouse_y = position.y;
                if self.selection.active {
                    if let Some((row, col)) = self.screen_to_grid(position.x, position.y) {
                        self.selection.end_row = row;
                        self.selection.end_col = col;
                        self.needs_redraw = true;
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } if button == winit::event::MouseButton::Left => {
                // Need cursor position — winit provides it via CursorMoved events
                // We store the mouse_down position via CursorMoved tracking
                match state {
                    winit::event::ElementState::Pressed => {
                        self.selection.active = true;
                        if let Some((row, col)) = self.last_mouse_grid_pos() {
                            self.selection.start_row = row;
                            self.selection.start_col = col;
                            self.selection.end_row = row;
                            self.selection.end_col = col;
                            self.mouse_down_row = row;
                            self.mouse_down_col = col;
                        }
                        self.needs_redraw = true;
                    }
                    winit::event::ElementState::Released => {
                        // Selection complete — keep end position
                        self.needs_redraw = true;
                    }
                }
            }
            WindowEvent::KeyboardInput {
                event: ke,
                ..
            } if ke.state == winit::event::ElementState::Pressed => {
                let text: Option<String> = match ke.logical_key {
                    Key::Named(NamedKey::Enter) => Some("\r".to_string()),
                    Key::Named(NamedKey::Backspace) => Some("\x7f".to_string()),
                    Key::Named(NamedKey::Tab) => Some("\t".to_string()),
                    Key::Named(NamedKey::Escape) => Some("\x1b".to_string()),
                    Key::Named(NamedKey::ArrowUp) => Some("\x1b[A".to_string()),
                    Key::Named(NamedKey::ArrowDown) => Some("\x1b[B".to_string()),
                    Key::Named(NamedKey::ArrowRight) => Some("\x1b[C".to_string()),
                    Key::Named(NamedKey::ArrowLeft) => Some("\x1b[D".to_string()),
                    _ => ke.text.as_ref().map(|s| s.to_string()),
                };
                if let Some(text) = text {
                    if let Some(ref tx) = self.input_tx {
                        let _ = tx.send(text.into_bytes());
                    } else {
                        self.processor.advance(text.as_bytes());
                    }
                    self.needs_redraw = true;
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // Check for theme reload signals
        if let Some(rx) = self.theme_reload_rx.take() {
            while rx.try_recv().is_ok() {
                self.reload_theme();
            }
            self.theme_reload_rx = Some(rx);
        }

        if let Some(ref mut rx) = self.output_rx {
            while let Ok(data) = rx.try_recv() {
                self.processor.advance(&data);
                self.needs_redraw = true;
            }
        }

        // Process completed async rasterizations
        if let Some(ref mut rx) = self.raster_result_rx {
            while let Ok(result) = rx.try_recv() {
                let key = result.key.clone();
                if let Some(ref mut atlas) = self.glyph_atlas {
                    if let Some(upload) = atlas.stage_rasterized(result) {
                        self.pending_uploads.push(upload);
                        self.needs_redraw = true;
                        self.processor.grid.damage.mark_all();
                    }
                    self.pending_raster.remove(&key);
                }
            }
        }

        // Cursor blink — independent from text invalidation
        let is_blinking = match self.processor.cursor.style {
            vte_core::state::CursorStyle::BlinkingBlock
            | vte_core::state::CursorStyle::BlinkingUnderline
            | vte_core::state::CursorStyle::BlinkingBeam => true,
            _ => self.processor.mode.contains(vte_core::state::TerminalMode::CURSOR_BLINK),
        };
        if is_blinking {
            let dt = self.cursor_blink_accum + std::time::Duration::from_millis(16);
            self.cursor_blink_accum = if dt >= std::time::Duration::from_millis(500) {
                self.cursor_blink_visible = !self.cursor_blink_visible;
                std::time::Duration::ZERO
            } else {
                dt
            };
            // Request redraw for cursor overlay only (no text invalidation)
            if let Some(ref window) = self.window {
                window.request_redraw();
            }
        } else {
            self.cursor_blink_visible = true;
        }

        if self.needs_redraw {
            if let Some(ref window) = self.window {
                window.request_redraw();
            }
        }
    }
}

pub struct WgpuRenderer {
    scrollback_lines: u32,
    atlas_dump: Option<PathBuf>,
    theme: Theme,
}

impl WgpuRenderer {
    pub async fn new(config: ApexConfig, atlas_dump: Option<PathBuf>) -> Result<Self> {
        let theme = Theme::from_name(&config.theme);
        Ok(WgpuRenderer { scrollback_lines: config.scrollback_lines, atlas_dump, theme })
    }

    pub async fn run(&mut self) -> Result<()> {
        let event_loop = EventLoop::new()?;
        let atlas_dump = self.atlas_dump.take();
        let theme = self.theme.clone();
        let mut app = ApexApp::new(self.scrollback_lines, theme, atlas_dump);

        // Set up theme hot‑reload if we can locate a theme file
        if let Some(theme_path) = locate_theme_path(&app.theme.name) {
            // Channel for reload notifications
            let (reload_tx, reload_rx) = std::sync::mpsc::channel();
            app.theme_reload_rx = Some(reload_rx);
            app.theme_file_path = Some(theme_path.clone());

            // Spawn a watcher thread that forwards file change events to the reload channel
            std::thread::spawn(move || {
                // Channel for notify crate events
                let (watch_tx, watch_rx) = std::sync::mpsc::channel();
                // Create watcher that forwards events onto watch_tx
                let mut watcher = match notify::recommended_watcher(move |res| {
                    let _ = watch_tx.send(res);
                }) {
                    Ok(w) => w,
                    Err(e) => {
                        log::error!("Failed to create theme watcher: {}", e);
                        return;
                    }
                };
                if let Err(e) = watcher.watch(&theme_path, notify::RecursiveMode::NonRecursive) {
                    log::error!("Failed to watch theme file {}: {}", theme_path.display(), e);
                    return;
                }
                // Loop over events; each triggers a reload signal (ignore details)
                for _ in watch_rx {
                    let _ = reload_tx.send(());
                }
            });
        }

        event_loop.run_app(&mut app)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_theme_path_builtin() {
        assert!(locate_theme_path("kali-dark").is_none());
        assert!(locate_theme_path("default").is_none());
        assert!(locate_theme_path("backtrack").is_none());
    }

    #[test]
    fn locate_theme_path_custom() {
        // Non-built-in names return None when file doesn't exist
        assert!(locate_theme_path("nonexistent_theme").is_none());
    }
}
