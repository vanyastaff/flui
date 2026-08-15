// Shared SDF helpers for the per-instance clip.
//
// This file is not a standalone shader: it is prepended to every shader that
// evaluates a clip (see `shaders/mod.rs`), because WGSL has no include
// directive and module-scope declarations are order-independent.
//
// It exists because these three functions were inlined byte-for-byte into
// `rect_instanced`, `circle_instanced`, and `texture_instanced`, and each new
// clip-carrying shader added another copy. Copies of a distance function drift
// silently: the same `ClipRRect` would round a rect and a gradient by
// different amounts, with nothing failing.
//
// `common/sdf.wgsl` is a broader reference library with no `include_str!`
// consumer — do not confuse the two. This file is the one that ships.

/// Rounded box SDF with per-corner radii.
///
/// `p` — point to test, centred at the origin.
/// `b` — half-extents (half width, half height).
/// `r` — corner radii [top-left, top-right, bottom-right, bottom-left].
fn sdRoundedBox(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    // Select radius based on quadrant (branchless).
    // r2 = (top, bottom) radii for the active horizontal side:
    //   right (p.x>0) → (tr=r.y, br=r.z); left → (tl=r.x, bl=r.w).
    let r2 = select(vec2<f32>(r.x, r.w), vec2<f32>(r.y, r.z), p.x > 0.0);
    // r3 = bottom (p.y>0) → r2.y; top → r2.x.
    let r3 = select(r2.x, r2.y, p.y > 0.0);

    let q = abs(p) - b + vec2<f32>(r3);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r3;
}

/// Rounded superellipse SDF (iOS-squircle, n=4) with per-corner radii.
fn sdRoundedSuperellipse(p: vec2<f32>, b: vec2<f32>, r: vec4<f32>) -> f32 {
    // (top, bottom) radii for the active side — see sdRoundedBox.
    let r2 = select(vec2<f32>(r.x, r.w), vec2<f32>(r.y, r.z), p.x > 0.0);
    let r3 = select(r2.x, r2.y, p.y > 0.0);

    let q = abs(p) - b + vec2<f32>(r3);

    // Inner rect: both components negative — the curve choice does not apply.
    if (q.x < 0.0 && q.y < 0.0) {
        return max(q.x, q.y) - r3;
    }

    // Degenerate corner: fall back to the sharp-rect SDF.
    if (r3 <= 0.0) {
        return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0)));
    }

    let ax = max(q.x, 0.0) / r3;
    let ay = max(q.y, 0.0) / r3;
    let n_norm = sqrt(sqrt(ax * ax * ax * ax + ay * ay * ay * ay));
    return (n_norm - 1.0) * r3;
}

/// Convert an SDF distance to coverage with adaptive antialiasing.
///
/// Uses the L2 (Euclidean) gradient magnitude so a diagonal or rotated edge
/// receives ~1 device pixel of AA exactly, not ~1.41× as with L1/fwidth.
fn sdfToAlpha(dist: f32) -> f32 {
    let edge_width = length(vec2<f32>(dpdx(dist), dpdy(dist))) * 0.5;
    return 1.0 - smoothstep(-edge_width, edge_width, dist);
}
