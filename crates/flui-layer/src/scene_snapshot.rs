//! `SceneSnapshot` — the owned per-presentation per-frame raster package.
//!
//! Compositing produces one `SceneSnapshot` per window per frame; it is the one
//! seam a `UiRealm` hands to a raster owner (Flutter parity:
//! `RenderView.compositeFrame` → `FlutterView.render` → dispose).

use flui_foundation::{FrameEpoch, PresentationAddress, SurfaceGeneration};

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
/// A frame's full identity is `(address, epoch)`, where `address` is the
/// full `(realm_id, presentation_id)` pair — never `presentation_id` alone,
/// since two different realm incarnations can mint an identical
/// `PresentationId` and only the full pair safely distinguishes them.
/// [`FrameEpoch`] is per-*realm* monotonic, so two presentations belonging to
/// the same realm's forest may composite in the same epoch — `address`
/// disambiguates them; it is not redundant with `epoch`.
/// `surface_generation` is a separate axis, scoped *per presentation*: it is
/// minted by that presentation's own raster seam (ADR-0037 §8), never by the
/// realm or by frame counting.
///
/// # `#[non_exhaustive]` does not make the constructor additive
///
/// Fields are `pub` for direct read/match access; `#[non_exhaustive]` makes
/// *matching* on this struct additive when a field is added later. It does
/// **not** make *construction* additive: [`SceneSnapshot::new`] is a
/// positional constructor, so every field this type gains breaks every
/// external call site. Before this type leaves `experimental`, the growing
/// positional constructor should become a builder or a fuller identity-group
/// value (`address`/`epoch`/`surface_generation` bundled together) so a
/// future field addition is actually additive.
#[non_exhaustive]
#[derive(Debug)]
pub struct SceneSnapshot {
    /// The full realm+presentation address that composited this frame.
    /// Disambiguates same-epoch frames from sibling presentations in a
    /// realm's forest, and from same-numbered presentations in an unrelated
    /// realm (see the type docs above).
    pub address: PresentationAddress,
    /// The runtime's per-frame counter at the time this frame was composited.
    pub epoch: FrameEpoch,
    /// The raster surface generation this frame was produced against.
    pub surface_generation: SurfaceGeneration,
    /// Which regions changed since the previous frame.
    pub damage: DamageRegion,
    /// The composited layer tree, ready to render.
    pub scene: Scene,
}

impl SceneSnapshot {
    /// Packages a composited [`Scene`] with the identity/versioning fields
    /// the raster boundary needs to accept, reject, or reconcile it.
    #[must_use]
    pub fn new(
        address: PresentationAddress,
        epoch: FrameEpoch,
        surface_generation: SurfaceGeneration,
        damage: DamageRegion,
        scene: Scene,
    ) -> Self {
        Self {
            address,
            epoch,
            surface_generation,
            damage,
            scene,
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_types::Size;

    use super::*;
    use crate::CanvasLayer;

    #[test]
    fn new_packages_all_fields() {
        let address = flui_foundation::PresentationAddress {
            realm_id: flui_foundation::RealmId::new(1),
            presentation_id: flui_foundation::PresentationId::new(1),
        };
        let epoch = FrameEpoch::ZERO.next();
        let surface_generation = SurfaceGeneration::ZERO;
        let scene = Scene::from_layer(Size::ZERO, crate::Layer::from(CanvasLayer::new()), 0);

        let frame = SceneSnapshot::new(
            address,
            epoch,
            surface_generation,
            DamageRegion::Full,
            scene,
        );

        assert_eq!(frame.address, address);
        assert_eq!(frame.epoch, epoch);
        assert_eq!(frame.surface_generation, surface_generation);
        assert_eq!(frame.damage, DamageRegion::Full);
    }
}
