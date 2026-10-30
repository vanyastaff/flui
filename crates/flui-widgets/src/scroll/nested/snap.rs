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

    /// [`target`](Self::target) for the release of a scroll that began at
    /// `origin`: where the offset stood when the contact went down, or the
    /// release's own offset when no contact began it (an assistive scroll, a
    /// key, a scroll handed in by a nested scrollable). A policy that bounds
    /// how far one gesture travels, one item per swipe however long the drag,
    /// reads it; the default ignores it.
    fn target_from(&self, metrics: &ScrollMetrics, velocity: f64, origin: f64) -> f64 {
        let _ = origin;
        self.target(metrics, velocity)
    }
}

/// The release speed, logical px/s, below which a policy settles on the
/// nearest target: a fixed logical threshold like the physics' minimum fling
/// velocities.
const VELOCITY_TOLERANCE: f64 = 20.0;

/// The share of the viewport one page takes: finite and greater than zero.
///
/// A [`PageController`](crate::PageController) and a [`PageSnap`] take their
/// page length from it, so a fraction computed from layout that came out
/// infinite or zero (an item extent over a collapsed viewport) is refused
/// once, where it is made, instead of when a page view mounts.
///
/// ```
/// use flui_widgets::ViewportFraction;
///
/// let half = ViewportFraction::try_new(0.5).expect("a positive share");
/// assert_eq!(half.get(), 0.5);
/// assert!(ViewportFraction::try_new(f64::INFINITY).is_err());
/// assert!(ViewportFraction::try_new(0.0).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct ViewportFraction(f64);

/// Why [`ViewportFraction::try_new`] refused a share of the viewport.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error("a viewport fraction must be finite and greater than zero, got {fraction}")]
pub struct InvalidViewportFraction {
    fraction: f64,
}

impl InvalidViewportFraction {
    /// The refused share.
    #[must_use]
    pub fn fraction(&self) -> f64 {
        self.fraction
    }
}

impl ViewportFraction {
    /// A page as long as the viewport.
    pub const FULL: Self = Self(1.0);

    /// `fraction` of the viewport per page.
    ///
    /// # Errors
    ///
    /// [`InvalidViewportFraction`] when `fraction` is NaN, infinite, zero or
    /// negative.
    pub fn try_new(fraction: f64) -> Result<Self, InvalidViewportFraction> {
        if fraction.is_finite() && fraction > 0.0 {
            Ok(Self(fraction))
        } else {
            Err(InvalidViewportFraction { fraction })
        }
    }

    /// The share as a number.
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

impl Default for ViewportFraction {
    fn default() -> Self {
        Self::FULL
    }
}

/// Snaps to whole pages of a [`ViewportFraction`] of the viewport.
///
/// A release faster than 20 logical pixels per second
/// commits to the next page in its direction, from anywhere on the current
/// one; a slower one settles on the nearest page.
///
/// ```
/// use flui_widgets::{PageSnap, ScrollMetrics, SnapPolicy, ViewportFraction};
///
/// let pages = PageSnap::new(ViewportFraction::FULL);
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
    #[must_use]
    pub fn new(viewport_fraction: ViewportFraction) -> Self {
        Self {
            viewport_fraction: viewport_fraction.get(),
        }
    }
}

impl SnapPolicy for PageSnap {
    fn target(&self, metrics: &ScrollMetrics, velocity: f64) -> f64 {
        let extent = metrics.viewport_dimension * self.viewport_fraction;
        // A page shorter than the logical pixel `ScrollMetrics::page` floors
        // its divisor at (a viewport collapsing to nothing) names no page:
        // page and offset would no longer convert both ways, and the
        // release would jump back. It rests where it is.
        if extent.is_nan() || extent < 1.0 {
            return metrics.pixels;
        }
        let page = metrics.page(self.viewport_fraction);
        let nearest = page.round();
        let page = if (page - nearest).abs() * extent <= metrics.rest_slop() {
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

/// Why [`AnchorSnap::try_new`] refused its anchors.
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
/// A release faster than 20 logical pixels per second
/// commits to the next anchor in its direction, also from an anchor's own
/// offset, or stays on the last one that way; a slower one settles on the
/// nearest anchor (the farther one, from exactly halfway). Anchors are
/// offsets, not identities: equal ones are one anchor, and an anchor past an
/// extent rests on that extent.
///
/// ```
/// use flui_widgets::{AnchorSnap, ScrollMetrics, SnapPolicy};
///
/// let stops = AnchorSnap::try_new([0.0, 120.0, 400.0]).expect("finite anchors");
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
    pub fn try_new(anchors: impl IntoIterator<Item = f64>) -> Result<Self, AnchorSnapError> {
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
}

impl SnapPolicy for AnchorSnap {
    fn target(&self, metrics: &ScrollMetrics, velocity: f64) -> f64 {
        let anchors = &*self.anchors;
        let (first, last) = (anchors[0], anchors[anchors.len() - 1]);
        let pixels = metrics.pixels;
        let slop = metrics.rest_slop();
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
