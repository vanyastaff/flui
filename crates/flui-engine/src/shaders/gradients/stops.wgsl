// Gradient stops, shared by the linear, radial and sweep gradients.
//
// The stops of a whole segment live in one `rgba32float` texture, two texels
// per stop: the colour, then the position in `.x`. A texture rather than a
// storage buffer because GLES 3.0 and WebGL2 have no storage buffer in a
// fragment stage (ADR-0244 §1), and rather than a uniform array because a
// segment holds any number of gradients, and a uniform array a fixed number
// of stops. `textureLoad` reads a texel exactly, so nothing is filtered.
//
// The row width is `GRADIENT_STOP_TEXTURE_WIDTH` in `pipeline_set.rs`.
const GRADIENT_STOP_ROW_TEXELS: u32 = 1024u;

struct GradientStop {
    color: vec4<f32>,
    position: f32,
}

@group(1) @binding(0)
var gradient_stops: texture_2d<f32>;

fn gradientStopTexel(texel: u32) -> vec4<f32> {
    let at = vec2<u32>(texel % GRADIENT_STOP_ROW_TEXELS, texel / GRADIENT_STOP_ROW_TEXELS);
    return textureLoad(gradient_stops, at, 0);
}

fn gradientStop(index: u32) -> GradientStop {
    let texel = index * 2u;
    return GradientStop(gradientStopTexel(texel), gradientStopTexel(texel + 1u).x);
}

/// Interpolate color from gradient stops
/// t: position along gradient [0.0, 1.0]
/// stop_count: number of stops to consider
fn interpolateGradient(t: f32, stop_count: u32, stop_offset: u32) -> vec4<f32> {
    let t_clamped = clamp(t, 0.0, 1.0);

    if (stop_count == 0u) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    if (stop_count == 1u) {
        return gradientStop(stop_offset).color;
    }

    var prev_stop = gradientStop(stop_offset);
    var next_stop = gradientStop(stop_offset + 1u);

    if (t_clamped <= prev_stop.position) {
        return prev_stop.color;
    }

    for (var i = 1u; i < stop_count; i++) {
        next_stop = gradientStop(stop_offset + i);

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
