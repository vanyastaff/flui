//! `SceneSnapshot` — the owned per-presentation per-frame raster package.
//!
//! Compositing produces one `SceneSnapshot` per window per frame; it is the one
//! seam a `UiRealm` hands to a raster owner.

use flui_foundation::FrameStamp;
use flui_foundation::geometry::Rect;

use crate::scene::Scene;

/// Which pixels of a [`SceneSnapshot`] changed since the scene submitted
/// before it.
///
/// Produced by [`LayerDiffer`](crate::LayerDiffer) (ADR-0087 §3). Relative to
/// the previous *submitted* scene, not the previous *presented* one: a
/// backend accumulates the damage of every frame it has not yet presented,
/// so a frame dropped on the way costs nothing but a larger next repaint.
///
/// `#[non_exhaustive]`, so a multi-rect variant is additive for matchers.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageRegion {
    /// Repaint the entire frame.
    Full,
    /// Only the pixels inside this rectangle changed.
    Partial(DamageRect),
    /// Nothing changed; a backend with no older debt may skip the frame.
    Unchanged,
}

impl DamageRegion {
    /// The damage of two frames taken together: `Full` absorbs everything,
    /// `Unchanged` is the identity, and two partial regions join into the
    /// rectangle covering both.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::Full, _) | (_, Self::Full) => Self::Full,
            (Self::Unchanged, region) | (region, Self::Unchanged) => region,
            (Self::Partial(a), Self::Partial(b)) => Self::Partial(a.union(b)),
        }
    }
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
    /// The anti-aliasing margin [`Self::covering`] adds on every side.
    ///
    /// An edge that falls inside a pixel changes that pixel, and an
    /// anti-aliased edge can reach the next one; one whole pixel past the
    /// rounded-out rectangle covers both.
    pub const AA_MARGIN: u32 = 1;

    /// The whole pixels `rect` (in surface pixels) touches, grown by
    /// [`Self::AA_MARGIN`] and clamped to a `surface` of `(width, height)`;
    /// `None` when nothing is left (an empty or off-surface rect, or a
    /// non-finite one).
    #[must_use]
    pub fn covering(rect: Rect<f64>, surface: (u32, u32)) -> Option<Self> {
        let (width, height) = surface;
        let (l, t, r, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
        if !(l.is_finite() && t.is_finite() && r.is_finite() && b.is_finite()) {
            return None;
        }
        if r <= l || b <= t {
            return None;
        }
        let margin = f64::from(Self::AA_MARGIN);
        // Clamped in `f64` first, so the casts below are in range by
        // construction (`0.0..=width` and `0.0..=height`).
        let clamp = |v: f64, max: u32| v.clamp(0.0, f64::from(max));
        let left = clamp((l - margin).floor(), width) as u32;
        let top = clamp((t - margin).floor(), height) as u32;
        let right = clamp((r + margin).ceil(), width) as u32;
        let bottom = clamp((b + margin).ceil(), height) as u32;
        (right > left && bottom > top).then_some(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    /// The whole surface of `(width, height)`; `None` for a zero-sized one.
    #[must_use]
    pub fn surface(surface: (u32, u32)) -> Option<Self> {
        let (right, bottom) = surface;
        (right > 0 && bottom > 0).then_some(Self {
            left: 0,
            top: 0,
            right,
            bottom,
        })
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
    use static_assertions::assert_impl_all;

    use super::*;
    use crate::{Layer, LayerTree, OffsetLayer};

    // The values that cross the raster boundary (and, via `Layer`, every
    // payload they carry) are plain data: `Send` because the snapshot moves to
    // the raster thread, `Sync` because nothing in them is interior-mutable.
    // A future field that breaks either trait breaks this line, not a caller.
    assert_impl_all!(Layer: Send, Sync);
    assert_impl_all!(LayerTree: Send, Sync);
    assert_impl_all!(Scene: Send, Sync);
    assert_impl_all!(SceneSnapshot: Send, Sync);

    fn test_stamp() -> FrameStamp {
        FrameStamp::new(
            flui_foundation::PresentationAddress {
                realm_id: flui_foundation::RealmId::new(1),
                presentation_id: flui_foundation::PresentationId::new(1),
            },
            flui_foundation::FrameEpoch::ZERO.next(),
            flui_foundation::SurfaceGeneration::ZERO,
            flui_foundation::GpuResourceGeneration::ZERO,
        )
    }

    #[test]
    fn new_packages_all_fields() {
        let stamp = test_stamp();
        let tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
        let root = tree.root();

        let frame = SceneSnapshot::new(stamp, DamageRegion::Full, Scene::new(tree));

        assert_eq!(frame.stamp, stamp);
        assert_eq!(frame.damage, DamageRegion::Full);
        assert_eq!(frame.scene.root(), root);
    }

    fn rect(l: u32, t: u32, r: u32, b: u32) -> DamageRect {
        DamageRect::covering(
            Rect::from_ltrb(
                f64::from(l) + 1.0,
                f64::from(t) + 1.0,
                f64::from(r) - 1.0,
                f64::from(b) - 1.0,
            ),
            (1000, 1000),
        )
        .expect("a non-empty rect on the surface")
    }

    #[test]
    fn union_table() {
        let a = DamageRegion::Partial(rect(0, 0, 10, 10));
        let b = DamageRegion::Partial(rect(20, 20, 30, 30));
        use DamageRegion::{Full, Unchanged};
        assert_eq!(Full.union(a), Full);
        assert_eq!(a.union(Full), Full);
        assert_eq!(Full.union(Unchanged), Full);
        assert_eq!(Unchanged.union(Unchanged), Unchanged);
        assert_eq!(Unchanged.union(a), a);
        assert_eq!(a.union(Unchanged), a);
        assert_eq!(a.union(b), DamageRegion::Partial(rect(0, 0, 30, 30)));
    }

    #[test]
    fn bounds_round_outward_with_aa_margin() {
        let damage = DamageRect::covering(Rect::from_ltrb(10.25, 20.5, 30.75, 40.0), (100, 100))
            .expect("inside the surface");
        assert_eq!(
            (damage.left(), damage.top(), damage.right(), damage.bottom()),
            (9, 19, 32, 41),
            "floor/ceil outward, then one pixel of anti-aliasing margin"
        );
        assert_eq!(damage.area(), 23 * 22);
        assert_eq!(damage.to_rect(), Rect::from_ltrb(9.0, 19.0, 32.0, 41.0));

        let clamped = DamageRect::covering(Rect::from_ltrb(-5.0, -5.0, 150.0, 5.0), (100, 100))
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
            DamageRect::covering(Rect::from_ltrb(200.0, 200.0, 300.0, 300.0), (100, 100)),
            None,
            "off the surface"
        );
        assert_eq!(
            DamageRect::covering(Rect::from_ltrb(10.0, 10.0, 10.0, 20.0), (100, 100)),
            None,
            "empty"
        );
        assert_eq!(
            DamageRect::covering(Rect::from_ltrb(f64::NAN, 0.0, 10.0, 10.0), (100, 100)),
            None
        );
    }
}
