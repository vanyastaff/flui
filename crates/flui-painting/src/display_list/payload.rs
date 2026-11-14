//! The payloads a display list keeps beside its commands (ADR-0333).
//!
//! A path, a pair of rounded rectangles, an image region's placement and a
//! colour filter are each larger than every other operation, so an op that
//! carries one holds a [`PayloadId`] into its list's payload buffer instead.
//! The buffer is the list's own and is cleared with it, keeping its room, so
//! a list recorded again into the same space allocates nothing for its
//! payloads; [`DrawCommand::payload`](super::DrawCommand::payload) reads one.

use std::fmt;
use std::marker::PhantomData;

use flui_foundation::geometry::{RRect, Rect};

use super::{ColorFilter, ImageRepeat};
use crate::paint::Path;

/// Names a payload of kind `T` in the display list that recorded it.
///
/// An id is a position in its list's buffer, so it means something only next
/// to the commands it was read with, and ids of two lists compare equal when
/// their positions do: a
/// [`DrawCommand`](super::DrawCommand) or a
/// [`DrawCommandBuf`](super::DrawCommandBuf) resolves it.
pub struct PayloadId<T> {
    index: u32,
    kind: PhantomData<fn() -> T>,
}

impl<T> PayloadId<T> {
    /// The id of the entry at `index` of a list's payload buffer.
    #[expect(
        clippy::expect_used,
        reason = "2^32 payloads of at least 84 bytes each are past any heap a display list records into"
    )]
    pub(crate) fn at(index: usize) -> Self {
        Self {
            index: u32::try_from(index)
                .expect("BUG: a display list holds fewer than 2^32 payloads"),
            kind: PhantomData,
        }
    }

    /// The entry's position in its list's payload buffer.
    pub(crate) fn index(self) -> usize {
        // A `u32` fits in `usize` on every target FLUI builds for.
        self.index as usize
    }
}

impl<T> Clone for PayloadId<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for PayloadId<T> {}

impl<T> PartialEq for PayloadId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<T> Eq for PayloadId<T> {}

impl<T> fmt::Debug for PayloadId<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PayloadId").field(&self.index).finish()
    }
}

/// The area inside `outer` and outside `inner`, which
/// [`DrawOp::DRRect`](super::DrawOp::DRRect) fills.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct NestedRRects {
    /// Outer rounded rectangle.
    pub outer: RRect,
    /// Inner rounded rectangle.
    pub inner: RRect,
}

impl NestedRRects {
    /// The area inside `outer` and outside `inner`.
    #[must_use]
    pub const fn new(outer: RRect, inner: RRect) -> Self {
        Self { outer, inner }
    }
}

/// Where [`DrawOp::ImageRegion`](super::DrawOp::ImageRegion) places its
/// source texels (ADR-0115).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ImageRegionPlacement {
    /// Positive in-bounds source rectangle in image texels.
    pub src: Rect<f64>,
    /// Logical destination; repeated draws cover this rectangle.
    pub dst: Rect<f64>,
    /// Fitted logical tile rectangle, including aligned phase.
    /// `None` keeps the natural source size and destination-origin phase.
    pub tile: Option<Rect<f64>>,
    /// Repeated axes, or a single destination quad.
    pub repeat: ImageRepeat,
    /// Filter decoded straight channels before Paint and sampling.
    pub filter: Option<ColorFilter>,
}

/// A kind of payload a display list keeps beside its commands: [`Path`],
/// [`NestedRRects`], [`ImageRegionPlacement`] and [`ColorFilter`].
pub trait Payload: private::Sealed {}

impl Payload for Path {}
impl Payload for NestedRRects {}
impl Payload for ImageRegionPlacement {}
impl Payload for ColorFilter {}

pub(crate) use private::StoredPayload;

mod private {
    use super::{ColorFilter, ImageRegionPlacement, NestedRRects, Path};

    /// One entry of a list's payload buffer.
    #[derive(Debug, Clone)]
    pub enum StoredPayload {
        Path(Path),
        NestedRRects(NestedRRects),
        ImageRegion(ImageRegionPlacement),
        ColorFilter(ColorFilter),
    }

    impl StoredPayload {
        /// Whether `self` is known to hold the same payload as `other`
        /// without any work that allocates, so an op can name the entry
        /// already stored instead of storing a copy.
        pub fn known_equal(&self, other: &Self) -> bool {
            match (self, other) {
                (Self::Path(mine), Self::Path(theirs)) => mine.known_equal(theirs),
                (Self::NestedRRects(mine), Self::NestedRRects(theirs)) => mine == theirs,
                (Self::ImageRegion(mine), Self::ImageRegion(theirs)) => mine == theirs,
                (Self::ColorFilter(mine), Self::ColorFilter(theirs)) => mine == theirs,
                _ => false,
            }
        }
    }

    /// Moves a payload into and reads it out of a buffer entry.
    pub trait Sealed: Sized {
        fn into_stored(self) -> StoredPayload;
        fn from_stored(stored: &StoredPayload) -> Option<&Self>;
    }

    impl Sealed for Path {
        fn into_stored(self) -> StoredPayload {
            StoredPayload::Path(self)
        }
        fn from_stored(stored: &StoredPayload) -> Option<&Self> {
            match stored {
                StoredPayload::Path(path) => Some(path),
                _ => None,
            }
        }
    }

    impl Sealed for NestedRRects {
        fn into_stored(self) -> StoredPayload {
            StoredPayload::NestedRRects(self)
        }
        fn from_stored(stored: &StoredPayload) -> Option<&Self> {
            match stored {
                StoredPayload::NestedRRects(rrects) => Some(rrects),
                _ => None,
            }
        }
    }

    impl Sealed for ImageRegionPlacement {
        fn into_stored(self) -> StoredPayload {
            StoredPayload::ImageRegion(self)
        }
        fn from_stored(stored: &StoredPayload) -> Option<&Self> {
            match stored {
                StoredPayload::ImageRegion(region) => Some(region),
                _ => None,
            }
        }
    }

    impl Sealed for ColorFilter {
        fn into_stored(self) -> StoredPayload {
            StoredPayload::ColorFilter(self)
        }
        fn from_stored(stored: &StoredPayload) -> Option<&Self> {
            match stored {
                StoredPayload::ColorFilter(filter) => Some(filter),
                _ => None,
            }
        }
    }
}

/// The payload `id` names in `payloads`.
#[expect(
    clippy::expect_used,
    reason = "an id is minted by its own list and rebased with its payloads, and the kind is sealed"
)]
pub(crate) fn resolve<T: Payload>(payloads: &[StoredPayload], id: PayloadId<T>) -> &T {
    lookup(payloads, id)
        .expect("BUG: a payload id names an entry of its own kind in the list it was read with")
}

/// The payload `id` names in `payloads`, or `None` when the entry is missing
/// or of another kind.
pub(crate) fn lookup<T: Payload>(payloads: &[StoredPayload], id: PayloadId<T>) -> Option<&T> {
    payloads.get(id.index()).and_then(T::from_stored)
}
