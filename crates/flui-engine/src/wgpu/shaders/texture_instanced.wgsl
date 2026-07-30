// Instanced texture shader for FLUI
//
// Renders multiple textured quads in a single draw call using GPU instancing.
// Each instance contains: destination rect, source UV, tint color, and transform.
//
// Performance: 100 images = 1 draw call (vs 100 without instancing)
// Supports: texture atlases, rotation, color tinting

// Vertex input (shared unit quad: [0,0] to [1,1])
struct VertexInput {
    @location(0) position: vec2<f32>,  // Quad corner [0 to 1]
}

// Instance input (per-image data)
struct InstanceInput {
    @location(2) dst_rect: vec4<f32>,      // [x, y, width, height] in screen space
    @location(3) src_uv: vec4<f32>,        // [u_min, v_min, u_max, v_max] in 0-1 range
    @location(4) tint: vec4<f32>,          // [r, g, b, a] in 0-1 range
    @location(5) transform: vec4<f32>,     // [cos(angle), sin(angle), tx, ty]
    @location(6) clip_bounds: vec4<f32>,   // [x, y, width, height] of clip
    @location(7) clip_radii: vec4<f32>,    // [tl, tr, br, bl] of clip
    @location(8) clip_kind: vec4<u32>,     // [kind, _, _, _]: 0=none, 1=rrect, 2=rsuperellipse
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
}

// =============================================================================
// SDF helpers
// =============================================================================
//
// Inlined rather than imported: WGSL here has no include mechanism, and
// `rect_instanced.wgsl` sets the convention of carrying its own copy. Both
// must stay identical — a clip that rounds differently per primitive is worse
// than no clip at all, because the seam only shows where two primitives meet.

/// Rounded box SDF with per-corner radii. See `rect_instanced.wgsl` for the
/// branchless quadrant-selection prose.
fn sdRoundedBox(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    let r2 = select(vec2<f32>(r.x, r.w), vec2<f32>(r.y, r.z), p.x > 0.0);
    let r3 = select(r2.x, r2.y, p.y > 0.0);

    let q = abs(p) - b + vec2<f32>(r3);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r3;
}

/// Rounded superellipse SDF (iOS-squircle, n=4) with per-corner radii.
fn sdRoundedSuperellipse(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    let r2 = select(vec2<f32>(r.x, r.w), vec2<f32>(r.y, r.z), p.x > 0.0);
    let r3 = select(r2.x, r2.y, p.y > 0.0);

    let q = abs(p) - b + vec2<f32>(r3);

    if (q.x < 0.0 && q.y < 0.0) {
        return max(q.x, q.y) - r3;
    }

    if (r3 <= 0.0) {
        return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0)));
    }

    let ax = max(q.x, 0.0) / r3;
    let ay = max(q.y, 0.0) / r3;
    let n_norm = sqrt(sqrt(ax * ax * ax * ax + ay * ay * ay * ay));
    return (n_norm - 1.0) * r3;
}

/// Convert SDF distance to alpha with adaptive antialiasing (L2 gradient, so a
/// rotated edge gets ~1 device pixel of AA rather than ~1.41).
fn sdfToAlpha(dist: f32) -> f32 {
    let edge_width = length(vec2<f32>(dpdx(dist), dpdy(dist))) * 0.5;
    return 1.0 - smoothstep(-edge_width, edge_width, dist);
}

// Viewport uniform (for screen-space to clip-space conversion)
struct Viewport {
    size: vec2<f32>,      // Viewport size in pixels
    _padding: vec2<f32>,
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

    // Extract destination rectangle components
    let dst_x = instance.dst_rect.x;
    let dst_y = instance.dst_rect.y;
    let dst_width = instance.dst_rect.z;
    let dst_height = instance.dst_rect.w;

    // Transform unit quad [0,1] to destination rectangle
    var local_pos = vertex.position * vec2<f32>(dst_width, dst_height);

    // Apply rotation if present
    let cos_angle = instance.transform.x;
    let sin_angle = instance.transform.y;

    // Rotate around center of destination rect
    if (abs(cos_angle - 1.0) > 0.001 || abs(sin_angle) > 0.001) {
        // Translate to origin (center of rect)
        let center = vec2<f32>(dst_width * 0.5, dst_height * 0.5);
        var centered = local_pos - center;

        // Apply rotation matrix
        let rotated = vec2<f32>(
            centered.x * cos_angle - centered.y * sin_angle,
            centered.x * sin_angle + centered.y * cos_angle
        );

        // Translate back
        local_pos = rotated + center;
    }

    // Apply position and additional translation
    let world_pos = vec2<f32>(dst_x, dst_y) + local_pos + instance.transform.zw;

    // Convert to clip space [-1, 1]
    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0; // Flip Y for screen coords

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
    // world position the vertex was placed at — post-rotation, pre-projection.
    out.world_pos = world_pos;
    out.clip_bounds = instance.clip_bounds;
    out.clip_radii = instance.clip_radii;
    out.clip_kind = instance.clip_kind.x;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample texture
    var tex_color = textureSample(texture_view, texture_sampler, in.uv);

    // Apply tint (multiply)
    tex_color = tex_color * in.tint;

    // --- SDF Clip Test ---
    // Same branch order and kind encoding as `rect_instanced.wgsl`: the kind
    // flag selects the SDF, the bounds+radii slot is shared between kinds, and
    // flat interpolation keeps the branch uniform across the draw call.
    //
    // A circle is expressed here as radii of half the shorter side. Because
    // this is a distance field rather than tessellated geometry, that is an
    // exact circle at any scale — the ~6% diagonal bulge that rules out
    // drrect for circular *geometry* does not apply to a clip.
    var clip_alpha = 1.0;
    if (in.clip_kind != 0u && in.clip_bounds.z > 0.0 && in.clip_bounds.w > 0.0) {
        let clip_center = in.clip_bounds.xy + in.clip_bounds.zw * 0.5;
        let clip_p = in.world_pos - clip_center;
        let clip_half = in.clip_bounds.zw * 0.5;

        var clip_dist = 0.0;
        if (in.clip_kind == 2u) {
            clip_dist = sdRoundedSuperellipse(clip_p, clip_half, in.clip_radii);
        } else {
            // clip_kind == 1u (rrect), and the safe default for a kind this
            // shader has not learned about yet.
            clip_dist = sdRoundedBox(clip_p, clip_half, in.clip_radii);
        }

        clip_alpha = sdfToAlpha(clip_dist);
    }

    tex_color.a = tex_color.a * clip_alpha;

    // Alpha test (discard fully transparent pixels for better performance).
    // Runs after the clip so fully clipped-out texels cost nothing downstream.
    if (tex_color.a < 0.01) {
        discard;
    }

    return tex_color;
}
