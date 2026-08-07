//! `SceneSnapshot` — the owned per-presentation per-frame raster package.
//!
//! Compositing produces one `SceneSnapshot` per window per frame; it is the one
//! seam a `UiRealm` hands to a raster owner (Flutter parity:
//! `RenderView.compositeFrame` → `FlutterView.render` → dispose).

use flui_foundation::FrameStamp;

use crate::scene::Scene;

/// Which regions of a [`SceneSnapshot`] changed since the previous frame.
///
/// Only [`DamageRegion::Full`] exists today: every fresh [`Scene`] forces a
/// full repaint (`flui-app`'s `binding.rs:837-844`). The type is
/// `#[non_exhaustive]` so fine-grained sub-rect damage is additive later
/// instead of a breaking change — a `match` on this
/// enum already needs a `_` arm today, so a future `Partial` variant slots in
/// without touching existing call sites.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageRegion {
    /// Repaint the entire frame. The only variant that exists today.
    Full,
}

/// The owned per-presentation per-frame raster package.
///
/// Produced by compositing and moved **by value** into the raster mailbox —
/// never `Arc<Scene>`. Ownership transfer, not shared reference counting, is
/// the seam: the raster owner is the sole reader once a `SceneSnapshot` is sent,
/// and it drops (or acks `Dropped`) the frame when done. This is one seam per
/// window per frame, mirroring Flutter's `RenderView.compositeFrame` →
/// `FlutterView.render` → dispose sequence.
///
/// # Frame identity
///
/// `stamp` carries the full identity/versioning group — which presentation,
/// which epoch, against which raster surface configuration. See
/// [`FrameStamp`]'s own doc for why those three values are bundled into one
/// type rather than three struct fields here.
///
/// # Construction is additive, not positional
///
/// Fields are `pub` for direct read/match access; `#[non_exhaustive]` makes
/// *matching* on this struct additive when a field is added later.
/// Construction goes through [`SceneSnapshot::builder`], never a positional
/// constructor — the pre-graduation gate this type used to carry (a growing
/// positional `new()` breaking every external call site) is discharged by
/// the builder: a future field addition here widens [`SceneSnapshotBuilder`]
/// with one more typestate slot and setter, and touches no existing call
/// site.
#[non_exhaustive]
#[derive(Debug)]
pub struct SceneSnapshot {
    /// This frame's identity: which presentation, which epoch, against
    /// which raster surface configuration. See the type doc above and
    /// [`FrameStamp`]'s own doc for the full disambiguation argument.
    pub stamp: FrameStamp,
    /// Which regions changed since the previous frame.
    pub damage: DamageRegion,
    /// The composited layer tree, ready to render.
    pub scene: Scene,
}

impl SceneSnapshot {
    /// Starts building a [`SceneSnapshot`]. Every field is required; the
    /// returned builder only exposes [`SceneSnapshotBuilder::build`] once
    /// [`SceneSnapshotBuilder::stamp`], [`SceneSnapshotBuilder::damage`], and
    /// [`SceneSnapshotBuilder::scene`] have all been called — enforced at
    /// compile time via the builder's typestate, not by a runtime check.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::{
    ///     FrameEpoch, FrameStamp, PresentationAddress, PresentationId, RealmId,
    ///     SurfaceGeneration,
    /// };
    /// use flui_layer::{CanvasLayer, DamageRegion, Layer, Scene, SceneSnapshot};
    /// use flui_types::Size;
    ///
    /// let stamp = FrameStamp::builder()
    ///     .address(PresentationAddress {
    ///         realm_id: RealmId::new(1),
    ///         presentation_id: PresentationId::new(1),
    ///     })
    ///     .epoch(FrameEpoch::ZERO)
    ///     .surface_generation(SurfaceGeneration::ZERO)
    ///     .build();
    /// let scene = Scene::from_layer(Size::ZERO, Layer::from(CanvasLayer::new()), 0);
    ///
    /// let snapshot = SceneSnapshot::builder()
    ///     .stamp(stamp)
    ///     .damage(DamageRegion::Full)
    ///     .scene(scene)
    ///     .build();
    ///
    /// assert_eq!(snapshot.stamp, stamp);
    /// ```
    #[must_use]
    pub fn builder() -> SceneSnapshotBuilder {
        SceneSnapshotBuilder::new()
    }
}

