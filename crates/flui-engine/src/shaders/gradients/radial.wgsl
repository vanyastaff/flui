// Two-circle radial gradients with validated storage-buffer stops.

// Vertex input (unit quad)
struct VertexInput {
    @location(0) position: vec2<f32>,
}

// Instance input
struct InstanceInput {
    @location(2) bounds: vec4<f32>,
    // Adjacent Rust fields share one attribute without changing their byte ABI:
    // radial second-circle centre/radius/tile mode.
    @location(3) geometry: vec4<f32>,
    @location(4) corner_radii: vec4<f32>,
    @location(5) stops: vec2<u32>, // count, offset
    @location(6) clip_bounds: vec4<f32>,
    @location(7) clip_radii: vec4<f32>,
    @location(8) clip_kind: vec4<u32>,
    @location(9) clip_device_to_local: vec4<f32>,
    @location(10) clip_local_origin: vec4<f32>,
    @location(11) transform: vec4<f32>,
    @location(12) transform_translate: vec4<f32>,
    @location(13) focal: vec4<f32>,
}

// Gradient stop (same as linear)
struct GradientStop {
    color: vec4<f32>,
    position: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

// Vertex output
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local_pos: vec2<f32>,
    @location(1) @interpolate(flat) center: vec2<f32>,
    @location(2) @interpolate(flat) radius: f32,
    @location(3) rect_size: vec2<f32>,
    @location(4) corner_radii: vec4<f32>,
    @location(5) @interpolate(flat) stop_count: u32,
    @location(6) @interpolate(flat) stop_offset: u32,
    // Device-space position, carried only for the clip SDF. Linear
    // already had one at location 0; these two did not.
    @location(7) world_pos: vec2<f32>,
    @location(8) clip_bounds: vec4<f32>,
    @location(9) clip_radii: vec4<f32>,
    @location(10) @interpolate(flat) clip_kind: u32,
    @location(11) clip_device_to_local: vec4<f32>,
    @location(12) clip_local_origin: vec4<f32>,
    @location(13) @interpolate(flat) focal: vec4<f32>,
    @location(14) @interpolate(flat) tile: f32,
}

