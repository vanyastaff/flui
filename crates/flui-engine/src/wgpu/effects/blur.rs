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

/// CPU-only tests for `kernel_radius`. These run in CI without a GPU.
#[cfg(test)]
mod kernel_radius_tests {
    use super::kernel_radius;

    /// `kernel_radius(0.0)` must return 0 (degenerate — no blur).
    #[test]
    fn zero_sigma_returns_zero() {
        assert_eq!(kernel_radius(0.0), 0);
    }

    /// Negative sigma is treated as no-blur.
    #[test]
    fn negative_sigma_returns_zero() {
        assert_eq!(kernel_radius(-1.0), 0);
    }

    /// `kernel_radius` must be monotonically non-decreasing as sigma grows.
    ///
    /// Tests sigma = 0.1, 0.5, 1.0, 1.5, 2.0, 3.0, 5.0, 10.0.
    #[test]
    fn monotonically_nondecreasing() {
        let sigmas = [0.1_f32, 0.5, 1.0, 1.5, 2.0, 3.0, 5.0, 10.0];
        let radii: Vec<u32> = sigmas.iter().map(|&s| kernel_radius(s)).collect();
        for window in radii.windows(2) {
            assert!(
                window[0] <= window[1],
                "kernel_radius not monotone: sigma pair produced radii {} > {}",
                window[0],
                window[1]
            );
        }
    }

    /// sigma = 2.0 → ceil(2.0 × 1.732_050_8) = ceil(3.464_101_6) = 4.
    ///
    /// This is the known-value anchor from the spec: the chief-architect
    /// table entry `(2.0) == 4`.
    #[test]
    fn sigma_two_gives_radius_four() {
        assert_eq!(kernel_radius(2.0), 4);
    }

    /// sigma = 1.0 → ceil(1.0 × 1.732_050_8) = ceil(1.732_050_8) = 2.
    #[test]
    fn sigma_one_gives_radius_two() {
        assert_eq!(kernel_radius(1.0), 2);
    }
}
