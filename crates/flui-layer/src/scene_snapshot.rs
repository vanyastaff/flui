//! `SceneSnapshot` — the owned per-presentation per-frame raster package —
//! and the damage it carries (ADR-0304 §5).
//!
//! Compositing produces one `SceneSnapshot` per window per frame; it is the one
//! seam a `UiRuntime` hands to a raster owner.

use std::num::NonZeroU8;

use flui_foundation::FrameStamp;
use flui_foundation::geometry::Rect;

use crate::scene::Scene;

/// Which pixels of a [`SceneSnapshot`] changed since the scene submitted
/// before it (ADR-0304 §5).
///
/// Relative to the previous *submitted* scene, not the previous *presented*
/// one: a backend accumulates the damage of every frame it has not yet
/// presented, so a frame dropped on the way costs nothing but a larger next
/// repaint.
///
/// Exhaustive, as `PresentDisposition` is: a consumer matches every variant
/// with no wildcard, so a new one is a compile error wherever damage is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageRegion {
    /// Nothing changed; a backend with no older debt may skip the frame.
    Unchanged,
    /// Only the pixels inside these rectangles changed.
    Region(Region),
    /// Repaint the entire frame, for this reason.
    Full(FullReason),
}

impl DamageRegion {
    /// The damage of two frames taken together: `Full` absorbs everything and
    /// keeps the first reason, `Unchanged` is the identity, and two regions
    /// merge rect by rect ([`Region::insert`]).
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::Full(reason), _) | (Self::Unchanged | Self::Region(_), Self::Full(reason)) => {
                Self::Full(reason)
            }
            (Self::Unchanged, region) | (region, Self::Unchanged) => region,
            (Self::Region(mut a), Self::Region(b)) => {
                for &rect in b.rects() {
                    a.insert(rect);
                }
                Self::Region(a)
            }
        }
    }
}

/// Why a frame repaints in full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FullReason {
    /// Nothing was presented before this frame to repaint over.
    FirstFrame,
    /// The surface's size changed, or it has no area.
    SurfaceChanged,
    /// The damaged area is above the mode's `full_above` fraction of the
    /// surface, where a partial repaint saves little.
    AboveThreshold,
    /// The damage has no bound.
    Unbounded,
    /// A damaged rectangle was not finite.
    NonFinite,
    /// Damage is switched off (`FLUI_DAMAGE=off`).
    DamageOff,
    /// The producer and the raster side lost their common base.
    Resync(ResyncReason),
}

/// Why a frame's damage could not be computed against the frame before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResyncReason {
    /// The frame names a base the raster side does not hold.
    BaseMismatch,
    /// The raster side dropped what it held (a device or surface reset, a
    /// raster panic).
    RasterReset,
    /// The scene could not be paired with the one before it: its root or a
    /// boundary was rebuilt.
    Rebuilt,
}

/// A rectangle given to [`DamageRect::try_covering`] was not finite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a damage rectangle is not finite")]
pub struct NonFinite;

/// Up to [`Region::MAX`] non-empty, pairwise disjoint damaged rectangles.
///
/// Never empty: no damage is [`DamageRegion::Unchanged`].
#[derive(Debug, Clone, Copy)]
pub struct Region {
    /// `rects[..len]` hold the region; the rest repeat its first rect.
    rects: [DamageRect; Region::MAX],
    len: NonZeroU8,
}

impl Region {
    /// The most rectangles a region holds; a ninth disjoint one merges the
    /// cheapest pair. A hypothesis the damage perf scenario measures.
    pub const MAX: usize = 8;

    /// The region of one rectangle.
    #[must_use]
    pub fn new(rect: DamageRect) -> Self {
        Self {
            rects: [rect; Self::MAX],
            len: NonZeroU8::MIN,
        }
    }

