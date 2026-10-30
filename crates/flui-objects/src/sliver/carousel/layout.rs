//! [`CarouselLayout`]: how a carousel sizes the items it shows, checked when it is made.

use flui_painting::Alignment;

/// The largest item extent, small item size or uncontained extent a carousel takes, in logical
/// pixels: with every item at this extent, the strip of any `usize` count of items and the
/// keylines over it stay finite.
pub const MAX_ITEM_EXTENT: f64 = 1e7;

/// Why a [`CarouselLayout`] constructor refused its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CarouselLayoutError {
    /// A size, a weight or an alignment is NaN or infinite.
    #[error("a carousel layout value is not finite")]
    NonFinite,
    /// A size is zero or negative, or a weight is negative.
    #[error("a carousel layout size must be greater than zero and a weight not negative")]
    NonPositive,
    /// A size is larger than [`MAX_ITEM_EXTENT`].
    #[error("a carousel layout size is larger than {MAX_ITEM_EXTENT} logical pixels")]
    TooLarge,
    /// The smallest small item size is larger than the largest.
    #[error("a carousel's smallest small item size is larger than its largest")]
    InvertedRange,
    /// No weight is greater than zero.
    #[error("a weighted carousel needs a weight greater than zero")]
    NoWeight,
    /// A hero alignment lies outside `[-1, 1]` on an axis.
    #[error("a hero carousel's alignment lies outside [-1, 1]")]
    AlignmentOutOfRange,
}

/// How a carousel sizes the items it shows: the keylines items move through as they scroll.
///
/// Every item is laid out at the layout's large size and masked down to the size of the keyline
/// it passes, never squeezed, so its content does not reflow while it scrolls. The constructors
/// check their values: every size is finite, greater than zero and at most
/// [`MAX_ITEM_EXTENT`], so no keyline computed from them for any viewport and item count is
/// infinite.
///
/// ```
/// use flui_objects::{CarouselLayout, CarouselLayoutError};
///
/// let browse = CarouselLayout::multi_browse(40.0, 56.0).expect("an ordered range");
/// assert_eq!(
///     CarouselLayout::multi_browse(56.0, 40.0).unwrap_err(),
///     CarouselLayoutError::InvertedRange
/// );
/// assert!(CarouselLayout::weighted([1.0, 7.0, 1.0]).is_ok());
/// # let _ = browse;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct CarouselLayout {
    pub(super) kind: LayoutKind,
}

/// The checked layout.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum LayoutKind {
    /// Large items, then a medium and a small one: the items ahead are browsed.
    /// `large` is the large extent aimed for; half the viewport when not set.
    MultiBrowse {
        min_small: f64,
        max_small: f64,
        large: Option<f64>,
    },
    /// Items of one extent, the last one cut by the viewport's edge.
    Uncontained { item_extent: f64 },
    /// One large item at `alignment` along the reading direction, small items beside it.
    Hero { alignment: f64 },
    /// One item filling the viewport.
    FullScreen,
    /// Items sized by their weights' shares of the viewport; the largest is the large item.
    /// The weights are divided by the largest, so each is in `(0, 1]` or zero.
    Weighted { weights: Box<[f64]> },
}

/// The Material small item range, logical pixels.
pub(super) const MIN_SMALL: f64 = 40.0;
pub(super) const MAX_SMALL: f64 = 56.0;

fn checked_size(size: f64) -> Result<f64, CarouselLayoutError> {
    if !size.is_finite() {
        Err(CarouselLayoutError::NonFinite)
    } else if size <= 0.0 {
        Err(CarouselLayoutError::NonPositive)
    } else if size > MAX_ITEM_EXTENT {
        Err(CarouselLayoutError::TooLarge)
    } else {
        Ok(size)
    }
}

impl CarouselLayout {
    /// Large items, then a medium and a small one, the small one between `min_small` and
    /// `max_small` logical pixels (Material: 40 and 56), the large ones as near half the
    /// viewport as the arrangement allows ([`with_large_extent`](Self::with_large_extent) aims
    /// for another extent).
    ///
    /// # Errors
    ///
    /// [`CarouselLayoutError::NonFinite`], [`NonPositive`](CarouselLayoutError::NonPositive) or
    /// [`TooLarge`](CarouselLayoutError::TooLarge) for a size, and
    /// [`InvertedRange`](CarouselLayoutError::InvertedRange) when `min_small > max_small`.
    pub fn multi_browse(min_small: f64, max_small: f64) -> Result<Self, CarouselLayoutError> {
        let (min_small, max_small) = (checked_size(min_small)?, checked_size(max_small)?);
        if min_small > max_small {
            return Err(CarouselLayoutError::InvertedRange);
        }
        Ok(Self {
            kind: LayoutKind::MultiBrowse {
                min_small,
                max_small,
                large: None,
            },
        })
    }

