//! `SceneSnapshot`: the owned per-presentation per-frame raster package.
//!
//! Compositing produces one `SceneSnapshot` per window per frame; it is the one
//! seam a `UiRuntime` hands to a raster executor slot. The damage it carries is
//! [`crate::damage`]'s.

use flui_foundation::FrameStamp;

use crate::DamageRegion;
use crate::scene::{Scene, SceneVersion, SceneVersionId};
use crate::texture_registry::TextureUpdates;

/// The owned per-presentation per-frame raster package.
///
/// Produced by compositing and moved **by value** to the raster executor:
/// ownership transfer, not reference counting, is the seam. The executor
/// is the sole holder once a snapshot is sent and drops it when done. The
/// type is `Send + Sync` (plain data), pinned below this type; the by-value contract is
/// enforced by the executor's send API taking ownership, not by the auto traits.
///
/// # Frame identity
///
/// `stamp` carries the full identity/versioning group — which presentation,
/// which epoch, against which surface generation. See
/// [`FrameStamp`]'s own doc for why those are one value rather than fields
/// here.
///
/// Fields are `pub` for direct read/match access; `#[non_exhaustive]` makes
/// matching additive when a field is added, never construction.
#[non_exhaustive]
#[derive(Debug)]
pub struct SceneSnapshot {
    /// This frame's identity: which presentation, which epoch, against which
    /// surface generation.
    pub stamp: FrameStamp,
    /// Which regions changed since the image the raster side holds.
    pub damage: DamageRegion,
    /// The version the damage is relative to, `None` when it is full
    /// (ADR-0304 §7).
    pub base: Option<SceneVersionId>,
    /// What is drawn: the published version the raster side walks, which it
    /// reports once shown, or a scene built by hand.
    pub scene: Scene,
    /// The presentation's external-texture frames and retirements since its
    /// previous snapshot. The raster executor hands them to the slot's own
    /// textures before this frame renders, whether or not it is shown (the
    /// slot keeps the frames and uploads them when it draws), and a
    /// frame that supersedes another unshown one carries the other's too
    /// ([`TextureUpdates::merge`]).
    pub textures: TextureUpdates,
}

impl SceneSnapshot {
    /// This snapshot carrying `textures`, the updates its presentation's
    /// registry gave since the previous snapshot.
    #[must_use]
    pub fn textures(mut self, textures: TextureUpdates) -> Self {
        self.textures = textures;
        self
    }

    /// Packages a composited [`Scene`] with the identity the raster boundary
    /// needs to accept, reject, or reconcile it.
    #[must_use]
    pub fn new(stamp: FrameStamp, damage: DamageRegion, scene: Scene) -> Self {
        Self {
            stamp,
            damage,
            base: None,
            scene,
            textures: TextureUpdates::default(),
        }
    }

    /// Packages a published `version`, drawn with `damage` against `base`.
    #[must_use]
    pub fn of_version(
        stamp: FrameStamp,
        base: Option<SceneVersionId>,
        damage: DamageRegion,
        version: SceneVersion,
    ) -> Self {
        Self {
            stamp,
            damage,
            base,
            scene: Scene::of_version(version),
            textures: TextureUpdates::default(),
        }
    }
}

// A snapshot crosses to the raster thread: it stays plain data.
const _: fn() = || {
    fn crosses_threads<T: Send + Sync>() {}
    crosses_threads::<SceneSnapshot>();
};
