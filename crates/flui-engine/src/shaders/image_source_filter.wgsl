#import color_filter::{ImageSourceFilter, filter_mode, filter_matrix, filter_gamma}
@group(3) @binding(0) var<uniform> source_filter: ImageSourceFilter;

// Filtering belongs to each decoded tap, before interpolation or tint.
fn filterSourceTexel(straight: vec4<f32>) -> vec4<f32> {
    if source_filter.kind == 0u {
        return filter_mode(source_filter.color, vec4<f32>(straight.rgb * straight.a, straight.a), source_filter.mode);
    }
    if source_filter.kind == 1u { return filter_matrix(straight, source_filter.matrix, source_filter.offset); }
    return filter_gamma(straight, source_filter.mode);
}
