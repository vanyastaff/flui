// Instanced glyph shader.
//
// One quad per rasterised glyph, sampled from the glyph atlas: a coverage
// page (R8, tinted by the instance colour) and a colour page (RGBA8, emoji
// and colour bitmap faces, drawn as-is under the instance alpha). The atlas
// texel origin arrives in texels and is normalised here against the bound
// page's dimensions, so an atlas that grew after the instance was recorded
// still samples the right texels (allocations keep their place on a grow).
//
// Sampling is bilinear. Under a uniform CTM a glyph is drawn 1:1 on integer
// device pixels, where bilinear and nearest agree texel for texel; under a
// rotated or anisotropic CTM the quad is resampled and bilinear is what
// keeps it from shimmering. The per-instance SDF clip is the
// same `clipAlpha` every other instanced primitive evaluates, prepended
// from `common/clip.wgsl`.

struct VertexInput {
    @location(0) position: vec2<f32>,  // Unit quad corner [0, 1]
}

struct InstanceInput {
    @location(2) dst_rect: vec4<f32>,             // [x, y, width, height] in device pixels
    @location(3) atlas: vec4<f32>,                // [u, v, page (0 mask / 1 colour), _]
    @location(4) color: vec4<f32>,                // straight-alpha [r, g, b, a]
    @location(5) transform: vec4<f32>,            // 2×2 linear part [a, b, c, d], column-major
    @location(6) origin: vec4<f32>,               // device translation [tx, ty, 0, 0]
    @location(7) clip_bounds: vec4<f32>,
    @location(8) clip_radii: vec4<f32>,
    @location(9) clip_kind: vec4<u32>,
    @location(10) clip_device_to_local: vec4<f32>,
    @location(11) clip_local_origin: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) page: u32,
    @location(3) world_pos: vec2<f32>,
    @location(4) clip_bounds: vec4<f32>,
    @location(5) clip_radii: vec4<f32>,
    @location(6) @interpolate(flat) clip_kind: u32,
    @location(7) clip_device_to_local: vec4<f32>,
    @location(8) clip_local_origin: vec4<f32>,
}

struct Viewport {
    size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

@group(1) @binding(0)
var mask_page: texture_2d<f32>;

@group(1) @binding(1)
var color_page: texture_2d<f32>;

@group(1) @binding(2)
var atlas_sampler: sampler;

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;

    let size = instance.dst_rect.zw;
    let local = instance.dst_rect.xy + vertex.position * size;
    // Column-major [a, b, c, d]: device = M * local + origin.
    let world_pos = vec2<f32>(
        instance.transform.x * local.x + instance.transform.z * local.y,
        instance.transform.y * local.x + instance.transform.w * local.y,
    ) + instance.origin.xy;

    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0;
    out.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);

    let page = u32(instance.atlas.z);
    var page_size: vec2<u32>;
    if (page == 0u) {
        page_size = textureDimensions(mask_page);
    } else {
        page_size = textureDimensions(color_page);
    }
    // The quad covers exactly `size` texels starting at the atlas origin.
    out.uv = (instance.atlas.xy + vertex.position * size) / vec2<f32>(page_size);
    out.page = page;
    out.color = instance.color;

    out.world_pos = world_pos;
    out.clip_bounds = instance.clip_bounds;
    out.clip_radii = instance.clip_radii;
    out.clip_device_to_local = instance.clip_device_to_local;
    out.clip_local_origin = instance.clip_local_origin;
    // Bit 2 carries the clip layer's Clip mode; `clipAlpha` unpacks it.
    out.clip_kind = instance.clip_kind.x | (instance.clip_kind.z << 2u);

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var color: vec4<f32>;
    if (in.page == 0u) {
        let coverage = textureSampleLevel(mask_page, atlas_sampler, in.uv, 0.0).r;
        color = vec4<f32>(in.color.rgb, in.color.a * coverage);
    } else {
        let texel = textureSampleLevel(color_page, atlas_sampler, in.uv, 0.0);
        color = vec4<f32>(texel.rgb, texel.a * in.color.a);
    }

    let clip_alpha = clipAlpha(
        in.world_pos,
        in.clip_bounds,
        in.clip_radii,
        in.clip_kind,
        in.clip_device_to_local,
        in.clip_local_origin,
    );
    color.a = color.a * clip_alpha;

    return color;
}