/// Builder for [`SceneSnapshot`].
///
/// A typestate builder, the same shape as [`FrameStampBuilder`](flui_foundation::FrameStampBuilder):
/// each setter is offered only while its own slot is still the unit type
/// `()` (unfilled), and [`Self::build`] is offered only once every slot
/// holds its real value. Calling the setters in any order reaches the same
/// buildable state.
#[derive(Debug)]
pub struct SceneSnapshotBuilder<Stamp = (), Damage = (), SceneValue = ()> {
    stamp: Stamp,
    damage: Damage,
    scene: SceneValue,
}

impl SceneSnapshotBuilder {
    fn new() -> Self {
        Self {
            stamp: (),
            damage: (),
            scene: (),
        }
    }
}

impl Default for SceneSnapshotBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl<Damage, SceneValue> SceneSnapshotBuilder<(), Damage, SceneValue> {
    /// Sets this frame's identity group.
    #[must_use]
    pub fn stamp(self, stamp: FrameStamp) -> SceneSnapshotBuilder<FrameStamp, Damage, SceneValue> {
        SceneSnapshotBuilder {
            stamp,
            damage: self.damage,
            scene: self.scene,
        }
    }
}

impl<Stamp, SceneValue> SceneSnapshotBuilder<Stamp, (), SceneValue> {
    /// Sets which regions changed since the previous frame.
    #[must_use]
    pub fn damage(
        self,
        damage: DamageRegion,
    ) -> SceneSnapshotBuilder<Stamp, DamageRegion, SceneValue> {
        SceneSnapshotBuilder {
            stamp: self.stamp,
            damage,
            scene: self.scene,
        }
    }
}

impl<Stamp, Damage> SceneSnapshotBuilder<Stamp, Damage, ()> {
    /// Sets the composited layer tree, ready to render.
    #[must_use]
    pub fn scene(self, scene: Scene) -> SceneSnapshotBuilder<Stamp, Damage, Scene> {
        SceneSnapshotBuilder {
            stamp: self.stamp,
            damage: self.damage,
            scene,
        }
    }
}

impl SceneSnapshotBuilder<FrameStamp, DamageRegion, Scene> {
    /// Builds the [`SceneSnapshot`]. Only reachable once every field has
    /// been set — see the type's own doc.
    #[must_use]
    pub fn build(self) -> SceneSnapshot {
        SceneSnapshot {
            stamp: self.stamp,
            damage: self.damage,
            scene: self.scene,
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_types::Size;
    use static_assertions::assert_impl_all;

    use super::*;
    use crate::CanvasLayer;

    // The retained-seam boundary value moved from the (owner-thread-confined)
    // rendering side to the raster/present side -- must stay `Send`
    // independent of `PipelineCell`'s `!Send` upstream. Not `Sync`: `Scene`
    // carries `Box<dyn FnOnce() + Send>` composition callbacks, which are
    // `Send` but never `Sync`, and a snapshot is moved across the boundary
    // (one owner at a time), never shared by reference.
    assert_impl_all!(SceneSnapshot: Send);

    fn test_stamp() -> FrameStamp {
        FrameStamp::builder()
            .address(flui_foundation::PresentationAddress {
                realm_id: flui_foundation::RealmId::new(1),
                presentation_id: flui_foundation::PresentationId::new(1),
            })
            .epoch(flui_foundation::FrameEpoch::ZERO.next())
            .surface_generation(flui_foundation::SurfaceGeneration::ZERO)
            .build()
    }

    #[test]
    fn builder_packages_all_fields() {
        let stamp = test_stamp();
        let scene = Scene::from_layer(Size::ZERO, crate::Layer::from(CanvasLayer::new()), 0);

        let frame = SceneSnapshot::builder()
            .stamp(stamp)
            .damage(DamageRegion::Full)
            .scene(scene)
            .build();

        assert_eq!(frame.stamp, stamp);
        assert_eq!(frame.damage, DamageRegion::Full);
    }

    #[test]
    fn builder_setters_are_order_independent() {
        let stamp = test_stamp();
        let scene_stamp_damage = SceneSnapshotBuilder::new()
            .scene(Scene::from_layer(
                Size::ZERO,
                crate::Layer::from(CanvasLayer::new()),
                0,
            ))
            .stamp(stamp)
            .damage(DamageRegion::Full)
            .build();

        assert_eq!(scene_stamp_damage.stamp, stamp);
        assert_eq!(scene_stamp_damage.damage, DamageRegion::Full);
    }
}
