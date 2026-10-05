// Textured affine quads. Consecutive compatible images may share a draw;
// texture, blend and scissor changes preserve recording order.

// Vertex input (shared unit quad: [0,0] to [1,1])
struct VertexInput {
    @location(0) position: vec2<f32>,  // Quad corner [0 to 1]
}

// Instance input (per-image data)
struct InstanceInput {
    @location(2) origin_and_x: vec4<f32>, // [ox, oy, xx, xy] device quad
    @location(3) src_uv: vec4<f32>,        // [u_min, v_min, u_max, v_max] in 0-1 range
    @location(4) tint: vec4<f32>,          // [r, g, b, a] in 0-1 range
    @location(5) axis_y: vec4<f32>,       // [yx, yy, 0, 0] device quad
    @location(6) clip_bounds: vec4<f32>,   // [x, y, width, height] of clip
    @location(7) clip_radii: vec4<f32>,    // [tl, tr, br, bl] of clip
    @location(8) clip_kind: vec4<u32>,           // [kind, source mode, hard clip, _]
    @location(9) clip_device_to_local: vec4<f32>,  // [a, b, c, d], columns first
    @location(10) clip_local_origin: vec4<f32>,    // [tx, ty, 0, 0]
    @location(11) original_image_uv: vec4<f32>, // full image in atlas, not crop
}

// Vertex output / Fragment input
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,            // Texture coordinates
    @location(1) tint: vec4<f32>,          // Tint color
    @location(2) world_pos: vec2<f32>,     // Device-pixel position for the clip SDF
    @location(3) clip_bounds: vec4<f32>,   // Clip rect bounds
    @location(4) clip_radii: vec4<f32>,    // Clip corner radii
    @location(5) @interpolate(flat) clip_kind: u32, // 0=none, 1=rrect, 2=rsuperellipse
    @location(6) clip_device_to_local: vec4<f32>,
    @location(7) clip_local_origin: vec4<f32>,
    @location(8) @interpolate(flat) source_mode: u32,
    @location(9) @interpolate(flat) original_image_uv: vec4<f32>,
}

// =============================================================================
// SDF helpers
// =============================================================================
//
// Inlined rather than imported: WGSL here has no include mechanism, and
// `rect_instanced.wgsl` sets the convention of carrying its own copy. Both
// must stay identical — a clip that rounds differently per primitive is worse
// than no clip at all, because the seam only shows where two primitives meet.

// Whether the sampled texel is PREMULTIPLIED.
//
// It changes what a clip's coverage has to scale. A straight-alpha texel keeps
// its colour and scales only alpha; a premultiplied one carries its alpha in
// every channel, so all four must scale together. Scaling only alpha on a
// premultiplied source leaves `rgb > a`, and the premultiplied blend (src
// factor `One`) then contributes the fringe's colour at FULL strength over a
// destination that was only partly uncovered — a bright halo along the clip
// edge, brightest exactly where the feather is doing its work.
//
// `bool`, not a scale factor: a texel is premultiplied or it is not, and a
// numeric override would make "half premultiplied" representable and silently
// meaningless. The host passes 1.0/0.0 and naga maps it (`Scalar::BOOL` in
// `map_value_to_literal`).
//
// A pipeline-overridable constant rather than an instance lane because it is a
// property of the PIPELINE (`instanced_texture` vs `instanced_texture_premul`
// and the SSAA tile composites), not of any one quad. `TextureSourceAlpha` in
// `pipelines.rs` chooses it and the blend state together; the default here is
// the straight-alpha pipeline, which passes no constants.
override premultiplied_source: bool = false;

// Whether the pipeline's blend replaces the destination under a transparent
// source (`Clear`, `Src`, `SrcIn`, `DstIn`, `SrcOut`, `DstATop`, `Modulate`).
//
// A layer composited with such a mode changes every pixel of its region, the
// ones its content left transparent included, so a transparent texel is a
// write, not a skip: discarding it would keep the destination exactly where
// the mode asks to replace it. Only a fragment the clip excludes outright is
// discarded then. Every other pipeline discards transparent texels, which
// leaves the destination as the blend would.
override replaces_destination: bool = false;

