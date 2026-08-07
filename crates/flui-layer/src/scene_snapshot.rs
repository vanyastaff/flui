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
/// # Construction is narrowed, not additive
///
/// Fields are `pub` for direct read/match access; `#[non_exhaustive]` makes
/// *matching* on this struct additive when a field is added later — it does
/// **not** make *construction* additive, and neither does bundling three of
/// this type's former fields into [`FrameStamp`].
///
/// This type used to carry a documented pre-graduation gate: a growing
/// positional `new()` with five arguments (`address`, `epoch`,
/// `surface_generation`, `damage`, `scene`) would break every call site on
/// its next field. That gate is **narrowed, not discharged**. The prior
/// revision of this doc claimed bundling the identity fields into
/// `FrameStamp` plus a typestate builder made the *next* field addition
/// additive; that claim was tested (a `resource_generation` field was
/// actually added to `FrameStamp`) and found false — see
/// [`FrameStamp`]'s own doc for why no construction shape makes a required
/// field additive, and why the correction there matters enough to repeat
/// here rather than silently drop. What genuinely improved: a positional
/// constructor argument list of five collapses to three
/// (`SceneSnapshot::new(stamp, damage, scene)`), and a future field on
/// `FrameStamp` breaks six call sites today — across `flui-foundation`,
/// this crate (the stamp helper below) and `flui-engine` — each named
/// directly by the compiler, rather than an unbounded set of external
/// callers. Smaller and compiler-guided, not additive.
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
    /// Packages a composited [`Scene`] with the identity/versioning
    /// [`FrameStamp`] and damage region the raster boundary needs to
    /// accept, reject, or reconcile it.
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
        FrameStamp::new(
            flui_foundation::PresentationAddress {
                realm_id: flui_foundation::RealmId::new(1),
                presentation_id: flui_foundation::PresentationId::new(1),
            },
            flui_foundation::FrameEpoch::ZERO.next(),
            flui_foundation::SurfaceGeneration::ZERO,
        )
    }

    #[test]
    fn new_packages_all_fields() {
        let stamp = test_stamp();
        let scene = Scene::from_layer(Size::ZERO, crate::Layer::from(CanvasLayer::new()), 0);

        let frame = SceneSnapshot::new(stamp, DamageRegion::Full, scene);

        assert_eq!(frame.stamp, stamp);
        assert_eq!(frame.damage, DamageRegion::Full);
    }
}
