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

/// Analytic inside-test for an axis-aligned circle centered at origin with
/// the given radius.
#[cfg(all(test, feature = "enable-wgpu-tests"))]
fn inside_circle(px: f32, py: f32, radius: f32) -> bool {
    px * px + py * py <= radius * radius
}

/// Analytic inside-test for an axis-aligned ellipse centered at origin with
/// semi-axes `(rx, ry)`.
#[cfg(all(test, feature = "enable-wgpu-tests"))]
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
#[cfg(all(test, feature = "enable-wgpu-tests"))]
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
#[cfg(all(test, feature = "enable-wgpu-tests"))]
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
/// Only used in GPU readback tests gated on `enable-wgpu-tests`.
#[cfg(all(test, feature = "enable-wgpu-tests"))]
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

// ── CPU-only (no GPU) oracle tests ────────────────────────────────────────────

#[cfg(test)]
mod oracle_unit_tests {
    use super::*;

    /// Oracle self-check: a pixel fully inside a rect must have coverage 1.0.
    #[test]
    fn oracle_interior_coverage_is_one() {
        let coverage = analytic_coverage(0.0, 0.0, |px, py| inside_rect(px, py, 50.0, 25.0));
        // All 8×8 sub-samples land inside the shape → sum/64 is exactly 1.0 (no float rounding).
        #[allow(
            clippy::float_cmp,
            reason = "analytic oracle: all sub-samples inside → coverage is exactly 1.0"
        )]
        {
            assert_eq!(coverage, 1.0, "interior pixel must have full coverage");
        }
    }

    /// Oracle self-check: a pixel fully outside a rect must have coverage 0.0.
    #[test]
    fn oracle_exterior_coverage_is_zero() {
        let coverage = analytic_coverage(60.0, 0.0, |px, py| inside_rect(px, py, 50.0, 25.0));
        // All 8×8 sub-samples land outside the shape → sum/64 is exactly 0.0.
        #[allow(
            clippy::float_cmp,
            reason = "analytic oracle: all sub-samples outside → coverage is exactly 0.0"
        )]
        {
            assert_eq!(coverage, 0.0, "exterior pixel must have zero coverage");
        }
    }

    /// Oracle self-check: a pixel on the 45° boundary of a rect has partial
    /// coverage strictly between 0 and 1.
    #[test]
    fn oracle_boundary_coverage_is_partial() {
        // Edge pixel at the right boundary of a 100×50 rect.
        // Center at x=50 puts exactly half the pixel inside (for an infinitely
        // thin edge), so ORACLE_GRID sub-samples give ≈0.5 coverage.
        let coverage = analytic_coverage(50.0, 0.0, |px, py| inside_rect(px, py, 50.0, 25.0));
        assert!(
            coverage > 0.0 && coverage < 1.0,
            "boundary pixel must have partial coverage; got {coverage}"
        );
    }

    /// Oracle self-check: corner coverage of a rounded rect is strictly less
    /// than 1.0 (the corner arc cuts into the pixel).
    #[test]
    fn oracle_rrect_corner_coverage_less_than_rect() {
        let half_w = 50.0_f32;
        let half_h = 25.0_f32;
        let r = [10.0_f32; 4];
        // Sample at the corner itself — rrect removes the sharp corner, so
        // coverage should be lower than for a plain rect.
        let sharp_corner_coverage =
            analytic_coverage(half_w, half_h, |px, py| inside_rect(px, py, half_w, half_h));
        let rounded_corner_coverage = analytic_coverage(half_w, half_h, |px, py| {
            inside_rounded_rect(px, py, half_w, half_h, r)
        });
        assert!(
            rounded_corner_coverage < sharp_corner_coverage,
            "rounded rect corner coverage ({rounded_corner_coverage}) must be < \
             rect coverage ({sharp_corner_coverage})"
        );
    }

    /// Oracle has teeth: verifies that a synthetic hard-aliased alpha map (all
    /// boundary pixels are either 0 or 1) does NOT pass a tolerance test
    /// against the analytic oracle at a boundary pixel.
    ///
    /// This proves the calibration tolerance would catch hard-aliased output —
    /// i.e., the oracle is non-vacuous.
    #[test]
    fn oracle_hard_aliased_map_fails_tolerance() {
        // Oracle says this boundary pixel has partial coverage ≈ 0.5.
        let oracle_coverage =
            analytic_coverage(50.0, 0.0, |px, py| inside_rect(px, py, 50.0, 25.0));

        // Hard-aliased value: pixel is either fully inside (1.0) or outside (0.0).
        // At x=50 the pixel center is exactly on the edge → hard-aliased = 1 or 0.
        // Either way it differs from oracle by ~0.5 * 255 ≈ 127.5 in alpha units.
        let hard_aliased_alpha_high: f32 = 1.0; // pixel fully inside
        let hard_aliased_alpha_low: f32 = 0.0; // pixel fully outside

        // Tolerance used by the calibration test (O1): 30/255 ≈ 0.118.
        let tolerance_f32 = 30.0_f32 / 255.0;

        let diff_high = (hard_aliased_alpha_high - oracle_coverage).abs();
        let diff_low = (hard_aliased_alpha_low - oracle_coverage).abs();

        assert!(
            diff_high > tolerance_f32,
            "hard-aliased high alpha should FAIL the oracle tolerance; diff={diff_high:.3} tolerance={tolerance_f32:.3}"
        );
        assert!(
            diff_low > tolerance_f32,
            "hard-aliased low alpha should FAIL the oracle tolerance; diff={diff_low:.3} tolerance={tolerance_f32:.3}"
        );
    }

    /// Arc oracle self-check: a pixel fully inside a 270° arc has coverage 1.0.
    #[test]
    fn oracle_arc_interior_coverage_is_one() {
        use super::inside_arc;
        // 270° arc (3π/2 sweep), radius 40. The arc covers 0 → 3π/2 (right→down→left).
        // Sample at (-20, 0): angle = π, well into the middle of the arc and far from
        // both angular edges. All 8×8 sub-samples land inside → coverage must be 1.0.
        let coverage = analytic_coverage(-20.0, 0.0, |px, py| {
            inside_arc(px, py, 40.0, 0.0, 3.0 * std::f32::consts::FRAC_PI_2)
        });
        #[allow(
            clippy::float_cmp,
            reason = "analytic oracle: all sub-samples inside → coverage exactly 1.0"
        )]
        {
            assert_eq!(coverage, 1.0, "arc interior pixel must have full coverage");
        }
    }

    /// Arc oracle self-check: a pixel fully outside (beyond radius) has coverage 0.0.
    #[test]
    fn oracle_arc_exterior_radial_coverage_is_zero() {
        use super::inside_arc;
        // Well outside the arc radially.
        let coverage = analytic_coverage(60.0, 0.0, |px, py| {
            inside_arc(px, py, 40.0, 0.0, std::f32::consts::FRAC_PI_2)
        });
        #[allow(
            clippy::float_cmp,
            reason = "analytic oracle: all sub-samples outside → coverage exactly 0.0"
        )]
        {
            assert_eq!(coverage, 0.0, "arc exterior pixel must have zero coverage");
        }
    }

    /// Arc oracle self-check: full-circle arc (|sweep| = 2π) covers all interior
    /// points (coverage = 1.0 at the center), proving the full-circle shortcut
    /// in `inside_arc` degrades correctly to a pure radial test.
    #[test]
    fn oracle_full_circle_arc_matches_circle() {
        use super::inside_arc;
        let r = 30.0_f32;
        // Full-circle arc with 2π sweep — must behave exactly like a circle.
        let arc_cov = analytic_coverage(0.0, 0.0, |px, py| {
            inside_arc(px, py, r, 0.0, 2.0 * std::f32::consts::PI)
        });
        // The center pixel is fully inside the circle → coverage must be 1.0.
        #[allow(
            clippy::float_cmp,
            reason = "analytic oracle: all sub-samples inside → coverage exactly 1.0"
        )]
        {
            assert_eq!(
                arc_cov, 1.0,
                "full-circle arc (2π sweep) must have full coverage at the center"
            );
        }
    }

    /// Arc oracle self-check: a pixel on the RADIAL boundary of an arc has
    /// partial coverage (the outer AA fringe is active).
    #[test]
    fn oracle_arc_radial_boundary_is_partial() {
        use super::inside_arc;
        // Sample right on the radial edge (radius=40, 90° sweep, pixel at (40,0)).
        let coverage = analytic_coverage(40.0, 0.0, |px, py| {
            inside_arc(px, py, 40.0, -0.1, std::f32::consts::FRAC_PI_2 + 0.2)
        });
        assert!(
            coverage > 0.0 && coverage < 1.0,
            "arc radial boundary pixel must have partial coverage; got {coverage}"
        );
    }

    /// Arc oracle self-check: a pixel on the ANGULAR boundary of an arc has
    /// partial coverage — the angular edge is anti-aliased.
    #[test]
    fn oracle_arc_angular_boundary_is_partial() {
        use super::inside_arc;
        // The angular edge of a 90° arc at start_angle=0 runs along the +X axis.
        // Sample at pixel center (10, 0): the 8×8 sub-samples span
        // y ∈ [-0.4375, +0.4375], so roughly half have y>0 (inside the [0,π/2]
        // arc) and half have y<0 (below the start edge, outside). Coverage must
        // be strictly between 0 and 1.
        let coverage = analytic_coverage(10.0, 0.0, |px, py| {
            // 90° arc (0 to π/2), radius 30. The angular boundary at angle=0 runs
            // along the +X ray; a pixel centered at y=0 straddles it.
            inside_arc(px, py, 30.0, 0.0, std::f32::consts::FRAC_PI_2)
        });
        // This pixel straddles the angular boundary — coverage must be partial.
        assert!(
            coverage > 0.0 && coverage < 1.0,
            "arc angular boundary pixel must have partial coverage; got {coverage}"
        );
    }

    /// CPU oracle: the ≤π / >π sector boundary (the seam where the GPU shader
    /// flips `min(d_start,d_end)` → `max(...)`) is consistent — a point at angle
    /// `start + 180.5°` is OUTSIDE a 180° arc but INSIDE a 181° arc. Guards the
    /// modular-angle logic right at the half-plane intersection→union switch.
    #[test]
    fn oracle_arc_sweep_seam_at_180_degrees() {
        use super::inside_arc;
        use std::f32::consts::PI;
        let r = 30.0_f32;
        // A point at 180.5° from start=0, at radius 15 (well inside the disk).
        let ang = (180.5_f32).to_radians();
        let px = 15.0 * ang.cos();
        let py = 15.0 * ang.sin();
        assert!(
            !inside_arc(px, py, r, 0.0, PI),
            "point at 180.5° must be OUTSIDE a 180° arc"
        );
        assert!(
            inside_arc(px, py, r, 0.0, (181.0_f32).to_radians()),
            "point at 180.5° must be INSIDE a 181° arc"
        );
    }

    /// CPU coverage ramp for a 30° rotated rect is monotonically decreasing as
    /// we step outward across the boundary (inside → outside).
    ///
    /// Tests the oracle's rotated-rect computation without any GPU involvement.
    #[test]
    fn oracle_rotated_rect_coverage_ramp_is_monotone() {
        use std::f32::consts::PI;
        let angle = PI / 6.0; // 30°
        let half_w = 40.0_f32;
        let half_h = 20.0_f32;

        // Sample perpendicular to the right edge, stepping outward.
        // The edge normal for a 30° rotated rect points in the direction
        // (cos30, sin30). Step along that direction across the boundary.
        let cos_a = angle.cos();
        let sin_a = angle.sin();

        // Step from 1px inside the edge to 1px outside.
        let samples: Vec<f32> = (-4..=4)
            .map(|step_i| {
                let step = step_i as f32 * 0.4; // 0.4px per step
                let cx = half_w * cos_a + step * cos_a;
                let cy = half_w * sin_a + step * sin_a;
                analytic_coverage(cx, cy, |px, py| {
                    inside_rotated_rect(px, py, half_w, half_h, angle)
                })
            })
            .collect();

        // Verify the ramp crosses from ~1.0 to ~0.0 monotonically.
        let mut any_decrease_seen = false;
        for window in samples.windows(2) {
            let prev = window[0];
            let next = window[1];
            if next < prev {
                any_decrease_seen = true;
            }
            // Allow small non-monotone jitter from grid quantization (≤1 sample step).
            let tolerance = 1.0 / (ORACLE_GRID * ORACLE_GRID) as f32;
            assert!(
                next <= prev + tolerance,
                "coverage ramp must be non-increasing across boundary; \
                 got {next:.3} after {prev:.3}"
            );
        }
        assert!(
            any_decrease_seen,
            "coverage ramp must actually decrease somewhere — oracle may be broken"
        );
        // First sample (inside) must have high coverage; last (outside) low.
        assert!(
            *samples.first().unwrap() > 0.8,
            "inside sample must have high coverage; got {}",
            samples.first().unwrap()
        );
        assert!(
            *samples.last().unwrap() < 0.2,
            "outside sample must have low coverage; got {}",
            samples.last().unwrap()
        );
    }
}

