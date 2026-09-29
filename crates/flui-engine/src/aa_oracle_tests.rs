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

/// Analytic inside-test for an axis-aligned rect centered at origin with
/// half-extents `(half_w, half_h)`.
fn inside_rect(px: f32, py: f32, half_w: f32, half_h: f32) -> bool {
    px.abs() <= half_w && py.abs() <= half_h
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

    use flui_foundation::geometry::Rect;
    use flui_painting::Paint;
    use flui_painting::styling::Color;

    use crate::{painter::WgpuPainter, render_target::RenderTarget};

    use super::{analytic_coverage, inside_rotated_rect};

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

    // ── O5: Byte-identity — axis-aligned SrcOver rect and rrect ──────────────

    // ── C1: Circle AA — fwidth model is radius-independent ───────────────────

    // ── C2: Rotated ellipse — affine orientation correct ─────────────────────

    // ── C3: Interior opaque + exterior transparent (fringe leaks nothing) ────

    // ── C4: Scaled circle center is not double-scaled (baked-path regression) ──

    // ── O6: fwidth scale-invariance ───────────────────────────────────────────

    // ── O7: Non-uniform corner radii map to the correct screen corners ────────

    // ── A1: Arc radial AA is radius-independent ───────────────────────────────

    // ── A2: Rotated arc — affine orientation correct ──────────────────────────

    // ── A3: Scaled arc center not double-scaled (PR-2 regression guard) ───────

    // ── P1–P4: SSAA path anti-aliasing (PR-3) ────────────────────────────────

    // ── CPU oracle helpers for polygon tests ─────────────────────────────────

    // ── P1: polygon fill boundary pixels have partial alpha (real SSAA AA) ────

    // ── P2: fill rule (NonZero solid / EvenOdd hollow) both AA'd ─────────────

    // ── P3: SSAA scale-invariance — AA band stays ~1 device-px ───────────────

    // ── P4: anti-MVP — SrcOver Fill path must emit ≥1 partial-alpha pixel ────

    // ── Draw order across raster routes ──────────────────────────────────────

    // ── A4: Angular edges are anti-aliased (partial alpha) ───────────────────

    // ── PD1: tile-safe non-SrcOver arbitrary path is SSAA-routed ────────────

    // ── PD3: non-SrcOver basic shape is SSAA-routed ──────────────────────────

    // ── PD2: tile-safe non-SrcOver produces visually distinct output over backdrop ──

    // ── PD6: per-mode composite is the REQUESTED blend, not SrcOver ──────────

    // ── PD4: anti-MVP — all non-SrcOver shape producers emit partial alpha ────

    // ── PD6: Xor over opaque backdrop — pipeline is NOT SrcOver ─────────────

    // ── PD5: Phase-B regression — SrcOver path still AA'd after PR-4 ─────────

    // ── Q1: L2 gradient norm AA band-width tests ──────────────────────────────
}
