//! `SceneSnapshot` — the owned per-presentation per-frame raster package.
//!
//! Compositing produces one `SceneSnapshot` per window per frame; it is the one
//! seam a `UiRealm` hands to a raster owner.

use flui_foundation::FrameStamp;

use crate::scene::Scene;

/// Which regions of a [`SceneSnapshot`] changed since the previous frame.
///
/// Only [`DamageRegion::Full`] exists today: the producer half of damage
/// tracking is not written (ADR-0061), so every frame repaints in full. The
/// type is `#[non_exhaustive]` so a sub-rect variant is additive for matchers
/// when the producer lands.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageRegion {
    /// Repaint the entire frame.
    Full,
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
