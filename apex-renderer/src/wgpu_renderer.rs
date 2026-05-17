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
use std::sync::Arc;
use std::time::Instant;
use std::os::fd::AsRawFd;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::atlas::{GlyphAtlas, GlyphVertex, ATLAS_SIZE};
use apex_pty::PtyInstance;
use vte_core::parser::VteProcessor;

const MIN_FRAME_TIME: std::time::Duration = std::time::Duration::from_millis(16); // ~60fps

struct ApexApp {
    window: Option<Arc<Window>>,
    surface: Option<Surface<'static>>,
    device: Option<Device>,
    queue: Option<Queue>,
    config: Option<SurfaceConfiguration>,
    pipeline: Option<RenderPipeline>,
    bind_group: Option<wgpu::BindGroup>,
    sampler: Option<wgpu::Sampler>,
    vertex_buf: Option<wgpu::Buffer>,
    index_buf: Option<wgpu::Buffer>,
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
}

impl ApexApp {
    fn new() -> Self {
        ApexApp {
            window: None,
            surface: None,
            device: None,
            queue: None,
            config: None,
            pipeline: None,
            bind_group: None,
            sampler: None,
            vertex_buf: None,
            index_buf: None,
            glyph_atlas: None,
            processor: VteProcessor::new(24, 80, 10000),
            last_render: Instant::now(),
            needs_redraw: true,
            cell_w: 0.0,
            cell_h: 0.0,
            scale_factor: 1.0,
            pty: None,
            input_tx: None,
            output_rx: None,
            pty_reader: None,
        }
    }

    fn build_pipeline(device: &Device, config: &SurfaceConfiguration, texture: &wgpu::Texture) -> (RenderPipeline, wgpu::BindGroup, wgpu::Sampler) {
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

        let vertex_buf_layout = VertexBufferLayout {
            array_stride: std::mem::size_of::<GlyphVertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2,  // position
                1 => Float32x2,  // uv
                2 => Float32x4,  // fg_color
                3 => Float32x4,  // bg_color
            ],
        };

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Terminal Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[vertex_buf_layout],
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

