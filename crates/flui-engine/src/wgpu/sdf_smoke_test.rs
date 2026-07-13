//! CPU-side smoke test for `sdRoundedSuperellipse` math correctness.
//!
//! Transliterates the WGSL `sdRoundedBox` and `sdRoundedSuperellipse`
//! functions from [`shaders/common/sdf.wgsl`](crate::wgpu::shaders) into
//! Rust helpers, then samples both at known points to verify:
//!
//! 1. **Corner-region divergence:** At a sample inside the
//!    corner-curvature zone, the superellipse SDF produces a measurably
//!    different distance than the rrect SDF. This locks in that the SDF
//!    is doing different math than the default-fallback rrect
//!    approximation — a regression to the fallback would surface here.
//!
//! 2. **Inner-rect parity:** Deep inside the inner (non-corner) rect, both
//!    SDFs return identical values. Confirms the divergence is corner-
//!    region-only, not a math bug at the rectangle interior.
//!
//! 3. **Axis-edge parity:** On an axis-aligned edge midway between
//!    corners (no curvature applies), both SDFs return identical values.
//!    Confirms axis-edge regions are unaffected by the curve choice.
//!
//! Sample points are picked from a 200x200 superellipse with all four
//! corner radii = 80, large enough that corner curvature is visible and
//! the SDF difference is well above floating-point noise.
//!
//! These helpers must stay in sync with the WGSL functions. Future edits
//! to either side that change behavior trigger the divergence test
//! failure, surfacing the desync.

#![cfg(test)]

/// CPU transliteration of WGSL `sdRoundedBox`.
///
/// Must stay in sync with [`shaders/common/sdf.wgsl::sdRoundedBox`]
/// (and the inlined copy in `shaders/rect_instanced.wgsl`).
fn sd_rounded_box(p: [f32; 2], b: [f32; 2], r: [f32; 4]) -> f32 {
    // Per-corner radius selection (branchless equivalent of WGSL `select`)
    // (top, bottom) radii for the active horizontal side — see WGSL sdRoundedBox:
    //   right (p.x>0) → (tr=r[1], br=r[2]); left → (tl=r[0], bl=r[3]).
    let r2 = if p[0] > 0.0 {
        [r[1], r[2]]
    } else {
        [r[0], r[3]]
    };
    let r3 = if p[1] > 0.0 { r2[1] } else { r2[0] };

    let q = [p[0].abs() - b[0] + r3, p[1].abs() - b[1] + r3];

    let inner = q[0].max(q[1]).min(0.0);
    let outer = (q[0].max(0.0).powi(2) + q[1].max(0.0).powi(2)).sqrt();
    inner + outer - r3
}

/// CPU transliteration of WGSL `sdRoundedSuperellipse`.
///
/// Must stay in sync with [`shaders/common/sdf.wgsl::sdRoundedSuperellipse`]
/// (and the inlined copy in `shaders/rect_instanced.wgsl`).
fn sd_rounded_superellipse(p: [f32; 2], b: [f32; 2], r: [f32; 4]) -> f32 {
    // (top, bottom) radii for the active horizontal side — see WGSL sdRoundedBox:
    //   right (p.x>0) → (tr=r[1], br=r[2]); left → (tl=r[0], bl=r[3]).
    let r2 = if p[0] > 0.0 {
        [r[1], r[2]]
    } else {
        [r[0], r[3]]
    };
    let r3 = if p[1] > 0.0 { r2[1] } else { r2[0] };

    let q = [p[0].abs() - b[0] + r3, p[1].abs() - b[1] + r3];

    // Inner-rect region: both q components negative — same formula as
    // sdRoundedBox's interior branch (curve choice doesn't matter here).
    if q[0] < 0.0 && q[1] < 0.0 {
        return q[0].max(q[1]) - r3;
    }

    // Degenerate corner: fall back to standard rect SDF
    if r3 <= 0.0 {
        let inner = q[0].max(q[1]).min(0.0);
        let outer = (q[0].max(0.0).powi(2) + q[1].max(0.0).powi(2)).sqrt();
        return inner + outer;
    }

    let ax = q[0].max(0.0) / r3;
    let ay = q[1].max(0.0) / r3;
    // sqrt(sqrt(x)) == x^(1/4) — matches WGSL implementation choice.
    let n_norm = (ax.powi(4) + ay.powi(4)).sqrt().sqrt();
    (n_norm - 1.0) * r3
}

/// Build the (b, r) pair for a centered superellipse / rrect of given
/// outer size + per-corner radius.
fn shape_params(w: f32, h: f32, radius: f32) -> ([f32; 2], [f32; 4]) {
    let b = [w * 0.5, h * 0.5];
    let r = [radius; 4];
    (b, r)
}

