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

    use flui_painting::{BlendMode, Paint};
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
        crate::wgpu::readback_dump::dump_frame(
            SURFACE_WIDTH,
            SURFACE_HEIGHT,
            bytemuck::cast_slice(&pixels),
        );
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
    #[allow(clippy::too_many_arguments)]
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
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
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

        let mut path = flui_types::painting::path::Path::new();
        path.move_to(flui_types::Point::new(Pixels(ax), Pixels(ay)));
        path.line_to(flui_types::Point::new(Pixels(bx), Pixels(by)));
        path.line_to(flui_types::Point::new(Pixels(dx), Pixels(dy)));
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
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
    fn p2_ssaa_fill_rule_honored_nonzero_vs_evenodd() {
        use flui_types::painting::PathFillType;

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
            let mut path = flui_types::painting::path::Path::with_fill_type(fill_type);
            let (first_x, first_y) = tips[pentagram_order[0]];
            path.move_to(flui_types::Point::new(Pixels(first_x), Pixels(first_y)));
            for &idx in &pentagram_order[1..] {
                let (x, y) = tips[idx];
                path.line_to(flui_types::Point::new(Pixels(x), Pixels(y)));
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

    /// P3: A triangle path rendered at 1× world scale and at 4× world scale
    /// must both produce comparable numbers of partial-alpha boundary pixels,
    /// proving the SSAA box-downsample produces a ~1-device-px AA band
    /// regardless of the local-space scale factor.
    ///
    /// ## Why this matters
    ///
    /// SSAA samples the 2× texture at sub-pixel positions and averages.  The AA
    /// band width in device pixels is set by the 2× factor and is scale-
    /// invariant (the 2× texture always captures 2 sub-pixels per output pixel
    /// regardless of how large the shape is in local space).  At 4× canvas
    /// scale the local-space triangle is 4× smaller to produce the same device
    /// footprint, but the tile is still 2× the output resolution → same band.
    ///
    /// A hard-aliased fallback would produce binary (0/255) pixels at any scale
    /// and would have zero partial-alpha pixels — this test would catch that.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
    fn p3_ssaa_aa_band_scale_invariant() {
        // Same device-space triangle at two scales.
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;

        // Device triangle (same for both renders):
        let ax = cx;
        let ay = cy - 35.0;
        let bx = cx + 30.0;
        let by = cy + 20.0;
        let dx = cx - 30.0;
        let dy = cy + 20.0;

        let render_triangle_at_scale = |scale: f32| -> Vec<[u8; 4]> {
            let (device, queue) = acquire_test_device_and_queue();
            let (surface, view) = create_render_surface(&device);
            clear_surface(&device, &queue, &view);

            // Shrink local coords by `scale` so the device footprint stays the same.
            let lax = (ax - cx) / scale + cx;
            let lay = (ay - cy) / scale + cy;
            let lbx = (bx - cx) / scale + cx;
            let lby = (by - cy) / scale + cy;
            let ldx = (dx - cx) / scale + cx;
            let ldy = (dy - cy) / scale + cy;

            let mut path = flui_types::painting::path::Path::new();
            path.move_to(flui_types::Point::new(Pixels(lax), Pixels(lay)));
            path.line_to(flui_types::Point::new(Pixels(lbx), Pixels(lby)));
            path.line_to(flui_types::Point::new(Pixels(ldx), Pixels(ldy)));
            path.close();

            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            // Apply the compensating world scale so device geometry = target triangle.
            painter.translate(flui_types::Offset::new(Pixels(cx), Pixels(cy)));
            painter.scale(scale, scale);
            painter.translate(flui_types::Offset::new(Pixels(-cx), Pixels(-cy)));
            painter.draw_path(&path, &Paint::fill(Color::WHITE));

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("P3 Scale Encoder"),
            });
            painter
                .render(RenderTarget::sampleable(&view, &surface), &mut encoder)
                .expect("painter.render must succeed");
            queue.submit(std::iter::once(encoder.finish()));

            readback_pixels(&device, &queue, &surface)
        };

        let pixels_1x = render_triangle_at_scale(1.0);
        let pixels_4x = render_triangle_at_scale(4.0);

        // Count partial-alpha boundary pixels (the SSAA AA band).
        let partial_1x = pixels_1x.iter().filter(|p| p[3] > 5 && p[3] < 250).count();
        let partial_4x = pixels_4x.iter().filter(|p| p[3] > 5 && p[3] < 250).count();

        assert!(
            partial_1x > 0,
            "P3: 1× render must have partial-alpha boundary pixels"
        );
        assert!(
            partial_4x > 0,
            "P3: 4× render must have partial-alpha boundary pixels — SSAA must produce AA at \
             any canvas scale"
        );

        // The two band widths must be close: the device footprint is IDENTICAL at
        // both scales (local coords are pre-shrunk by `scale`), so a scale-invariant
        // SSAA band should yield near-identical partial-pixel counts — well within
        // 2×. A hard-aliased renderer has zero partials; a band that collapses at
        // high scale (e.g. a missing CTM update) pushes the ratio past 2×.
        let ratio = if partial_1x > partial_4x {
            partial_1x as f32 / partial_4x as f32
        } else {
            partial_4x as f32 / partial_1x as f32
        };
        assert!(
            ratio < 2.0,
            "P3: AA band width differs too much between 1× ({partial_1x} pixels) and \
             4× ({partial_4x} pixels) — ratio {ratio:.1}×. SSAA should give ~1 device-px AA \
             at any scale; a large ratio means the band is collapsing at high scale."
        );
    }

    // ── P4: anti-MVP — SrcOver Fill path must emit ≥1 partial-alpha pixel ────

    /// P4: A SrcOver-filled arbitrary path (the canonical SSAA target) must
    /// emit at least one partial-alpha pixel on its geometric boundary —
    /// proving the SSAA reroute actually executes the GPU path rather than
    /// silently being a no-op or returning a stub opaque tile.
    ///
    /// ## Anti-MVP rationale (Definition of Done §anti-cheating)
    ///
    /// A minimum-viable implementation that tessellates the path and submits it
    /// directly (bypassing SSAA) would pass P1–P3 only if the tessellated geometry
    /// happens to be AA'd by some other mechanism.  P4 is a targeted falsification:
    /// it checks the ONE property that a fake SSAA implementation without a real
    /// 2× render cannot produce — partial alpha at the exact pixel positions the
    /// oracle identifies as boundary, EVEN for a shape that is far from axis-aligned.
    ///
    /// A non-axis-aligned triangle rendered without SSAA or SDF produces hard edges
    /// (boundary pixels are either 0 or 255) because `shape.wgsl` has no distance
    /// field for tessellated geometry.  With SSAA the 2× offscreen capture resolves
    /// sub-pixel coverage and the downsample produces genuine fractional alpha.
    ///
    /// Test structure: explicitly disabled oracle (we do NOT check oracle closeness
    /// here — only existence of partial alpha), so it cannot degenerate to "check
    /// nothing" even if the oracle boundary is small.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index/coordinate arithmetic over a small fixed-size test surface"
    )]
    fn p4_anti_mvp_srcover_fill_has_partial_alpha_at_boundary() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        // Deliberately non-axis-aligned irregular quadrilateral so that EVERY
        // edge is diagonal and thus has fractional-coverage boundary pixels under
        // 2× SSAA.  An axis-aligned rectangle edge falls on a pixel row/column and
        // can produce zero partial pixels (all boundary pixels round to 0 or 255).
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        // Irregular kite: four vertices, none axis-aligned relative to each other.
        let v = [
            (cx + 1.3, cy - 38.7), // near-top, slightly off-center
            (cx + 42.2, cy + 5.5), // right
            (cx - 2.7, cy + 33.1), // near-bottom
            (cx - 38.4, cy - 9.2), // left
        ];

        let mut path = flui_types::painting::path::Path::new();
        path.move_to(flui_types::Point::new(Pixels(v[0].0), Pixels(v[0].1)));
        for &(x, y) in &v[1..] {
            path.line_to(flui_types::Point::new(Pixels(x), Pixels(y)));
        }
        path.close();

        // SrcOver fill — the ONLY variant that the SSAA reroute handles.
        let paint = Paint::fill(Color::WHITE); // blend_mode defaults to SrcOver

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.draw_path(&path, &paint);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("P4 Anti-MVP Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Count partial-alpha pixels anywhere on the surface.
        // Any partial-alpha pixel is evidence of sub-pixel coverage resolution —
        // it is impossible to produce with a hard-aliased (0/255) tessellated renderer.
        let partial_count = pixels.iter().filter(|p| p[3] > 5 && p[3] < 250).count();

        assert!(
            partial_count >= 4,
            "P4 FAILED: only {partial_count} partial-alpha pixels in the frame — the SSAA \
             reroute appears to be a no-op or hard-aliased. A real 2× SSAA box-downsample \
             MUST produce sub-pixel coverage (partial alpha) on EVERY non-axis-aligned edge."
        );

        // Also confirm the shape is actually rendered (interior is opaque, not blank).
        let interior_col = cx as usize;
        let interior_row = cy as usize;
        let interior_idx = interior_row * SURFACE_WIDTH as usize + interior_col;
        assert!(
            pixels[interior_idx][3] > 200,
            "P4: shape interior (cx, cy) must be opaque; got alpha={} — \
             the SSAA composite path may be broken",
            pixels[interior_idx][3]
        );
    }

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

        let full = flui_types::Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        {
            // Through the SAME `Backend` adapter the layer-tree walk uses —
            // draws arrive as `render_*(…, transform)` with per-command
            // matrices, exactly the collapsed-app-bar stream: translated
            // row rects + glyphs first, then the surface path at identity.
            use crate::traits::CommandRenderer;
            let mut backend = super::super::backend::Backend::new(&mut painter);
            // The real stream runs inside the viewport's clip.
            backend.clip_rect(
                full,
                flui_types::painting::ClipOp::Intersect,
                flui_types::painting::Clip::HardEdge,
                &flui_types::Matrix4::IDENTITY,
            );
            // The failing frame's exact tail: the row rect and the surface
            // path share IDENTITY transforms and the SAME rectangle.
            backend.render_rect(
                full,
                &Paint::fill(Color::rgb(255, 0, 0)),
                &flui_types::Matrix4::IDENTITY,
            );
            // Color rides the STYLE: `render_text` derives its paint from
            // `style.color` and ignores the paint parameter.
            let blue_style = flui_types::typography::TextStyle {
                color: Some(Color::rgb(0, 0, 255)),
                ..flui_types::typography::TextStyle::default()
            };
            backend.render_text(
                "Row",
                flui_types::Offset::new(Pixels(4.0), Pixels(30.0)),
                &blue_style,
                &Paint::fill(Color::rgb(0, 0, 255)),
                &flui_types::Matrix4::IDENTITY,
            );
            let path = flui_types::painting::path::Path::rectangle(full);
            backend.render_path(
                &path,
                &Paint::fill(Color::rgb(0, 255, 0)),
                &flui_types::Matrix4::IDENTITY,
            );
            let yellow_style = flui_types::typography::TextStyle {
                color: Some(Color::rgb(255, 255, 0)),
                ..flui_types::typography::TextStyle::default()
            };
            backend.render_text(
                "Title",
                flui_types::Offset::new(Pixels(4.0), Pixels(50.0)),
                &yellow_style,
                &Paint::fill(Color::rgb(255, 255, 0)),
                &flui_types::Matrix4::IDENTITY,
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

        let full = flui_types::Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        {
            use crate::traits::CommandRenderer;
            let mut backend = super::super::backend::Backend::new(&mut painter);
            let blue_style = flui_types::typography::TextStyle {
                color: Some(Color::rgb(0, 0, 255)),
                ..flui_types::typography::TextStyle::default()
            };
            backend.render_text(
                "Covered",
                flui_types::Offset::new(Pixels(4.0), Pixels(30.0)),
                &blue_style,
                &Paint::fill(Color::rgb(0, 0, 255)),
                &flui_types::Matrix4::IDENTITY,
            );
            // A plain instanced rect — the route that does NOT split the
            // segment on its own.
            backend.render_rect(
                full,
                &Paint::fill(Color::rgb(0, 255, 0)),
                &flui_types::Matrix4::IDENTITY,
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

    /// Text drawn INSIDE an opacity layer, followed by covering geometry
    /// inside the SAME layer, must be buried by that geometry — the layer's
    /// own segments carry glyph ranges stamped at record time, and the
    /// recursive offscreen replay must range-render them at their
    /// z-position inside the layer, not leave them to the trailing gap
    /// passes (which draw over everything on the main target, at full
    /// opacity, ignoring the layer's own opacity).
    ///
    /// Born red: before the recursion range-rendered text, the pinned form
    /// of this test observed the covered label floating above the later
    /// fill (blue glyph pixels > 0).
    #[test]
    fn later_rect_covers_earlier_text_inside_an_opacity_layer() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let full = flui_types::Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Alpha 250/255 ≈ 0.98 ≠ 1.0 forces the offscreen Composite path
        // (the Reintegrate fast-path would splice the content back into the
        // top-level draw order and never exercise the recursion).
        painter.save_layer(None, &Paint::fill(Color::rgba(0, 0, 0, 250)));
        painter.text(
            "Covered",
            flui_types::Point::new(Pixels(4.0), Pixels(30.0)),
            20.0,
            &Paint::fill(Color::rgb(0, 0, 255)),
        );
        painter.rect(full, &Paint::fill(Color::rgb(0, 255, 0)));
        painter.restore_layer();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Layer Text Order Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("painter.render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);
        // Count pure-blue glyph pixels over the whole region — center-pixel
        // sampling misses this class.
        let blue_pixels = pixels
            .iter()
            .filter(|p| p[2] > 200 && p[0] < 50 && p[1] < 50)
            .count();
        assert_eq!(
            blue_pixels, 0,
            "text followed by covering geometry inside the same opacity \
             layer must be buried by that geometry; {blue_pixels} blue \
             glyph pixels visible — the layer's glyphs composited out of \
             draw order (trailing gap pass instead of in-layer range render)"
        );
        // The layer's fill must still composite (sanity that the layer path ran).
        let center =
            (SURFACE_HEIGHT as usize / 2) * SURFACE_WIDTH as usize + SURFACE_WIDTH as usize / 2;
        let [r, g, _b, a] = pixels[center];
        assert!(
            g > 200 && r < 50 && a > 200,
            "the layer's green fill must composite onto the surface; got center rgba=({r}, {g}, _, {a})"
        );
    }

    /// Text obeys an active clip.
    ///
    /// Every glyph run in a frame is handed to glyphon with the SAME
    /// `TextBounds`, built from the whole viewport, and the text pass sets no
    /// scissor — so no clip of any kind reaches text. A `ListView`'s rows paint
    /// straight through the app bar above them.
    ///
    /// Scope: this is the RECTANGULAR clip. A `ClipRRect` still leaks its
    /// label past the rounded corner, because `current_scissor()` carries only
    /// the rounded rect's bounding box — the per-corner SDF lives in a separate
    /// slot that no glyph pipeline reads. Text should join the shared clip
    /// uniform when that lands rather than grow a second clip model.
    ///
    /// Driven through `WgpuPainter`, deliberately: the clip is discarded at the
    /// RECORD seam, so a test that calls `TextRenderer::add_text` directly with
    /// a hand-built bound would pass with the production read reverted.
    ///
    /// If reverted (`build_text_areas` back to a shared whole-viewport bound):
    /// glyph pixels appear below the clip and this fails.
    #[test]
    fn text_is_clipped_by_the_active_clip_rect() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        const CLIP_BOTTOM: f32 = 24.0;

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save();
        painter.clip_rect(flui_types::Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(CLIP_BOTTOM),
        ));
        // Positioned so the run straddles the clip edge: some of it is legally
        // inside, the rest must be cut.
        painter.text(
            "Spill",
            flui_types::Point::new(Pixels(4.0), Pixels(12.0)),
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
            painter.text(
                "Ab",
                flui_types::Point::new(Pixels(4.0), Pixels(10.0)),
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

    /// KNOWN LIMIT, pinned: a non-uniform transform stretches glyphs uniformly.
    ///
    /// `TextPlacement::scale` is one `f32` because `glyphon::TextArea::scale`
    /// is one `f32`; glyphon 0.11 cannot express a different scale per axis. So
    /// under `scale(2.0, 1.0)` the larger axis wins and a run is stretched both
    /// ways, while the geometry around it stretches one way.
    ///
    /// This is not a regression — before, glyphs ignored the transform entirely
    /// and were wrong on both axes — and it does not affect the case the scale
    /// exists for, the device-pixel ratio, which is uniform by construction.
    ///
    /// It is pinned rather than left implicit so that carrying a real transform
    /// for text (a per-area matrix, or routing such runs through the
    /// tessellated path) is a deliberate change: this test will fail, and
    /// whoever makes it should read the note on `TextPlacement::scale` and
    /// replace the assertion with the correct anisotropic one.
    #[test]
    fn anisotropic_scale_stretches_glyphs_uniformly() {
        fn ink(sx: f32, sy: f32) -> usize {
            let (device, queue) = acquire_test_device_and_queue();
            let (surface_texture, surface_view) = create_render_surface(&device);
            clear_surface(&device, &queue, &surface_view);

            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            painter.save();
            painter.scale(sx, sy);
            painter.text(
                "Ab",
                flui_types::Point::new(Pixels(4.0), Pixels(10.0)),
                16.0,
                &Paint::fill(Color::rgb(0, 0, 255)),
            );
            painter.restore();

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Anisotropic Text Encoder"),
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

        let uniform_2x = ink(2.0, 2.0);
        let wide_only = ink(2.0, 1.0);
        assert!(uniform_2x > 0, "precondition: the label paints");

        // Correct anisotropic behaviour would put `wide_only` near HALF of
        // `uniform_2x` (twice the width, unchanged height). It sits near
        // parity instead, because `max_scale()` reports 2.0 for both.
        let ratio = wide_only as f32 / uniform_2x as f32;
        assert!(
            ratio > 0.75,
            "known limit: scale(2,1) is expected to stretch glyphs like scale(2,2) \
             until text can carry a real transform; got {wide_only}/{uniform_2x} = \
             {ratio:.2} — if this now reports ~0.5, anisotropic scaling started \
             working and this test should be replaced, not relaxed"
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
        painter.clip_rrect(flui_types::geometry::RRect::from_rect_circular(
            flui_types::Rect::from_xywh(
                Pixels(0.0),
                Pixels(0.0),
                Pixels(SURFACE_WIDTH as f32),
                Pixels(SURFACE_HEIGHT as f32),
            ),
            Pixels(R),
        ));
        // Radius 90 about the centre of a 128x128 surface: every corner of the
        // surface is well inside this circle.
        painter.circle(
            flui_types::Point::new(
                Pixels(SURFACE_WIDTH as f32 / 2.0),
                Pixels(SURFACE_HEIGHT as f32 / 2.0),
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

    /// A gradient-filled rrect rounds the corner the caller asked for.
    ///
    /// The gradient shaders carried their own `sdRoundedBox` whose quadrant
    /// selection was a permutation of the canonical one:
    ///
    /// ```text
    /// canonical: r2 = select(vec2(r.x, r.w), vec2(r.y, r.z), p.x > 0.0)
    ///            r3 = select(r2.x, r2.y, p.y > 0.0)
    /// gradients: r2 = select(r.zw, r.xy, p.x > 0.0)
    ///            r3 = select(r2.y, r2.x, p.y > 0.0)
    /// ```
    ///
    /// With `r = [tl, tr, br, bl]` the gradient form resolves top-left to `bl`,
    /// bottom-left to `br`, and bottom-right to `tl` — only top-right is right.
    /// Four equal radii hide it completely, which is why it survived: the
    /// single-`f32` `gradient_rect` entry point passes `[r; 4]`. It becomes
    /// visible through `dispatch_shader_rect`, which carries genuinely
    /// per-corner radii from `RRect` (`batches/shapes.rs`) — a card rounded on
    /// top only, filled with a gradient.
    ///
    /// Geometry: only the BOTTOM-LEFT corner is rounded, and hugely (56 px on a
    /// 128 px surface). Sampling the bottom-left corner pixel distinguishes the
    /// two: correct → cut away; permuted → bottom-left resolves to `br` = 0,
    /// square, painted.
    #[test]
    fn a_gradient_rrect_rounds_the_corner_it_was_given() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        const R: f32 = 56.0;
        let full = flui_types::Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // [tl, tr, br, bl] = [0, 0, 0, R] — bottom-left only.
        use flui_types::geometry::{RRect, Radius};
        let zero = Radius::circular(Pixels(0.0));
        let rrect = RRect::new(full, zero, zero, zero, Radius::circular(Pixels(R)));

        // The base colour is RED and the gradient is BLUE on purpose: a blue
        // centre pixel proves the gradient pipeline ran at all, so a failure
        // here cannot be the tessellated fallback quietly standing in.
        let mut paint = Paint::fill(Color::rgb(255, 0, 0));
        paint.shader = Some(flui_types::painting::Shader::LinearGradient {
            from: flui_types::Offset::new(Pixels(0.0), Pixels(0.0)),
            to: flui_types::Offset::new(Pixels(0.0), Pixels(SURFACE_HEIGHT as f32)),
            colors: vec![Color::rgb(0, 0, 255), Color::rgb(0, 0, 255)],
            stops: Some(vec![0.0, 1.0]),
            tile_mode: flui_types::painting::TileMode::Clamp,
        });
        painter.rrect(rrect, &paint);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Gradient RRect Corner Encoder"),
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
        let h = SURFACE_HEIGHT as usize;
        let at = |x: usize, y: usize| pixels[y * w + x];

        let centre = at(w / 2, h / 2);
        assert!(
            centre[2] > 200 && centre[0] < 64,
            "precondition: the centre must be the gradient's BLUE, not the base \
             RED — a red centre means the gradient pipeline never ran and this \
             test is measuring the tessellated fallback; got {centre:?}"
        );

        // Top-left: radius 0, so this corner stays square and painted. Without
        // it the test would pass on a shader that rounded every corner.
        let top_left = at(2, 2);
        assert!(
            top_left[3] > 200,
            "precondition: the top-left corner has radius 0 and must stay \
             square; got {top_left:?}"
        );

        // Bottom-left: radius R, so this pixel is outside the shape.
        let bottom_left = at(2, h - 3);
        assert!(
            bottom_left[3] < 32,
            "the bottom-left corner was given radius {R} and must be cut away; \
             got rgba={bottom_left:?} — the gradient shader resolved this \
             corner to a different entry of [tl, tr, br, bl]"
        );
    }

    /// A rounded clip actually rounds a gradient fill.
    ///
    /// The same gap circles had, one primitive over: the gradient instances did
    /// not implement `ClippableInstance`, so a gradient received only the
    /// clip's axis-aligned bounding scissor and a `ClipRRect` around a gradient
    /// card left square corners.
    ///
    /// The geometry avoids the coincide-with-the-clip trap: the gradient is
    /// drawn with SQUARE corners over the whole surface, so a sampled corner
    /// pixel is inside the drawn shape and outside the clip. A gradient whose
    /// own `corner_radius` matched the clip would pass with no clip support at
    /// all.
    ///
    /// Reverted (drop `apply_active_clip` at the gradient record sites, or the
    /// `clipAlpha` call in the gradient shaders): the corner is painted and
    /// this fails.
    #[test]
    fn a_rounded_clip_rounds_a_gradient() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        const R: f32 = 40.0;
        let full = flui_types::Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save();
        painter.clip_rrect(flui_types::geometry::RRect::from_rect_circular(
            full,
            Pixels(R),
        ));
        painter.gradient_rect(
            full,
            glam::Vec2::new(0.0, 0.0),
            glam::Vec2::new(0.0, SURFACE_HEIGHT as f32),
            &[
                crate::wgpu::effects::GradientStop::start(Color::rgb(0, 0, 255)),
                crate::wgpu::effects::GradientStop::end(Color::rgb(0, 0, 255)),
            ],
            // Square corners: the clip, not the shape, is what must round this.
            0.0,
        );
        painter.restore();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Gradient Clip Encoder"),
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

        let centre = at(w / 2, SURFACE_HEIGHT as usize / 2);
        assert!(
            centre[2] > 200,
            "precondition: the gradient must paint inside the clip; got {centre:?}"
        );

        let corner = at(3, 3);
        assert!(
            corner[3] < 32,
            "a rounded clip must round the gradient inside it; corner rgba={corner:?} \
             — the gradient received only the clip's bounding scissor"
        );
    }

    /// A rounded clip actually rounds tessellated geometry.
    ///
    /// Strokes and path fills never reach the instanced SDF path — they are
    /// tessellated by lyon and drawn through `shape.wgsl`, which receives only
    /// the clip's axis-aligned bounding scissor. A stroked border inside a
    /// `ClipRRect` therefore keeps square corners while the rrect fill beside
    /// it is correctly rounded.
    ///
    /// Geometry: a stroke wide enough to cover the whole surface, so a corner
    /// pixel is inside the stroked band and outside the clip. The stroke is
    /// centred on the surface so it cannot coincide with the clip's own
    /// boundary — a stroke that merely traced the clip would leave the corner
    /// unpainted with no clip support at all.
    ///
    /// Reverted (drop the clip uniform from the tessellated draw, or the
    /// `clipAlpha` call in `shape.wgsl`): the corner is painted and this fails.
    #[test]
    fn a_rounded_clip_rounds_tessellated_geometry() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        const R: f32 = 40.0;
        let full = flui_types::Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save();
        painter.clip_rrect(flui_types::geometry::RRect::from_rect_circular(
            full,
            Pixels(R),
        ));
        // A stroke this wide covers the surface, corners included. `Paint`
        // with a Stroke style routes through the tessellator, not the
        // instanced rect path.
        painter.rect(
            flui_types::Rect::from_xywh(
                Pixels(SURFACE_WIDTH as f32 / 2.0),
                Pixels(SURFACE_HEIGHT as f32 / 2.0),
                Pixels(1.0),
                Pixels(1.0),
            ),
            &Paint::stroke(Color::rgb(0, 0, 255), SURFACE_WIDTH as f32 * 2.0),
        );
        painter.restore();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Tess Clip Encoder"),
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

        let centre = at(w / 2, SURFACE_HEIGHT as usize / 2);
        assert!(
            centre[2] > 200,
            "precondition: the stroke must paint inside the clip; got {centre:?}"
        );

        let corner = at(3, 3);
        assert!(
            corner[3] < 32,
            "a rounded clip must round tessellated geometry inside it; corner \
             rgba={corner:?} — the stroke received only the clip's bounding scissor"
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
        let region =
            flui_types::Rect::from_xywh(Pixels(48.0), Pixels(48.0), Pixels(80.0), Pixels(80.0));

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save();
        painter.clip_rrect(flui_types::geometry::RRect::from_rect_circular(
            region,
            Pixels(40.0),
        ));
        // A FILL, and a large one: the SSAA gate reads the rect's own area
        // (80x80 = 6400 px², over the 256 px² threshold), not the painted
        // area — a hairline rect with a huge stroke width stays under it and
        // silently takes the direct path instead.
        painter.rect(
            region,
            &Paint::fill(Color::rgb(0, 0, 255))
                .with_blend_mode(flui_types::painting::BlendMode::Plus),
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

        let full = flui_types::Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.save_layer(None, &Paint::fill(Color::rgba(0, 0, 0, 250)));
        painter.text(
            "Ghost",
            flui_types::Point::new(Pixels(4.0), Pixels(30.0)),
            20.0,
            &Paint::fill(Color::rgb(0, 0, 255)),
        );
        painter.restore_layer();
        // Top-level opaque fill AFTER the layer — must bury the layer's text.
        painter.rect(full, &Paint::fill(Color::rgb(0, 255, 0)));

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

    /// Complement guard: text ordering ACROSS the opacity boundary must not
    /// regress. Top-level text recorded BEFORE the layer is buried by the
    /// layer's fill; top-level text recorded AFTER the layer draws over it.
    #[test]
    fn text_ordering_across_an_opacity_layer_boundary() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let full = flui_types::Rect::from_xywh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(SURFACE_WIDTH as f32),
            Pixels(SURFACE_HEIGHT as f32),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        // Top-level text BEFORE the layer — pure blue.
        painter.text(
            "Under",
            flui_types::Point::new(Pixels(4.0), Pixels(30.0)),
            20.0,
            &Paint::fill(Color::rgb(0, 0, 255)),
        );
        // Composite-path layer whose fill covers the whole surface.
        painter.save_layer(None, &Paint::fill(Color::rgba(0, 0, 0, 250)));
        painter.rect(full, &Paint::fill(Color::rgb(0, 255, 0)));
        painter.restore_layer();
        // Top-level text AFTER the layer — yellow, must stay visible.
        painter.text(
            "Over",
            flui_types::Point::new(Pixels(4.0), Pixels(60.0)),
            20.0,
            &Paint::fill(Color::rgb(255, 255, 0)),
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Boundary Text Order Encoder"),
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
            "top-level text recorded before the layer must be buried by the \
             layer's fill; {blue_pixels} blue glyph pixels visible"
        );
        let yellow_pixels = pixels
            .iter()
            .filter(|p| p[0] > 200 && p[1] > 200 && p[2] < 50)
            .count();
        assert!(
            yellow_pixels > 0,
            "top-level text recorded after the layer must draw over its fill"
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

    // ── PD1: tile-safe non-SrcOver arbitrary path is SSAA-routed ────────────

    /// PD1: A tile-safe non-SrcOver arbitrary path fill (Xor mode on a
    /// transparent surface) must produce partial alpha at its boundary —
    /// proving the PR-4 SSAA routing fires for tile-safe non-SrcOver fills.
    ///
    /// ## Blend-mode selection rationale
    ///
    /// `Xor` on a transparent destination is equivalent to `SrcOver` (both give
    /// `src * 1 + 0 * (1-src_a) = src`), so the pixel output is identical to the
    /// SrcOver SSAA path.  This makes Xor the ideal probe for routing: the SSAA
    /// tile is rendered with the SrcOver pipeline (a known-correct path), and the
    /// composite is also effectively SrcOver — so the test validates that Xor IS
    /// routed through the SSAA tile without requiring a per-mode texture composite
    /// pipeline (TODO T12, deferred).
    ///
    /// Other tile-safe modes (DstOut, DstOver, Plus) composite differently and
    /// their correct pixel output requires TODO T12.
    ///
    /// ## Routing proof (what this test guards)
    ///
    /// If Xor is NOT routed through SSAA (stays tessellated, aliased), boundary
    /// pixels are hard 0 or 255 — no partial alpha. SSAA produces genuine fractional
    /// alpha at every non-axis-aligned edge. The same kite geometry as p4 is used.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index arithmetic over a small fixed-size test surface"
    )]
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
        let mut path = flui_types::painting::path::Path::new();
        path.move_to(flui_types::Point::new(Pixels(v[0].0), Pixels(v[0].1)));
        for &(x, y) in &v[1..] {
            path.line_to(flui_types::Point::new(Pixels(x), Pixels(y)));
        }
        path.close();

        // Xor on transparent background = SrcOver (same pixel output, different mode
        // tag). Proves tile-safe routing fires without needing TODO T12 blend fix.
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

    /// PD3: A non-SrcOver rrect fill (Xor on transparent background) must
    /// produce partial alpha at its curved corner boundary — proving the
    /// shapes batcher routes non-SrcOver fills through SSAA (PR-4).
    ///
    /// ## Blend-mode selection rationale (same as PD1)
    ///
    /// `Xor` on a transparent destination equals `SrcOver` pixel-output-wise,
    /// so the composite step does not need the per-mode pipeline (TODO T12).
    /// The test validates that the shapes SSAA routing fires and the rounded
    /// corner pixels receive genuine sub-pixel coverage.
    ///
    /// Uses an unrotated rrect with 10 px corner radii — the curved corner
    /// band is the best probe for SSAA sub-pixel coverage vs hard aliasing.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index arithmetic over a small fixed-size test surface"
    )]
    fn pd3_non_srcover_basic_shape_has_partial_alpha_at_boundary() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        // Large rrect (60×60) with 10 px corner radii — well above SSAA threshold.
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let rrect = RRect::from_rect_circular(
            Rect::from_xywh(px(cx - 30.0), px(cy - 30.0), px(60.0), px(60.0)),
            px(10.0),
        );

        // Xor on transparent = SrcOver; proves routing fires without TODO T12.
        let paint = Paint::fill(Color::WHITE).with_blend_mode(BlendMode::Xor);

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.rrect(rrect, &paint);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("PD3 Xor Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Partial alpha at the curved corners and straight edges proves SSAA ran.
        let partial_count = pixels.iter().filter(|p| p[3] > 5 && p[3] < 250).count();

        assert!(
            partial_count >= 4,
            "PD3 FAILED: only {partial_count} partial-alpha pixels — the Xor rrect fill \
             appears hard-aliased. PR-4 must route non-SrcOver basic shapes through SSAA."
        );
    }

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
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index arithmetic over a small fixed-size test surface"
    )]
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
            px(0.0),
            px(0.0),
            px(SURFACE_WIDTH as f32),
            px(SURFACE_HEIGHT as f32),
        );
        let green = Color::rgba(0, 255, 0, 255);
        painter.rect(full_rect, &Paint::fill(green));

        // Step 2: paint a large white circle with DstOut blend (tile-safe, non-SrcOver).
        // Radius 40 → bounding box area = 80×80 = 6400 px² >> SSAA_AREA_THRESHOLD_PX_SQ=256.
        // The circle is placed slightly off-pixel-center to ensure non-axis-aligned
        // edges and genuine partial-alpha boundary pixels from the SSAA downsample.
        let center = flui_types::Point::new(px(cx + 0.5), px(cy + 0.5));
        painter.circle(
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
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index arithmetic over a small fixed-size test surface"
    )]
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

            painter.rect(
                Rect::from_xywh(
                    px(0.0),
                    px(0.0),
                    px(SURFACE_WIDTH as f32),
                    px(SURFACE_HEIGHT as f32),
                ),
                &Paint::fill(backdrop),
            );
            painter.circle(
                flui_types::Point::new(px(cx + 0.5), px(cy + 0.5)),
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

    /// PD4: Every non-SrcOver shape producer (rect, rrect, circle, oval, arc)
    /// with a tile-safe blend mode on a large-enough geometry must emit at least
    /// one partial-alpha boundary pixel — proving no producer is permanently
    /// hard-aliased after PR-4.
    ///
    /// ## Anti-MVP rationale
    ///
    /// PD1 (path) and PD3 (rrect) cover only two shape types.  If the SSAA
    /// routing in `DrawBatcher` is broken for one specific shape type
    /// (e.g. the `rect` non-SrcOver branch falls through to tessellated), that
    /// producer would silently regress to aliased output.  PD4 closes this gap by
    /// requiring partial-alpha evidence from EVERY shape category.
    ///
    /// ## Shape placement
    ///
    /// All shapes are placed at fractional pixel offsets (e.g. center x = cx+0.3)
    /// to ensure at least one edge is not pixel-grid-aligned.  A pixel-aligned
    /// axis-aligned edge can produce zero partial-alpha pixels even with correct
    /// SSAA (coverage is exactly 0 or 1 for grid-aligned edges); the fractional
    /// offset guarantees a non-trivial SSAA result on every boundary.
    ///
    /// ## Blend mode
    ///
    /// `Xor` on a transparent destination: pixel-output-identical to SrcOver
    /// (both give `src`), so this test validates SSAA routing fires without
    /// requiring the per-mode composite pipeline (same rationale as PD1/PD3).
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index arithmetic over a small fixed-size test surface"
    )]
    fn pd4_all_non_srcover_shape_producers_emit_partial_alpha() {
        // Helper: render `draw_fn` onto a fresh clear surface and count
        // partial-alpha pixels (3 < alpha < 252).
        let count_partial = |draw_fn: &dyn Fn(&mut WgpuPainter)| -> usize {
            let (device, queue) = acquire_test_device_and_queue();
            let (surface_texture, surface_view) = create_render_surface(&device);
            clear_surface(&device, &queue, &surface_view);
            let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
            draw_fn(&mut painter);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("PD4 Encoder"),
            });
            painter
                .render(
                    RenderTarget::sampleable(&surface_view, &surface_texture),
                    &mut encoder,
                )
                .expect("PD4 render must succeed");
            queue.submit(std::iter::once(encoder.finish()));
            let pixels = readback_pixels(&device, &queue, &surface_texture);
            pixels.iter().filter(|p| p[3] > 3 && p[3] < 252).count()
        };

        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        // Fractional offset ensures no edge lands exactly on a pixel grid line.
        let off = 0.3_f32;
        let xor_paint = Paint::fill(Color::WHITE).with_blend_mode(BlendMode::Xor);

        // ── Rect: 60×60 at fractional offset, Xor blend ──────────────────────
        // Non-SrcOver rect falls into the tessellate+SSAA branch when
        // `is_tile_safe_for_ssaa(Xor) = true` and area ≥ 256 px².
        // 60×60 = 3600 >> 256.  Fractional offset forces non-aligned top/left edge.
        {
            let partial = count_partial(&|p: &mut WgpuPainter| {
                p.rect(
                    Rect::from_xywh(px(cx - 30.0 + off), px(cy - 30.0 + off), px(60.0), px(60.0)),
                    &xor_paint,
                );
            });
            assert!(
                partial >= 1,
                "PD4/rect FAILED: {partial} partial-alpha pixels — non-SrcOver rect \
                 appears hard-aliased; expected SSAA routing to fire for Xor mode."
            );
        }

        // ── RRect: 60×60 with 12px corner radius, Xor, fractional offset ─────
        {
            let partial = count_partial(&|p: &mut WgpuPainter| {
                let r = RRect::from_rect_circular(
                    Rect::from_xywh(px(cx - 30.0 + off), px(cy - 30.0 + off), px(60.0), px(60.0)),
                    px(12.0),
                );
                p.rrect(r, &xor_paint);
            });
            assert!(
                partial >= 4,
                "PD4/rrect FAILED: {partial} partial-alpha pixels — non-SrcOver rrect \
                 appears hard-aliased; curved corners must produce sub-pixel AA via SSAA."
            );
        }

        // ── Circle: radius 40, Xor, fractional center ────────────────────────
        // Diameter 80 → area = 80² = 6400 >> 256.
        {
            let partial = count_partial(&|p: &mut WgpuPainter| {
                p.circle(
                    flui_types::Point::new(px(cx + off), px(cy + off)),
                    40.0,
                    &xor_paint,
                );
            });
            assert!(
                partial >= 4,
                "PD4/circle FAILED: {partial} partial-alpha pixels — non-SrcOver circle \
                 appears hard-aliased; curved boundary must produce sub-pixel AA via SSAA."
            );
        }

        // ── Oval: 80×60 at fractional offset, Xor ─────────────────────────────
        // Area = 80×60 = 4800 >> 256.
        {
            let partial = count_partial(&|p: &mut WgpuPainter| {
                p.oval(
                    Rect::from_xywh(px(cx - 40.0 + off), px(cy - 30.0 + off), px(80.0), px(60.0)),
                    &xor_paint,
                );
            });
            assert!(
                partial >= 4,
                "PD4/oval FAILED: {partial} partial-alpha pixels — non-SrcOver oval \
                 appears hard-aliased; curved ellipse boundary must produce sub-pixel AA via SSAA."
            );
        }

        // ── Arc: 80px bounding box, 90° sweep at 45° start, Xor ─────────────
        // Starting at 45° so both angular edges are diagonal (non-axis-aligned),
        // guaranteeing partial-alpha pixels from the angular AA boundary.
        // Bounding-box area = 80×80 = 6400 >> 256.
        {
            let partial = count_partial(&|p: &mut WgpuPainter| {
                use std::f32::consts::FRAC_PI_4;
                // use_center=true → pie sector; sweep 90° → two diagonal edges.
                let arc_rect =
                    Rect::from_xywh(px(cx - 40.0 + off), px(cy - 40.0 + off), px(80.0), px(80.0));
                p.draw_arc(
                    arc_rect,
                    FRAC_PI_4,
                    std::f32::consts::FRAC_PI_2,
                    true,
                    &xor_paint,
                );
            });
            assert!(
                partial >= 4,
                "PD4/arc FAILED: {partial} partial-alpha pixels — non-SrcOver arc \
                 appears hard-aliased; the diagonal radial/angular edges must produce \
                 sub-pixel AA via SSAA."
            );
        }
    }

    // ── PD6: Xor over opaque backdrop — pipeline is NOT SrcOver ─────────────

    /// PD6: `Xor` composited over an **opaque red backdrop** must produce
    /// near-transparent interior pixels — NOT white as SrcOver would give.
    ///
    /// ## Why this test is necessary (gap in PD1 / PD3 / PD4)
    ///
    /// PD1, PD3, and the Xor sub-cases in PD4 all use `BlendMode::Xor` on a
    /// **cleared (transparent)** destination.  On a transparent dst, Xor and
    /// SrcOver produce identical pixel output:
    ///
    ///   Xor:    out = src×(1−dst_a) + dst×(1−src_a)
    ///           at dst_a=0: out = src×1 + 0×(1−src_a) = src
    ///   SrcOver: out = src + dst×(1−src_a) = src + 0 = src
    ///
    /// Those tests verify that SSAA routing fires, but they cannot detect if
    /// the composite step incorrectly dispatches to the SrcOver pipeline instead
    /// of the Xor pipeline.  PD6 forces an opaque backdrop so the two pipelines
    /// diverge sharply:
    ///
    /// ## Expected pixel values
    ///
    /// Setup:
    ///   - Background: opaque red `(255, 0, 0, 255)` painted with SrcOver.
    ///   - Foreground: large white circle (r=40) with `Xor` blend.
    ///
    /// Xor formula at interior pixel (src = white premul = (1,1,1,1),
    ///                                dst = red premul   = (1,0,0,1)):
    ///
    ///   src_factor = OneMinusDstAlpha = 1 − dst_a = 1 − 1 = 0
    ///   dst_factor = OneMinusSrcAlpha = 1 − src_a = 1 − 1 = 0
    ///
    ///   out.rgb   = (1,1,1)×0 + (1,0,0)×0 = (0,0,0)
    ///   out.alpha = 1×0        + 1×0       = 0          → transparent
    ///
    /// Under SrcOver the same pixel would be opaque white (255, 255, 255, 255).
    /// This test asserts interior alpha < 30, which is impossible under SrcOver.
    ///
    /// ## Partial-alpha proof at the boundary
    ///
    /// At the circle boundary the SSAA tile has partial alpha `a_tile ∈ (0,1)`.
    /// Xor composite:
    ///   out.alpha = a_tile×(1−dst_a) + dst_a×(1−a_tile)
    ///             = a_tile×0          + 1×(1−a_tile)
    ///             = 1 − a_tile  ∈ (0, 1)   → partial alpha
    ///
    /// These partial-alpha boundary pixels also confirm SSAA ran.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index arithmetic over a small fixed-size test surface"
    )]
    fn pd6_xor_over_opaque_red_backdrop_is_not_srcover() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let radius = 40.0_f32;

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));

        // Step 1: fill the entire surface with opaque red using SrcOver.
        let full_rect = Rect::from_xywh(
            px(0.0),
            px(0.0),
            px(SURFACE_WIDTH as f32),
            px(SURFACE_HEIGHT as f32),
        );
        painter.rect(full_rect, &Paint::fill(Color::rgba(255, 0, 0, 255)));

        // Step 2: draw a white circle (r=40, area >> SSAA threshold) with Xor blend.
        // The circle is placed at a fractional offset to ensure non-axis-aligned
        // edges, producing genuine partial-alpha pixels from the SSAA downsample.
        painter.circle(
            flui_types::Point::new(px(cx + 0.5), px(cy + 0.5)),
            radius,
            &Paint::fill(Color::WHITE).with_blend_mode(BlendMode::Xor),
        );

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("PD6 Xor over Red Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Assertion 1: interior pixel is near-transparent (Xor zeros both src
        // and dst when src_a=1 and dst_a=1). SrcOver would give alpha=255 (white).
        let interior_idx = cy as usize * SURFACE_WIDTH as usize + cx as usize;
        let interior_alpha = pixels[interior_idx][3];
        assert!(
            interior_alpha < 30,
            "PD6 FAILED: interior alpha={interior_alpha}, expected near-0. \
             Xor over an opaque backdrop zeros the output when both src_a=1 and \
             dst_a=1.  SrcOver would produce alpha=255 (white).  If this fails, \
             the SSAA composite step used the SrcOver pipeline instead of the Xor \
             pipeline (flush_texture_batch_premultiplied_with_mode was not called \
             with BlendMode::Xor, or is_tile_safe_for_ssaa incorrectly excluded Xor)."
        );

        // Assertion 2: exterior is still opaque red (Xor only touches the circle area).
        // A point well outside the circle (2 px beyond the radius) must retain red.
        let ext_col = (cx + radius + 2.0 + 0.5).min(SURFACE_WIDTH as f32 - 1.0) as usize;
        let ext_row = cy as usize;
        let ext_idx = ext_row * SURFACE_WIDTH as usize + ext_col;
        assert!(
            pixels[ext_idx][3] > 200,
            "PD6: exterior pixel (outside circle) must remain opaque red; \
             got alpha={} — Xor should not touch pixels outside the SSAA tile",
            pixels[ext_idx][3]
        );

        // Assertion 3: boundary band shows partial alpha, proving SSAA ran.
        // At dist ≈ radius, the SSAA tile has 0 < a_tile < 1, so
        // Xor gives out.alpha = 1 − a_tile ∈ (0,1) — partial alpha.
        let mut boundary_partial = 0usize;
        for row in 0..SURFACE_HEIGHT {
            for col in 0..SURFACE_WIDTH {
                let dx = col as f32 + 0.5 - (cx + 0.5);
                let dy = row as f32 + 0.5 - (cy + 0.5);
                let dist = (dx * dx + dy * dy).sqrt();
                if (radius - 2.0..=radius + 2.0).contains(&dist) {
                    let alpha = pixels[row as usize * SURFACE_WIDTH as usize + col as usize][3];
                    // 1−a_tile ∈ (10/255, 245/255) iff a_tile ∈ (10/255, 245/255).
                    if alpha > 10 && alpha < 245 {
                        boundary_partial += 1;
                    }
                }
            }
        }
        assert!(
            boundary_partial >= 4,
            "PD6 FAILED: only {boundary_partial} partial-alpha boundary pixels — \
             SSAA must produce sub-pixel coverage at the Xor circle boundary. \
             This also confirms the Xor blend pipeline was used at composite time."
        );
    }

    // ── PD5: Phase-B regression — SrcOver path still AA'd after PR-4 ─────────

    /// PD5: A SrcOver arbitrary path fill must still produce partial alpha at
    /// its boundary after PR-4 changes (regression guard: PR-4 must not break
    /// the PR-3 SrcOver SSAA path).
    ///
    /// This is a Phase-B regression guard (the naming in the spec doc). It is
    /// structurally equivalent to p4_anti_mvp_srcover_fill_has_partial_alpha_at_boundary
    /// but documents its role as a regression gate specifically for PR-4.
    #[test]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel index arithmetic over a small fixed-size test surface"
    )]
    fn pd5_srcover_path_still_has_partial_alpha_after_pr4() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        // Same kite as p4 — SrcOver, just verifying it still routes through SSAA.
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let v = [
            (cx + 1.3, cy - 38.7),
            (cx + 42.2, cy + 5.5),
            (cx - 2.7, cy + 33.1),
            (cx - 38.4, cy - 9.2),
        ];
        let mut path = flui_types::painting::path::Path::new();
        path.move_to(flui_types::Point::new(Pixels(v[0].0), Pixels(v[0].1)));
        for &(x, y) in &v[1..] {
            path.line_to(flui_types::Point::new(Pixels(x), Pixels(y)));
        }
        path.close();

        let paint = Paint::fill(Color::WHITE); // SrcOver default

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.draw_path(&path, &paint);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("PD5 SrcOver Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        let partial_count = pixels.iter().filter(|p| p[3] > 5 && p[3] < 250).count();

        assert!(
            partial_count >= 4,
            "PD5 FAILED: only {partial_count} partial-alpha pixels — the SrcOver SSAA path \
             (PR-3) appears broken after PR-4 changes. The PR-4 routing must not regress \
             the SrcOver SSAA path."
        );

        // Shape must be actually rendered (interior opaque).
        let interior_col = cx as usize;
        let interior_row = cy as usize;
        let interior_idx = interior_row * SURFACE_WIDTH as usize + interior_col;
        assert!(
            pixels[interior_idx][3] > 200,
            "PD5: shape interior must be opaque after PR-4; got alpha={}",
            pixels[interior_idx][3]
        );
    }

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
        painter.translate(flui_types::Offset::new(Pixels(cx), Pixels(cy)));
        painter.rotate(angle_deg.to_radians());
        let rotated_rect = Rect::from_ltrb(
            Pixels(-half_w),
            Pixels(-half_h),
            Pixels(half_w),
            Pixels(half_h),
        );
        painter.rect(rotated_rect, &Paint::fill(Color::WHITE));
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

    /// Q1-b: Axis-aligned straight edge — L2 and L1 are equivalent (guard test).
    ///
    /// On an axis-aligned edge one partial derivative is zero, so:
    ///   L1: |dpdx| + |dpdy| = |∂/∂x| + 0 = |∂/∂x|
    ///   L2: sqrt(dpdx² + dpdy²) = |∂/∂x|
    /// The two norms are identical. This test verifies the AA band on a straight
    /// horizontal/vertical edge is ≤ 2 pixels under both norms.
    ///
    /// If this test fails when Q1a passes, the `dpdx`/`dpdy` change introduced
    /// a regression on axis-aligned edges.
    #[test]
    fn q1b_axis_aligned_edge_aa_band_is_within_expected_width() {
        let (device, queue) = acquire_test_device_and_queue();
        let (surface_texture, surface_view) = create_render_surface(&device);
        clear_surface(&device, &queue, &surface_view);

        // Draw a 60×40 axis-aligned white rect centered in the frame.
        let cx = SURFACE_WIDTH as f32 / 2.0;
        let cy = SURFACE_HEIGHT as f32 / 2.0;
        let half_w = 30.0_f32;
        let half_h = 20.0_f32;

        let rect = Rect::from_ltrb(
            Pixels(cx - half_w),
            Pixels(cy - half_h),
            Pixels(cx + half_w),
            Pixels(cy + half_h),
        );

        let mut painter = build_painter(Arc::clone(&device), Arc::clone(&queue));
        painter.rect(rect, &Paint::fill(Color::WHITE));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Q1b Axis-Aligned Encoder"),
        });
        painter
            .render(
                RenderTarget::sampleable(&surface_view, &surface_texture),
                &mut encoder,
            )
            .expect("render must succeed");
        queue.submit(std::iter::once(encoder.finish()));

        let pixels = readback_pixels(&device, &queue, &surface_texture);

        // Scan the right edge of the rect (the column around cx + half_w = 94).
        let edge_col = (cx + half_w) as usize; // ~94
        let scan_row_start = (cy as usize).saturating_sub(5);
        let scan_row_end = (cy as usize + 5).min(SURFACE_HEIGHT as usize);

        // Count partial-alpha pixels in the edge column and its immediate neighbours.
        let mut max_partial_per_row = 0usize;
        for row in scan_row_start..scan_row_end {
            let count = (edge_col.saturating_sub(1)
                ..=(edge_col + 1).min(SURFACE_WIDTH as usize - 1))
                .filter(|&col| {
                    let idx = row * SURFACE_WIDTH as usize + col;
                    let a = pixels[idx][3];
                    a > 5 && a < 250
                })
                .count();
            if count > max_partial_per_row {
                max_partial_per_row = count;
            }
        }

        // Interior must be opaque.
        let interior_idx = cy as usize * SURFACE_WIDTH as usize + cx as usize;
        assert!(
            pixels[interior_idx][3] > 200,
            "Q1b: interior must be opaque; got alpha={}",
            pixels[interior_idx][3]
        );

        // Axis-aligned edge: ≤ 2 partial pixels in a 3-pixel horizontal scan.
        assert!(
            max_partial_per_row <= 2,
            "Q1b: axis-aligned right edge has {max_partial_per_row} partial-alpha pixels in a \
             3-wide scan — expected ≤ 2 (both L1 and L2 are identical on axis-aligned edges)"
        );
    }
}