    /// This multi-browse layout aiming for large items of `extent` logical pixels instead of
    /// half the viewport: the viewport takes as many as fit with a medium and a small item.
    /// Other layouts are returned as they are.
    ///
    /// # Errors
    ///
    /// [`CarouselLayoutError::NonFinite`], [`NonPositive`](CarouselLayoutError::NonPositive) or
    /// [`TooLarge`](CarouselLayoutError::TooLarge).
    pub fn with_large_extent(mut self, extent: f64) -> Result<Self, CarouselLayoutError> {
        let extent = checked_size(extent)?;
        if let LayoutKind::MultiBrowse { large, .. } = &mut self.kind {
            *large = Some(extent);
        }
        Ok(self)
    }

    /// Items `item_extent` logical pixels long, as many as fit, the next one cut by the
    /// viewport's edge.
    ///
    /// # Errors
    ///
    /// [`CarouselLayoutError::NonFinite`], [`NonPositive`](CarouselLayoutError::NonPositive) or
    /// [`TooLarge`](CarouselLayoutError::TooLarge).
    pub fn uncontained(item_extent: f64) -> Result<Self, CarouselLayoutError> {
        Ok(Self {
            kind: LayoutKind::Uncontained {
                item_extent: checked_size(item_extent)?,
            },
        })
    }

    /// One large item with small ones beside it. `alignment.x` places the large item along the
    /// reading direction: `-1` at the start with a small item after it, `0` centred between two,
    /// `1` at the end; `alignment.y` is checked but does not move it.
    ///
    /// # Errors
    ///
    /// [`CarouselLayoutError::NonFinite`] or
    /// [`AlignmentOutOfRange`](CarouselLayoutError::AlignmentOutOfRange) when a component is
    /// outside `[-1, 1]`.
    pub fn hero(alignment: Alignment) -> Result<Self, CarouselLayoutError> {
        for value in [alignment.x, alignment.y] {
            if !value.is_finite() {
                return Err(CarouselLayoutError::NonFinite);
            }
            if !(-1.0..=1.0).contains(&value) {
                return Err(CarouselLayoutError::AlignmentOutOfRange);
            }
        }
        Ok(Self {
            kind: LayoutKind::Hero {
                alignment: alignment.x,
            },
        })
    }

    /// One item filling the viewport.
    #[must_use]
    pub fn full_screen() -> Self {
        Self {
            kind: LayoutKind::FullScreen,
        }
    }

    /// Items sized by their weights' shares of the viewport, in order; the largest weight is the
    /// large item. A zero weight is an item that is not shown in that slot. The weights are
    /// divided by the largest before they are summed, so even `[f64::MAX, f64::MAX]` gives two
    /// equal halves.
    ///
    /// # Errors
    ///
    /// [`CarouselLayoutError::NonFinite`], [`NonPositive`](CarouselLayoutError::NonPositive) for
    /// a negative weight, and [`NoWeight`](CarouselLayoutError::NoWeight) when none is greater
    /// than zero.
    pub fn weighted(weights: impl IntoIterator<Item = f64>) -> Result<Self, CarouselLayoutError> {
        let weights: Vec<f64> = weights.into_iter().collect();
        if weights.iter().any(|weight| !weight.is_finite()) {
            return Err(CarouselLayoutError::NonFinite);
        }
        if weights.iter().any(|&weight| weight < 0.0) {
            return Err(CarouselLayoutError::NonPositive);
        }
        let largest = weights.iter().copied().fold(0.0_f64, f64::max);
        if largest <= 0.0 {
            return Err(CarouselLayoutError::NoWeight);
        }
        Ok(Self {
            kind: LayoutKind::Weighted {
                weights: weights.iter().map(|weight| weight / largest).collect(),
            },
        })
    }
}

impl Default for CarouselLayout {
    /// Material's multi-browse layout: small items between 40 and 56 logical pixels.
    fn default() -> Self {
        Self {
            kind: LayoutKind::MultiBrowse {
                min_small: MIN_SMALL,
                max_small: MAX_SMALL,
                large: None,
            },
        }
    }
}
