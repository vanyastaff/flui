// Geometry specializations remove unused membership code before driver compilation.
override HAS_PATHS: bool = true;
override HAS_CURVES: bool = true;

struct ClipNode {
    bounds: vec4<f32>,
    radii_x: vec4<f32>,
    radii_y: vec4<f32>,
    inverse: vec4<f32>,
    translation: vec4<f32>,
    flags: vec4<u32>,
    path_range: vec4<u32>,
};
struct MaskMapping {
    attachment_to_root: vec4<f32>,
    translation: vec4<f32>,
    extent_counts: vec4<u32>,
};
@group(0) @binding(0) var<uniform> nodes: array<ClipNode, 64>;
@group(0) @binding(1) var<uniform> edges: array<vec4<f32>, 512>;
@group(0) @binding(2) var<uniform> mapping: MaskMapping;

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}
fn root_point(p: vec2<f32>) -> vec2<f32> {
    let q = p + mapping.translation.zw;
    let m = mapping.attachment_to_root;
    return vec2<f32>(m.x*q.x + m.z*q.y, m.y*q.x + m.w*q.y) + mapping.translation.xy;
}
fn path_contains(n: ClipNode, p: vec2<f32>) -> bool {
    // Admission bounds payloads by 2^20 and attachments by 2^14. Two
    // affine mappings then bound p below 2^64; cross products below 2^88,
    // so finite payloads cannot overflow this membership arithmetic.
    var winding = 0i;
    var parity = false;
    for (var i = 0u; i < n.path_range.y; i += 1u) {
        let e = edges[n.path_range.x + i];
        // Half-open Y intervals count a vertex once; horizontal edges do not cross.
        let up = e.y <= p.y && e.w > p.y;
        let down = e.w <= p.y && e.y > p.y;
        let cross = (e.z-e.x)*(p.y-e.y) - (p.x-e.x)*(e.w-e.y);
        if ((up && cross > 0.0) || (down && cross < 0.0)) {
            parity = !parity;
            winding += select(-1i, 1i, up);
        }
    }
    return select(winding != 0i, parity, n.flags.w == 1u);
}
fn contains(n: ClipNode, root: vec2<f32>) -> bool {
    let m = n.inverse;
    let p = vec2<f32>(m.x*root.x + m.z*root.y, m.y*root.x + m.w*root.y) + n.translation.xy;
    if (HAS_PATHS && n.flags.x == 3u) { return path_contains(n, p); }
    let lo = n.bounds.xy;
    let hi = lo + n.bounds.zw;
    if (any(p < lo) || any(p >= hi)) { return false; }
    if (!HAS_CURVES || n.flags.x == 0u) { return true; }
    // Evaluate each corner zone: asymmetric radii may extend past the midpoint.
    for (var corner = 0u; corner < 4u; corner += 1u) {
        let right = corner == 1u || corner == 2u;
        let bottom = corner >= 2u;
        let radius = vec2<f32>(n.radii_x[corner], n.radii_y[corner]);
        if (any(radius <= vec2<f32>(0.0))) { continue; }
        let center = vec2<f32>(select(lo.x+radius.x, hi.x-radius.x, right), select(lo.y+radius.y, hi.y-radius.y, bottom));
        let delta = vec2<f32>(select(center.x-p.x, p.x-center.x, right), select(center.y-p.y, p.y-center.y, bottom));
        if (all(delta > vec2<f32>(0.0))) {
            let q = delta / radius;
            let q2 = q*q;
            if (select(q2.x+q2.y, dot(q2,q2), n.flags.x == 2u) > 1.0) { return false; }
        }
    }
    return true;
}
@fragment fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) f32 {
    let pixel = floor(position.xy);
    let center = root_point(pixel + vec2<f32>(0.5));
    // Keep the sample count uniform-driven: constant nested loops can cause
    // costly driver expansion. Hard leaves use the same center; AA leaves
    // retain the original 8x8 grid, including mixed hard/AA chains.
    var covered = 0u;
    for (var sample_index = 0u; sample_index < mapping.extent_counts.w; sample_index += 1u) {
        let sx = sample_index % 8u;
        let sy = sample_index / 8u;
        let sample = root_point(pixel + (vec2<f32>(f32(sx),f32(sy))+vec2<f32>(0.5))/8.0);
        var inside = true;
        for (var i = 0u; i < mapping.extent_counts.z; i += 1u) {
            let n = nodes[i];
            let hit = contains(n, select(sample, center, n.flags.z != 0u));
            inside = inside && select(hit, !hit, n.flags.y == 1u);
        }
        covered += select(0u, 1u, inside);
    }
    return f32(covered)/f32(mapping.extent_counts.w);
}
