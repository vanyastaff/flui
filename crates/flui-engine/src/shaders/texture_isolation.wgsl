// Cropped replay keeps image alpha separate from clip coverage.
struct ImageIsolationMapping {
    origin: vec2<f32>,
    extent: vec2<f32>,
    viewport_size: vec2<f32>,
    padding: vec2<f32>,
};
@group(2) @binding(0) var<uniform> image_isolation: ImageIsolationMapping;

@vertex
fn vs_isolation(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var output = vs_main(vertex, instance);
    output.position.x = (output.position.x + 1.0) * image_isolation.viewport_size.x / image_isolation.extent.x - 1.0 - 2.0 * image_isolation.origin.x / image_isolation.extent.x;
    output.position.y = (output.position.y - 1.0) * image_isolation.viewport_size.y / image_isolation.extent.y + 1.0 + 2.0 * image_isolation.origin.y / image_isolation.extent.y;
    return output;
}
struct ImageIsolationOutput {
    @location(0) source: vec4<f32>,
    @location(1) coverage: f32,
};
@fragment
fn fs_main(in: VertexOutput) -> ImageIsolationOutput {
    var output: ImageIsolationOutput;
    output.source = sampledImage(in);
    output.coverage = clipAlpha(in.position.xy + image_isolation.origin, in.world_pos,
        in.clip_bounds, in.clip_radii, in.clip_kind,
        in.clip_device_to_local, in.clip_local_origin);
    return output;
}
