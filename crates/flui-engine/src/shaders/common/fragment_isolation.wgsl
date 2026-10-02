struct IsolationMapping {
    origin: vec2<f32>,
    extent: vec2<f32>,
    viewport_size: vec2<f32>,
    padding: vec2<f32>,
}
@group(2) @binding(0) var<uniform> isolation_mapping: IsolationMapping;

fn isolateVertex(input: VertexOutput) -> VertexOutput {
    var output = input;
    output.clip_position.x = (input.clip_position.x + 1.0) * isolation_mapping.viewport_size.x / isolation_mapping.extent.x - 1.0 - 2.0 * isolation_mapping.origin.x / isolation_mapping.extent.x;
    output.clip_position.y = (input.clip_position.y - 1.0) * isolation_mapping.viewport_size.y / isolation_mapping.extent.y + 1.0 + 2.0 * isolation_mapping.origin.y / isolation_mapping.extent.y;
    return output;
}

// Independent geometric coverage survives even when the paint is transparent.
struct IsolatedFragment {
    @location(0) source: vec4<f32>,
    @location(1) coverage: vec4<f32>,
}

@fragment
fn fs_main(input: VertexOutput) -> IsolatedFragment {
    var translated = input;
    translated.clip_position = vec4<f32>(input.clip_position.xy + isolation_mapping.origin, input.clip_position.zw);
    let shaded = shadeFragment(translated);
    var output: IsolatedFragment;
    output.source = premultipliedSource(shaded);
    output.coverage = vec4<f32>(shaded.coverage);
    return output;
}