    /// Adds `rect`, keeping the rects pairwise disjoint.
    ///
    /// `rect` merges with every rect it overlaps into their bounding box,
    /// repeatedly until none overlap. When that leaves more than
    /// [`Self::MAX`] rects, the pair whose bounding box adds the least area
    /// merges, under the same repetition.
    pub fn insert(&mut self, rect: DamageRect) {
        let mut rects = [rect; Self::MAX + 1];
        let mut len = usize::from(self.len.get());
        rects[..len].copy_from_slice(self.rects());
        let mut pending = rect;
        loop {
            pending = absorb_overlapping(&mut rects, &mut len, pending);
            if len < Self::MAX {
                rects[len] = pending;
                len += 1;
                break;
            }
            // Full: merge the cheapest pair among the kept rects and the
            // pending one, and insert the merged box in its place.
            rects[len] = pending;
            let (i, j) = cheapest_pair(&rects[..=len]);
            pending = rects[i].union(rects[j]);
            // Remove `j` then `i` (`i < j`), keeping the order of the rest.
            rects.copy_within(j + 1..=len, j);
            rects.copy_within(i + 1..len, i);
            len -= 1;
        }
        let filler = rects[0];
        self.rects = [filler; Self::MAX];
        self.rects[..len].copy_from_slice(&rects[..len]);
        self.len =
            NonZeroU8::new(u8::try_from(len).expect("BUG: a region holds at most MAX rects"))
                .expect("BUG: a region holds at least the rect just inserted");
    }

    /// The rects, pairwise disjoint and non-empty.
    #[must_use]
    pub fn rects(&self) -> &[DamageRect] {
        &self.rects[..usize::from(self.len.get())]
    }

    /// The smallest rectangle covering every rect.
    #[must_use]
    pub fn bounds(&self) -> DamageRect {
        let (first, rest) = self
            .rects()
            .split_first()
            .expect("BUG: a region holds at least one rect");
        rest.iter().fold(*first, |acc, rect| acc.union(*rect))
    }

    /// The pixels covered: the sum of the rects' areas, since they are
    /// disjoint. Cannot overflow: disjoint rects inside a `u32` surface cover
    /// at most its area.
    #[must_use]
    pub fn area(&self) -> u64 {
        self.rects().iter().map(|rect| rect.area()).sum()
    }
}

impl PartialEq for Region {
    fn eq(&self, other: &Self) -> bool {
        self.rects() == other.rects()
    }
}

impl Eq for Region {}

/// Merges every rect of `rects[..*len]` that overlaps `pending` into it,
/// repeatedly until none does, and removes the merged ones.
fn absorb_overlapping(
    rects: &mut [DamageRect; Region::MAX + 1],
    len: &mut usize,
    mut pending: DamageRect,
) -> DamageRect {
    let mut index = 0;
    while index < *len {
        if rects[index].overlaps(pending) {
            pending = pending.union(rects[index]);
            rects.copy_within(index + 1..*len, index);
            *len -= 1;
            // The grown box may now reach a rect already passed.
            index = 0;
        } else {
            index += 1;
        }
    }
    pending
}

/// The indexes `(i, j)`, `i < j`, of the pair whose bounding box adds the
/// least area over the two rects; the first such pair on a tie.
fn cheapest_pair(rects: &[DamageRect]) -> (usize, usize) {
    let mut best = (0, 1);
    let mut best_cost = u64::MAX;
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            let (a, b) = (rects[i], rects[j]);
            // Disjoint, so the box holds both areas whole.
            let cost = a.union(b).area() - a.area() - b.area();
            if cost < best_cost {
                best = (i, j);
                best_cost = cost;
            }
        }
    }
    best
}

/// A damaged rectangle in physical surface pixels: whole pixels, never empty.
///
/// Integers, because the consumer is a scissor: a fractional or empty damage
/// rectangle has no scissor to become, and this type cannot express one.
/// Edges are half-open: `left..right` and `top..bottom`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DamageRect {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

