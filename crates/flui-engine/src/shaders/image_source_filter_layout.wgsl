// A standalone binding schema for code generation. Runtime image pipelines
// bind this same layout at group 3, after viewport, source and isolation.
#import color_filter::ImageSourceFilter
@group(0) @binding(0) var<uniform> source_filter: ImageSourceFilter;
@fragment fn fs_main() -> @location(0) vec4<f32> { return source_filter.offset; }
