//! `SceneSnapshot` — the owned per-presentation per-frame raster package.
//!
//! Compositing produces one `SceneSnapshot` per window per frame; it is the one
//! seam a `UiRealm` hands to a raster owner (Flutter parity:
//! `RenderView.compositeFrame` → `FlutterView.render` → dispose).

use flui_foundation::{FrameEpoch, RealmId, SurfaceGeneration};

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
/// `#[non_exhaustive]`: fields are `pub` for direct read/match access, but
/// external construction goes through [`SceneSnapshot::new`] so a future field
/// (e.g. presentation timing) is additive, not breaking.
#[non_exhaustive]
#[derive(Debug)]
pub struct SceneSnapshot {
    /// Identifies which `UiRealm` incarnation produced this frame.
    pub realm_id: RealmId,
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
        realm_id: RealmId,
        epoch: FrameEpoch,
        surface_generation: SurfaceGeneration,
        damage: DamageRegion,
        scene: Scene,
    ) -> Self {
        Self {
            realm_id,
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
        let realm_id = RealmId::new(1);
        let epoch = FrameEpoch::ZERO.next();
        let surface_generation = SurfaceGeneration::ZERO;
        let scene = Scene::from_layer(Size::ZERO, crate::Layer::from(CanvasLayer::new()), 0);

        let frame = SceneSnapshot::new(
            realm_id,
            epoch,
            surface_generation,
            DamageRegion::Full,
            scene,
        );

        assert_eq!(frame.realm_id, realm_id);
        assert_eq!(frame.epoch, epoch);
        assert_eq!(frame.surface_generation, surface_generation);
        assert_eq!(frame.damage, DamageRegion::Full);
    }
}