impl DamageRect {
    /// The anti-aliasing margin [`Self::try_covering`] adds on every side.
    ///
    /// An edge that falls inside a pixel changes that pixel, and an
    /// anti-aliased edge can reach the next one; one whole pixel past the
    /// rounded-out rectangle covers both.
    pub const AA_MARGIN: u32 = 1;

    /// The whole pixels `rect` (in surface pixels) touches, grown by
    /// [`Self::AA_MARGIN`] and clamped to a `surface` of `(width, height)`.
    ///
    /// `Ok(None)` means empty or off the surface, nothing else.
    ///
    /// # Errors
    ///
    /// [`NonFinite`] when an edge of `rect` is not finite: a producer turns it
    /// into [`FullReason::NonFinite`].
    pub fn try_covering(rect: Rect<f64>, surface: (u32, u32)) -> Result<Option<Self>, NonFinite> {
        let (width, height) = surface;
        let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
        if !(l.is_finite() && t.is_finite() && r.is_finite() && b.is_finite()) {
            return Err(NonFinite);
        }
        if r <= l || b <= t {
            return Ok(None);
        }
        let margin = f64::from(Self::AA_MARGIN);
        // Clamped in `f64` first, so the casts below are in range by
        // construction (`0.0..=width` and `0.0..=height`).
        let clamp = |v: f64, max: u32| v.clamp(0.0, f64::from(max));
        let left = clamp((l - margin).floor(), width) as u32;
        let top = clamp((t - margin).floor(), height) as u32;
        let right = clamp((r + margin).ceil(), width) as u32;
        let bottom = clamp((b + margin).ceil(), height) as u32;
        Ok((right > left && bottom > top).then_some(Self {
            left,
            top,
            right,
            bottom,
        }))
    }

    /// The smallest rectangle covering both.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    /// Whether the two share a pixel. Rects that only touch along an edge do
    /// not.
    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        self.left < other.right
            && other.left < self.right
            && self.top < other.bottom
            && other.top < self.bottom
    }

    /// Left edge, inclusive.
    #[inline]
    #[must_use]
    pub fn left(self) -> u32 {
        self.left
    }

    /// Top edge, inclusive.
    #[inline]
    #[must_use]
    pub fn top(self) -> u32 {
        self.top
    }

    /// Right edge, exclusive.
    #[inline]
    #[must_use]
    pub fn right(self) -> u32 {
        self.right
    }

    /// Bottom edge, exclusive.
    #[inline]
    #[must_use]
    pub fn bottom(self) -> u32 {
        self.bottom
    }

    /// The number of pixels covered.
    #[must_use]
    pub fn area(self) -> u64 {
        u64::from(self.right - self.left) * u64::from(self.bottom - self.top)
    }

    /// The same rectangle as surface-pixel geometry, for a backend's scissor.
    #[must_use]
    pub fn to_rect(self) -> Rect<f64> {
        Rect::from_ltrb(
            f64::from(self.left),
            f64::from(self.top),
            f64::from(self.right),
            f64::from(self.bottom),
        )
    }
}

/// The owned per-presentation per-frame raster package.
///
/// Produced by compositing and moved **by value** into the raster mailbox:
/// ownership transfer, not reference counting, is the seam. The raster owner
/// is the sole holder once a snapshot is sent and drops it when done. The
/// type is `Send + Sync` (plain data), pinned below; the by-value contract is
/// enforced by the mailbox API taking ownership, not by the auto traits.
///
/// # Frame identity
///
/// `stamp` carries the full identity/versioning group — which presentation,
/// which epoch, against which surface and GPU-resource generation. See
/// [`FrameStamp`]'s own doc for why those are one value rather than fields
/// here.
///
/// Fields are `pub` for direct read/match access; `#[non_exhaustive]` makes
/// matching additive when a field is added, never construction.
#[non_exhaustive]
#[derive(Debug)]
pub struct SceneSnapshot {
    /// This frame's identity: which presentation, which epoch, against which
    /// surface and GPU-resource generation.
    pub stamp: FrameStamp,
    /// Which regions changed since the previous frame.
    pub damage: DamageRegion,
    /// The composited layer tree, ready to render.
    pub scene: Scene,
}

