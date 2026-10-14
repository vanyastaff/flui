//! Where a released scroll comes to rest (ADR-0209 §9).

use crate::scroll::ScrollMetrics;

/// Picks the offset a released scroll settles on: a page, an anchor.
///
/// One open policy shaped like [`ScrollPhysics`](crate::ScrollPhysics):
/// stateless, `Send + Sync`, held as `Arc<dyn SnapPolicy>` and compared by
/// [`Arc::ptr_eq`](std::sync::Arc::ptr_eq). A scrollable with a policy springs to its
/// target on every release — a drag lifting, a press on a moving scrollable
/// lifting, an assistive scroll — instead of coasting; a target that is not
/// finite is replaced by the nearest extent with a diagnostic, and a finite
/// one is clamped to the extents.
pub trait SnapPolicy: Send + Sync + std::fmt::Debug {
    /// The offset to settle on from `metrics` at `velocity` logical pixels
    /// per second along the scroll axis (positive toward the maximum
    /// extent).
    fn target(&self, metrics: &ScrollMetrics, velocity: f64) -> f64;
}

/// Snaps to whole pages of `viewport_fraction` of the viewport.
///
/// A release faster than [`velocity_tolerance`](Self::velocity_tolerance)
/// commits to the next page in its direction past the halfway point;
/// a slower one settles on the nearest page.
///
/// ```
/// use flui_widgets::{PageSnap, ScrollMetrics, SnapPolicy};
///
/// let pages = PageSnap::new(1.0);
/// let metrics = ScrollMetrics::new(130.0, 0.0, 900.0, 300.0);
/// assert_eq!(pages.target(&metrics, 0.0), 0.0, "nearest page at rest");
/// assert_eq!(pages.target(&metrics, 500.0), 300.0, "a forward fling commits");
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSnap {
    viewport_fraction: f64,
    velocity_tolerance: f64,
}

impl PageSnap {
    /// Pages of `viewport_fraction` of the viewport, with a 20 logical px/s
    /// velocity tolerance.
    ///
    /// # Panics
    ///
    /// When `viewport_fraction` is not finite and positive.
    #[must_use]
    pub fn new(viewport_fraction: f64) -> Self {
        assert!(
            viewport_fraction.is_finite() && viewport_fraction > 0.0,
            "PageSnap viewport_fraction must be finite and > 0.0 (got {viewport_fraction})"
        );
        Self {
            viewport_fraction,
            velocity_tolerance: 20.0,
        }
    }

    /// Pages with an explicit release-speed tolerance, as
    /// [`PageScrollPhysics`](crate::PageScrollPhysics) authors it.
    pub(crate) fn with_velocity_tolerance(viewport_fraction: f64, tolerance: f64) -> Self {
        Self {
            viewport_fraction,
            velocity_tolerance: tolerance,
        }
    }

    /// The release speed, logical px/s, below which the nearest page wins.
    #[must_use]
    pub fn velocity_tolerance(&self) -> f64 {
        self.velocity_tolerance
    }
}

impl SnapPolicy for PageSnap {
    fn target(&self, metrics: &ScrollMetrics, velocity: f64) -> f64 {
        let mut page = metrics.page(self.viewport_fraction);
        if velocity < -self.velocity_tolerance {
            page -= 0.5;
        } else if velocity > self.velocity_tolerance {
            page += 0.5;
        }
        metrics.pixels_from_page(self.viewport_fraction, page.round())
    }
}