        (pipeline, bind_group, sampler)
    }

    fn build_vertex_data(&self) -> (Vec<GlyphVertex>, Vec<u16>) {
        let rows = self.processor.grid.rows_visible;
        let cols = self.processor.grid.cols;
        let cw = self.cell_w.max(8.0);
        let ch = self.cell_h.max(20.0);
        let atlas = self.glyph_atlas.as_ref();

        let (win_w, win_h) = self.config
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

        let capacity = rows.saturating_mul(cols).saturating_mul(6);
        let mut vertices = Vec::with_capacity(capacity);
        let mut indices = Vec::with_capacity(capacity);

        for row in 0..rows {
            for col in 0..cols {
                if row >= self.processor.grid.rows.len() || col >= self.processor.grid.cols {
                    continue;
                }
                let cell = &self.processor.grid.rows[row].cells[col];
                let (fg_r, fg_g, fg_b) = color_to_f32(cell.fg_color, cell.reverse);
                let (bg_r, bg_g, bg_b) = color_to_f32(cell.bg_color, !cell.reverse);
                let dim = if cell.dim { 0.5 } else { 1.0 };
                let fg_color = [fg_r * dim, fg_g * dim, fg_b * dim, 1.0];
                let bg_color = [bg_r, bg_g, bg_b, 1.0];

                let x = col as f32 * cw;
                let y = row as f32 * ch;
                let (w, h) = (cw, ch);
                let tl = px_to_ndc(x, y);
                let tr = px_to_ndc(x + w, y);
                let bl = px_to_ndc(x, y + h);
                let br = px_to_ndc(x + w, y + h);

                // Background quad (no glyph texture, use a zero UV)
                vertices.extend_from_slice(&[
                    GlyphVertex { position: tl, uv: [0.0, 0.0], fg_color, bg_color },
                    GlyphVertex { position: tr, uv: [0.0, 0.0], fg_color, bg_color },
                    GlyphVertex { position: bl, uv: [0.0, 0.0], fg_color, bg_color },
                    GlyphVertex { position: br, uv: [0.0, 0.0], fg_color, bg_color },
                    GlyphVertex { position: tr, uv: [0.0, 0.0], fg_color, bg_color },
                    GlyphVertex { position: bl, uv: [0.0, 0.0], fg_color, bg_color },
                ]);

                // Foreground quad with glyph texture
                if cell.character != ' ' {
                    let glyph_uv = atlas.and_then(|a| a.get_glyph(cell.character, 14)).map(|g| {
                        let u0 = g.x as f32 / ATLAS_SIZE as f32;
                        let v0 = g.y as f32 / ATLAS_SIZE as f32;
                        let u1 = (g.x + g.width) as f32 / ATLAS_SIZE as f32;
                        let v1 = (g.y + g.height) as f32 / ATLAS_SIZE as f32;
                        (u0, v0, u1, v1)
                    }).unwrap_or((0.0, 0.0, 0.0, 0.0));

                    let (u0, v0, u1, v1) = glyph_uv;
                    vertices.extend_from_slice(&[
                        GlyphVertex { position: tl, uv: [u0, v0], fg_color, bg_color },
                        GlyphVertex { position: tr, uv: [u1, v0], fg_color, bg_color },
                        GlyphVertex { position: bl, uv: [u0, v1], fg_color, bg_color },
                        GlyphVertex { position: br, uv: [u1, v1], fg_color, bg_color },
                        GlyphVertex { position: tr, uv: [u1, v0], fg_color, bg_color },
                        GlyphVertex { position: bl, uv: [u0, v1], fg_color, bg_color },
                    ]);
                }
            }
        }

        let count = vertices.len().min(u16::MAX as usize);
        for i in 0..count as u16 {
            indices.push(i);
        }

        (vertices, indices)
    }

    fn render(&mut self) {
        if !self.needs_redraw {
            return;
        }

        let now = Instant::now();
        if now.duration_since(self.last_render) < MIN_FRAME_TIME {
            self.needs_redraw = true; // wait for next cycle
            return;
        }
        self.last_render = now;

        let (surface, device, queue, config, pipeline, bind_group) = match (
            &self.surface, &self.device, &self.queue, &self.config,
            &self.pipeline, &self.bind_group,
        ) {
            (Some(s), Some(d), Some(q), Some(c), Some(p), Some(bg)) => (s, d, q, c, p, bg),
            _ => { self.needs_redraw = true; return; }
        };

        let (vertices, indices) = self.build_vertex_data();
        if vertices.is_empty() {
            log::warn!("No vertices to render");
            self.needs_redraw = false;
            return;
        }

        // Upload vertex data
        let vert_bytes = bytemuck::cast_slice(&vertices);
        let idx_bytes = bytemuck::cast_slice(&indices);

        if self.vertex_buf.as_ref().map_or(true, |vb| vb.size() < vert_bytes.len() as u64) {
            self.vertex_buf = Some(device.create_buffer(&BufferDescriptor {
                label: Some("Terminal Verts"),
                size: vert_bytes.len().max(1024) as u64,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(vb) = self.vertex_buf.as_ref() else {
            self.needs_redraw = true;
            return;
        };
        queue.write_buffer(vb, 0, vert_bytes);

        if self.index_buf.as_ref().map_or(true, |ib| ib.size() < idx_bytes.len() as u64) {
            self.index_buf = Some(device.create_buffer(&BufferDescriptor {
                label: Some("Terminal Idx"),
                size: idx_bytes.len().max(1024) as u64,
                usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(ib) = self.index_buf.as_ref() else {
            self.needs_redraw = true;
            return;
        };
        queue.write_buffer(ib, 0, idx_bytes);

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
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, bind_group, &[]);
            rp.set_vertex_buffer(0, vb.slice(..));
            rp.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint16);
            rp.draw_indexed(0..indices.len() as u32, 0, 0..1);
        }

        queue.submit(Some(encoder.finish()));
        frame.present();
        self.needs_redraw = false;
    }
}

fn color_to_f32(color: vte_core::grid::Color, is_bg: bool) -> (f32, f32, f32) {
    use vte_core::grid::Color;
    match color {
        Color::Default => {
            if is_bg { (0.05, 0.05, 0.07) } else { (0.83, 0.83, 0.83) }
        }
        Color::Black => (0.0, 0.0, 0.0),
        Color::Red => (0.80, 0.16, 0.16),
        Color::Green => (0.0, 0.59, 0.0),
        Color::Yellow => (0.80, 0.59, 0.0),
        Color::Blue => (0.16, 0.32, 0.75),
        Color::Magenta => (0.64, 0.16, 0.64),
        Color::Cyan => (0.0, 0.59, 0.59),
        Color::White => (0.83, 0.83, 0.83),
        Color::BrightBlack => (0.33, 0.33, 0.33),
        Color::BrightRed => (1.0, 0.33, 0.33),
        Color::BrightGreen => (0.33, 1.0, 0.33),
        Color::BrightYellow => (1.0, 1.0, 0.33),
        Color::BrightBlue => (0.33, 0.33, 1.0),
        Color::BrightMagenta => (1.0, 0.33, 1.0),
        Color::BrightCyan => (0.33, 1.0, 1.0),
        Color::BrightWhite => (1.0, 1.0, 1.0),
        Color::Indexed(i) => {
            if i < 16 {
                let standard = match i {
                    0 => Color::Black, 1 => Color::Red, 2 => Color::Green, 3 => Color::Yellow,
                    4 => Color::Blue, 5 => Color::Magenta, 6 => Color::Cyan, 7 => Color::White,
                    _ => Color::BrightBlack,
                };
                color_to_f32(standard, is_bg)
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
}

impl Drop for ApexApp {
    fn drop(&mut self) {
        self.destroy_pty();
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
            label: Some("Terminal Verts"),
            size: initial_vert_size,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buf = device.create_buffer(&BufferDescriptor {
            label: Some("Terminal Idx"),
            size: 1024 * 6 * 2,
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let glyph_atlas = match GlyphAtlas::new(&device, &queue) {
            Ok(a) => a,
            Err(e) => {
                log::error!("Failed to create glyph atlas: {e}");
                return;
            }
        };

        let (pipeline, bind_group, sampler) = Self::build_pipeline(&device, &config, &glyph_atlas.texture);

        let win_w = config.width;
        let win_h = config.height;
        self.scale_factor = window.scale_factor();
        self.cell_w = glyph_atlas.cell_width * self.scale_factor as f32;
        self.cell_h = glyph_atlas.cell_height * self.scale_factor as f32;
        self.window = Some(window);
        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.config = Some(config);
        self.pipeline = Some(pipeline);
        self.bind_group = Some(bind_group);
        self.sampler = Some(sampler);
        self.vertex_buf = Some(vertex_buf);
        self.index_buf = Some(index_buf);
        self.glyph_atlas = Some(glyph_atlas);
        self.needs_redraw = true;

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
                unsafe {
                    let flags = libc::fcntl(pty.master_fd, libc::F_GETFL, 0);
                    if flags >= 0 {
                        libc::fcntl(pty.master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
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
                                if let Some(n) = read_result {
                                    if n > 0 {
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
                    self.cell_w = atlas.cell_width * scale_factor as f32;
                    self.cell_h = atlas.cell_height * scale_factor as f32;
                }
                if let Some(ref config) = self.config {
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
                    (&mut self.config, &self.device, &self.surface)
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
        if let Some(ref mut rx) = self.output_rx {
            while let Ok(data) = rx.try_recv() {
                self.processor.advance(&data);
                self.needs_redraw = true;
            }
        }
        if self.needs_redraw {
            if let Some(ref window) = self.window {
                window.request_redraw();
            }
        }
    }
}

pub struct WgpuRenderer;

impl WgpuRenderer {
    pub async fn new() -> Result<Self> {
        Ok(WgpuRenderer)
    }

    pub async fn run(&mut self) -> Result<()> {
        let event_loop = EventLoop::new()?;
        let mut app = ApexApp::new();
        event_loop.run_app(&mut app)?;
        Ok(())
    }
}