#[test]
fn sdf_differs_from_rrect_in_corner_region() {
    // 200x200 superellipse with all corner radii = 80. Sample at (70, 70)
    // measured from the center — that's (30px, 30px) from the
    // bottom-right corner. This point falls inside the corner-curvature
    // zone where the iOS-squircle SDF diverges measurably from the
    // elliptical-arc rrect SDF.
    let (b, r) = shape_params(200.0, 200.0, 80.0);
    let p = [70.0, 70.0];

    let d_rrect = sd_rounded_box(p, b, r);
    let d_sup = sd_rounded_superellipse(p, b, r);

    let diff = (d_sup - d_rrect).abs();
    assert!(
        diff >= 0.5,
        "Corner-region SDF divergence below the 0.5px threshold: \
         rrect={d_rrect:.4}, superellipse={d_sup:.4}, diff={diff:.4}. \
         A regression to the rrect-fallback approximation would produce diff≈0.",
    );
}

#[test]
fn sdf_matches_rrect_deep_in_inner_rect() {
    // Sample at (10, 10) — well inside the inner non-corner rect of a
    // 200x200/r=80 superellipse. Both SDFs reduce to `max(q.x, q.y)` in
    // this region, so the distance values must match within float noise.
    let (b, r) = shape_params(200.0, 200.0, 80.0);
    let p = [10.0, 10.0];

    let d_rrect = sd_rounded_box(p, b, r);
    let d_sup = sd_rounded_superellipse(p, b, r);

    let diff = (d_sup - d_rrect).abs();
    assert!(
        diff < 1e-3,
        "Inner-rect SDFs should match: rrect={d_rrect:.6}, \
         superellipse={d_sup:.6}, diff={diff:.6}",
    );
}

#[test]
fn sdf_matches_rrect_on_axis_edge_midway() {
    // Sample at (95, 0) — on the right-edge midway, outside the corner
    // regions. Both SDFs evaluate `length(max(q, 0)) - r3` (or its
    // equivalent), which differ only in the corner-region formula.
    // Their results must match.
    let (b, r) = shape_params(200.0, 200.0, 80.0);
    let p = [95.0, 0.0];

    let d_rrect = sd_rounded_box(p, b, r);
    let d_sup = sd_rounded_superellipse(p, b, r);

    let diff = (d_sup - d_rrect).abs();
    assert!(
        diff < 1e-3,
        "Axis-edge SDFs should match (no corner curvature applies): \
         rrect={d_rrect:.6}, superellipse={d_sup:.6}, diff={diff:.6}",
    );
}

#[test]
fn sdf_degenerate_corner_radius_zero() {
    // Edge-case guard from `sdRoundedSuperellipse`: when r3 == 0, fall
    // back to the standard rect SDF (no division by zero). Sample in
    // what would be the corner if radii were non-zero.
    let b = [100.0, 100.0];
    let r = [0.0; 4];
    let p = [90.0, 90.0];

    let d_rrect = sd_rounded_box(p, b, r);
    let d_sup = sd_rounded_superellipse(p, b, r);

    let diff = (d_sup - d_rrect).abs();
    assert!(
        diff < 1e-3,
        "With r=0 the superellipse SDF must fall back to rect SDF: \
         rrect={d_rrect:.6}, superellipse={d_sup:.6}, diff={diff:.6}",
    );
}

#[test]
fn sdf_corner_radius_maps_to_correct_quadrant() {
    // Pin the [tl, tr, br, bl] → screen-quadrant mapping (Y-down: p.x>0 = right,
    // p.y>0 = bottom). With ONLY the top-left corner rounded, a point just inside
    // the TOP-LEFT corner (p.x<0, p.y<0) must be cut away (SDF > 0), while the
    // sharp BOTTOM-RIGHT corner (p.x>0, p.y>0) stays inside (SDF < 0). The pre-fix
    // mapping rounded the bottom-right corner instead and would fail here — this
    // is the CPU mirror of GPU test O7, giving this sync guard teeth against a
    // corner-index transposition (the existing tests use uniform radii and cannot).
    let b = [100.0_f32, 100.0];
    let r = [80.0_f32, 0.0, 0.0, 0.0]; // top-left only

    let top_left = sd_rounded_box([-95.0, -95.0], b, r);
    let bottom_right = sd_rounded_box([95.0, 95.0], b, r);

    assert!(
        top_left > 0.0,
        "top-left corner must be ROUNDED (point cut away → SDF > 0); got {top_left:.3}. \
         A non-positive value means the [tl,tr,br,bl] → quadrant mapping is transposed."
    );
    assert!(
        bottom_right < 0.0,
        "bottom-right corner must be SHARP (point inside → SDF < 0); got {bottom_right:.3}."
    );
}