// ── GPU readback tests ─────────────────────────────────────────────────────────

// All intentional: pixel-coordinate and alpha-value casts (f32 → u8 / usize)
// are clamped or derived from fixed [0,1] oracle values; sign loss is impossible
// (oracle returns non-negative; pixel coords are positive screen positions).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "oracle alpha: f32 in [0,1]×255 → [0,255] fits u8; pixel coords are non-negative \
              device-px — both casts are analytically safe"
)]
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::f32::consts::PI;
    use std::sync::Arc;

    use flui_painting::Paint;
    use flui_types::{
        Color, Rect,
        geometry::{Pixels, RRect, px},
    };

    use crate::wgpu::{painter::WgpuPainter, render_target::RenderTarget};

    use super::{
        analytic_coverage, inside_arc, inside_circle, inside_rotated_arc, inside_rotated_ellipse,
        inside_rotated_rect, inside_rotated_rounded_rect, inside_rounded_rect,
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
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available for aa_oracle_tests");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("AA Oracle Test Device"),
            ..Default::default()
        }))
        .expect("a GPU device must be available for aa_oracle_tests");
        (Arc::new(device), Arc::new(queue))
    }

    fn create_render_surface(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("AA Oracle Test Surface"),
            size: wgpu::Extent3d {
                width: SURFACE_WIDTH,
                height: SURFACE_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SURFACE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    fn clear_surface(device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("AA Oracle Clear"),
        });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("AA Oracle Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Read all pixels from a texture and return RGBA bytes (row-major).
    fn readback_pixels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) -> Vec<[u8; 4]> {
        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = SURFACE_WIDTH * bytes_per_pixel;
        let row_alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(row_alignment) * row_alignment;
        let staging_size = u64::from(padded_bytes_per_row * SURFACE_HEIGHT);

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("AA Oracle Readback Staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("AA Oracle Readback Encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(SURFACE_HEIGHT),
                },
            },
            wgpu::Extent3d {
                width: SURFACE_WIDTH,
                height: SURFACE_HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("GPU readback poll must complete");

        let raw = staging.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((SURFACE_WIDTH * SURFACE_HEIGHT) as usize);
        for row in 0..SURFACE_HEIGHT {
            let row_start = (row * padded_bytes_per_row) as usize;
            for col in 0..SURFACE_WIDTH {
                let byte_offset = row_start + col as usize * 4;
                pixels.push([
                    raw[byte_offset],
                    raw[byte_offset + 1],
                    raw[byte_offset + 2],
                    raw[byte_offset + 3],
                ]);
            }
        }
        pixels
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

    /// O1: The existing axis-aligned SDF rrect (known-correct AA) must match
    /// the analytic-coverage oracle within `CALIBRATION_TOLERANCE_U8` on every
    /// boundary pixel.
    ///
    /// Calibrates the oracle against the known-correct baseline before using
    /// it to gate the new affine path.
    ///
    /// Fails if the oracle itself is wrong, or if the existing SDF AA path
    /// regresses (correctness guard for the pre-existing baseline).
    #[test]
    fn o1_calibration_axis_aligned_sdf_rrect_matches_oracle() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        // Draw a 60×40 opaque white rrect centered in the surface, with 8px radii.
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let half_w = 30.0_f32;
        let half_h = 20.0_f32;
        let radius = 8.0_f32;

        let rrect = RRect::from_rect_circular(
            Rect::from_ltrb(
                Pixels(cx - half_w),
                Pixels(cy - half_h),
                Pixels(cx + half_w),
                Pixels(cy + half_h),
            ),
            Pixels(radius),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.rrect(rrect, &Paint::fill(Color::WHITE));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("O1 Calibration Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Find boundary pixels according to the oracle.
        let radii_arr = [radius; 4];
        let boundary = boundary_pixel_indices(|px, py| {
            inside_rounded_rect(px - cx, py - cy, half_w, half_h, radii_arr)
        });

        assert!(
            !boundary.is_empty(),
            "O1: no boundary pixels found — oracle or shape parameters may be wrong"
        );

        let mut failed_count = 0usize;
        for (pixel_idx, oracle_coverage) in &boundary {
            let readback_alpha = pixels[*pixel_idx][3];
            let oracle_alpha = (oracle_coverage * 255.0).round() as u8;
            let diff = (i16::from(readback_alpha) - i16::from(oracle_alpha)).unsigned_abs();
            #[allow(
                clippy::cast_possible_truncation,
                reason = "diff of two u8-range values fits in u8"
            )]
            let diff_u8 = diff as u8;
            if diff_u8 > CALIBRATION_TOLERANCE_U8 {
                failed_count += 1;
            }
        }

        // Allow up to 5% of boundary pixels to exceed tolerance (subpixel
        // differences between fwidth-smoothstep and grid supersampling can
        // diverge slightly at very thin corners).
        let boundary_count = boundary.len();
        let max_failures = (boundary_count as f32 * 0.05).ceil() as usize;
        assert!(
            failed_count <= max_failures,
            "O1 calibration failed: {failed_count}/{boundary_count} boundary pixels exceed \
             tolerance {CALIBRATION_TOLERANCE_U8}; max allowed = {max_failures}. \
             Either the oracle or the existing SDF path has regressed."
        );
    }

    // ── O2: Oracle has teeth (control test) ──────────────────────────────────

    /// O2: A synthetic hard-aliased alpha map (boundary pixels = 0 or 255)
    /// must NOT pass the calibration tolerance.
    ///
    /// Proves the oracle is non-vacuous: the tolerance chosen in O1 would catch
    /// un-antialiased (hard-aliased) edges, not just accept anything.
    ///
    /// This is a CPU-only test — no GPU required.
    #[test]
    fn o2_oracle_hard_aliased_fails_calibration_tolerance() {
        // Build a synthetic hard-aliased alpha map for an axis-aligned rect.
        // Any pixel whose center is inside = 255, outside = 0.
        let cx = 64.0_f32;
        let cy = 64.0_f32;
        let half_w = 30.0_f32;
        let half_h = 20.0_f32;
        let radii = [8.0_f32; 4];

        let boundary = boundary_pixel_indices(|px, py| {
            inside_rounded_rect(px - cx, py - cy, half_w, half_h, radii)
        });

        assert!(
            !boundary.is_empty(),
            "O2: no boundary pixels found — oracle shape parameters may be wrong"
        );

        // Synthetic hard-aliased: alpha = 255 if inside at pixel center, else 0.
        let mut exceeded_count = 0usize;
        for (pixel_idx, oracle_coverage) in &boundary {
            let col = pixel_idx % SURFACE_WIDTH as usize;
            let row = pixel_idx / SURFACE_WIDTH as usize;
            let center_x = col as f32 + 0.5;
            let center_y = row as f32 + 0.5;
            let hard_aliased_alpha =
                if inside_rounded_rect(center_x - cx, center_y - cy, half_w, half_h, radii) {
                    255u8
                } else {
                    0u8
                };
            let oracle_alpha = (oracle_coverage * 255.0).round() as u8;
            let diff = (i16::from(hard_aliased_alpha) - i16::from(oracle_alpha)).unsigned_abs();
            #[allow(
                clippy::cast_possible_truncation,
                reason = "diff of two u8-range values fits in u8"
            )]
            let diff_u8 = diff as u8;
            if diff_u8 > CALIBRATION_TOLERANCE_U8 {
                exceeded_count += 1;
            }
        }

        // A hard-aliased edge should fail for the MAJORITY of boundary pixels.
        let boundary_count = boundary.len();
        let min_failures = (boundary_count as f32 * 0.5).ceil() as usize;
        assert!(
            exceeded_count >= min_failures,
            "O2 teeth check FAILED: only {exceeded_count}/{boundary_count} pixels exceeded the \
             tolerance — the oracle may be too lenient to catch hard-aliased edges"
        );
    }

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
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
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
            Pixels(-half_w),
            Pixels(-half_h),
            Pixels(half_w),
            Pixels(half_h),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Translate to center, then rotate.
        painter.translate(flui_types::Offset::new(Pixels(cx), Pixels(cy)));
        painter.rotate(angle);
        painter.rect(local_rect, &Paint::fill(Color::WHITE));

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
                Pixels(-half_w),
                Pixels(-half_h),
                Pixels(half_w),
                Pixels(half_h),
            ),
            Pixels(radius),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.translate(flui_types::Offset::new(Pixels(cx), Pixels(cy)));
        painter.rotate(angle);
        painter.rrect(local_rrect, &Paint::fill(Color::WHITE));

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
            #[allow(
                clippy::cast_possible_truncation,
                reason = "diff of two u8-range values fits in u8"
            )]
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

    /// C1: A filled SrcOver circle must have boundary pixels that match the
    /// analytic-coverage oracle within `CALIBRATION_TOLERANCE_U8` at two radii
    /// spanning a ~4× range: 12 px and 50 px (both fit the 128² test surface).
    ///
    /// ## Red→green proof
    ///
    /// The OLD `edge_softness = 0.02` (radius-relative) model produced an AA band
    /// that scaled with the radius, so the two radii diverge sharply:
    ///   - r=12: AA band = 0.02 * 12 * 2 = 0.48 px → sub-pixel → nearly aliased.
    ///     The boundary would be mostly 0 or 255, failing the oracle (diff ≈ 127 >> 30).
    ///   - r=50: AA band = 0.02 * 50 * 2 = 2 px → too wide; boundary pixels sit at
    ///     coverages the box oracle does not expect.
    ///
    /// A single relative-softness value cannot satisfy the oracle at both radii —
    /// that 4× span is the teeth. The NEW `fwidth`-based model gives ~1 device-px
    /// AA at any radius, so both pass.
    ///
    /// Additionally, the test verifies that the interior is fully opaque and an
    /// exterior point is transparent (C3 properties), consolidating three checks.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
    fn c1_circle_aa_is_radius_independent() {
        // We test one radius per isolated surface to keep the test self-contained
        // and avoid shape overlap. Use r=12 and r=50 on the 128×128 surface (r=200
        // would not fit, so we use r=50 for the large-radius case — the point of
        // C1 is radius-independence which is covered by comparing r=12 vs r=50).
        for radius in [12.0_f32, 50.0_f32] {
            let (device, queue) = acquire_test_device_and_queue();
            let (surface_texture, surface_view) = create_render_surface(&device);
            clear_surface(&device, &queue, &surface_view);

            let cx = SURFACE_WIDTH as f32 / 2.0;
            let cy = SURFACE_HEIGHT as f32 / 2.0;

            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.circle(
                flui_types::Point::new(
                    flui_types::geometry::Pixels(cx),
                    flui_types::geometry::Pixels(cy),
                ),
                radius,
                &Paint::fill(Color::WHITE),
            );

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("C1 Circle Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&surface_view, &surface_texture),
                    &mut encoder,
                )
                .expect("painter.render must succeed");
            queue.submit(std::iter::once(encoder.finish()));

            let pixels = readback_pixels(&device, &queue, &surface_texture);

            let boundary = boundary_pixel_indices(|px, py| inside_circle(px - cx, py - cy, radius));

            assert!(
                boundary.len() >= 4,
                "C1 r={radius}: fewer than 4 boundary pixels found ({}) — shape may be off-screen \
                 or oracle broken",
                boundary.len()
            );

            let mut failed_count = 0usize;
            for (pixel_idx, oracle_coverage) in &boundary {
                let readback_alpha = pixels[*pixel_idx][3];
                let oracle_alpha = (oracle_coverage * 255.0).round() as u8;
                let diff =
                    (i16::from(readback_alpha) - i16::from(oracle_alpha)).unsigned_abs() as u8;
                if diff > CALIBRATION_TOLERANCE_U8 {
                    failed_count += 1;
                }
            }

            let boundary_count = boundary.len();
            let max_failures = (boundary_count as f32 * 0.05).ceil() as usize;
            assert!(
                failed_count <= max_failures,
                "C1 FAILED at r={radius}: {failed_count}/{boundary_count} boundary pixels exceed \
                 oracle tolerance {CALIBRATION_TOLERANCE_U8}. \
                 The fwidth AA model must give ~1-device-px AA at all radii — \
                 the old edge_softness=0.02 would fail this at r=12 and r=50."
            );

            // Interior must be opaque.
            let interior_col = cx.round() as usize;
            let interior_row = cy.round() as usize;
            let interior_idx = interior_row * SURFACE_WIDTH as usize + interior_col;
            assert!(
                pixels[interior_idx][3] > 200,
                "C1 r={radius}: interior pixel must be nearly opaque; got alpha={}",
                pixels[interior_idx][3]
            );

            // Exterior (2 px beyond the edge) must be transparent.
            let exterior_col = (cx + radius + 2.0).min(SURFACE_WIDTH as f32 - 1.0) as usize;
            let exterior_row = cy.round() as usize;
            let exterior_idx = exterior_row * SURFACE_WIDTH as usize + exterior_col;
            assert!(
                pixels[exterior_idx][3] < CALIBRATION_TOLERANCE_U8,
                "C1 r={radius}: exterior pixel (2px beyond edge) must be transparent; \
                 got alpha={} — fringe expansion may be leaking output",
                pixels[exterior_idx][3]
            );
        }
    }

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
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
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
            Pixels(cx - rx),
            Pixels(cy - ry),
            Pixels(cx + rx),
            Pixels(cy + ry),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Rotate around the canvas center so the ellipse center stays at (cx, cy).
        painter.translate(flui_types::Offset::new(Pixels(cx), Pixels(cy)));
        painter.rotate(angle);
        painter.translate(flui_types::Offset::new(Pixels(-cx), Pixels(-cy)));
        painter.oval(local_rect, &Paint::fill(Color::WHITE));

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

    /// C3: For a filled SrcOver circle:
    /// 1. Every pixel whose center is ≥2px inside the circle must be fully opaque
    ///    (interior correctness).
    /// 2. Every pixel whose center is ≥2px outside the circle must be transparent
    ///    (fringe expansion must not leak any visible output beyond the AA band).
    ///
    /// This guards against the fringe quad expansion producing visible artifacts
    /// outside the shape boundary.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
    fn c3_circle_interior_opaque_exterior_transparent() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let radius = 40.0_f32;

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.circle(
            flui_types::Point::new(
                flui_types::geometry::Pixels(cx),
                flui_types::geometry::Pixels(cy),
            ),
            radius,
            &Paint::fill(Color::WHITE),
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("C3 Circle Interior/Exterior Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        let guard_px = 2.0_f32; // minimum inset/outset from the geometric edge

        let mut interior_failures = 0usize;
        let mut exterior_failures = 0usize;
        let mut interior_total = 0usize;
        let mut exterior_total = 0usize;

        for row in 0..SURFACE_HEIGHT {
            for col in 0..SURFACE_WIDTH {
                let px = col as f32 + 0.5 - cx;
                let py = row as f32 + 0.5 - cy;
                let dist_from_edge = (px * px + py * py).sqrt() - radius;
                let idx = row as usize * SURFACE_WIDTH as usize + col as usize;
                let alpha = pixels[idx][3];

                if dist_from_edge < -guard_px {
                    // Clearly inside: must be opaque.
                    interior_total += 1;
                    if alpha < 200 {
                        interior_failures += 1;
                    }
                } else if dist_from_edge > guard_px {
                    // Clearly outside: must be transparent.
                    exterior_total += 1;
                    if alpha > CALIBRATION_TOLERANCE_U8 {
                        exterior_failures += 1;
                    }
                }
            }
        }

        assert!(
            interior_total > 0,
            "C3: no interior pixels found — circle may be too small or off-screen"
        );
        assert!(
            exterior_total > 0,
            "C3: no exterior pixels found — circle may fill the whole surface"
        );

        assert!(
            interior_failures == 0,
            "C3 FAILED: {interior_failures}/{interior_total} interior pixels (≥2px inside the \
             circle edge) are not fully opaque — interior fill is wrong."
        );
        assert!(
            exterior_failures == 0,
            "C3 FAILED: {exterior_failures}/{exterior_total} exterior pixels (≥2px outside the \
             circle edge) are not transparent — fringe expansion is leaking visible output."
        );
    }

    // ── C4: Scaled circle center is not double-scaled (baked-path regression) ──

    /// C4: Under a non-unit canvas scale, a circle's CENTER must land at the
    /// transformed position — NOT at scale × position.
    ///
    /// Regression guard for the baked fast-path bug where the device center was
    /// placed inside the local vector, so `M = diag(sx,sy)` scaled it a second
    /// time. With `scale(2,2)` and a circle at local (32,32) r=10, the device
    /// center is (64,64) and device radius 20. The bug rendered the center at
    /// (128,128) — off this 128² surface — leaving (64,64) empty. C1–C3 use
    /// identity scale and cannot catch this; production hits it on every HiDPI
    /// (DPR>1) display, which pushes a root `scale(dpr)` into the painter CTM.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
    fn c4_scaled_circle_center_not_double_scaled() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.scale(2.0, 2.0);
        painter.circle(
            flui_types::Point::new(
                flui_types::geometry::Pixels(32.0),
                flui_types::geometry::Pixels(32.0),
            ),
            10.0,
            &Paint::fill(Color::WHITE),
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("C4 Scaled Circle Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Expected device geometry: center (64,64), radius 20.
        let dcx = 64.0_f32;
        let dcy = 64.0_f32;
        let dradius = 20.0_f32;

        // The center must be opaque. The double-scale bug placed it at (128,128)
        // (off-surface), so (64,64) would be transparent → this assertion fails.
        let center_idx = dcy as usize * SURFACE_WIDTH as usize + dcx as usize;
        assert!(
            pixels[center_idx][3] > 200,
            "C4: scaled circle center must be opaque at device (64,64); got alpha={} — \
             the baked path likely double-scaled the center (rendered it at scale × position)",
            pixels[center_idx][3]
        );

        // The boundary at device center (64,64), radius 20 must match the oracle —
        // proving both correct position AND correct (scaled) radius extent.
        let boundary = boundary_pixel_indices(|px, py| inside_circle(px - dcx, py - dcy, dradius));
        assert!(
            boundary.len() >= 4,
            "C4: fewer than 4 boundary pixels at the expected device position ({}) — \
             the circle is not where the transform says it should be",
            boundary.len()
        );
        let mut failed = 0usize;
        for (pixel_idx, oracle_coverage) in &boundary {
            let a = pixels[*pixel_idx][3];
            let oracle_alpha = (oracle_coverage * 255.0).round() as u8;
            let diff = (i16::from(a) - i16::from(oracle_alpha)).unsigned_abs() as u8;
            if diff > CALIBRATION_TOLERANCE_U8 {
                failed += 1;
            }
        }
        let max_failures = (boundary.len() as f32 * 0.05).ceil() as usize;
        assert!(
            failed <= max_failures,
            "C4 FAILED: {failed}/{} boundary pixels exceed tolerance at device radius 20 — \
             the scaled circle's geometry/position is wrong",
            boundary.len()
        );
    }

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
    #[allow(
        clippy::cast_precision_loss,
        reason = "pixel index → f32 device coordinate over a 128×128 test surface"
    )]
    fn o5_axis_aligned_src_over_rect_and_rrect_byte_identical() {
        let (device, queue) = acquire_test_device_and_queue();

        // ── Rect (sharp-cornered) ──
        let (surface_a, view_a) = create_render_surface(&device);
        let (surface_b, view_b) = create_render_surface(&device);
        clear_surface(&device, &queue, &view_a);
        clear_surface(&device, &queue, &view_b);

        let flat_rect = Rect::from_ltrb(Pixels(20.0), Pixels(15.0), Pixels(108.0), Pixels(113.0));
        let color = Color::rgba(180, 80, 40, 200);

        for (surface, view) in [(&surface_a, &view_a), (&surface_b, &view_b)] {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.rect(flat_rect, &Paint::fill(color));
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

        let rounded_rect_shape = RRect::from_rect_circular(
            Rect::from_ltrb(Pixels(20.0), Pixels(15.0), Pixels(108.0), Pixels(113.0)),
            Pixels(8.0),
        );

        for (surface, view) in [(&surface_c, &view_c), (&surface_d, &view_d)] {
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.rrect(rounded_rect_shape, &Paint::fill(color));
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

    /// O6: The AA band width (in device pixels) must stay approximately 1 device-px
    /// when the same shape is rendered at 1× world scale and at 8× world scale.
    ///
    /// Verifies the fwidth correctness guarantee: `fwidth(dist)` measures the
    /// screen-space derivative of the SDF distance, which equals the reciprocal
    /// of the scale factor, keeping the AA band width constant in device pixels
    /// regardless of world scale.
    ///
    /// Test strategy: render a rotated rect at 1× and 8× scale (the 8× shape is
    /// 8× smaller in local units but the same device footprint). The number of
    /// partial-alpha boundary pixels must be comparable (within a 3× factor).
    /// A hard-aliased implementation would have zero partial-alpha pixels at 8×.
    #[test]
    fn o6_fwidth_aa_band_scale_invariant() {
        let (device, queue) = acquire_test_device_and_queue();

        // Render 1× scale: a 45°-rotated 60×30 rect, scaled at 1:1.
        let (surface_1x, view_1x) = create_render_surface(&device);
        clear_surface(&device, &queue, &view_1x);
        {
            let angle = PI / 4.0;
            let half_w = 30.0_f32;
            let half_h = 15.0_f32;
            let cx = SURFACE_WIDTH as f32 / 2.0;
            let cy = SURFACE_HEIGHT as f32 / 2.0;

            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.translate(flui_types::Offset::new(Pixels(cx), Pixels(cy)));
            painter.rotate(angle);
            painter.rect(
                Rect::from_ltrb(
                    Pixels(-half_w),
                    Pixels(-half_h),
                    Pixels(half_w),
                    Pixels(half_h),
                ),
                &Paint::fill(Color::WHITE),
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("O6 1x Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&view_1x, &surface_1x),
                    &mut encoder,
                )
                .expect("render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        // Render 8× scale: the rect is 8× smaller in local space but occupies
        // the same device footprint (we scale the canvas by 8 and shrink local bounds).
        let (surface_8x, view_8x) = create_render_surface(&device);
        clear_surface(&device, &queue, &view_8x);
        {
            let angle = PI / 4.0;
            // Local bounds shrunk by 8× to compensate the 8× canvas scale.
            let local_half_w = 30.0_f32 / 8.0;
            let local_half_h = 15.0_f32 / 8.0;
            let cx = SURFACE_WIDTH as f32 / 2.0;
            let cy = SURFACE_HEIGHT as f32 / 2.0;

            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.translate(flui_types::Offset::new(Pixels(cx), Pixels(cy)));
            painter.scale(8.0, 8.0);
            painter.rotate(angle);
            painter.rect(
                Rect::from_ltrb(
                    Pixels(-local_half_w),
                    Pixels(-local_half_h),
                    Pixels(local_half_w),
                    Pixels(local_half_h),
                ),
                &Paint::fill(Color::WHITE),
            );
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("O6 8x Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&view_8x, &surface_8x),
                    &mut encoder,
                )
                .expect("render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
        }

        let pixels_1x = readback_pixels(&device, &queue, &surface_1x);
        let pixels_8x = readback_pixels(&device, &queue, &surface_8x);

        // Count boundary (partial-alpha) pixels for each render.
        let partial_1x = pixels_1x.iter().filter(|p| p[3] > 10 && p[3] < 245).count();
        let partial_8x = pixels_8x.iter().filter(|p| p[3] > 10 && p[3] < 245).count();

        assert!(
            partial_1x > 0,
            "O6: 1× render must have partial-alpha boundary pixels (AA must be active)"
        );
        assert!(
            partial_8x > 0,
            "O6: 8× render must have partial-alpha boundary pixels (fwidth must stay ~1px)"
        );

        // The band should be comparable within 3×.  A hard-aliased implementation
        // at 8× scale would have very few (or zero) partial-alpha pixels.
        let ratio = if partial_1x > partial_8x {
            partial_1x as f32 / partial_8x as f32
        } else {
            partial_8x as f32 / partial_1x as f32
        };
        assert!(
            ratio < 3.0,
            "O6: AA band width differs too much between 1× ({partial_1x} pixels) and \
             8× ({partial_8x} pixels) — ratio {ratio:.1}×. fwidth should keep the band ~1 \
             device-px at any scale. A ratio > 3 suggests the AA is NOT scale-invariant."
        );
    }

    // ── O7: Non-uniform corner radii map to the correct screen corners ────────

    /// O7: A rrect with ONLY the top-left corner rounded must round the TOP-LEFT
    /// screen corner and leave the other three sharp.
    ///
    /// Uniform-radii tests (O1/O4) cannot detect a corner-index transposition in
    /// `sdRoundedBox`'s quadrant `select`; production rrects use distinct
    /// per-corner radii, so this pins the `[tl,tr,br,bl]` → screen-corner mapping.
    #[test]
    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "fixed positive device-pixel coordinates on a 128² test surface"
    )]
    fn o7_non_uniform_corner_radii_map_to_correct_corners() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface, view) = create_render_surface(&device);
        clear_surface(&device, &queue, &view);

        // 80×80 axis-aligned rrect, ONLY the top-left corner rounded (r=24);
        // the other three corners are sharp.
        let bounds = Rect::from_ltrb(Pixels(24.0), Pixels(24.0), Pixels(104.0), Pixels(104.0));
        let rrect = RRect::new(
            bounds,
            flui_types::geometry::Radius::circular(Pixels(24.0)), // top-left
            flui_types::geometry::Radius::ZERO,                   // top-right
            flui_types::geometry::Radius::ZERO,                   // bottom-right
            flui_types::geometry::Radius::ZERO,                   // bottom-left
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.rrect(rrect, &Paint::fill(Color::WHITE));
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

    /// A1: A filled SrcOver arc must have RADIAL boundary pixels that match the
    /// analytic-coverage oracle within `CALIBRATION_TOLERANCE_U8` at two radii
    /// spanning a ~4× range: 12 px and 50 px.
    ///
    /// ## Red→green proof
    ///
    /// The OLD `edge_softness = 0.02` (radius-relative) model produced an AA band
    /// that scaled with the radius — the same bug class as the old circle shader:
    ///   - r=12: AA band = 0.02*12*2 = 0.48 px → sub-pixel → nearly aliased.
    ///   - r=50: AA band = 0.02*50*2 = 2 px → too wide.
    ///
    /// The NEW `fwidth(length(unit_pos) - 1.0)` model gives ~1 device-px AA at
    /// any radius, so both pass the oracle.
    ///
    /// The arc used is a 270° sweep (wide arc) so most of the circle boundary
    /// is present; the angular edges are kept away from the boundary sample pixels.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
    fn a1_arc_radial_aa_is_radius_independent() {
        for radius in [12.0_f32, 50.0_f32] {
            let (device, queue) = acquire_test_device_and_queue();
            let (surface_texture, surface_view) = create_render_surface(&device);
            clear_surface(&device, &queue, &surface_view);

            let cx = SURFACE_WIDTH as f32 / 2.0;
            let cy = SURFACE_HEIGHT as f32 / 2.0;

            // 270° arc starting at 0 (right), sweeping CW — large arc so the
            // radial boundary has many boundary pixels.
            let start = 0.0_f32;
            let sweep = 3.0 * std::f32::consts::FRAC_PI_2; // 270°

            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            let rect = Rect::from_xywh(
                flui_types::geometry::Pixels(cx - radius),
                flui_types::geometry::Pixels(cy - radius),
                px(radius * 2.0),
                px(radius * 2.0),
            );
            painter.draw_arc(rect, start, sweep, true, &Paint::fill(Color::WHITE));

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("A1 Arc Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&surface_view, &surface_texture),
                    &mut encoder,
                )
                .expect("painter.render must succeed");
            queue.submit(std::iter::once(encoder.finish()));

            let pixels = readback_pixels(&device, &queue, &surface_texture);

            // Boundary pixels: only the RADIAL edge (exclude angular edge vicinity).
            // The angular edges are at angle=0 (+X) and angle=3π/2 (+Y rotated back
            // to 270° = −Y direction = pointing up). Exclude sectors near those edges.
            // Angular exclusion: skip samples within ±10° of the angular cut edges.
            let angular_exclusion_rad = 10.0_f32 * std::f32::consts::PI / 180.0;
            let end_angle = start + sweep;
            let boundary =
                boundary_pixel_indices(|px, py| inside_arc(px - cx, py - cy, radius, start, sweep));

            // Filter to radial-only boundary pixels (not near angular edges).
            let radial_boundary: Vec<(usize, f32)> = boundary
                .into_iter()
                .filter(|(pixel_idx, _)| {
                    let col = (*pixel_idx % SURFACE_WIDTH as usize) as f32 + 0.5 - cx;
                    let row = (*pixel_idx / SURFACE_WIDTH as usize) as f32 + 0.5 - cy;
                    // Skip pixels near angular edges.
                    let sample_angle = row.atan2(col);
                    let dist_to_start = angle_diff_abs(sample_angle, start);
                    let dist_to_end = angle_diff_abs(sample_angle, end_angle);
                    // Only keep pixels whose radial distance is close to the arc edge
                    // (not near the angular cut).
                    let r = (col * col + row * row).sqrt();
                    let near_radial_edge = (r - radius).abs() < 2.0;
                    near_radial_edge
                        && dist_to_start > angular_exclusion_rad
                        && dist_to_end > angular_exclusion_rad
                })
                .collect();

            assert!(
                radial_boundary.len() >= 4,
                "A1 r={radius}: fewer than 4 radial boundary pixels ({}) — shape may be \
                 off-screen or oracle broken",
                radial_boundary.len()
            );

            let mut failed_count = 0usize;
            for (pixel_idx, oracle_coverage) in &radial_boundary {
                let readback_alpha = pixels[*pixel_idx][3];
                let oracle_alpha = (oracle_coverage * 255.0).round() as u8;
                let diff =
                    (i16::from(readback_alpha) - i16::from(oracle_alpha)).unsigned_abs() as u8;
                if diff > CALIBRATION_TOLERANCE_U8 {
                    failed_count += 1;
                }
            }

            let boundary_count = radial_boundary.len();
            let max_failures = (boundary_count as f32 * 0.1).ceil() as usize;
            assert!(
                failed_count <= max_failures,
                "A1 FAILED at r={radius}: {failed_count}/{boundary_count} radial boundary pixels \
                 exceed tolerance {CALIBRATION_TOLERANCE_U8}. The fwidth radial AA must give ~1 \
                 device-px AA at all radii — old edge_softness=0.02 would fail at r=12 and r=50."
            );
        }
    }

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
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
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
        painter.translate(flui_types::Offset::new(
            flui_types::geometry::Pixels(cx),
            flui_types::geometry::Pixels(cy),
        ));
        painter.rotate(angle);
        // The arc rect is centered at origin in local space.
        let rect = Rect::from_xywh(
            flui_types::geometry::Pixels(-radius),
            flui_types::geometry::Pixels(-radius),
            px(radius * 2.0),
            px(radius * 2.0),
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

    /// A3: Under a non-unit canvas scale, an arc's CENTER must land at the
    /// transformed position — NOT at scale × position.
    ///
    /// This is the exact PR-2 double-scale regression guard applied to arcs.
    /// A circle with `scale(2,2)` at local (32,32) must appear at device (64,64)
    /// with device radius 20. The old `center_radius.xy`-in-local bug would place
    /// the center at (128,128) — off this 128² surface — leaving (64,64) empty.
    ///
    /// Note: C1–C3 use identity scale and cannot catch this; production hits it on
    /// every HiDPI (DPR>1) display.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
    fn a3_scaled_arc_center_not_double_scaled() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.scale(2.0, 2.0);
        // Local arc: center at (32,32), radius=10. Under scale(2,2) the device
        // center is (64,64) and device radius is 20.
        let local_radius = 10.0_f32;
        let rect = Rect::from_xywh(
            flui_types::geometry::Pixels(32.0 - local_radius),
            flui_types::geometry::Pixels(32.0 - local_radius),
            px(local_radius * 2.0),
            px(local_radius * 2.0),
        );
        painter.draw_arc(
            rect,
            0.0,
            3.0 * std::f32::consts::FRAC_PI_2, // 270°
            true,
            &Paint::fill(Color::WHITE),
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("A3 Scaled Arc Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Expected device geometry: center (64,64), radius 20.
        let dcx = 64.0_f32;
        let dcy = 64.0_f32;
        let dradius = 20.0_f32;

        // The center must be opaque. The double-scale bug placed it at (128,128)
        // (off-surface), so (64,64) would be transparent → this assertion fails.
        let center_idx = dcy as usize * SURFACE_WIDTH as usize + dcx as usize;
        assert!(
            pixels[center_idx][3] > 200,
            "A3: scaled arc center must be opaque at device (64,64); got alpha={} — \
             the baked path likely double-scaled the center (rendered it at scale × position)",
            pixels[center_idx][3]
        );

        // The boundary at device center (64,64), radius 20 must have boundary pixels
        // — proving both correct position AND correct (scaled) radius extent.
        let start = 0.0_f32;
        let sweep = 3.0 * std::f32::consts::FRAC_PI_2;
        let end_angle = start + sweep;
        let angular_excl = 15.0_f32 * std::f32::consts::PI / 180.0;
        let radial_boundary: Vec<(usize, f32)> = {
            let all = boundary_pixel_indices(|px, py| {
                inside_arc(px - dcx, py - dcy, dradius, start, sweep)
            });
            all.into_iter()
                .filter(|(pixel_idx, _)| {
                    let col = (*pixel_idx % SURFACE_WIDTH as usize) as f32 + 0.5 - dcx;
                    let row = (*pixel_idx / SURFACE_WIDTH as usize) as f32 + 0.5 - dcy;
                    let r = (col * col + row * row).sqrt();
                    let ang = row.atan2(col);
                    (r - dradius).abs() < 2.0
                        && angle_diff_abs(ang, start) > angular_excl
                        && angle_diff_abs(ang, end_angle) > angular_excl
                })
                .collect()
        };

        assert!(
            radial_boundary.len() >= 4,
            "A3: fewer than 4 radial boundary pixels at expected device position ({}) — \
             the arc is not where the transform says it should be",
            radial_boundary.len()
        );

        let mut failed = 0usize;
        for (pixel_idx, oracle_coverage) in &radial_boundary {
            let a = pixels[*pixel_idx][3];
            let oracle_alpha = (oracle_coverage * 255.0).round() as u8;
            let diff = (i16::from(a) - i16::from(oracle_alpha)).unsigned_abs() as u8;
            if diff > CALIBRATION_TOLERANCE_U8 {
                failed += 1;
            }
        }
        let max_failures = (radial_boundary.len() as f32 * 0.1).ceil() as usize;
        assert!(
            failed <= max_failures,
            "A3 FAILED: {failed}/{} radial boundary pixels exceed tolerance at device radius 20 — \
             the scaled arc's geometry/position is wrong",
            radial_boundary.len()
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
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
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
        painter.translate(flui_types::Offset::new(
            flui_types::geometry::Pixels(cx),
            flui_types::geometry::Pixels(cy),
        ));
        painter.rotate(rotation);
        let rect = Rect::from_xywh(
            flui_types::geometry::Pixels(-radius),
            flui_types::geometry::Pixels(-radius),
            px(radius * 2.0),
            px(radius * 2.0),
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
}