// Viewport uniform (for screen-space to clip-space conversion)
struct Viewport {
    size: vec2<f32>,      // Viewport size in pixels
    root_origin: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

@group(1) @binding(0)
var texture_sampler: sampler;

@group(1) @binding(1)
var texture_view: texture_2d<f32>;

@vertex
fn vs_main(
    vertex: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;

    let world_pos = instance.origin_and_x.xy
        + vertex.position.x * instance.origin_and_x.zw
        + vertex.position.y * instance.axis_y.xy;

    // Convert to clip space [-1, 1]
    let clip_x = ((world_pos.x - viewport.root_origin.x) / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - ((world_pos.y - viewport.root_origin.y) / viewport.size.y) * 2.0; // Flip Y for screen coords

    out.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);

    // Calculate UV coordinates from source UV and vertex position
    let u_min = instance.src_uv.x;
    let v_min = instance.src_uv.y;
    let u_max = instance.src_uv.z;
    let v_max = instance.src_uv.w;

    out.uv = vec2<f32>(
        mix(u_min, u_max, vertex.position.x),
        mix(v_min, v_max, vertex.position.y)
    );

    out.tint = instance.tint;

    // The clip is evaluated in device pixels, so hand the fragment the same
    // world position the vertex was placed at, before viewport projection.
    out.world_pos = world_pos;
    out.clip_bounds = instance.clip_bounds;
    out.clip_radii = instance.clip_radii;
    out.clip_device_to_local = instance.clip_device_to_local;
    out.clip_local_origin = instance.clip_local_origin;
    // Bit 2 carries the clip layer's Clip mode; `clipAlpha` unpacks it.
    out.clip_kind = instance.clip_kind.x | (instance.clip_kind.z << 2u);
    out.source_mode = instance.clip_kind.y;
    out.original_image_uv = instance.original_image_uv;

    return out;
}

// Straight-alpha coverage must be premultiplied BEFORE filtering; multiplying
// the hardware-filtered RGB by its filtered alpha compounds edge attenuation.
fn load_premultiplied_straight(coord: vec2<i32>, size: vec2<i32>, bounds: vec4<f32>) -> vec4<f32> {
    let first = vec2<i32>(round(bounds.xy * vec2<f32>(size)));
    let last = vec2<i32>(round(bounds.zw * vec2<f32>(size))) - vec2<i32>(1);
    let clamped = clamp(coord, first, last);
    let texel = textureLoad(texture_view, clamped, 0);
    return vec4<f32>(texel.rgb * texel.a, texel.a);
}

fn filter_straight_coverage(uv: vec2<f32>, bounds: vec4<f32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(texture_view, 0));
    let position = uv * vec2<f32>(size) - vec2<f32>(0.5);
    let base = vec2<i32>(floor(position));
    let weight = fract(position);
    let tl = load_premultiplied_straight(base, size, bounds);
    let tr = load_premultiplied_straight(base + vec2<i32>(1, 0), size, bounds);
    let bl = load_premultiplied_straight(base + vec2<i32>(0, 1), size, bounds);
    let br = load_premultiplied_straight(base + vec2<i32>(1, 1), size, bounds);
    return mix(mix(tl, tr, weight.x), mix(bl, br, weight.x), weight.y);
}

fn sampledImage(in: VertexOutput) -> vec4<f32> {
    // Derivatives are computed in uniform control flow; source mode is per
    // instance and does not justify implicit derivatives inside either branch.
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);
    var tex_color: vec4<f32>;
    if (in.source_mode == 2u) {
        tex_color = filter_straight_coverage(in.uv, in.original_image_uv);
    } else {
        // Preserve hardware sampling and LOD for all existing image paths.
        tex_color = textureSampleGrad(texture_view, texture_sampler, in.uv, uv_dx, uv_dy);
        if (in.source_mode == 1u) {
            tex_color.a = 1.0;
        }
    }

    // Apply tint (multiply)
    tex_color = tex_color * in.tint;

    return tex_color;
}

fn folded_image(in: VertexOutput) -> vec4<f32> {
    var tex_color = sampledImage(in);

    // Clip coverage — see `clipAlpha` in `common/clip.wgsl`.
    let clip_alpha = clipAlpha(
        in.position.xy,
        in.world_pos,
        in.clip_bounds,
        in.clip_radii,
        in.clip_kind,
        in.clip_device_to_local,
        in.clip_local_origin,
    );

    // Premultiplied sources scale every channel by the coverage; straight-alpha
    // ones scale alpha alone. See `premultiplied_source`.
    let rgb_scale = select(1.0, clip_alpha, premultiplied_source);
    tex_color = vec4<f32>(tex_color.rgb * rgb_scale, tex_color.a * clip_alpha);

    // Preserve low-alpha fades. Destination-replacing modes also write zero-
    // alpha sources, but must leave fully clipped-out pixels untouched.
    if (replaces_destination && clip_alpha <= 0.0) {
        discard;
    }

    return tex_color;
}
