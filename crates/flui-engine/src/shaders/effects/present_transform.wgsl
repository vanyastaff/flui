// Presentation blit with colour conversion: working space (extended-range
// sRGB-encoded, premultiplied) -> the surface's gamut and transfer function.
//
// Mirrors `surface_color::convert_pixel`; the readback test in `surface_color`
// holds the two together. The surface is written without blending.

struct Params {
    // Rows of the linear sRGB -> output primaries matrix.
    matrix: array<vec4<f32>, 3>,
    // 0 sRGB (clamped), 1 sRGB (extended), 2 linear, 3 PQ.
    transfer: u32,
    headroom: f32,
    reference_white_nits: f32,
    _pad: f32,
}

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0) var working_texture: texture_2d<f32>;
@group(0) @binding(1) var nearest_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.uv = input.uv;
    return output;
}

fn srgb_decode(v: vec3<f32>) -> vec3<f32> {
    let a = abs(v);
    let low = a / 12.92;
    let high = pow((a + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return sign(v) * select(high, low, a <= vec3<f32>(0.04045));
}

fn srgb_encode(v: vec3<f32>) -> vec3<f32> {
    let a = abs(v);
    let low = a * 12.92;
    let high = 1.055 * pow(a, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return sign(v) * select(high, low, a <= vec3<f32>(0.0031308));
}

fn pq_encode(y: vec3<f32>) -> vec3<f32> {
    let m1 = 0.1593017578125;
    let m2 = 78.84375;
    let c1 = 0.8359375;
    let c2 = 18.8515625;
    let c3 = 18.6875;
    let p = pow(y, vec3<f32>(m1));
    return pow((c1 + c2 * p) / (1.0 + c3 * p), vec3<f32>(m2));
}

// Compress the brightest channel above SDR white toward the headroom, scaling
// all three so hue holds.
fn tone_map(linear: vec3<f32>, headroom: f32) -> vec3<f32> {
    let peak = max(linear.r, max(linear.g, linear.b));
    if (peak <= 1.0) {
        return linear;
    }
    let room = headroom - 1.0;
    var mapped = 1.0;
    if (room > 0.0) {
        let x = (peak - 1.0) / room;
        mapped = 1.0 + room * x / (1.0 + x);
    }
    return linear * (mapped / peak);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let pixel = textureSample(working_texture, nearest_sampler, input.uv);
    let alpha = pixel.a;
    var straight = vec3<f32>(0.0);
    if (alpha > 0.0) {
        straight = pixel.rgb / alpha;
    }
    let linear = tone_map(srgb_decode(straight), params.headroom);
    let mapped = vec3<f32>(
        dot(params.matrix[0].xyz, linear),
        dot(params.matrix[1].xyz, linear),
        dot(params.matrix[2].xyz, linear),
    );
    var encoded: vec3<f32>;
    switch params.transfer {
        case 0u: { encoded = clamp(srgb_encode(mapped), vec3<f32>(0.0), vec3<f32>(1.0)); }
        case 1u: { encoded = srgb_encode(mapped); }
        case 2u: { encoded = mapped; }
        default: {
            encoded = pq_encode(max(mapped, vec3<f32>(0.0)) * params.reference_white_nits / 10000.0);
        }
    }
    return vec4<f32>(encoded * alpha, alpha);
}
