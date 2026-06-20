// Compute shader for GPU-accelerated glyph effects pipeline.
// Demonstrates compute infrastructure by processing glyph position data.

struct GlyphData {
    position: vec2<f32>,
    size: vec2<f32>,
    color: vec4<f32>,
}

@group(1) @binding(0) var<storage, read> input_glyphs: array<GlyphData>;
@group(1) @binding(1) var<storage, read_write> output_glyphs: array<GlyphData>;
@group(1) @binding(2) var<uniform> offset: vec2<f32>;

@compute @workgroup_size(64, 1, 1)
fn cs_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let idx = id.x;
    if idx >= arrayLength(&input_glyphs) {
        return;
    }
    let g = input_glyphs[idx];
    output_glyphs[idx] = GlyphData(
        g.position + offset,
        g.size,
        g.color,
    );
}
