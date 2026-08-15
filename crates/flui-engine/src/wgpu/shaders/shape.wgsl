// Vertex shader for tessellated shape rendering
//
// Handles paths, polygons, strokes, and any tessellated geometry.
// Converts pixel coordinates to clip space using viewport uniform.

// Viewport uniform (for screen-space to clip-space conversion)
struct Viewport {
    size: vec2<f32>,      // Viewport size in pixels
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

// Per-batch SDF clip. Tessellated geometry has no instances to hang a clip
// slot on, so the clip is bound once per `TessellatedBatch` — the granularity
// the batcher already splits at, since it refuses to merge across a clip change.
struct ClipUniform {
    bounds: vec4<f32>,  // Clip-local [x, y, w, h]; device_to_local maps into it
    radii: vec4<f32>,   // [tl, tr, br, bl]
    kind: vec4<u32>,    // [kind, _, _, _]: 0 = none, 1 = rrect, 2 = rsuperellipse
    device_to_local: vec4<f32>, // [a, b, c, d], columns first
    local_origin: vec4<f32>,    // [tx, ty, 0, 0]
}

@group(1) @binding(0)
var<uniform> clip: ClipUniform;

struct VertexInput {
    @location(0) position: vec2<f32>,  // Position in pixel coordinates
    @location(1) color: vec4<f32>,     // RGBA color [0-1]
    @location(2) uv: vec2<f32>,        // UV coordinates (unused for now)
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Device-space position, carried for the clip SDF, which maps it
    // into clip-local space itself. `clip_position` is in
    // NDC by the time the fragment stage sees it, so it cannot serve here.
    @location(1) world_pos: vec2<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;

    // Convert pixel coordinates to clip space [-1, 1] using viewport uniform
    let clip_x = (input.position.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (input.position.y / viewport.size.y) * 2.0; // Flip Y for screen coords

    output.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    output.color = input.color;
    output.world_pos = input.position;

    return output;
}

// Fragment shader
//
// Emits PREMULTIPLIED alpha: rgb is scaled by the final alpha. This is the
// correct input form for fixed-function Porter-Duff blending, which the
// tessellated pipeline selects per `PipelineKey::blend_mode`. The default
// SrcOver case pairs this with `BlendState::PREMULTIPLIED_ALPHA_BLENDING`
// (src factor One), making it visually identical to the previous straight
// `input.color` + `ALPHA_BLENDING` output. The batch clip is folded into alpha
// BEFORE premultiplying, so `a` below is the full final alpha.

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let c = input.color;
    // Clip coverage — see `clipAlpha` in `common/clip.wgsl`.
    let a = c.a
        * clipAlpha(
            input.world_pos,
            clip.bounds,
            clip.radii,
            clip.kind.x,
            clip.device_to_local,
            clip.local_origin,
        );
    return vec4<f32>(c.rgb * a, a);
}
