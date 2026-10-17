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
/// one is clamped to the extents. [`PageSnap`] targets whole pages,
/// [`AnchorSnap`] a set of offsets.
pub trait SnapPolicy: Send + Sync + std::fmt::Debug {
    /// The offset to settle on from `metrics` at `velocity` logical pixels
    /// per second along the scroll axis (positive toward the maximum
    /// extent).
    fn target(&self, metrics: &ScrollMetrics, velocity: f64) -> f64;
}

/// The release speed, logical px/s, below which a policy settles on the
/// nearest target: a fixed logical threshold like the physics' minimum fling
/// velocities.
const VELOCITY_TOLERANCE: f64 = 20.0;

/// How far off a target, in logical pixels, still counts as on it: the half
/// device pixel a ballistic run rests within (ADR-0155), so a scrollable that
/// came to rest a hair off its target flings on to the next one.
fn rest_slop(metrics: &ScrollMetrics) -> f64 {
    let ratio = metrics.device_pixel_ratio;
    if ratio.is_finite() && ratio > 0.0 {
        0.5 / ratio
    } else {
        0.5
    }
}

/// Snaps to whole pages of `viewport_fraction` of the viewport.
///
/// A release faster than [`velocity_tolerance`](Self::velocity_tolerance)
/// commits to the next page in its direction, from anywhere on the current
/// one; a slower one settles on the nearest page.
///
/// ```
/// use flui_widgets::{PageSnap, ScrollMetrics, SnapPolicy};
///
/// let pages = PageSnap::new(1.0);
/// let metrics = ScrollMetrics::new(130.0, 0.0, 900.0, 300.0);
/// assert_eq!(pages.target(&metrics, 0.0), 0.0, "nearest page at rest");
/// assert_eq!(pages.target(&metrics, 500.0), 300.0, "a forward fling commits");
///
/// let on_a_page = ScrollMetrics::new(300.0, 0.0, 900.0, 300.0);
/// assert_eq!(pages.target(&on_a_page, -500.0), 0.0, "a backward fling leaves it");
/// assert_eq!(pages.target(&on_a_page, 500.0), 600.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSnap {
    viewport_fraction: f64,
}

impl PageSnap {
    /// Pages of `viewport_fraction` of the viewport.
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
        Self { viewport_fraction }
    }

    /// The release speed, logical px/s, below which the nearest page wins.
    #[must_use]
    pub fn velocity_tolerance(&self) -> f64 {
        VELOCITY_TOLERANCE
    }
}

impl SnapPolicy for PageSnap {
    fn target(&self, metrics: &ScrollMetrics, velocity: f64) -> f64 {
        let page = metrics.page(self.viewport_fraction);
        let nearest = page.round();
        // The page length `ScrollMetrics::page` divides by.
        let extent = (metrics.viewport_dimension * self.viewport_fraction).max(1.0);
        let page = if (page - nearest).abs() * extent <= rest_slop(metrics) {
            nearest
        } else {
            page
        };
        // A fling takes the next page in its direction, also from a page's
        // exact offset, where rounding half a page off would stay put.
        let page = if velocity < -VELOCITY_TOLERANCE {
            (page - 1.0).ceil()
        } else if velocity > VELOCITY_TOLERANCE {
            page.floor() + 1.0
        } else {
            nearest
        };
        metrics.pixels_from_page(self.viewport_fraction, page)
    }
}

/// Why [`AnchorSnap::new`] refused its anchors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AnchorSnapError {
    /// No anchor was given: there is nowhere to rest.
    #[error("an anchor snap needs at least one anchor")]
    Empty,
    /// The anchor at `index`, in the order given, is NaN or infinite.
    #[error("anchor {index} is not finite")]
    NonFinite {
        /// The anchor's position in the input.
        index: usize,
    },
}

/// Snaps to the nearest of a set of anchor offsets along the scroll axis: the
/// stops of a sheet, a row's revealed actions, item starts of uneven size.
///
/// A release faster than [`velocity_tolerance`](Self::velocity_tolerance)
/// commits to the next anchor in its direction, also from an anchor's own
/// offset, or stays on the last one that way; a slower one settles on the
/// nearest anchor (the farther one, from exactly halfway). Anchors are
/// offsets, not identities: equal ones are one anchor, and an anchor past an
/// extent rests on that extent.
///
/// ```
/// use flui_widgets::{AnchorSnap, ScrollMetrics, SnapPolicy};
///
/// let stops = AnchorSnap::new([0.0, 120.0, 400.0]).expect("finite anchors");
/// let at = |pixels| ScrollMetrics::new(pixels, 0.0, 400.0, 300.0);
/// assert_eq!(stops.target(&at(50.0), 0.0), 0.0, "nearest at rest");
/// assert_eq!(stops.target(&at(50.0), 500.0), 120.0, "a fling takes the next one");
/// assert_eq!(stops.target(&at(120.0), 500.0), 400.0, "also from an anchor");
/// assert_eq!(stops.target(&at(400.0), 500.0), 400.0, "and stays on the last");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorSnap {
    /// Ascending, without duplicates, never empty.
    anchors: Box<[f64]>,
}

impl AnchorSnap {
    /// Anchors at these offsets, in any order.
    ///
    /// # Errors
    ///
    /// [`AnchorSnapError::Empty`] for no anchors,
    /// [`AnchorSnapError::NonFinite`] for a NaN or infinite one.
    pub fn new(anchors: impl IntoIterator<Item = f64>) -> Result<Self, AnchorSnapError> {
        let mut anchors: Vec<f64> = anchors.into_iter().collect();
        if let Some(index) = anchors.iter().position(|anchor| !anchor.is_finite()) {
            return Err(AnchorSnapError::NonFinite { index });
        }
        if anchors.is_empty() {
            return Err(AnchorSnapError::Empty);
        }
        anchors.sort_by(f64::total_cmp);
        anchors.dedup();
        Ok(Self {
            anchors: anchors.into_boxed_slice(),
        })
    }

    /// The anchors, ascending and without duplicates.
    #[must_use]
    pub fn anchors(&self) -> &[f64] {
        &self.anchors
    }

    /// The release speed, logical px/s, below which the nearest anchor wins.
    #[must_use]
    pub fn velocity_tolerance(&self) -> f64 {
        VELOCITY_TOLERANCE
    }
}

impl SnapPolicy for AnchorSnap {
    fn target(&self, metrics: &ScrollMetrics, velocity: f64) -> f64 {
        let anchors = &*self.anchors;
        let (first, last) = (anchors[0], anchors[anchors.len() - 1]);
        let pixels = metrics.pixels;
        let slop = rest_slop(metrics);
        if velocity > VELOCITY_TOLERANCE {
            let next = anchors.partition_point(|&anchor| anchor <= pixels + slop);
            return anchors.get(next).copied().unwrap_or(last);
        }
        if velocity < -VELOCITY_TOLERANCE {
            let next = anchors.partition_point(|&anchor| anchor < pixels - slop);
            return next.checked_sub(1).map_or(first, |index| anchors[index]);
        }
        let above = anchors.partition_point(|&anchor| anchor < pixels);
        match (
            above.checked_sub(1).map(|index| anchors[index]),
            anchors.get(above),
        ) {
            (Some(below), Some(&above)) if pixels - below < above - pixels => below,
            (_, Some(&above)) => above,
            (Some(below), None) => below,
            (None, None) => first,
        }
    }
}
