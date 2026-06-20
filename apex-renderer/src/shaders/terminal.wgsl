struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) fg_color: vec4<f32>,
    @location(3) bg_color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fg_color: vec4<f32>,
    @location(2) bg_color: vec4<f32>,
}

@group(0) @binding(0) var glyph_atlas: texture_2d<f32>;
@group(0) @binding(1) var glyph_sampler: sampler;

@group(1) @binding(0) var overlay_tex: texture_2d<f32>;
@group(1) @binding(1) var overlay_sampler: sampler;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.uv = input.uv;
    output.fg_color = input.fg_color;
    output.bg_color = input.bg_color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let glyph_alpha = textureSample(glyph_atlas, glyph_sampler, input.uv).r;
    let bg = input.bg_color;
    let fg = input.fg_color;
    let blended = mix(bg, fg, glyph_alpha);
    return vec4<f32>(blended.rgb, 1.0);
}

@fragment
fn fs_overlay(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(overlay_tex, overlay_sampler, input.uv);
    return color;
}