impl SceneSnapshot {
    /// Packages a composited [`Scene`] with the identity the raster boundary
    /// needs to accept, reject, or reconcile it.
    #[must_use]
    pub fn new(stamp: FrameStamp, damage: DamageRegion, scene: Scene) -> Self {
        Self {
            stamp,
            damage,
            scene,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(l: u32, t: u32, r: u32, b: u32) -> DamageRect {
        DamageRect::try_covering(
            Rect::from_ltrb(
                f64::from(l) + 1.0,
                f64::from(t) + 1.0,
                f64::from(r) - 1.0,
                f64::from(b) - 1.0,
            ),
            (1000, 1000),
        )
        .expect("finite")
        .expect("a non-empty rect on the surface")
    }

    fn region(rects: &[DamageRect]) -> DamageRegion {
        let (first, rest) = rects.split_first().expect("a region has a rect");
        let mut region = Region::new(*first);
        for &rect in rest {
            region.insert(rect);
        }
        DamageRegion::Region(region)
    }

    #[test]
    fn union_table() {
        use DamageRegion::{Full, Unchanged};
        let a = region(&[rect(0, 0, 10, 10)]);
        let b = region(&[rect(20, 20, 30, 30)]);
        let first = Full(FullReason::FirstFrame);
        assert_eq!(first.union(a), first);
        assert_eq!(a.union(first), first);
        assert_eq!(first.union(Unchanged), first);
        assert_eq!(Unchanged.union(Unchanged), Unchanged);
        assert_eq!(Unchanged.union(a), a);
        assert_eq!(a.union(Unchanged), a);
        assert_eq!(
            a.union(b),
            region(&[rect(0, 0, 10, 10), rect(20, 20, 30, 30)]),
            "two disjoint rects stay two"
        );
        assert_eq!(
            a.union(region(&[rect(5, 5, 15, 15)])),
            region(&[rect(0, 0, 15, 15)]),
            "overlapping rects merge into their box"
        );
    }

    #[test]
    fn bounds_round_outward_with_aa_margin() {
        let covering = |rect, surface| DamageRect::try_covering(rect, surface);
        let damage = covering(Rect::from_ltrb(10.25, 20.5, 30.75, 40.0), (100, 100))
            .expect("finite")
            .expect("inside the surface");
        assert_eq!(
            (damage.left(), damage.top(), damage.right(), damage.bottom()),
            (9, 19, 32, 41),
            "floor/ceil outward, then one pixel of anti-aliasing margin"
        );
        assert_eq!(damage.area(), 23 * 22);
        assert_eq!(damage.to_rect(), Rect::from_ltrb(9.0, 19.0, 32.0, 41.0));

        let clamped = covering(Rect::from_ltrb(-5.0, -5.0, 150.0, 5.0), (100, 100))
            .expect("finite")
            .expect("overlaps the surface");
        assert_eq!(
            (
                clamped.left(),
                clamped.top(),
                clamped.right(),
                clamped.bottom()
            ),
            (0, 0, 100, 6)
        );
        assert_eq!(
            covering(Rect::from_ltrb(200.0, 200.0, 300.0, 300.0), (100, 100)),
            Ok(None),
            "off the surface"
        );
        assert_eq!(
            covering(Rect::from_ltrb(10.0, 10.0, 10.0, 20.0), (100, 100)),
            Ok(None),
            "empty"
        );
        assert_eq!(
            covering(Rect::from_ltrb(f64::NAN, 0.0, 10.0, 10.0), (100, 100)),
            Err(NonFinite),
            "a non-finite rect is an error, not an empty one"
        );
    }
}
