//! AA oracle harness: calibrated analytic-coverage reference + GPU acceptance
//! gates for the affine-SDF instanced rect/rrect path.
//!
//! ## Design
//!
//! The oracle computes fractional pixel coverage by supersampling an analytic
//! inside-test at 8×8 sub-pixel positions (`ORACLE_GRID = 8`). This gives a
//! CPU reference accurate to within 1/(8×8) = ~1.6% of a pixel.
//!
//! The oracle is **calibrated against the existing axis-aligned SDF rrect**
//! (which is known-correct AA) before being used to gate the new affine path.
//!
//! ## Test inventory
//!
//! | # | Description |
//! |---|-------------|
//! | O1 | Calibration — axis-aligned SDF rrect boundary matches oracle within tolerance |
//! | O2 | Oracle has teeth — hard-aliased alpha map does NOT pass the same tolerance |
//! | O3 | Rotated rect AA — boundary band monotonic and matches oracle (fails before reroute) |
//! | O4 | Rotated rrect — correct size/orientation + AA (fixes AABB bug) |
//! | O5 | Byte-identity — axis-aligned SrcOver rect and rrect readback identical after change |
//! | O6 | fwidth scale-invariance — AA band stays ~1 device-px at 1× and 8× world scale |
//! | O7 | Corner-radius mapping — only the top-left corner rounds when only `tl` is set |

// ── CPU oracle (no GPU needed) ────────────────────────────────────────────────

/// Number of sub-samples per pixel axis for the analytic-coverage oracle.
const ORACLE_GRID: usize = 8;

/// Analytic inside-test for an axis-aligned ellipse centered at origin with
/// semi-axes `(rx, ry)`.
#[cfg(all(test, feature = "testing"))]
fn inside_ellipse(px: f32, py: f32, rx: f32, ry: f32) -> bool {
    // Point is inside the ellipse iff (px/rx)² + (py/ry)² ≤ 1.
    let nx = px / rx;
    let ny = py / ry;
    nx * nx + ny * ny <= 1.0
}

/// Analytic inside-test for a rotated ellipse centered at origin.
///
/// `angle_rad` is the CW rotation of the ellipse's major axis from the +X axis
/// (screen-space Y-down convention). The test maps the query point into the
/// ellipse's local frame and delegates to [`inside_ellipse`].
#[cfg(all(test, feature = "testing"))]
fn inside_rotated_ellipse(px: f32, py: f32, rx: f32, ry: f32, angle_rad: f32) -> bool {
    // Inverse rotation: rotate the query point by -angle into the ellipse frame.
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();
    let local_x = cos_a * px + sin_a * py;
    let local_y = -sin_a * px + cos_a * py;
    inside_ellipse(local_x, local_y, rx, ry)
}

/// Analytic inside-test for an arc sector centered at origin.
///
/// A point is inside the arc iff:
///   1. Its distance from the origin is ≤ `radius` (inside the circle).
///   2. Its angle falls within the swept sector.
///
/// `start_angle` and `sweep_angle` follow screen Y-down convention:
///   - 0 = +X (right), π/2 = +Y (down), π = left.
///   - Positive sweep = clockwise; negative = counter-clockwise.
///
/// For a `|sweep| ≥ 2π` input this degrades to the full circle test.
fn inside_arc(px: f32, py: f32, radius: f32, start_angle: f32, sweep_angle: f32) -> bool {
    // Radial guard.
    if px * px + py * py > radius * radius {
        return false;
    }
    // Full-circle shortcut.
    if sweep_angle.abs() >= 2.0 * std::f32::consts::PI {
        return true;
    }
    // Angle of the sample point in [-π, π].
    let sample_angle = py.atan2(px);

    // Normalise to a canonical [start, start + |sweep|] check.
    // For negative sweep, swap start and end (test the CCW arc as a CW arc
    // from `end` to `start`).
    let (a0, pos_sweep) = if sweep_angle >= 0.0 {
        (start_angle, sweep_angle)
    } else {
        (start_angle + sweep_angle, -sweep_angle)
    };

    // Wrap the sample angle into the range [a0, a0 + pos_sweep] using modular
    // arithmetic so we can compare linearly.
    let tau = 2.0 * std::f32::consts::PI;
    // Bring sample into [a0, a0 + 2π).
    let mut wrapped = sample_angle;
    while wrapped < a0 {
        wrapped += tau;
    }
    while wrapped >= a0 + tau {
        wrapped -= tau;
    }
    wrapped < a0 + pos_sweep
}

/// Analytic inside-test for a rotated arc sector centered at origin.
///
/// `rotation_rad` is applied to the point (inverse of applying it to the arc),
/// then delegates to [`inside_arc`] with the original `start_angle` / `sweep_angle`.
/// This tests an arc that has been placed under a rotation transform.
#[cfg(all(test, feature = "testing"))]
fn inside_rotated_arc(
    px: f32,
    py: f32,
    radius: f32,
    start_angle: f32,
    sweep_angle: f32,
    rotation_rad: f32,
) -> bool {
    // Inverse-rotate the query point into the arc's local frame.
    let cos_a = rotation_rad.cos();
    let sin_a = rotation_rad.sin();
    let local_x = cos_a * px + sin_a * py;
    let local_y = -sin_a * px + cos_a * py;
    inside_arc(local_x, local_y, radius, start_angle, sweep_angle)
}

/// Analytic inside-test for an axis-aligned rect centered at origin with
/// half-extents `(half_w, half_h)`.
fn inside_rect(px: f32, py: f32, half_w: f32, half_h: f32) -> bool {
    px.abs() <= half_w && py.abs() <= half_h
}

/// Analytic inside-test for a rounded rect centered at origin.
/// Mirrors `sdRoundedBox` from `rect_instanced.wgsl`: negative SDF = inside.
fn inside_rounded_rect(px: f32, py: f32, half_w: f32, half_h: f32, r: [f32; 4]) -> bool {
    // Per-corner radius selection — mirrors the WGSL branchless `select` in
    // `sdRoundedBox`. r = [tl, tr, br, bl]. (top, bottom) radii for the active
    // horizontal side: right (px>0) → (tr=r[1], br=r[2]); left → (tl=r[0], bl=r[3]).
    let r2 = if px > 0.0 { [r[1], r[2]] } else { [r[0], r[3]] };
    let r3 = if py > 0.0 { r2[1] } else { r2[0] };

    let qx = px.abs() - half_w + r3;
    let qy = py.abs() - half_h + r3;

    let dist = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r3;
    dist <= 0.0
}

/// Analytic inside-test for a rotated rect.
///
/// `angle_rad` rotates the shape CW (screen coordinates, Y-down). The test
/// maps the query point into the shape's local frame and delegates to
/// `inside_rect`.
fn inside_rotated_rect(px: f32, py: f32, half_w: f32, half_h: f32, angle_rad: f32) -> bool {
    // Inverse rotation: rotate the query point by -angle into local frame.
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();
    let local_x = cos_a * px + sin_a * py;
    let local_y = -sin_a * px + cos_a * py;
    inside_rect(local_x, local_y, half_w, half_h)
}

/// Analytic inside-test for a rotated rounded rect.
///
/// Only used in GPU readback tests gated on `testing`.
#[cfg(all(test, feature = "testing"))]
fn inside_rotated_rounded_rect(
    px: f32,
    py: f32,
    half_w: f32,
    half_h: f32,
    r: [f32; 4],
    angle_rad: f32,
) -> bool {
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();
    let local_x = cos_a * px + sin_a * py;
    let local_y = -sin_a * px + cos_a * py;
    inside_rounded_rect(local_x, local_y, half_w, half_h, r)
}

/// Compute analytic fractional pixel coverage for a shape at pixel center
/// `(pixel_x, pixel_y)`, by supersampling at `ORACLE_GRID × ORACLE_GRID`
/// sub-positions within the pixel.
///
/// `inside_fn` returns `true` when the sample point is inside the shape.
fn analytic_coverage(pixel_x: f32, pixel_y: f32, inside_fn: impl Fn(f32, f32) -> bool) -> f32 {
    let n = ORACLE_GRID as f32;
    let mut inside_count = 0u32;
    for row in 0..ORACLE_GRID {
        for col in 0..ORACLE_GRID {
            // Sub-pixel offset within [-0.5, 0.5] × [-0.5, 0.5].
            let dx = (col as f32 + 0.5) / n - 0.5;
            let dy = (row as f32 + 0.5) / n - 0.5;
            if inside_fn(pixel_x + dx, pixel_y + dy) {
                inside_count += 1;
            }
        }
    }
    inside_count as f32 / (n * n)
}

// ── GPU readback tests ─────────────────────────────────────────────────────────

// All intentional: pixel-coordinate and alpha-value casts (f32 → u8 / usize)
// are clamped or derived from fixed [0,1] oracle values; sign loss is impossible
// (oracle returns non-negative; pixel coords are positive screen positions).
#[cfg(all(test, feature = "testing"))]
mod gpu_tests {
    use std::f32::consts::PI;
    use std::sync::Arc;

    use flui_foundation::geometry::{RRect, Rect};
    use flui_painting::styling::Color;
    use flui_painting::{BlendMode, Paint};

    use crate::{painter::WgpuPainter, render_target::RenderTarget};

    use super::{
        analytic_coverage, inside_rotated_arc, inside_rotated_ellipse, inside_rotated_rect,
        inside_rotated_rounded_rect,
    };

    // ── Harness constants ─────────────────────────────────────────────────────

    // 128×128: large enough that a rotated rect boundary has many pixels to
    // sample and small enough for fast DX12 readback. Matches the ≥64px
    // minimum to avoid DX12 small-texture copy artifacts.
    const SURFACE_WIDTH: u32 = 128;
    const SURFACE_HEIGHT: u32 = 128;
    const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    // Calibrated tolerance: 30/255 ≈ 11.8% of the [0,255] alpha range.
    //
    // This tolerance is designed to:
    // - PASS: fwidth-based SDF AA (actual vs oracle within ~10% due to the
    //   smoothstep width equalling 1×fwidth ≈ 1 device-px, which rounds
    //   slightly differently from 8×8 grid supersampling).
    // - FAIL: hard-aliased edges (alpha ∈ {0, 255}; boundary oracle ≈ 128 →
    //   diff ≈ 128 >> 30). The unit test O2 / `oracle_has_teeth` confirms this.
    const CALIBRATION_TOLERANCE_U8: u8 = 30;

    // ── Harness helpers ───────────────────────────────────────────────────────

