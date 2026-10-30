//! [`KeylineSnap`]: where a released carousel comes to rest.

use flui_objects::{CarouselGeometry, CarouselLayout};

use crate::scroll::{ScrollMetrics, SnapPolicy};

/// The release speed, logical px/s, below which a carousel settles on the nearest item.
const VELOCITY_TOLERANCE: f64 = 20.0;

/// The velocity a fling keeps after one second, the friction a browsing release coasts on
/// before it settles on an item.
const COAST_DRAG: f64 = 0.135;

/// How a carousel settles after the user lets go.
///
/// Both rest on an item's keyline; without a snap the carousel scrolls freely. A settle after
/// the user's fling is the user's own motion, so reduced motion does not change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeylineSnap {
    /// Exactly one item per swipe, however fast or long: the next or previous item from the
    /// one the contact went down on, or the nearest of the two when the release is slow.
    SingleAdvance,
    /// A fling coasts and settles on the item nearest where it would come to rest.
    MultiBrowse,
}

/// [`KeylineSnap`] over one carousel's layout, spacing and item count; the viewport comes
/// from each release's metrics.
#[derive(Debug)]
pub(super) struct CarouselSnap {
    pub(super) kind: KeylineSnap,
    pub(super) layout: CarouselLayout,
    pub(super) spacing: f64,
    pub(super) count: usize,
}

impl CarouselSnap {
    fn geometry(&self, metrics: &ScrollMetrics) -> Option<CarouselGeometry> {
        CarouselGeometry::new(
            &self.layout,
            metrics.viewport_dimension,
            self.spacing,
            self.count,
        )
    }
}

/// The next item from `from` in the direction of `step` whose resting offset differs: two
/// items resting on the same offset (both on large keylines) are one stop.
fn advance(geometry: &CarouselGeometry, from: usize, forward: bool) -> usize {
    let count = geometry.count();
    let origin = geometry.rest_offset(from);
    let mut index = from;
    loop {
        let next = if forward {
            if index + 1 >= count {
                return index;
            }
            index + 1
        } else {
            match index.checked_sub(1) {
                Some(next) => next,
                None => return index,
            }
        };
        index = next;
        if geometry.rest_offset(index) != origin {
            return index;
        }
    }
}

impl SnapPolicy for CarouselSnap {
    fn target(&self, metrics: &ScrollMetrics, velocity: f64) -> f64 {
        self.target_from(metrics, velocity, metrics.pixels)
    }

    fn target_from(&self, metrics: &ScrollMetrics, velocity: f64, origin: f64) -> f64 {
        let Some(geometry) = self.geometry(metrics) else {
            return metrics.pixels;
        };
        let nearest = |offset: f64| geometry.nearest_item(offset).unwrap_or(0);
        let item = match self.kind {
            KeylineSnap::SingleAdvance => {
                let start = nearest(origin);
                if velocity > VELOCITY_TOLERANCE {
                    advance(&geometry, start, true)
                } else if velocity < -VELOCITY_TOLERANCE {
                    advance(&geometry, start, false)
                } else {
                    let low = advance(&geometry, start, false);
                    let high = advance(&geometry, start, true);
                    nearest(metrics.pixels).clamp(low, high)
                }
            }
            KeylineSnap::MultiBrowse => {
                let coast = if velocity.is_finite() {
                    velocity / -COAST_DRAG.ln()
                } else {
                    0.0
                };
                let item = nearest(metrics.pixels + coast);
                let here = nearest(metrics.pixels);
                // A fling that coasts short of the next item still leaves this one.
                if item == here && velocity > VELOCITY_TOLERANCE {
                    advance(&geometry, here, true)
                } else if item == here && velocity < -VELOCITY_TOLERANCE {
                    advance(&geometry, here, false)
                } else {
                    item
                }
            }
        };
        geometry.rest_offset(item)
    }
}
