struct Mode { value: vec4<u32>, };
@group(0) @binding(0) var paint: texture_2d<f32>;
@group(0) @binding(1) var coverage: texture_2d<f32>;
@group(0) @binding(2) var destination: texture_2d<f32>;
@group(0) @binding(3) var<uniform> mode: Mode;
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x*2.0-1.0, y*2.0-1.0, 0.0, 1.0);
}
@fragment fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy) - vec2<i32>(mode.value.yz);
    // P is coverage-weighted premultiplied paint; C is geometry independently
    // of paint alpha. Division would lose transparent-paint coverage.
    let p = textureLoad(paint, pixel, 0);
    let c = textureLoad(coverage, pixel, 0).r;
    let d = textureLoad(destination, pixel, 0);
    switch mode.value.x {
        case 0u: { return d*(1.0-c); }
        case 1u: { return p+d*(1.0-c); }
        case 2u: { return d; }
        case 3u: { return p+d*(1.0-p.a); }
        case 4u: { return p*(1.0-d.a)+d; }
        case 5u: { return p*d.a+d*(1.0-c); }
        case 6u: { return d*(1.0-c+p.a); }
        case 7u: { return p*(1.0-d.a)+d*(1.0-c); }
        case 8u: { return d*(1.0-p.a); }
        case 9u: { return p*d.a+d*(1.0-p.a); }
        case 10u: { return p*(1.0-d.a)+d*(1.0-c+p.a); }
        case 11u: { return p*(1.0-d.a)+d*(1.0-p.a); }
        case 12u: { return min(p+d*c, vec4<f32>(c))+d*(1.0-c); }
        case 13u: { return p*d+d*(1.0-c); }
        default: { return d; }
    }
}
