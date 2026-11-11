//! Gaussian-blur kernel-radius helper, shared by the blur pass driver and its
//! CPU oracle.

// =============================================================================
// Blur tap-count helper
// =============================================================================

/// Impeller's kernel-radius-per-sigma constant (`kKernelRadiusPerSigma`, sigma.h:24).
///
/// Value: √3 ≈ 1.732 050 8. Chosen so the Gaussian evaluated at ±radius drops
/// below ½ of its peak value — the Impeller standard for "sufficient tap coverage".
///
/// (Impeller's exact formula is `(sigma - 0.5) × √3`; we omit the `−0.5` as a
/// conservative over-estimate documented in the spec.)
const KERNEL_RADIUS_PER_SIGMA: f32 = 1.732_050_8;

/// Gaussian-blur kernel radius (in source pixels) for a given Gaussian sigma.
///
/// Computes `ceil(sigma × √3)` — Impeller's `CalculateBlurRadius` from
/// `impeller/geometry/sigma.h:24`. The integer result is both:
///
/// - the **sampling extent** per sub-pass (H or V scans `[-r..=r]` texels), and
/// - the **coverage radius** for `grown_bounds` expansion in `restore_layer`.
///
/// The full kernel spans `2 × kernel_radius + 1` taps.
///
/// Returns `0` for non-positive sigma (degenerate / no blur).
///
/// Single authoritative home for the blur-pass driver (`apply_blur`) and the
/// CPU oracle in `blur_filter_tests` — do NOT compute `ceil(sigma * N)` inline
/// at other call sites.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    // non-negative result: sigma > 0.0 guard ensures (sigma * √3).ceil() ≥ 0;
    // truncation: u32::MAX ≈ 4.3 × 10^9, overflowable only at sigma > ~2.5 × 10^9 px
)]
#[must_use]
pub(crate) fn kernel_radius(sigma: f32) -> u32 {
    if sigma <= 0.0 {
        return 0;
    }
    (sigma * KERNEL_RADIUS_PER_SIGMA).ceil() as u32
}

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::{Matrix4, Rect};
    use flui_layer::{BackdropFilterLayer, Closure, Layer, read_closure};
    use flui_painting::paint::{BlendMode, ImageFilter};

    use super::kernel_radius;

    /// The taps the renderer's blur takes around a backdrop's bounds lie
    /// inside what `read_closure` says it reads, at every sigma and under
    /// shrinking, unit and magnifying transforms (ADR-0304 §6).
    #[test]
    fn the_renderer_kernel_stays_inside_read_closure() {
        let bounds = Rect::from_xywh(100.0, 100.0, 50.0, 50.0);
        for sigma in [0.5_f32, 1.0, 2.0, 3.3, 8.0, 25.0, 100.0] {
            for scale in [0.25, 0.5, 1.0, 2.0, 3.0] {
                let layer = Layer::from(BackdropFilterLayer::new(
                    ImageFilter::blur(f64::from(sigma)),
                    BlendMode::SrcOver,
                    bounds,
                ));
                let ctm = Matrix4::scaling(scale, scale, 1.0);
                let Closure::Bounded { reads, writes } = read_closure(&layer, &ctm, Rect::ZERO)
                else {
                    panic!("a blur under a scale has a bounded closure");
                };
                // The renderer blurs the backdrop in device pixels, with the
                // sigma taken as physical pixels whatever the transform.
                let taps = f64::from(kernel_radius(sigma));
                assert!(
                    reads.left() <= writes.left() - taps
                        && reads.top() <= writes.top() - taps
                        && reads.right() >= writes.right() + taps
                        && reads.bottom() >= writes.bottom() + taps,
                    "sigma {sigma}, scale {scale}: reads {reads:?} miss {taps} taps around {writes:?}"
                );
            }
        }
    }
}