// Uniforms
struct Viewport {
    size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> viewport: Viewport;

@group(1) @binding(0)
var<storage, read> gradient_stops: array<GradientStop>;

// =============================================================================
// SDF Functions
// =============================================================================

// =============================================================================
// Gradient Interpolation (same as linear)
// =============================================================================

fn interpolateGradient(t: f32, stop_count: u32, stop_offset: u32) -> vec4<f32> {
    let t_clamped = clamp(t, 0.0, 1.0);

    if (stop_count == 0u) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    if (stop_count == 1u) {
        return gradient_stops[stop_offset].color;
    }

    var prev_stop = gradient_stops[stop_offset];
    var next_stop = gradient_stops[stop_offset + 1u];

    if (t_clamped <= prev_stop.position) {
        return prev_stop.color;
    }

    for (var i = 1u; i < stop_count; i++) {
        next_stop = gradient_stops[stop_offset + i];

        if (t_clamped <= next_stop.position) {
            let range = next_stop.position - prev_stop.position;
            if (range > 0.0) {
                let local_t = (t_clamped - prev_stop.position) / range;
                return mix(prev_stop.color, next_stop.color, local_t);
            } else {
                return next_stop.color;
            }
        }

        prev_stop = next_stop;
    }

    return next_stop.color;
}

// =============================================================================
// Vertex Shader
// =============================================================================

@vertex
fn vs_main(
    vertex: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;

    let local_pos = vertex.position * instance.bounds.zw;
    let local_absolute = local_pos + instance.bounds.xy;
    let world_pos = mat2x2<f32>(instance.transform.xy, instance.transform.zw) * local_absolute + instance.transform_translate.xy;

    let clip_x = (world_pos.x / viewport.size.x) * 2.0 - 1.0;
    let clip_y = 1.0 - (world_pos.y / viewport.size.y) * 2.0;

    out.clip_position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.local_pos = local_pos;
    out.center = instance.geometry.xy;
    out.radius = instance.geometry.z;
    out.focal = instance.focal;
    out.tile = instance.geometry.w;
    out.rect_size = instance.bounds.zw;
    out.corner_radii = instance.corner_radii;
    out.stop_count = instance.stops.x;
    out.stop_offset = instance.stops.y;
    out.world_pos = world_pos;
    out.clip_bounds = instance.clip_bounds;
    out.clip_radii = instance.clip_radii;
    // Bit 2 carries the clip layer's Clip mode; `clipAlpha` unpacks it.
    out.clip_kind = instance.clip_kind.x | (instance.clip_kind.z << 2u);
    out.clip_device_to_local = instance.clip_device_to_local;
    out.clip_local_origin = instance.clip_local_origin;

    return out;
}

// =============================================================================
// Fragment shading
// =============================================================================
//
// `common/coverage.wgsl` owns `ShadedFragment` and `premultipliedSource`, and
// `common/fragment_folded.wgsl` / `common/fragment_second_source.wgsl` own the
// two entry points. All this module owes them is `shadeFragment`.

// A finite solution must also describe a circle with nonnegative radius.
fn admissibleRoot(t: f32, r0: f32, dr: f32) -> bool {
    let radius = r0 + t * dr;
    return abs(t) <= 3.402823e38 && radius >= 0.0 && radius <= 3.402823e38;
}

// The second component is coverage: points outside the cone have no solution.
fn radialParameter(p: vec2<f32>, center: vec2<f32>, radius: f32, focal: vec4<f32>) -> vec2<f32> {
    let d = center - focal.xy;
    let dr = radius - focal.z;
    let q = p * focal.w - focal.xy;
    if (all(d == vec2<f32>(0.0)) && radius == 0.0 && focal.z == 0.0) {
        return vec2<f32>(0.0, 1.0);
    }
    let a = dot(d, d) - dr * dr;
    let b = -2.0 * (dot(q, d) + focal.z * dr);
    let c = dot(q, q) - focal.z * focal.z;
    if (a == 0.0) {
        if (b == 0.0) { return vec2<f32>(0.0); }
        let t = -c / b;
        return vec2<f32>(t, select(0.0, 1.0, admissibleRoot(t, focal.z, dr)));
    }
    let discriminant = b * b - 4.0 * a * c;
    if (discriminant < 0.0) { return vec2<f32>(0.0); }
    if (discriminant == 0.0) {
        let t = -b / (2.0 * a);
        return vec2<f32>(t, select(0.0, 1.0, admissibleRoot(t, focal.z, dr)));
    }
    // Avoid cancellation in one root; the other follows from their product.
    let root = sqrt(discriminant);
    let stable = -0.5 * (b + select(-root, root, b >= 0.0));
    let t0 = stable / a;
    let t1 = c / stable;
    let valid0 = admissibleRoot(t0, focal.z, dr);
    let valid1 = admissibleRoot(t1, focal.z, dr);
    if (valid0 && valid1) { return vec2<f32>(max(t0, t1), 1.0); }
    if (valid0) { return vec2<f32>(t0, 1.0); }
    if (valid1) { return vec2<f32>(t1, 1.0); }
    return vec2<f32>(0.0);
}

fn shadeFragment(in: VertexOutput) -> ShadedFragment {
    // Check if inside rounded corners
    let centered_pos = (in.local_pos / in.rect_size - 0.5) * in.rect_size;
    let dist = sdRoundedBox(centered_pos, in.rect_size * 0.5, in.corner_radii);

    // Local distance is not device distance under affine scaling. Evaluate
    // derivatives unconditionally; coverage, rather than a local-unit cutoff,
    // determines the edge contribution.

    // The gradient's own rounded-box edge and the clip's edge are both partial
    // coverage, and both belong on the coverage channel rather than folded into
    // the paint's alpha — see `common/coverage.wgsl`.
    // (`sdfToAlpha` takes derivatives, so it must be reached from uniform
    // control flow.)
    let edge_alpha = sdfToAlpha(dist);
    // Clip coverage — see `clipAlpha` in `common/clip.wgsl`.
    let clip_alpha = clipAlpha(
        in.clip_position.xy,
        in.world_pos,
        in.clip_bounds,
        in.clip_radii,
        in.clip_kind,
        in.clip_device_to_local,
        in.clip_local_origin,
    );

    let parameter = radialParameter(in.local_pos, in.center, in.radius, in.focal);
    var t = parameter.x;
    var valid = parameter.y;
    if (in.tile == 1.0) {
        t = t - floor(t);
    } else if (in.tile == 2.0) {
        let repeated = t - 2.0 * floor(t * 0.5);
        t = 1.0 - abs(repeated - 1.0);
    } else if (in.tile == 3.0 && (t < 0.0 || t > 1.0)) {
        valid = 0.0;
    }
    // Invalid roots must not reach stop interpolation with a NaN parameter.
    let color = interpolateGradient(select(0.0, t, valid != 0.0), in.stop_count, in.stop_offset);

    var shaded: ShadedFragment;
    shaded.color = color;
    shaded.coverage = edge_alpha * clip_alpha * valid;
    return shaded;
}