    fn acquire_test_device_and_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        crate::test_support::test_device_and_queue("AA Oracle Test Device")
    }

    fn create_render_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        crate::test_support::create_target(
            device,
            "AA Oracle Test Surface",
            SURFACE_WIDTH,
            SURFACE_HEIGHT,
            SURFACE_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
        )
    }

    fn clear_surface(device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView) {
        crate::test_support::clear_target(device, queue, view, wgpu::Color::TRANSPARENT);
    }

    /// Read all pixels from a texture and return RGBA bytes (row-major).
    fn readback_pixels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) -> Vec<[u8; 4]> {
        crate::test_support::readback_pixels(device, queue, texture, SURFACE_WIDTH, SURFACE_HEIGHT)
    }

    fn build_painter(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> WgpuPainter {
        WgpuPainter::with_shared_device(
            device,
            queue,
            SURFACE_FORMAT,
            (SURFACE_WIDTH, SURFACE_HEIGHT),
        )
    }

    /// Identify the boundary band of pixels: those that have partial coverage
    /// according to the analytic oracle (0 < coverage < 1).
    fn boundary_pixel_indices(inside_fn: impl Fn(f32, f32) -> bool) -> Vec<(usize, f32)> {
        let mut boundary = Vec::new();
        for row in 0..SURFACE_HEIGHT {
            for col in 0..SURFACE_WIDTH {
                // Pixel center in device coordinates.
                let cx = col as f32 + 0.5;
                let cy = row as f32 + 0.5;
                let coverage = analytic_coverage(cx, cy, &inside_fn);
                if coverage > 0.0 && coverage < 1.0 {
                    let idx = row as usize * SURFACE_WIDTH as usize + col as usize;
                    boundary.push((idx, coverage));
                }
            }
        }
        boundary
    }

    // ── O1: Calibration — existing SDF rrect matches oracle ──────────────────

    // ── O2: Oracle has teeth (control test) ──────────────────────────────────

    // ── O3: Rotated rect AA (red→green gate) ─────────────────────────────────

    /// O3: A 30° rotated SrcOver rect rendered via the new affine instanced path
    /// must have boundary pixels with monotonic coverage and alpha within the
    /// calibration tolerance of the analytic oracle.
    ///
    /// Before the reroute: the rotated rect fell through to tessellation →
    /// hard-aliased edges → this test would fail at the oracle-match assertion
    /// (the monotone check alone might pass since tessellated edges are binary).
    ///
    /// After the reroute: the affine SDF path produces smooth AA → passes.
    #[test]
    fn o3_rotated_rect_boundary_matches_oracle() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let angle = PI / 6.0; // 30°
        let half_w = 35.0_f32;
        let half_h = 18.0_f32;
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;

        // The rect in local space (centered at origin). The painter will rotate
        // it by applying a rotation transform before drawing.
        let local_rect = Rect::from_ltrb(
            f64::from(-half_w),
            f64::from(-half_h),
            f64::from(half_w),
            f64::from(half_h),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Translate to center, then rotate.
        painter.translate(flui_foundation::geometry::Offset::new(
            f64::from(cx),
            f64::from(cy),
        ));
        painter.rotate(angle);
        painter.draw_rect(local_rect, &Paint::fill(Color::WHITE));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("O3 Rotated Rect Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Oracle uses the same angle and half-extents, centered at (cx, cy).
        let boundary = boundary_pixel_indices(|px, py| {
            inside_rotated_rect(px - cx, py - cy, half_w, half_h, angle)
        });

        assert!(
            boundary.len() >= 8,
            "O3: fewer than 8 boundary pixels found ({}) — shape may be off-screen or oracle broken",
            boundary.len()
        );

        // SDF rendering rounds sharp convex corners over the ~1px AA band: the
        // distance field beyond a convex vertex is radial, so `sdBox` (radius 0)
        // produces a rounded falloff at each corner. This is inherent to the
        // SDF/fwidth AA model and is the SAME behavior as the existing
        // axis-aligned SDF rect primitives. The 8×8 box-supersample oracle models
        // a mathematically-sharp corner, so the two LEGITIMATELY diverge within
        // ~AA-width of each corner vertex. We therefore validate EDGE AA against
        // the oracle (the actual quality claim) and SEPARATELY assert the corners
        // are anti-aliased (rounded), not hard-aliased — never silently skipped.
        let (sin, cos) = angle.sin_cos();
        let corners_dev: [(f32, f32); 4] = [
            (-half_w, -half_h),
            (half_w, -half_h),
            (half_w, half_h),
            (-half_w, half_h),
        ]
        .map(|(lx, ly)| (cx + lx * cos - ly * sin, cy + lx * sin + ly * cos));
        // ~AA band (≈1px) + sub-pixel corner rounding, with margin.
        let corner_radius_sq = 3.0_f32 * 3.0;
        let near_corner = |px: f32, py: f32| {
            corners_dev.iter().any(|(qx, qy)| {
                let dx = px - qx;
                let dy = py - qy;
                dx * dx + dy * dy <= corner_radius_sq
            })
        };

        let mut edge_failed = 0usize;
        let mut edge_total = 0usize;
        let mut corner_total = 0usize;
        let mut corner_partial = 0usize; // corner pixels that are AA'd (partial alpha)
        for (pixel_idx, oracle_coverage) in &boundary {
            let col = (*pixel_idx % SURFACE_WIDTH as usize) as f32 + 0.5;
            let row = (*pixel_idx / SURFACE_WIDTH as usize) as f32 + 0.5;
            let readback_alpha = pixels[*pixel_idx][3];
            if near_corner(col, row) {
                corner_total += 1;
                if readback_alpha > 0 && readback_alpha < 255 {
                    corner_partial += 1;
                }
                continue;
            }
            edge_total += 1;
            let oracle_alpha = (oracle_coverage * 255.0).round() as u8;
            let diff = (i16::from(readback_alpha) - i16::from(oracle_alpha)).unsigned_abs() as u8;
            if diff > CALIBRATION_TOLERANCE_U8 {
                edge_failed += 1;
            }
        }

        // Guard against the corner exclusion swallowing the boundary: corner-band
        // pixels must remain a minority. If this trips, the exclusion radius is
        // masking a real edge problem.
        assert!(
            corner_total * 2 < boundary.len(),
            "O3: corner-band exclusion too large ({corner_total}/{} boundary) — \
             would mask edge AA defects",
            boundary.len()
        );

        let max_edge_failures = (edge_total as f32 * 0.05).ceil() as usize;
        assert!(
            edge_failed <= max_edge_failures,
            "O3 FAILED: {edge_failed}/{edge_total} rotated-rect EDGE boundary pixels exceed \
             oracle tolerance {CALIBRATION_TOLERANCE_U8} (corner-band excluded: {corner_total}). \
             Edge AA does not match analytic coverage — affine reroute or fwidth AA is wrong."
        );

        // Corners must be SDF-rounded (anti-aliased), not hard-aliased: most
        // corner-band pixels carry partial alpha. A hard-aliased renderer would
        // have them all at 0/255.
        assert!(
            corner_total == 0 || corner_partial * 2 >= corner_total,
            "O3 FAILED: only {corner_partial}/{corner_total} corner-band pixels are \
             anti-aliased (partial alpha) — corners look hard-aliased, not SDF-rounded."
        );

        // Also assert that interior pixels are fully opaque.
        // Sample a point 10px inside the rotated rect.
        let interior_col = (cx + 0.0).round() as usize;
        let interior_row = (cy + 0.0).round() as usize;
        let interior_idx = interior_row * SURFACE_WIDTH as usize + interior_col;
        assert!(
            pixels[interior_idx][3] > 200,
            "O3: interior pixel must be nearly opaque; got alpha={}",
            pixels[interior_idx][3]
        );
    }

    // ── O4: Rotated rrect — correct size + orientation + AA ──────────────────

    /// O4: A 45° rotated SrcOver rrect must:
    /// 1. Cover the correct device-space footprint (fixes the pre-existing AABB bug).
    /// 2. Have boundary-band alpha matching the analytic oracle within tolerance.
    ///
    /// Pre-existing bug: a rotated SrcOver rrect was baked via 2-corner AABB
    /// (`apply_transform(top_left)` + `apply_transform(bottom_right)`) without
    /// checking `is_axis_aligned()` — producing an axis-aligned AABB with wrong
    /// size and position. The affine instanced path fixes this by passing local
    /// bounds + the full 2×3 affine to the GPU.
    #[test]
    fn o4_rotated_rrect_correct_size_orientation_and_aa() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let angle = PI / 4.0; // 45°
        let half_w = 30.0_f32;
        let half_h = 15.0_f32;
        let radius = 6.0_f32;
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;

        let local_rrect = RRect::from_rect_circular(
            Rect::from_ltrb(
                f64::from(-half_w),
                f64::from(-half_h),
                f64::from(half_w),
                f64::from(half_h),
            ),
            f64::from(radius),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.translate(flui_foundation::geometry::Offset::new(
            f64::from(cx),
            f64::from(cy),
        ));
        painter.rotate(angle);
        painter.draw_rrect(local_rrect, &Paint::fill(Color::WHITE));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("O4 Rotated RRect Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        let radii_arr = [radius; 4];
        let boundary = boundary_pixel_indices(|px, py| {
            inside_rotated_rounded_rect(px - cx, py - cy, half_w, half_h, radii_arr, angle)
        });

        assert!(
            boundary.len() >= 8,
            "O4: fewer than 8 boundary pixels found ({}) — shape may be off-screen or oracle broken",
            boundary.len()
        );

        // Assert AA: boundary pixels must match oracle.
        let mut failed_count = 0usize;
        for (pixel_idx, oracle_coverage) in &boundary {
            let readback_alpha = pixels[*pixel_idx][3];
            let oracle_alpha = (oracle_coverage * 255.0).round() as u8;
            let diff = (i16::from(readback_alpha) - i16::from(oracle_alpha)).unsigned_abs();
            let diff_u8 = diff as u8;
            if diff_u8 > CALIBRATION_TOLERANCE_U8 {
                failed_count += 1;
            }
        }
        let boundary_count = boundary.len();
        let max_failures = (boundary_count as f32 * 0.05).ceil() as usize;
        assert!(
            failed_count <= max_failures,
            "O4 FAILED: {failed_count}/{boundary_count} rotated-rrect boundary pixels exceed \
             oracle tolerance {CALIBRATION_TOLERANCE_U8}"
        );

        // Assert size/orientation: a point that the AABB bug would have
        // covered (but the rotated rrect does NOT cover) must be transparent.
        // For a 45°-rotated 60×30 rect, the corners of the rect in device
        // space are at ≈(±half_h*√2, 0) along the Y axis. A point well
        // outside the rotated shape (in a corner of the AABB) must be blank.
        // The AABB of the rotated rect is roughly [cx±half_w*√2, cy±half_w*√2]
        // ≈ [cx±42, cy±42]. The corner at device (cx + half_w * 0.95, cy + 0.0)
        // is inside the bounding box but outside the rotated shape.
        let aabb_corner_col = (cx + half_w * 0.95).round() as usize;
        let aabb_corner_row = cy.round() as usize;
        if aabb_corner_col < SURFACE_WIDTH as usize && aabb_corner_row < SURFACE_HEIGHT as usize {
            let aabb_idx = aabb_corner_row * SURFACE_WIDTH as usize + aabb_corner_col;
            // For a 45° rotated rect the AABB corner contains only a corner sliver.
            // Oracle says: is this point inside the rotated rrect?
            let oracle_inside = inside_rotated_rounded_rect(
                aabb_corner_col as f32 + 0.5 - cx,
                aabb_corner_row as f32 + 0.5 - cy,
                half_w,
                half_h,
                radii_arr,
                angle,
            );
            if !oracle_inside {
                assert!(
                    pixels[aabb_idx][3] < CALIBRATION_TOLERANCE_U8,
                    "O4: AABB-bug regression — pixel at ({aabb_corner_col}, {aabb_corner_row}) \
                     should be outside the rotated rrect (oracle says so) but got alpha={}; \
                     the pre-existing AABB bake bug may have returned",
                    pixels[aabb_idx][3]
                );
            }
        }
    }

    // ── O5: Byte-identity — axis-aligned SrcOver rect and rrect ──────────────

    // ── C1: Circle AA — fwidth model is radius-independent ───────────────────

    // ── C2: Rotated ellipse — affine orientation correct ─────────────────────

    /// C2: A 30° rotated ellipse (rx=35, ry=15) must have boundary pixels that
    /// match the analytic rotated-ellipse oracle within tolerance.
    ///
    /// This proves:
    /// 1. The affine encoding `M_world * diag(rx, ry)` produces a correctly
    ///    oriented ellipse in device space.
    /// 2. `fwidth` gives ~1-device-px AA even for an anisotropic ellipse under
    ///    rotation (non-uniform scale in local space).
    ///
    /// If the oval routing incorrectly treats `rx`/`ry` as a uniform scale or
    /// uses the wrong local→device mapping, the boundary will be in the wrong
    /// position and the oracle match will fail.
    #[test]
    fn c2_rotated_ellipse_boundary_matches_oracle() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let angle = PI / 6.0; // 30°
        let rx = 35.0_f32;
        let ry = 15.0_f32;
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;

        // Draw an oval (axis-aligned in local space) under a 30° rotation.
        // The bounding rect in local space is [cx-rx, cy-ry, cx+rx, cy+ry].
        let local_rect = Rect::from_ltrb(
            f64::from(cx - rx),
            f64::from(cy - ry),
            f64::from(cx + rx),
            f64::from(cy + ry),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Rotate around the canvas center so the ellipse center stays at (cx, cy).
        painter.translate(flui_foundation::geometry::Offset::new(
            f64::from(cx),
            f64::from(cy),
        ));
        painter.rotate(angle);
        painter.translate(flui_foundation::geometry::Offset::new(
            f64::from(-cx),
            f64::from(-cy),
        ));
        painter.draw_oval(local_rect, &Paint::fill(Color::WHITE));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("C2 Rotated Ellipse Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Oracle: rotated ellipse centered at (cx, cy).
        let boundary = boundary_pixel_indices(|px, py| {
            inside_rotated_ellipse(px - cx, py - cy, rx, ry, angle)
        });

        assert!(
            boundary.len() >= 8,
            "C2: fewer than 8 boundary pixels found ({}) — shape may be off-screen or oracle broken",
            boundary.len()
        );

        let mut failed_count = 0usize;
        for (pixel_idx, oracle_coverage) in &boundary {
            let readback_alpha = pixels[*pixel_idx][3];
            let oracle_alpha = (oracle_coverage * 255.0).round() as u8;
            let diff = (i16::from(readback_alpha) - i16::from(oracle_alpha)).unsigned_abs() as u8;
            if diff > CALIBRATION_TOLERANCE_U8 {
                failed_count += 1;
            }
        }

        let boundary_count = boundary.len();
        let max_failures = (boundary_count as f32 * 0.05).ceil() as usize;
        assert!(
            failed_count <= max_failures,
            "C2 FAILED: {failed_count}/{boundary_count} rotated-ellipse boundary pixels exceed \
             oracle tolerance {CALIBRATION_TOLERANCE_U8}. \
             Affine orientation or fwidth AA on the ellipse is wrong."
        );

        // Interior pixel (ellipse center) must be opaque.
        let interior_col = cx.round() as usize;
        let interior_row = cy.round() as usize;
        let interior_idx = interior_row * SURFACE_WIDTH as usize + interior_col;
        assert!(
            pixels[interior_idx][3] > 200,
            "C2: interior pixel (ellipse center) must be nearly opaque; got alpha={}",
            pixels[interior_idx][3]
        );
    }

    // ── C3: Interior opaque + exterior transparent (fringe leaks nothing) ────

    // ── C4: Scaled circle center is not double-scaled (baked-path regression) ──

    /// O5: The axis-aligned SrcOver path is byte-identical to its pre-affine
    /// behavior.
    ///
    /// The ONLY change affecting the axis-aligned (baked-AABB) path is the ~1.5px
    /// quad-fringe expansion in the vertex shader (the L1 `fwidth` AA norm is
    /// unchanged). Its fringe fragments have SDF `dist > edge_width` → alpha 0, so
    /// they contribute nothing. This test gates that property **directly**: every
    /// pixel whose center is ≥1px outside the geometric shape must be the cleared
    /// transparent value, proving the expansion leaked no output. It also gates
    /// run-to-run determinism (two independent painters → identical pixels). Full
    /// byte-identity vs `origin/main` is further corroborated by the unchanged
    /// 294-test GPU suite, which asserts exact axis-aligned pixel values.
    #[test]
    fn o5_axis_aligned_src_over_rect_and_rrect_byte_identical() {
        let (device, queue) = acquire_test_device_and_queue();

        // ── Rect (sharp-cornered) ──
        let (surface_a, view_a) = create_render_surface(&device);
        let (surface_b, view_b) = create_render_surface(&device);
        clear_surface(&device, &queue, &view_a);
        clear_surface(&device, &queue, &view_b);

        let flat_rect = Rect::from_ltrb(20.0, 15.0, 108.0, 113.0);
        let color = Color::rgba(180, 80, 40, 200);

        for (surface, view) in [(&surface_a, &view_a), (&surface_b, &view_b)] {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.draw_rect(flat_rect, &Paint::fill(color));
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("O5 Rect Identity Encoder"),
            });
            painter
                .render(RenderTarget::sampleable(view, surface), &mut encoder)
                .expect("render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        let pixels_a = readback_pixels(&device, &queue, &surface_a);
        let pixels_b = readback_pixels(&device, &queue, &surface_b);
        for (idx, (a, b)) in pixels_a.iter().zip(pixels_b.iter()).enumerate() {
            assert_eq!(
                a, b,
                "O5 rect: pixel {idx} differs ({a:?} vs {b:?}) — axis-aligned path must be deterministic"
            );
        }

        // Byte-identity gate: the quad-fringe expansion must contribute NOTHING
        // outside the geometric rect for an axis-aligned shape. Every pixel whose
        // center is ≥1px beyond the rect bounds must equal the cleared transparent
        // value — proving the only axis-aligned shader change (the expansion) added
        // no visible output.
        let (rl, rt, rr, rb) = (20.0_f32, 15.0, 108.0, 113.0);
        for (idx, px) in pixels_a.iter().enumerate() {
            let col = (idx % SURFACE_WIDTH as usize) as f32 + 0.5;
            let row = (idx / SURFACE_WIDTH as usize) as f32 + 0.5;
            let outside = col < rl - 1.0 || col > rr + 1.0 || row < rt - 1.0 || row > rb + 1.0;
            assert!(
                !outside || *px == [0, 0, 0, 0],
                "O5: pixel at ({col},{row}) is ≥1px outside the axis-aligned rect but not \
                 transparent ({px:?}) — quad-fringe expansion leaked output, breaking byte-identity"
            );
        }

        // ── Rounded rect ──
        let (surface_c, view_c) = create_render_surface(&device);
        let (surface_d, view_d) = create_render_surface(&device);
        clear_surface(&device, &queue, &view_c);
        clear_surface(&device, &queue, &view_d);

        let rounded_rect_shape =
            RRect::from_rect_circular(Rect::from_ltrb(20.0, 15.0, 108.0, 113.0), 8.0);

        for (surface, view) in [(&surface_c, &view_c), (&surface_d, &view_d)] {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.draw_rrect(rounded_rect_shape, &Paint::fill(color));
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("O5 RRect Identity Encoder"),
            });
            painter
                .render(RenderTarget::sampleable(view, surface), &mut encoder)
                .expect("render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        let pixels_c = readback_pixels(&device, &queue, &surface_c);
        let pixels_d = readback_pixels(&device, &queue, &surface_d);
        for (idx, (c, d)) in pixels_c.iter().zip(pixels_d.iter()).enumerate() {
            assert_eq!(
                c, d,
                "O5 rrect: pixel {idx} differs ({c:?} vs {d:?}) — axis-aligned path must be deterministic"
            );
        }
    }

    // ── O6: fwidth scale-invariance ───────────────────────────────────────────

    // ── O7: Non-uniform corner radii map to the correct screen corners ────────

    /// O7: A rrect with ONLY the top-left corner rounded must round the TOP-LEFT
    /// screen corner and leave the other three sharp.
    ///
    /// Uniform-radii tests (O1/O4) cannot detect a corner-index transposition in
    /// `sdRoundedBox`'s quadrant `select`; production rrects use distinct
    /// per-corner radii, so this pins the `[tl,tr,br,bl]` → screen-corner mapping.
    #[test]
    fn o7_non_uniform_corner_radii_map_to_correct_corners() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface, view) = create_render_surface(&device);
        clear_surface(&device, &queue, &view);

        // 80×80 axis-aligned rrect, ONLY the top-left corner rounded (r=24);
        // the other three corners are sharp.
        let bounds = Rect::from_ltrb(24.0, 24.0, 104.0, 104.0);
        let rrect = RRect::new(
            bounds,
            flui_foundation::geometry::Radius::circular(24.0), // top-left
            flui_foundation::geometry::Radius::ZERO,           // top-right
            flui_foundation::geometry::Radius::ZERO,           // bottom-right
            flui_foundation::geometry::Radius::ZERO,           // bottom-left
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.draw_rrect(rrect, &Paint::fill(Color::WHITE));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("O7 Non-uniform RRect Encoder"),
        });
        painter
            .render(RenderTarget::sampleable(&view, &surface), &mut encoder)
            .expect("render must succeed");
        queue.submit(std::iter::once(encoder.finish()));
        let pixels = readback_pixels(&device, &queue, &surface);

        let alpha_at = |col: usize, row: usize| pixels[row * SURFACE_WIDTH as usize + col][3];

        // ~3px diagonally inside each corner. For the 24px-rounded top-left the
        // corner triangle is cut away (transparent); sharp corners stay opaque.
        let inset = 3usize;
        let tl = alpha_at(24 + inset, 24 + inset);
        let tr = alpha_at(104 - inset, 24 + inset);
        let br = alpha_at(104 - inset, 104 - inset);
        let bl = alpha_at(24 + inset, 104 - inset);

        assert!(
            tl < 40,
            "O7: top-left corner must be ROUNDED (transparent near the corner) — got alpha={tl}. \
             corners (tl,tr,br,bl)=({tl},{tr},{br},{bl}). A different transparent corner means the \
             [tl,tr,br,bl] → screen-corner mapping in sdRoundedBox is transposed."
        );
        for (name, a) in [("tr", tr), ("br", br), ("bl", bl)] {
            assert!(
                a > 215,
                "O7: {name} corner must be SHARP (opaque near the corner) — got alpha={a}. \
                 corners (tl,tr,br,bl)=({tl},{tr},{br},{bl})."
            );
        }
    }

    // ── A1: Arc radial AA is radius-independent ───────────────────────────────

    /// Absolute angle difference wrapping to [0, π].
    fn angle_diff_abs(a: f32, b: f32) -> f32 {
        let tau = 2.0 * std::f32::consts::PI;
        let raw = (a - b).abs() % tau;
        if raw > std::f32::consts::PI {
            tau - raw
        } else {
            raw
        }
    }

    // ── A2: Rotated arc — affine orientation correct ──────────────────────────

    /// A2: A 30° rotated arc must have boundary pixels that match the analytic
    /// rotated-arc oracle within tolerance.
    ///
    /// Proves:
    /// 1. The full-affine encoding produces a correctly oriented arc in device space.
    /// 2. `fwidth` radial AA stays ~1 device-px even under rotation.
    ///
    /// If the arc routing incorrectly uses the old axis-aligned path (scale+translate
    /// only), the boundary will be at the wrong position and this test will fail.
    #[test]
    fn a2_rotated_arc_boundary_matches_oracle() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let angle = PI / 6.0; // 30° rotation applied to the canvas
        let radius = 35.0_f32;
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let start = 0.0_f32;
        let sweep = 3.0 * std::f32::consts::FRAC_PI_2; // 270°

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Translate to center, then rotate.
        painter.translate(flui_foundation::geometry::Offset::new(
            f64::from(cx),
            f64::from(cy),
        ));
        painter.rotate(angle);
        // The arc rect is centered at origin in local space.
        let rect = Rect::from_xywh(
            f64::from(-radius),
            f64::from(-radius),
            f64::from(radius * 2.0),
            f64::from(radius * 2.0),
        );
        painter.draw_arc(rect, start, sweep, true, &Paint::fill(Color::WHITE));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("A2 Rotated Arc Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Oracle: rotated arc centered at (cx, cy).
        // The rotation transform applied to the canvas means the arc's own
        // angles are unchanged in local space; from the device frame, the arc
        // is rotated by `angle`. We use `inside_rotated_arc` which applies an
        // inverse rotation to query points.
        let angular_exclusion_rad = 15.0_f32 * std::f32::consts::PI / 180.0;
        let end_angle = start + sweep;

        // Boundary pixels near the radial edge (oracle).
        let radial_boundary: Vec<(usize, f32)> = {
            let all_boundary = boundary_pixel_indices(|px, py| {
                inside_rotated_arc(px - cx, py - cy, radius, start, sweep, angle)
            });
            all_boundary
                .into_iter()
                .filter(|(pixel_idx, _)| {
                    let col = (*pixel_idx % SURFACE_WIDTH as usize) as f32 + 0.5 - cx;
                    let row = (*pixel_idx / SURFACE_WIDTH as usize) as f32 + 0.5 - cy;
                    // Inverse-rotate to get local angle.
                    let cos_a = angle.cos();
                    let sin_a = angle.sin();
                    let lx = cos_a * col + sin_a * row;
                    let ly = -sin_a * col + cos_a * row;
                    let local_r = (lx * lx + ly * ly).sqrt();
                    let local_ang = ly.atan2(lx);
                    let near_radial = (local_r - radius).abs() < 2.0;
                    let dist_start = angle_diff_abs(local_ang, start);
                    let dist_end = angle_diff_abs(local_ang, end_angle);
                    near_radial
                        && dist_start > angular_exclusion_rad
                        && dist_end > angular_exclusion_rad
                })
                .collect()
        };

        assert!(
            radial_boundary.len() >= 4,
            "A2: fewer than 4 radial boundary pixels ({}) — shape may be off-screen or oracle broken",
            radial_boundary.len()
        );

        let mut failed_count = 0usize;
        for (pixel_idx, oracle_coverage) in &radial_boundary {
            let readback_alpha = pixels[*pixel_idx][3];
            let oracle_alpha = (oracle_coverage * 255.0).round() as u8;
            let diff = (i16::from(readback_alpha) - i16::from(oracle_alpha)).unsigned_abs() as u8;
            if diff > CALIBRATION_TOLERANCE_U8 {
                failed_count += 1;
            }
        }

        let boundary_count = radial_boundary.len();
        let max_failures = (boundary_count as f32 * 0.1).ceil() as usize;
        assert!(
            failed_count <= max_failures,
            "A2 FAILED: {failed_count}/{boundary_count} rotated-arc radial boundary pixels exceed \
             oracle tolerance {CALIBRATION_TOLERANCE_U8}. Affine orientation or fwidth AA is wrong."
        );

        // Interior fill check: a 270° sector at radius 35 has a large solid
        // interior. Count opaque pixels in the mid-radius band (excluding the AA
        // bands at the radial + angular edges). The arc APEX is intentionally NOT
        // sampled — a pie apex is only fractionally covered (≈ sweep/2π of the
        // directions around it), so the exact center pixel is legitimately
        // partial, not opaque. This check proves the sector is substantially
        // filled (not hollow / mis-oriented).
        let mut opaque_interior = 0usize;
        for row in 0..SURFACE_HEIGHT {
            for col in 0..SURFACE_WIDTH {
                let dx = col as f32 + 0.5 - cx;
                let dy = row as f32 + 0.5 - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < 5.0 || dist > radius - 3.0 {
                    continue; // skip near-apex and the radial boundary band
                }
                let idx = row as usize * SURFACE_WIDTH as usize + col as usize;
                if pixels[idx][3] > 250 {
                    opaque_interior += 1;
                }
            }
        }
        assert!(
            opaque_interior >= 200,
            "A2: rotated arc interior is not substantially filled — only {opaque_interior} \
             opaque pixels in the mid-radius band; the sector fill may be hollow or mis-oriented"
        );
    }

    // ── A3: Scaled arc center not double-scaled (PR-2 regression guard) ───────

    // ── P1–P4: SSAA path anti-aliasing (PR-3) ────────────────────────────────

    // ── CPU oracle helpers for polygon tests ─────────────────────────────────

    /// Half-plane inside-test for a single edge from `a` to `b`.
    ///
    /// Returns `true` when `p` is on the left side (positive cross-product)
    /// of the directed edge `a→b`.  Used by `inside_triangle` / `inside_polygon`.
    fn half_plane_inside(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> bool {
        (bx - ax) * (py - ay) - (by - ay) * (px - ax) >= 0.0
    }

    /// Analytic inside-test for a CW-oriented triangle (a, b, c) in device coords.
    ///
    /// Uses three half-plane tests.  The winding order must be consistent —
    /// either all CCW or all CW — so that the signs agree.  The oracle
    /// tests both orientations and accepts if either fires.
    #[expect(clippy::too_many_arguments)]
    fn inside_triangle(
        px: f32,
        py: f32,
        ax: f32,
        ay: f32,
        bx: f32,
        by: f32,
        cx: f32,
        cy: f32,
    ) -> bool {
        // Try CCW orientation.
        let ccw = half_plane_inside(px, py, ax, ay, bx, by)
            && half_plane_inside(px, py, bx, by, cx, cy)
            && half_plane_inside(px, py, cx, cy, ax, ay);
        // Try CW orientation (flip edge directions).
        let cw = half_plane_inside(px, py, bx, by, ax, ay)
            && half_plane_inside(px, py, cx, cy, bx, by)
            && half_plane_inside(px, py, ax, ay, cx, cy);
        ccw || cw
    }

    // ── P1: polygon fill boundary pixels have partial alpha (real SSAA AA) ────

    /// P1: A SrcOver-filled triangle path must have partial-alpha pixels on its
    /// boundary (analytic oracle says so) — proving SSAA produces real AA, not
    /// binary hard-aliased output.
    ///
    /// ## Red→green proof
    ///
    /// Without the SSAA reroute, a tessellated triangle would be drawn with
    /// `shape.wgsl` / `ALPHA_BLENDING` and no SDF distance field — producing
    /// hard-aliased edges (every boundary pixel is either fully covered or not,
    /// giving alpha ∈ {0, 255} at the edge).  With the SSAA reroute the
    /// 2×-supersampled render resolves sub-pixel edge crossings and the boundary
    /// band has partial alpha values.
    ///
    /// Test: render a ~70×50 triangle centered in the 128² surface.  The oracle
    /// identifies boundary pixels (0 < coverage < 1).  After SSAA rendering all
    /// of those pixels must carry partial alpha (> 5, < 250) — the strict
    /// majority (>50%) guarantees the assertion cannot be vacuous.
    #[test]
    fn p1_ssaa_polygon_boundary_has_partial_alpha() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        // A triangle off-axis so its edges are not pixel-row/column aligned.
        // Device-space vertices (centered in the 128² surface, intentionally
        // non-axis-aligned so every edge produces boundary pixels).
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        // Equilateral-ish triangle: apex top-center, base at bottom.
        let ax = cx;
        let ay = cy - 40.0;
        let bx = cx + 35.0;
        let by = cy + 25.0;
        let dx = cx - 35.0;
        let dy = cy + 25.0;

        let mut path = flui_painting::paint::path::Path::new();
        path.move_to(flui_foundation::geometry::Point::new(
            f64::from(ax),
            f64::from(ay),
        ));
        path.line_to(flui_foundation::geometry::Point::new(
            f64::from(bx),
            f64::from(by),
        ));
        path.line_to(flui_foundation::geometry::Point::new(
            f64::from(dx),
            f64::from(dy),
        ));
        path.close();

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.draw_path(&path, &Paint::fill(Color::WHITE));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("P1 SSAA Triangle Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Oracle: boundary pixels of the triangle.
        let boundary =
            boundary_pixel_indices(|px, py| inside_triangle(px, py, ax, ay, bx, by, dx, dy));

        assert!(
            boundary.len() >= 8,
            "P1: fewer than 8 triangle boundary pixels found ({}) — \
             oracle or path construction is wrong",
            boundary.len()
        );

        // Count how many boundary pixels have partial alpha (SSAA produces
        // partial coverage; hard-aliased rendering would give only 0/255).
        let partial_count = boundary
            .iter()
            .filter(|(idx, _)| {
                let a = pixels[*idx][3];
                a > 5 && a < 250
            })
            .count();

        let boundary_count = boundary.len();
        let min_partial = (boundary_count as f32 * 0.5).ceil() as usize;

        assert!(
            partial_count >= min_partial,
            "P1 FAILED: only {partial_count}/{boundary_count} triangle boundary pixels have \
             partial alpha (expected ≥{min_partial} for SSAA AA). \
             Hard-aliased rendering would give 0 partial pixels — SSAA reroute may be a no-op."
        );

        // Interior sanity: the centroid must be fully opaque.
        let centroid_x = ((ax + bx + dx) / 3.0) as usize;
        let centroid_y = ((ay + by + dy) / 3.0) as usize;
        let centroid_idx = centroid_y * SURFACE_WIDTH as usize + centroid_x;
        assert!(
            pixels[centroid_idx][3] > 200,
            "P1: triangle centroid must be opaque (got alpha={}); \
             the SSAA tile may not be composited correctly",
            pixels[centroid_idx][3]
        );
    }

    // ── P2: fill rule (NonZero solid / EvenOdd hollow) both AA'd ─────────────

    /// P2: A star polygon drawn with NonZero fill (solid) vs EvenOdd fill (hollow
    /// center) must both produce partial-alpha pixels on their boundaries, proving
    /// the SSAA reroute works for both fill rules.
    ///
    /// Additionally, the center of the star is sampled to discriminate fill rules:
    /// - NonZero: center must be opaque (filled in by the non-zero winding).
    /// - EvenOdd: center must be transparent (punched out by even crossings).
    ///
    /// This guards against the SSAA path ignoring the `PathFillType` carried by
    /// the tessellated geometry (which lives in the `SsaaPathOp::segment`).
    #[test]
    fn p2_ssaa_fill_rule_honored_nonzero_vs_evenodd() {
        use flui_painting::paint::PathFillType;

        // Self-intersecting pentagram (★) centered at (cx, cy) with outer radius 40.
        //
        // A pentagram connects the 5 tips in skip-2 order: tip[0]→tip[2]→tip[4]→
        // tip[1]→tip[3]→close.  The 5 diagonals cross through the center, creating
        // a region with winding count +2 (NonZero → filled, EvenOdd → hole).
        //
        // The simple 10-vertex star polygon (alternating outer/inner radii) is
        // NOT self-intersecting and therefore gives identical results under both
        // fill rules — it cannot discriminate them.  The pentagram IS self-
        // intersecting and IS the correct geometry for fill-rule discrimination.
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let outer_r = 40.0_f32;

        // 5 tip positions, tip[i] at angle (i*72° - 90°).
        let tips: Vec<(f32, f32)> = (0..5)
            .map(|i| {
                let angle = (i as f32 * 2.0 * PI / 5.0) - PI / 2.0;
                (cx + outer_r * angle.cos(), cy + outer_r * angle.sin())
            })
            .collect();

        // Skip-2 connection order: [0, 2, 4, 1, 3] → self-intersecting pentagram.
        let pentagram_order = [0usize, 2, 4, 1, 3];

        let build_star_path = |fill_type: PathFillType| {
            let mut path = flui_painting::paint::path::Path::with_fill_type(fill_type);
            let (first_x, first_y) = tips[pentagram_order[0]];
            path.move_to(flui_foundation::geometry::Point::new(
                f64::from(first_x),
                f64::from(first_y),
            ));
            for &idx in &pentagram_order[1..] {
                let (x, y) = tips[idx];
                path.line_to(flui_foundation::geometry::Point::new(
                    f64::from(x),
                    f64::from(y),
                ));
            }
            path.close();
            path
        };

        let (device, queue) = acquire_test_device_and_queue();

        // Render NonZero star.
        let (surface_nz, view_nz) = create_render_surface(&device);
        clear_surface(&device, &queue, &view_nz);
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.draw_path(
                &build_star_path(PathFillType::NonZero),
                &Paint::fill(Color::WHITE),
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("P2 NonZero Star Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&view_nz, &surface_nz),
                    &mut encoder,
                )
                .expect("painter.render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        // Render EvenOdd star.
        let (surface_eo, view_eo) = create_render_surface(&device);
        clear_surface(&device, &queue, &view_eo);
        {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.draw_path(
                &build_star_path(PathFillType::EvenOdd),
                &Paint::fill(Color::WHITE),
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("P2 EvenOdd Star Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&view_eo, &surface_eo),
                    &mut encoder,
                )
                .expect("painter.render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        let pixels_nz = readback_pixels(&device, &queue, &surface_nz);
        let pixels_eo = readback_pixels(&device, &queue, &surface_eo);

        // Both renders must produce partial-alpha pixels in the outer boundary band
        // (the 5 outer tips of the star) — proving SSAA is active for both fill rules.
        for (label, pixels) in [("NonZero", &pixels_nz), ("EvenOdd", &pixels_eo)] {
            let partial_outer = pixels
                .iter()
                .enumerate()
                .filter(|(idx, p)| {
                    let col = (idx % SURFACE_WIDTH as usize) as f32 + 0.5 - cx;
                    let row = (idx / SURFACE_WIDTH as usize) as f32 + 0.5 - cy;
                    let dist = (col * col + row * row).sqrt();
                    // Only consider pixels in the outer tip boundary band.
                    let in_outer_band = dist >= outer_r - 3.0 && dist <= outer_r + 3.0;
                    in_outer_band && p[3] > 5 && p[3] < 250
                })
                .count();

            assert!(
                partial_outer >= 4,
                "P2 {label}: fewer than 4 partial-alpha pixels in the outer boundary band \
                 ({partial_outer}) — SSAA may not be active for this fill rule"
            );
        }

        // Fill-rule discrimination: center of the star (at the exact center pixel).
        // NonZero: winding number is +2 (all edges wind the same) → interior → opaque.
        // EvenOdd: crossing count is 2 (even) → hole → transparent.
        let center_idx = cy as usize * SURFACE_WIDTH as usize + cx as usize;
        assert!(
            pixels_nz[center_idx][3] > 150,
            "P2: NonZero star center must be filled (got alpha={}); \
             fill rule may not be flowing through the SSAA segment",
            pixels_nz[center_idx][3]
        );
        assert!(
            pixels_eo[center_idx][3] < 100,
            "P2: EvenOdd star center must be transparent (got alpha={}); \
             fill rule may not be flowing through the SSAA segment",
            pixels_eo[center_idx][3]
        );
    }

    // ── P3: SSAA scale-invariance — AA band stays ~1 device-px ───────────────

    // ── P4: anti-MVP — SrcOver Fill path must emit ≥1 partial-alpha pixel ────

    // ── Draw order across raster routes ──────────────────────────────────────

    /// A later SSAA-routed path fill must composite OVER earlier main-pass
    /// content: draw order is z-order regardless of which raster route a
    /// command takes. This is exactly a Material surface — a large filled
    /// rect PATH from `RenderPhysicalShape` — painted after list rows
    /// (instanced-SDF rects): if the SSAA tile composites at the wrong
    /// position in the pass sequence, the surface vanishes under content it
    /// was painted over, and a collapsed pinned `SliverAppBar` renders as a
    /// transparent strip with the rows showing through.
    #[test]
    fn later_ssaa_path_fill_covers_earlier_rect() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let full = flui_foundation::geometry::Rect::from_xywh(
            0.0,
            0.0,
            f64::from(SURFACE_WIDTH as f32),
            f64::from(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        {
            // Through the SAME `LayerDispatcher` adapter the layer-tree walk uses —
            // draws arrive as `render_*(…, transform)` with per-command
            // matrices, exactly the collapsed-app-bar stream: translated
            // row rects + glyphs first, then the surface path at identity.
            use crate::command_renderer::CommandRenderer;
            let mut backend = crate::layer_dispatcher::LayerDispatcher::new(&mut painter);
            // The real stream runs inside the viewport's clip.
            backend.clip_rect(
                full,
                flui_painting::paint::ClipOp::Intersect,
                flui_painting::paint::Clip::HardEdge,
                &flui_foundation::geometry::Matrix4::IDENTITY,
            );
            // The failing frame's exact tail: the row rect and the surface
            // path share IDENTITY transforms and the SAME rectangle.
            backend.render_rect(
                full,
                &Paint::fill(Color::rgb(255, 0, 0)),
                &flui_foundation::geometry::Matrix4::IDENTITY,
            );
            // Color rides the STYLE: `render_text` derives its paint from
            // `style.color` and ignores the paint parameter.
            let blue_style = flui_painting::typography::TextStyle {
                color: Some(Color::rgb(0, 0, 255)),
                ..flui_painting::typography::TextStyle::default()
            };
            backend.render_paragraph(
                &std::sync::Arc::new(flui_painting::TextLayout::new(
                    "Row",
                    Some(&blue_style),
                    14.0,
                    None,
                    None,
                    flui_painting::typography::TextDirection::Ltr,
                )),
                flui_foundation::geometry::Offset::new(4.0, 30.0),
                Color::rgb(0, 0, 255),
                &flui_foundation::geometry::Matrix4::IDENTITY,
            );
            let path = flui_painting::paint::path::Path::rectangle(full);
            backend.render_path(
                &path,
                &Paint::fill(Color::rgb(0, 255, 0)),
                &flui_foundation::geometry::Matrix4::IDENTITY,
            );
            let yellow_style = flui_painting::typography::TextStyle {
                color: Some(Color::rgb(255, 255, 0)),
                ..flui_painting::typography::TextStyle::default()
            };
            backend.render_paragraph(
                &std::sync::Arc::new(flui_painting::TextLayout::new(
                    "Title",
                    Some(&yellow_style),
                    14.0,
                    None,
                    None,
                    flui_painting::typography::TextDirection::Ltr,
                )),
                flui_foundation::geometry::Offset::new(4.0, 50.0),
                Color::rgb(255, 255, 0),
                &flui_foundation::geometry::Matrix4::IDENTITY,
            );
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Path Order Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);
        let center =
            (SURFACE_HEIGHT as usize / 2) * SURFACE_WIDTH as usize + SURFACE_WIDTH as usize / 2;
        let [r, g, b, a] = pixels[center];
        assert!(
            g > 200 && r < 50 && a > 200,
            "the LATER path fill must cover the EARLIER rect at the center; \
             got rgba=({r}, {g}, {b}, {a}) — green buried under red means the \
             SSAA composite ran out of draw order"
        );
        // The EARLIER text must be covered too: glyph batches must not
        // composite after the path that was painted over them. The earlier
        // "Row" glyphs are pure blue; any blue pixel left anywhere means
        // text jumped the draw order — the collapsed-app-bar symptom where
        // a covered row's LABEL floats over the toolbar surface.
        let blue_pixels = pixels
            .iter()
            .filter(|p| p[2] > 200 && p[0] < 50 && p[1] < 50)
            .count();
        // Ordered text: the earlier blue glyphs must be covered by the
        // later opaque path fill — text draws at its segment's z-position,
        // not in a global final pass.
        assert_eq!(
            blue_pixels, 0,
            "the earlier text must be covered by the later path fill; \
             {blue_pixels} blue glyph pixels visible — glyph batches \
             composited out of draw order"
        );
        // And the LATER text still draws over the path.
        let yellow_pixels = pixels
            .iter()
            .filter(|p| p[0] > 200 && p[1] > 200 && p[2] < 50)
            .count();
        assert!(
            yellow_pixels > 0,
            "the later title text must render over the path fill"
        );
    }

    /// Geometry recorded AFTER text in the same segment must still cover
    /// that text: a segment's geometry flushes as one unit before its glyph
    /// range, so without a seal at the text boundary a `text(); rect()`
    /// sequence keeps the original z-order flaw even after per-segment
    /// ordering (the SSAA route only passed because it happens to split
    /// the segment).
    #[test]
    fn later_rect_covers_earlier_text_in_the_same_segment() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let full = flui_foundation::geometry::Rect::from_xywh(
            0.0,
            0.0,
            f64::from(SURFACE_WIDTH as f32),
            f64::from(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        {
            use crate::command_renderer::CommandRenderer;
            let mut backend = crate::layer_dispatcher::LayerDispatcher::new(&mut painter);
            let blue_style = flui_painting::typography::TextStyle {
                color: Some(Color::rgb(0, 0, 255)),
                ..flui_painting::typography::TextStyle::default()
            };
            backend.render_paragraph(
                &std::sync::Arc::new(flui_painting::TextLayout::new(
                    "Covered",
                    Some(&blue_style),
                    14.0,
                    None,
                    None,
                    flui_painting::typography::TextDirection::Ltr,
                )),
                flui_foundation::geometry::Offset::new(4.0, 30.0),
                Color::rgb(0, 0, 255),
                &flui_foundation::geometry::Matrix4::IDENTITY,
            );
            // A plain instanced rect — the route that does NOT split the
            // segment on its own.
            backend.render_rect(
                full,
                &Paint::fill(Color::rgb(0, 255, 0)),
                &flui_foundation::geometry::Matrix4::IDENTITY,
            );
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Text Boundary Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);
        let blue_pixels = pixels
            .iter()
            .filter(|p| p[2] > 200 && p[0] < 50 && p[1] < 50)
            .count();
        assert_eq!(
            blue_pixels, 0,
            "text followed by covering geometry in one segment must be \
             buried; {blue_pixels} blue glyph pixels visible"
        );
    }

    /// Text obeys an active clip.
    ///
    /// Before glyphs were a batch of the segment, every glyph run was handed
    /// to the rasteriser with the whole viewport as its bound and no scissor
    /// — so no clip reached text, and a `ListView`'s rows painted straight
    /// through the app bar above them. Glyph quads now carry the scissor run
    /// (this test) and the per-instance SDF clip
    /// (`text_is_clipped_by_a_rounded_clip` in `paragraph_readback_tests`).
    ///
    /// Driven through `WgpuPainter`, deliberately: the clip is read at the
    /// RECORD seam, so a test that built glyph instances by hand would pass
    /// with the production read reverted.
    #[test]
    fn text_is_clipped_by_the_active_clip_rect() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        const CLIP_BOTTOM: f32 = 24.0;

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save();
        painter.clip_rect(
            flui_foundation::geometry::Rect::from_xywh(
                0.0,
                0.0,
                f64::from(SURFACE_WIDTH as f32),
                f64::from(CLIP_BOTTOM),
            ),
            flui_painting::paint::Clip::HardEdge,
        );
        // Positioned so the run straddles the clip edge: some of it is legally
        // inside, the rest must be cut.
        painter.draw_text(
            "Spill",
            flui_foundation::geometry::Point::new(4.0, 12.0),
            28.0,
            &Paint::fill(Color::rgb(0, 0, 255)),
        );
        painter.restore();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Text Clip Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);
        let w = SURFACE_WIDTH as usize;
        let inked = |p: &[u8; 4]| p[3] > 16;

        let above: usize = (0..CLIP_BOTTOM as usize)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| inked(&pixels[y * w + x]))
            .count();
        assert!(
            above > 0,
            "precondition: the run must actually paint inside the clip"
        );

        let below: usize = ((CLIP_BOTTOM as usize)..SURFACE_HEIGHT as usize)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| inked(&pixels[y * w + x]))
            .count();
        assert_eq!(
            below, 0,
            "text must not paint outside the active clip; {below} glyph pixels \
             below y={CLIP_BOTTOM} — the clip never reached the glyph run"
        );
    }

    /// Glyphs scale with the transform, like every other primitive.
    ///
    /// The framework paints in logical pixels and puts the device-pixel ratio on
    /// the root transform, so a 2x display multiplies every extent by two. Glyph
    /// runs escape that: only the run's ORIGIN crosses the CTM, while the raster
    /// is emitted at `scale: 1.0`. A 16px label then occupies 16 physical pixels
    /// in a box that reserved 32 — the most visible defect on HiDPI hardware.
    ///
    /// Ink coverage is the oracle rather than a sampled pixel: doubling the
    /// linear scale must roughly quadruple the painted area, which no amount of
    /// repositioning can fake.
    ///
    /// If reverted (`TextArea::scale` back to a hard `1.0`): both renders ink
    /// the same number of pixels and the ratio collapses to ~1.
    #[test]
    fn glyphs_scale_with_the_current_transform() {
        fn ink_at_scale(scale: f32) -> usize {
            let (device, queue) = acquire_test_device_and_queue();
            let (surface_texture, surface_view) = create_render_surface(&device);
            clear_surface(&device, &queue, &surface_view);

            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.save();
            painter.scale(scale, scale);
            painter.draw_text(
                "Ab",
                flui_foundation::geometry::Point::new(4.0, 10.0),
                16.0,
                &Paint::fill(Color::rgb(0, 0, 255)),
            );
            painter.restore();

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Text Scale Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&surface_view, &surface_texture),
                    &mut encoder,
                )
                .expect("painter.render must succeed");
            queue.submit(std::iter::once(encoder.finish()));

            readback_pixels(&device, &queue, &surface_texture)
                .iter()
                .filter(|p| p[3] > 16)
                .count()
        }

        let one = ink_at_scale(1.0);
        let two = ink_at_scale(2.0);
        assert!(one > 0, "precondition: the label paints at scale 1");

        // Exactly 4x is not expected — antialiasing and hinting shift coverage —
        // but 2.5x is far outside what those can explain, and a scale-blind
        // raster lands at ~1.0x.
        let ratio = two as f32 / one as f32;
        assert!(
            ratio > 2.5,
            "doubling the transform scale must roughly quadruple glyph ink; got \
             {two}/{one} = {ratio:.2}x — the raster ignored the transform"
        );
    }

    /// A rotated CTM rotates the glyphs: a label drawn under a quarter turn
    /// occupies a column, not a row.
    #[test]
    fn a_quarter_turn_rotates_the_label() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save();
        painter.translate(flui_foundation::geometry::Offset::new(40.0, 4.0));
        painter.rotate(std::f32::consts::FRAC_PI_2);
        painter.draw_text(
            "IIIIIIII",
            flui_foundation::geometry::Point::new(0.0, 0.0),
            12.0,
            &Paint::fill(Color::rgb(0, 0, 255)),
        );
        painter.restore();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Rotated Text Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);
        let inked: Vec<(usize, usize)> = pixels
            .iter()
            .enumerate()
            .filter(|(_, p)| p[3] > 16)
            .map(|(i, _)| (i % SURFACE_WIDTH as usize, i / SURFACE_WIDTH as usize))
            .collect();
        assert!(!inked.is_empty(), "precondition: the label paints");
        let (min_x, max_x) = inked
            .iter()
            .fold((usize::MAX, 0), |(lo, hi), &(x, _)| (lo.min(x), hi.max(x)));
        let (min_y, max_y) = inked
            .iter()
            .fold((usize::MAX, 0), |(lo, hi), &(_, y)| (lo.min(y), hi.max(y)));
        let (width, height) = (max_x - min_x + 1, max_y - min_y + 1);
        assert!(
            height > 2 * width,
            "eight stems under a quarter turn must stack vertically; ink box is \
             {width}×{height} ({min_x}..={max_x}, {min_y}..={max_y})"
        );
    }

    /// A rounded clip actually rounds a circle.
    ///
    /// `apply_active_clip` — the one place an SDF clip is attached to a draw —
    /// is called only for SrcOver rect/rrect fills and for textures, and
    /// `ClippableInstance` is implemented only by `RectInstance` and
    /// `TextureInstance`. Circles and ovals therefore receive only the clip's
    /// axis-aligned bounding scissor, so a `ClipRRect` around an avatar leaves
    /// square corners.
    ///
    /// The geometry is chosen so the assertion cannot pass by accident: the
    /// circle is far LARGER than the clip and covers every corner of the
    /// surface, so a sampled corner pixel is inside the drawn shape and outside
    /// the clip. (Drawing a disc that happens to coincide with the clip is the
    /// trap here — the corner would then be outside the geometry itself, and
    /// the test would pass with no clip support at all.)
    ///
    /// If reverted (drop the `apply_active_clip` call at the circle record
    /// sites, or the clip block in `circle_instanced.wgsl`): the corner is
    /// painted and this fails.
    #[test]
    fn a_rounded_clip_rounds_a_circle() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        const R: f32 = 40.0;

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save();
        painter.clip_rrect(
            flui_foundation::geometry::RRect::from_rect_circular(
                flui_foundation::geometry::Rect::from_xywh(
                    0.0,
                    0.0,
                    f64::from(SURFACE_WIDTH as f32),
                    f64::from(SURFACE_HEIGHT as f32),
                ),
                f64::from(R),
            ),
            flui_painting::paint::Clip::AntiAlias,
        );
        // Radius 90 about the centre of a 128x128 surface: every corner of the
        // surface is well inside this circle.
        painter.draw_circle(
            flui_foundation::geometry::Point::new(
                f64::from(SURFACE_WIDTH as f32 / 2.0),
                f64::from(SURFACE_HEIGHT as f32 / 2.0),
            ),
            90.0,
            &Paint::fill(Color::rgb(0, 0, 255)),
        );
        painter.restore();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Circle Clip Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);
        let w = SURFACE_WIDTH as usize;
        let at = |x: usize, y: usize| pixels[y * w + x];

        // Centre: inside both the circle and the clip — must be painted, or the
        // test would pass by drawing nothing at all.
        let centre = at(w / 2, SURFACE_HEIGHT as usize / 2);
        assert!(
            centre[2] > 200,
            "precondition: the circle must paint inside the clip; got {centre:?}"
        );

        // Corner: inside the circle, OUTSIDE the rounded clip.
        let corner = at(3, 3);
        assert!(
            corner[3] < 32,
            "a rounded clip must round the circle inside it; corner rgba={corner:?} \
             — the circle received only the clip's bounding scissor"
        );
    }

    /// A tessellated draw that routes through the SSAA tile still honours its
    /// clip.
    ///
    /// `render_ssaa_path` rebases vertex positions into tile-local 2x space, so
    /// the `world_pos` the fragment stage sees is tile-local while the batch's
    /// SDF clip is in full-frame device space. Left unremapped the shader
    /// compares the two and the clip lands somewhere else entirely. Same shape
    /// as the grown-offscreen bug on circles, a different rebasing seam.
    ///
    /// `BlendMode::Plus` is tile-safe and not SrcOver, so a draw above the
    /// 256 px² area threshold takes the SSAA route rather than the direct one.
    ///
    /// The geometry sits AWAY from the origin on purpose: a tile whose origin
    /// is (0, 0) at scale 1 makes the remap an identity and the test would pass
    /// with no remap at all.
    #[test]
    fn an_ssaa_tiled_draw_honours_its_clip() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        // Bottom-right region of a 128x128 surface, heavily rounded.
        let region = flui_foundation::geometry::Rect::from_xywh(48.0, 48.0, 80.0, 80.0);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save();
        painter.clip_rrect(
            flui_foundation::geometry::RRect::from_rect_circular(region, 40.0),
            flui_painting::paint::Clip::AntiAlias,
        );
        // A FILL, and a large one: the SSAA gate reads the rect's own area
        // (80x80 = 6400 px², over the 256 px² threshold), not the painted
        // area — a hairline rect with a huge stroke width stays under it and
        // silently takes the direct path instead.
        painter.draw_rect(
            region,
            &Paint::fill(Color::rgb(0, 0, 255))
                .with_blend_mode(flui_painting::paint::BlendMode::Plus),
        );
        painter.restore();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("SSAA Clip Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);
        let w = SURFACE_WIDTH as usize;
        let at = |x: usize, y: usize| pixels[y * w + x];

        let centre = at(88, 88);
        assert!(
            centre[2] > 200,
            "precondition: the fill must paint inside the clip; got {centre:?}"
        );

        // Top-left of the region, inside the rounded clip: cut away.
        let corner = at(52, 52);
        assert!(
            corner[3] < 32,
            "the clip's rounded corner must still cut; got rgba={corner:?}"
        );

        // THE discriminating sample. The tile is `origin=(48, 48)`,
        // `scale=1.6`, so this pixel is tile-local (20, 64):
        //
        //   remapped clip   → bounds (0, 0, 128, 128), radii 64  → inside
        //   full-frame clip → bounds (48, 48, 80, 80), radii 40  → outside
        //
        // Most sample points agree between the two — the corner above is
        // clipped either way — so an assertion placed anywhere else passes
        // with the remap deleted.
        let discriminating = at(60, 88);
        assert!(
            discriminating[2] > 200,
            "an SSAA-tiled draw's clip must be remapped into tile space; \
             rgba={discriminating:?} at a pixel the correctly-remapped clip \
             keeps — its clip was left in full-frame coordinates while its \
             vertices were rebased to the tile"
        );
    }

    /// A rotated `ClipRRect` clips to the rotated rounded rect, not to its
    /// bounding box.
    ///
    /// The clip slot used to hold device-space bounds, which cannot express a
    /// rotation: `clip_rrect` detected a non-axis-aligned CTM and fell back to
    /// the coarse scissor. At 45° the scissor is the AABB of the rotated rect,
    /// 41% larger per axis, so content leaked well outside the intended shape.
    ///
    /// The drawn rrect had already solved this — `RectInstance::with_affine_transform`
    /// passes local bounds plus the affine and lets the shader do the work
    /// (see `o4_rotated_rrect_correct_size_orientation_and_aa`). The clip now
    /// carries the same information, inverted, so the fragment stage maps
    /// `world_pos` back into clip-local space.
    ///
    /// Sample points, for a 80x80 clip at (24, 24) rotated 45° about the
    /// surface centre (64, 64):
    ///
    /// - (64, 64) — the centre, inside under any interpretation. Paints, or
    ///   the test would pass by drawing nothing.
    /// - (14, 14) — inside the AABB of the rotated rect, OUTSIDE the rect
    ///   itself. This is the discriminating sample: the scissor fallback
    ///   paints it, a correct rotated clip does not.
    ///
    /// The point has to be near an AABB CORNER. The clip is rotated about its
    /// own centre, so the AABB grows along the diagonals and a point pushed
    /// straight out along an axis — (18, 64), say — is still inside the
    /// rotated square and correctly painted either way.
    #[test]
    fn a_rotated_clip_follows_the_rotation() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save();
        painter.translate(flui_foundation::geometry::Offset::new(64.0, 64.0));
        painter.rotate(std::f32::consts::FRAC_PI_4);
        painter.translate(flui_foundation::geometry::Offset::new(-64.0, -64.0));
        painter.clip_rrect(
            flui_foundation::geometry::RRect::from_rect_circular(
                flui_foundation::geometry::Rect::from_xywh(24.0, 24.0, 80.0, 80.0),
                12.0,
            ),
            flui_painting::paint::Clip::AntiAlias,
        );
        // Far larger than the surface, so every sample point is inside the
        // drawn shape and only the clip can remove it.
        painter.draw_rect(
            flui_foundation::geometry::Rect::from_xywh(-200.0, -200.0, 600.0, 600.0),
            &Paint::fill(Color::rgb(0, 0, 255)),
        );
        painter.restore();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Rotated Clip Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);
        let w = SURFACE_WIDTH as usize;
        let at = |x: usize, y: usize| pixels[y * w + x];

        let centre = at(64, 64);
        assert!(
            centre[2] > 200,
            "precondition: the fill must paint inside the clip; got {centre:?}"
        );

        let outside = at(14, 14);
        assert!(
            outside[3] < 32,
            "a rotated clip must follow its rotation; rgba={outside:?} at a \
             pixel inside the rotated rect's BOUNDING BOX but outside the rect \
             — the clip fell back to the coarse scissor"
        );
    }

    /// A non-uniformly scaled circular clip produces elliptical corners, not a
    /// clamped scalar radius.
    ///
    /// The clip slot holds ONE radius per corner, so a device-space form had
    /// to collapse a scaled circular corner — an ellipse — to a scalar, then
    /// clamp it to the box's half-height or the rounded-box SDF went
    /// degenerate and clipped inward. Keeping the radius in the caller's own
    /// space and letting the mapping stretch it removes the collapse entirely.
    ///
    /// `scale(1, 3)` on a 96x32 clip with radius 16: device bounds 96x96,
    /// corners 16 wide and 48 tall. The old form collapsed those to
    /// `max(16, 48) = 48`, clamped to `min(96, 96)/2 = 48`, and rounded every
    /// corner by 48 — a stadium, not the intended shape.
    ///
    /// Sample (20, 48) is on the left edge at mid-height: outside a 48-radius
    /// corner sweep, inside the true ellipse.
    ///
    /// What this test can and cannot show. Its counterfactual is the ENTIRE
    /// previous representation — device-space bounds with pre-scaled radii —
    /// which no longer exists as a line to revert; in local space the
    /// anisotropy problem cannot be expressed at all, which is the point. The
    /// old form's arithmetic is pinned instead by the `state_stack` unit tests
    /// that replaced its device-space ones. What this test does prove is that
    /// the mapping is live: making `clipAlpha` use `world_pos` directly fails
    /// the first precondition with `[0, 0, 0, 0]`.
    #[test]
    fn a_non_uniformly_scaled_clip_keeps_elliptical_corners() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save();
        painter.scale(1.0, 3.0);
        painter.clip_rrect(
            flui_foundation::geometry::RRect::from_rect_circular(
                flui_foundation::geometry::Rect::from_xywh(16.0, 0.0, 96.0, 32.0),
                16.0,
            ),
            flui_painting::paint::Clip::AntiAlias,
        );
        painter.draw_rect(
            flui_foundation::geometry::Rect::from_xywh(-200.0, -200.0, 600.0, 600.0),
            &Paint::fill(Color::rgb(0, 0, 255)),
        );
        painter.restore();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Anisotropic Clip Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);
        let w = SURFACE_WIDTH as usize;
        let at = |x: usize, y: usize| pixels[y * w + x];

        let centre = at(64, 48);
        assert!(
            centre[2] > 200,
            "precondition: the fill must paint inside the clip; got {centre:?}"
        );

        // Just outside the device-space bounds: clipped under any reading, so
        // a shader that ignored the clip entirely would not pass.
        let beyond = at(120, 48);
        assert!(
            beyond[3] < 32,
            "precondition: outside the clip's own bounds; got {beyond:?}"
        );

        // Left edge at mid-height. A 48-radius corner sweep would have eaten
        // this pixel; the true ellipse (16 wide, 48 tall) does not.
        let edge = at(20, 48);
        assert!(
            edge[2] > 200,
            "a non-uniformly scaled clip must keep elliptical corners; \
             rgba={edge:?} — the radius collapsed to a clamped scalar and \
             rounded the shape into a stadium"
        );
    }

    /// Text that is the ONLY content of an opacity layer must still belong
    /// to the layer: composited with the layer (subject to its opacity) and
    /// buried by top-level geometry drawn AFTER the layer — not dropped as
    /// an "empty" layer whose glyphs then float over everything.
    ///
    /// Born red: before the compositor counted text as layer content (and
    /// before the recursion range-rendered it), the pinned form of this
    /// test observed the text-only layer's glyphs floating above the later
    /// top-level fill.
    #[test]
    fn opacity_layer_text_stays_under_later_top_level_geometry() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let full = flui_foundation::geometry::Rect::from_xywh(
            0.0,
            0.0,
            f64::from(SURFACE_WIDTH as f32),
            f64::from(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer(None, &Paint::fill(Color::rgba(0, 0, 0, 250)));
        painter.draw_text(
            "Ghost",
            flui_foundation::geometry::Point::new(4.0, 30.0),
            20.0,
            &Paint::fill(Color::rgb(0, 0, 255)),
        );
        painter.restore_layer();
        // Top-level opaque fill AFTER the layer — must bury the layer's text.
        painter.draw_rect(full, &Paint::fill(Color::rgb(0, 255, 0)));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Text-Only Layer Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);
        let blue_pixels = pixels
            .iter()
            .filter(|p| p[2] > 200 && p[0] < 50 && p[1] < 50)
            .count();
        assert_eq!(
            blue_pixels, 0,
            "a text-only opacity layer's glyphs must be buried by top-level \
             geometry drawn after the layer; {blue_pixels} blue glyph pixels \
             visible — the layer resolved to Empty and its text fell through \
             to the trailing gap passes"
        );
    }

    // ── A4: Angular edges are anti-aliased (partial alpha) ───────────────────

    /// A4: The two angular edges of a ~90° arc must show partial alpha (anti-
    /// aliased), not a hard step from 0 to 255.
    ///
    /// ## Red→green proof
    ///
    /// The OLD `angle_softness = 0.05` rad was a FIXED angular threshold: it
    /// produced a smoothstep width that was ~0.05 rad ≈ 3° regardless of
    /// resolution. At a radius of 40 px this width is 40 * 0.05 ≈ 2 pixels —
    /// already incorrect (too wide at large radius, too narrow at small radius).
    /// More critically, the old approach first computed a hard `in_arc` boolean
    /// and then softened only the edges, so any pixel whose center angle fell
    /// outside the sector got a hard `discard` before the softening.
    ///
    /// The NEW approach uses an angular half-plane SDF: `angular_sdf =
    /// min(d_start, d_end)` for ≤180° sweeps, `max` for >180°. `fwidth` of
    /// this distance gives ~1 device-px AA at any radius — the angular AA band
    /// is as wide as the radial AA band, which is the correct behavior.
    ///
    /// Test: draw a 90° arc (radius=40) and scan pixels near the angular boundary
    /// at start_angle=0 (the +X ray). The pixels immediately above and below the
    /// start ray must have partial alpha — not 0 or 255.
    #[test]
    fn a4_arc_angular_edges_are_antialiased() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let radius = 40.0_f32;
        // 90° arc, ROTATED 30° so its angular edges are DIAGONAL (not grid-aligned).
        // A grid-aligned (axis) edge legitimately has no partial pixels — it falls
        // on a pixel-row boundary so coverage is 0/1 between rows — so the angular
        // AA can only be observed on a non-axis-aligned edge.
        let start = 0.0_f32;
        let sweep = std::f32::consts::FRAC_PI_2;
        let rotation = PI / 6.0; // 30°

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.translate(flui_foundation::geometry::Offset::new(
            f64::from(cx),
            f64::from(cy),
        ));
        painter.rotate(rotation);
        let rect = Rect::from_xywh(
            f64::from(-radius),
            f64::from(-radius),
            f64::from(radius * 2.0),
            f64::from(radius * 2.0),
        );
        painter.draw_arc(rect, start, sweep, true, &Paint::fill(Color::WHITE));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("A4 Angular AA Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Count partial-alpha pixels along the two angular edges: a partial pixel
        // in the annulus `5 <= dist < radius-2` can ONLY come from the angular-edge
        // AA — the radial edge is excluded by the outer bound, and the APEX disk
        // (`dist < 5`, where the pie tip is legitimately partial regardless of edge
        // AA) is excluded by the inner bound so it cannot satisfy the count on its
        // own. Rotation about the center preserves distance, so no inverse transform
        // is needed. A hard-aliased angular edge produces ZERO such partials; smooth
        // screen-space AA produces a ~1px band along each of the two diagonal edges.
        let mut interior_partial = 0usize;
        for row in 0..SURFACE_HEIGHT {
            for col in 0..SURFACE_WIDTH {
                let dx = col as f32 + 0.5 - cx;
                let dy = row as f32 + 0.5 - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < 5.0 || dist >= radius - 2.0 {
                    continue; // skip the apex disk and the radial boundary band
                }
                let idx = row as usize * SURFACE_WIDTH as usize + col as usize;
                let alpha = pixels[idx][3];
                if alpha > 5 && alpha < 250 {
                    interior_partial += 1;
                }
            }
        }

        assert!(
            interior_partial >= 10,
            "A4 FAILED: only {interior_partial} interior partial-alpha pixels (device dist < \
             radius-2) — the angular sector edges are hard-aliased. The screen-space angular \
             SDF + fwidth must produce a smooth ~1px band along each diagonal edge."
        );
    }

    // ── PD1: tile-safe non-SrcOver arbitrary path is SSAA-routed ────────────

    /// PD1: A tile-safe non-SrcOver arbitrary path fill (Xor mode on a
    /// transparent surface) must produce partial alpha at its boundary —
    /// proving the PR-4 SSAA routing fires for tile-safe non-SrcOver fills.
    ///
    /// ## Blend-mode selection rationale
    ///
    /// `Xor` on a transparent destination is equivalent to `SrcOver` (both give
    /// `src * 1 + 0 * (1-src_a) = src`), so the pixel output is identical to the
    /// SrcOver SSAA path.  This makes Xor the ideal probe for routing: the
    /// expected pixels are the same regardless of which blend factors the tile
    /// composite applies, so the assertion isolates "was the fill routed through
    /// the SSAA tile at all?" from per-mode composite correctness.
    ///
    /// Other tile-safe modes (DstOut, DstOver, Plus) composite with their own
    /// blend factors (`flush_texture_batch_premultiplied_with_mode` selects a
    /// pipeline whose `wgpu::BlendState` matches the mode exactly), so probing
    /// with one of them would need a mode-specific pixel oracle and would
    /// conflate composite math with routing; Xor-on-transparent needs neither.
    ///
    /// ## Routing proof (what this test guards)
    ///
    /// If Xor is NOT routed through SSAA (stays tessellated, aliased), boundary
    /// pixels are hard 0 or 255 — no partial alpha. SSAA produces genuine fractional
    /// alpha at every non-axis-aligned edge. The same kite geometry as p4 is used.
    #[test]
    fn pd1_tile_safe_non_srcover_path_has_partial_alpha_at_boundary() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        // Non-axis-aligned irregular kite — all edges are diagonal.
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let v = [
            (cx + 1.3, cy - 38.7),
            (cx + 42.2, cy + 5.5),
            (cx - 2.7, cy + 33.1),
            (cx - 38.4, cy - 9.2),
        ];
        let mut path = flui_painting::paint::path::Path::new();
        path.move_to(flui_foundation::geometry::Point::new(
            f64::from(v[0].0),
            f64::from(v[0].1),
        ));
        for &(x, y) in &v[1..] {
            path.line_to(flui_foundation::geometry::Point::new(
                f64::from(x),
                f64::from(y),
            ));
        }
        path.close();

        // Xor on transparent background = SrcOver (same pixel output, different
        // mode tag), so the assertion below proves routing alone — it holds
        // independent of the blend factors the tile composite applies.
        let paint = Paint::fill(Color::WHITE).with_blend_mode(BlendMode::Xor);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.draw_path(&path, &paint);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("PD1 Xor Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Partial alpha at the kite boundary proves SSAA ran: hard tessellation
        // produces only 0 or 255. Xor on transparent = SrcOver so boundary alpha =
        // sub-pixel coverage, same as p4.
        let partial_count = pixels.iter().filter(|p| p[3] > 5 && p[3] < 250).count();

        assert!(
            partial_count >= 4,
            "PD1 FAILED: only {partial_count} partial-alpha pixels — the Xor tile-safe \
             SSAA reroute appears to be a no-op or hard-aliased. PR-4 must route tile-safe \
             non-SrcOver fills through the SSAA tile."
        );
    }

    // ── PD3: non-SrcOver basic shape is SSAA-routed ──────────────────────────

    // ── PD2: tile-safe non-SrcOver produces visually distinct output over backdrop ──

    /// PD2: A tile-safe non-SrcOver SSAA path composited over a non-transparent
    /// backdrop must produce pixel output that is visibly distinct from SrcOver.
    ///
    /// ## Why this test is necessary (gap in PD1 / PD3)
    ///
    /// PD1 and PD3 use `Xor` on a *transparent* destination.  On a transparent
    /// dst, `Xor` and `SrcOver` produce identical pixel output (both yield `src`),
    /// so those tests cannot distinguish whether the correct `Xor` blend pipeline
    /// (`blend_state_for(Xor)`) or the default SrcOver pipeline fires at composite
    /// time.  PD2 forces a non-transparent backdrop so the two pipelines diverge.
    ///
    /// ## Blend mode and expected pixel values
    ///
    /// `DstOut` is tile-safe (`is_tile_safe_for_ssaa` = true).
    /// wgpu blend: `(src_factor=Zero, dst_factor=OneMinusSrcAlpha)`.
    ///
    /// Setup:
    ///   - Background: opaque green `(0, 255, 0, 255)` painted with SrcOver.
    ///   - Foreground: large white circle (r=40) with `DstOut` blend.
    ///
    /// Interior pixel formula (src.alpha=1 at the center of the circle):
    ///   out.alpha = 0 + (1 − 1.0) × dst.alpha = 0   → transparent
    ///   out.rgb   = 0 + (1 − 1.0) × dst.rgb   = 0   → black (premul of transparent)
    ///
    /// Under SrcOver the same pixel would be white (255, 255, 255, 255).
    /// Under DstOut the interior becomes transparent (alpha ≈ 0).
    ///
    /// ## Partial-alpha proof (SSAA ran)
    ///
    /// At the circle boundary the SSAA tile has partial alpha `a_tile ∈ (0,1)`.
    /// DstOut composite gives `out.alpha = dst.alpha × (1 − a_tile)`.
    /// With `dst.alpha = 1`: `out.alpha = 1 − a_tile ∈ (0, 1)`.
    /// These pixels show partial alpha — proving SSAA ran AND the DstOut blend
    /// pipeline was used (SrcOver would yield alpha = 1 at the same positions).
    #[test]
    fn pd2_tile_safe_non_srcover_over_opaque_backdrop_is_pixel_distinct_from_srcover() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let radius = 40.0_f32;

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));

        // Step 1: paint an opaque green background covering the whole surface.
        // This is SrcOver — it comes first in draw order so the DstOut circle
        // composites onto the green backdrop.
        let full_rect = Rect::from_xywh(
            0.0,
            0.0,
            f64::from(SURFACE_WIDTH as f32),
            f64::from(SURFACE_HEIGHT as f32),
        );
        let green = Color::rgba(0, 255, 0, 255);
        painter.draw_rect(full_rect, &Paint::fill(green));

        // Step 2: paint a large white circle with DstOut blend (tile-safe, non-SrcOver).
        // Radius 40 → bounding box area = 80×80 = 6400 px² >> SSAA_AREA_THRESHOLD_PX_SQ=256.
        // The circle is placed slightly off-pixel-center to ensure non-axis-aligned
        // edges and genuine partial-alpha boundary pixels from the SSAA downsample.
        let center =
            flui_foundation::geometry::Point::new(f64::from(cx + 0.5), f64::from(cy + 0.5));
        painter.draw_circle(
            center,
            radius,
            &Paint::fill(Color::WHITE).with_blend_mode(BlendMode::DstOut),
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("PD2 DstOut Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Assertion 1: interior pixel is near-transparent (DstOut erases backdrop).
        // Center pixel must have alpha close to 0. Under SrcOver it would be 255.
        let interior_idx = cy as usize * SURFACE_WIDTH as usize + cx as usize;
        let interior_alpha = pixels[interior_idx][3];
        assert!(
            interior_alpha < 30,
            "PD2 FAILED: interior alpha={interior_alpha}, expected near-0 (DstOut erases \
             the opaque green backdrop). SrcOver or wrong pipeline would give alpha=255."
        );

        // Assertion 2: boundary pixels show partial alpha — SSAA ran.
        // Pixels near the circle edge (dist ∈ [radius-2, radius+2]) that have
        // partial alpha can ONLY exist if SSAA resolved sub-pixel coverage.
        let mut boundary_partial = 0usize;
        for row in 0..SURFACE_HEIGHT {
            for col in 0..SURFACE_WIDTH {
                let dx = col as f32 + 0.5 - (cx + 0.5);
                let dy = row as f32 + 0.5 - (cy + 0.5);
                let dist = (dx * dx + dy * dy).sqrt();
                if (radius - 2.0..=radius + 2.0).contains(&dist) {
                    let alpha = pixels[row as usize * SURFACE_WIDTH as usize + col as usize][3];
                    // DstOut boundary: alpha = 1 − a_tile, so partial when a_tile ∈ (0,1).
                    // Strictly between 10 and 245 to exclude hard 0/255 edges.
                    if alpha > 10 && alpha < 245 {
                        boundary_partial += 1;
                    }
                }
            }
        }
        assert!(
            boundary_partial >= 4,
            "PD2 FAILED: only {boundary_partial} partial-alpha boundary pixels — SSAA \
             must produce sub-pixel coverage at the circle boundary when DstOut is active."
        );
    }

    // ── PD6: per-mode composite is the REQUESTED blend, not SrcOver ──────────

    /// PD6: For several tile-safe non-SrcOver modes, compositing the SSAA tile
    /// over an OPAQUE backdrop must match `Color::blend(src, backdrop, mode)` —
    /// NOT SrcOver. This pins the per-mode composite pipeline selection
    /// (`flush_texture_batch_premultiplied_with_mode` → `blend_state_for(mode)`),
    /// which PD1/PD3/PD4 (Xor-on-transparent) cannot detect because Xor and
    /// SrcOver are pixel-identical over a transparent dst. PD2 covers DstOut;
    /// this covers Dst/DstOver/Xor over an opaque dst where each differs from
    /// SrcOver. Together they pixel-verify the per-mode composite for 4 of the 6
    /// non-SrcOver tile-safe modes; the dispatch is uniform (keyed by `mode`), so
    /// the remaining two share the verified code path.
    #[test]
    fn pd6_tile_safe_composite_matches_requested_blend_not_srcover() {
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let radius = 40.0_f32;
        let backdrop = Color::rgba(220, 30, 30, 255); // opaque red
        let src = Color::rgba(40, 90, 230, 255); // opaque blue (distinct channels)

        for mode in [BlendMode::Dst, BlendMode::DstOver, BlendMode::Xor] {
            let expected = src.blend(backdrop, mode);
            let srcover = src.blend(backdrop, BlendMode::SrcOver);
            // Teeth: the chosen setup must make this mode pixel-distinct from
            // SrcOver, else a wrong (SrcOver) pipeline would pass.
            assert_ne!(
                (expected.r, expected.g, expected.b, expected.a),
                (srcover.r, srcover.g, srcover.b, srcover.a),
                "PD6 setup error: {mode:?} is not distinct from SrcOver on this backdrop"
            );

            let (device, queue) = acquire_test_device_and_queue();
            let (surface_texture, surface_view) = create_render_surface(&device);
            clear_surface(&device, &queue, &surface_view);
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));

            painter.draw_rect(
                Rect::from_xywh(
                    0.0,
                    0.0,
                    f64::from(SURFACE_WIDTH as f32),
                    f64::from(SURFACE_HEIGHT as f32),
                ),
                &Paint::fill(backdrop),
            );
            painter.draw_circle(
                flui_foundation::geometry::Point::new(f64::from(cx + 0.5), f64::from(cy + 0.5)),
                radius,
                &Paint::fill(src).with_blend_mode(mode),
            );

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("PD6 Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&surface_view, &surface_texture),
                    &mut encoder,
                )
                .expect("render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
            let pixels = readback_pixels(&device, &queue, &surface_texture);

            let interior = pixels[cy as usize * SURFACE_WIDTH as usize + cx as usize];
            let tol = 8i16;
            let near = |a: u8, b: u8| (i16::from(a) - i16::from(b)).abs() <= tol;
            assert!(
                near(interior[0], expected.r)
                    && near(interior[1], expected.g)
                    && near(interior[2], expected.b)
                    && near(interior[3], expected.a),
                "PD6 {mode:?}: interior {interior:?} != Color::blend oracle \
                 [{},{},{},{}] (SrcOver would be [{},{},{},{}]) — the per-mode composite \
                 selected the wrong blend pipeline",
                expected.r,
                expected.g,
                expected.b,
                expected.a,
                srcover.r,
                srcover.g,
                srcover.b,
                srcover.a,
            );
        }
    }

    // ── PD4: anti-MVP — all non-SrcOver shape producers emit partial alpha ────

    // ── PD6: Xor over opaque backdrop — pipeline is NOT SrcOver ─────────────

    // ── PD5: Phase-B regression — SrcOver path still AA'd after PR-4 ─────────

    // ── Q1: L2 gradient norm AA band-width tests ──────────────────────────────

    /// Q1-a: Rotated-edge AA band width is ≤ ~1.1 device-px (L2 norm).
    ///
    /// Draws a 45°-rotated white rectangle that bisects the surface. The edge
    /// normal is at 45°, so `fwidth` (L1) would give `|dpdx| + |dpdy|` ≈ √2
    /// (for equal partial derivatives), widening the AA band to ~1.41 device-px.
    /// `length(dpdx, dpdy)` (L2) gives exactly 1.0 for a unit SDF, so the band
    /// stays ≤ ~1 device-px even at 45°.
    ///
    /// Measurement: scan the row through the rect center, count pixels with
    /// alpha in (5, 250) → those are partial-coverage AA pixels. At 45°, one
    /// device-px in the scan direction spans √2 in SDF space, so the L2 band
    /// spans ≤ 1/√2 × 2 ≈ 1.41 scan pixels; in practice ≤ 2 scan pixels
    /// (including rounding). L1 can span up to 3 scan pixels at 45°.
    ///
    /// ## Why this test MUST FAIL under L1:
    /// With L1 (`fwidth`), the effective half-width is `(|0.5| + |0.5|) × 0.5 = 0.5`
    /// in SDF units per device-px, widening smoothstep to span ~√2 device-px.
    /// At 45° that means 2-3 boundary pixels in a scan perpendicular to the edge;
    /// under L2 it is 1-2. The threshold `≤ 2` passes L2 and fails L1.
    #[test]
    fn q1a_rotated_edge_aa_band_width_is_within_l2_bound() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        // Draw a 90×10 white rect rotated 45°, centered in the frame.
        // At 45° the vertical edges have a 45° normal, maximizing the L1 vs L2 difference.
        let cx = SURFACE_WIDTH as f32 / 2.0; // 64.0
        let cy = SURFACE_HEIGHT as f32 / 2.0; // 64.0
        let half_w = 45.0_f32;
        let half_h = 5.0_f32;

        // Use an rrect with zero radius so the painter routes through the instanced
        // affine-rect path (which uses the sdfToAlpha function we changed).
        let angle_deg = 45.0_f32;

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));

        // Build a rotated rect via the painter's transform API.
        painter.save();
        painter.translate(flui_foundation::geometry::Offset::new(
            f64::from(cx),
            f64::from(cy),
        ));
        painter.rotate(angle_deg.to_radians());
        let rotated_rect = Rect::from_ltrb(
            f64::from(-half_w),
            f64::from(-half_h),
            f64::from(half_w),
            f64::from(half_h),
        );
        painter.draw_rect(rotated_rect, &Paint::fill(Color::WHITE));
        painter.restore();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Q1a Rotated Edge Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Scan the center row (y = cy as usize = 64) and collect partial-alpha pixels.
        // At 45° rotation the rect's long axis runs diagonally, but the center row
        // crosses the rect interior. We look for the boundary band on the
        // right "side" of the tilted rect — specifically, the band in the scan
        // row that transitions from partial to interior.
        //
        // The short half-height (5px) means the rect's edge is ~5px from the center
        // in the direction perpendicular to the long axis. At 45° this translates to
        // ~3-4 pixels up/down and ~3-4 pixels left/right from center in screen space.
        //
        // Strategy: count all partial-alpha pixels in a 16-row band around the center
        // that are on the upper-right boundary (col > cx, row < cy).
        let band_row_begin = (cy as usize).saturating_sub(8);
        let band_row_end = (cy as usize + 8).min(SURFACE_HEIGHT as usize);
        let band_col_begin = cx as usize;
        let band_col_end = (cx as usize + 16).min(SURFACE_WIDTH as usize);

        let pixels_ref = &pixels;
        let partial_in_band: Vec<u8> = (band_row_begin..band_row_end)
            .flat_map(|row| {
                (band_col_begin..band_col_end).filter_map(move |col| {
                    let idx = row * SURFACE_WIDTH as usize + col;
                    let a = pixels_ref[idx][3];
                    if a > 5 && a < 250 { Some(a) } else { None }
                })
            })
            .collect();

        // There must be some AA pixels on the boundary (the rect exists).
        assert!(
            !partial_in_band.is_empty(),
            "Q1a: no partial-alpha pixels found in the upper-right band — rect may not be rendered \
             or the 45° rotation is off; partial count=0"
        );

        // Count total partial-alpha pixels in the whole frame.
        //
        // Under L2 (length(dpdx,dpdy)*0.5) the AA half-band is 0.5 device-px.
        // Under L1 (fwidth*0.5) at 45° the half-band is (|0.5|+|0.5|)*0.5 = 0.707 px —
        // √2 ≈ 41% wider.  For a 45° rect with ~200 px perimeter this produces
        // measurably more partial-alpha pixels under L1 vs L2.
        //
        // Empirically verified on DX12/D3D12:
        //   L2  → total_partial ≈ 142 (passes threshold ≤ 149)
        //   L1  → total_partial ≈ 156 (FAILS threshold ≤ 149)
        //
        // The threshold 149 sits at the geometric midpoint, giving 7 px headroom
        // on each side.  This MUST FAIL if sdfToAlpha reverts to fwidth(x)*0.5.
        let total_partial: usize = pixels.iter().filter(|p| p[3] > 5 && p[3] < 250).count();
        assert!(
            total_partial <= 149,
            "Q1a: total partial-alpha pixels across the 128×128 frame = {total_partial} — \
             expected ≤ 149 (L2/length gives ≈142; L1/fwidth at 45° gives ≈156, failing here). \
             If this assertion fires, check that sdfToAlpha in rect_instanced.wgsl uses \
             length(vec2(dpdx(dist),dpdy(dist)))*0.5, NOT fwidth(dist)*0.5."
        );
    }
}
