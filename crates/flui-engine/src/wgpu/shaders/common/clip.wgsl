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

/// Mask selecting the distance-function selector out of the packed clip lane.
const CLIP_KIND_MASK: u32 = 3u;

/// Bit 2 of the packed clip lane: set means `Clip::HardEdge`.
///
/// Kept next to the mask so the CPU-side packing (`RectInstance::with_clip`
/// and friends) and this unpacking cannot drift apart on a magic number.
const CLIP_HARD_BIT: u32 = 4u;

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

/// Coverage from the SDF clip; 1.0 when no clip is attached.
///
/// `clip_bounds` is `[x, y, w, h]` and `clip_radii` is `[tl, tr, br, bl]`, both
/// in CLIP-LOCAL space — the shape the caller asked for, untransformed.
/// `device_to_local` maps a device-space fragment position into that space:
/// `[a, b, c, d]` columns first, `local_origin.xy` the translation.
///
/// Evaluating in local space is what lets a rotated or non-uniformly scaled
/// clip be exact. Device-space bounds cannot express a rotation at all, and a
/// scaled circular corner is an ellipse that one radius per corner cannot
/// hold — so the old form refused rotations and clamped scaled radii.
///
/// AA survives the change: `sdfToAlpha` measures the distance field's rate of
/// change per SCREEN pixel via `dpdx`/`dpdy`, so it picks up the mapping's
/// Jacobian automatically and the band stays ~1 device pixel wide.
///
/// `clip_kind` selects the distance function: 0 = none, 2 = rounded
/// superellipse, anything else = rounded rect (the safe default for a kind
/// this shader has not learned about yet).
///
/// The layer's `Clip` mode rides in bit 2 of the same value
/// ([`CLIP_HARD_BIT`]): clear feathers the boundary (`Clip::AntiAlias`), set
/// thresholds it (`Clip::HardEdge`). Packed rather than carried as its own
/// varying so no shader has to grow an interpolant, and every existing call
/// site keeps its arity.
///
/// It is separate from the instance's own `aliased` lane on purpose — that one
/// is the *paint's* `anti_alias`, and a shape drawn with a hard edge inside a
/// smooth clip must still get the smooth clip. Flutter treats the two as
/// independent for the same reason: `Clip` belongs to the clip layer,
/// `isAntiAlias` to the paint.
///
/// The whole evaluation lives here rather than being pasted into each fragment
/// shader for the same reason the distance functions do: every clip-capable
/// primitive must agree on what a clip means, and a pasted copy is free to
/// disagree silently.
fn clipAlpha(
    world_pos: vec2<f32>,
    clip_bounds: vec4<f32>,
    clip_radii: vec4<f32>,
    clip_kind_packed: u32,
    device_to_local: vec4<f32>,
    local_origin: vec4<f32>,
) -> f32 {
    let clip_kind = clip_kind_packed & CLIP_KIND_MASK;
    let clip_hard = (clip_kind_packed & CLIP_HARD_BIT) != 0u;
    var alpha = 1.0;
    if (clip_kind != 0u && clip_bounds.z > 0.0 && clip_bounds.w > 0.0) {
        let local = vec2<f32>(
            device_to_local.x * world_pos.x + device_to_local.z * world_pos.y + local_origin.x,
            device_to_local.y * world_pos.x + device_to_local.w * world_pos.y + local_origin.y,
        );

        let clip_center = clip_bounds.xy + clip_bounds.zw * 0.5;
        let clip_p = local - clip_center;
        let clip_half = clip_bounds.zw * 0.5;

        var clip_dist = 0.0;
        if (clip_kind == 2u) {
            clip_dist = sdRoundedSuperellipse(clip_p, clip_half, clip_radii);
        } else {
            clip_dist = sdRoundedBox(clip_p, clip_half, clip_radii);
        }

        if (clip_hard) {
            // `Clip::HardEdge`: inside or out, no partial coverage. The
            // half-open rule matches the SDF's own sign convention — a
            // fragment exactly on the boundary is inside.
            alpha = select(0.0, 1.0, clip_dist <= 0.0);
        } else {
            alpha = sdfToAlpha(clip_dist);
        }

        // Fully clipped-out fragments are DISCARDED, not merely made
        // transparent.
        //
        // Returning zero alpha is enough for `SrcOver` — nothing is
        // contributed either way — but not for a destination-destructive mode.
        // `Clear`, `Src`, `SrcIn` and `DstIn` clear or replace the destination
        // from their blend FACTORS, which do not consult source alpha, so a
        // full-surface `Clear` through a rounded clip wiped the clip's whole
        // bounding box, corners included.
        //
        // The threshold is exactly zero, not "small". At any partial coverage
        // the fragment must still reach the blender: `sdfToAlpha` feathers the
        // edge, and discarding a fringe fragment because its coverage is
        // merely low would harden every anti-aliased clip.
        if (alpha <= 0.0) {
            discard;
        }
    }
    return alpha;
}
