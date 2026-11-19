//! # flui-layer — the compositor scene
//!
//! The paint walk in `flui-rendering` writes the repaint boundaries a frame
//! repaints into the retained scene ([`SceneOwner`]) through a
//! [`SceneWriter`], whose patch `flui-runtime` applies all or nothing; the
//! published [`SceneVersion`] is held by a [`Scene`], stamped into a
//! [`SceneSnapshot`], moved by value to the raster side, and lowered to GPU
//! work by `flui-engine`.
//!
//! ```text
//! RenderObject::paint  ──►  SceneWriter  ──►  SceneVersion  ──►  Scene  ──►  SceneSnapshot  ──►  flui-engine
//!   (flui-rendering)        (this crate)                                    (raster boundary)     (wgpu)
//! ```
//!
//! ## What is here
//!
//! - [`SceneOwner`] and [`SceneVersion`]: the retained scene (ADR-0304), a
//!   persistent version changed by [`SceneOp`] patches applied all or
//!   nothing.
//! - [`Layer`]: the closed vocabulary of compositor primitives, one variant
//!   per leaf/clip/transform/effect/link kind, each with its payload type
//!   (`PictureLayer`, `ClipRectLayer`, `OpacityLayer`, …).
//! - [`SceneBuilder`]: a scene authored by hand with `push`, `add` and
//!   `pop`. [`LayerTree`] / [`LayerNode`] hold the same layers as an
//!   append-only arena born with its root ([`LayerTree::new`]) and grown
//!   pre-order through [`LayerTree::push_child`], for a caller that keeps
//!   their ids; [`Scene::new`] converts it into a version
//!   ([`SceneVersion::from_tree`]).
//! - [`SceneVersion::follower_transform`]: a follower's position under its
//!   leader (ADR-0276), each chain layer contributing its matrix, a nested
//!   follower's resolved transform or [`Layer::local_translation`], the one
//!   place the set of translating variants is written down. The
//!   `testing` feature keeps the same rule over a hand-built tree
//!   (`resolve_follower_transform`) as its oracle.
//! - [`Scene`] and [`SceneSnapshot`] / [`DamageRegion`]: the version the
//!   raster side draws and the per-frame package that crosses the raster
//!   boundary.
//! - `testing::inspect` (feature `testing`): structural walkers for tests.
//!
//! ## Design
//!
//! A closed `enum` (the GPU backend's exhaustive `match` is the contract); a
//! persistent version that shares every boundary a patch did not change; and
//! a `Scene` that holds one version, not an engine handle. The
//! reference-parity accounting is in this crate's `ARCHITECTURE.md`.
//!
//! ```rust
//! use flui_layer::{Layer, LayerTree, OffsetLayer, PictureLayer, Scene};
//!
//! let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
//! tree.push_child(tree.root(), Layer::from(PictureLayer::default()));
//!
//! let scene = Scene::new(tree);
//! let root = scene.version().node(scene.version().root()).expect("a root");
//! assert_eq!(root.content().entries().len(), 2);
//! ```

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// Lint levels come from `[workspace.lints]`. Every public item is documented,
// and a consuming builder that returns `Self` is `#[must_use]`.
#![deny(missing_docs)]
#![warn(clippy::return_self_not_must_use)]
// A table keyed by ids the process mints hashes with FxHash, not SipHash.
#![warn(clippy::disallowed_types)]

mod compositor;
mod damage;
mod inspect;
mod layer;
mod link;
mod scene;
mod scene_snapshot;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod texture_registry;
mod tree;

pub use compositor::SceneBuilder;
#[cfg(feature = "testing")]
pub use damage::{Closure, Closures, LayerDiffer, MAX_CLOSURE_ROUNDS, close_over, read_closure};
pub use damage::{
    DamageMode, DamageRect, DamageRegion, FullReason, NonFinite, RasterBase, Region, ResyncReason,
};
pub use flui_foundation::LayerId;
pub use layer::{
    AnnotatedRegionLayer, AnnotationValue, BackdropFilterLayer, BackdropGroupKey, ClipPathLayer,
    ClipRRectLayer, ClipRectLayer, ClipSuperellipseLayer, ColorFilterLayer, Effect, FollowerLayer,
    ImageFilterLayer, Layer, LeaderClip, LeaderLayer, OffsetLayer, OpacityLayer, PictureLayer,
    SemanticLabel, ShaderMaskLayer, TransformLayer,
};
pub use link::LayerLink;
pub use scene::{
    Applied, BoundaryContent, Bounds, Chunk, ContentBuilder, ContentVersion, Entry, EntryAddress,
    EntryIndex, Lineage, Placement, Reclaimed, ReleasedContents, Scene, SceneError, SceneNode,
    SceneNodeId, SceneNodeMarker, SceneOp, SceneOwner, ScenePublication, SceneVersion,
    SceneVersionId, SceneWriter, SubtreeCull, SurfaceKey, TextureDamageScratch,
};
pub use scene_snapshot::SceneSnapshot;
pub use texture_registry::{
    TextureIdsExhausted, TextureRegistration, TextureRegistry, TextureUpdates,
};
#[cfg(feature = "testing")]
pub use tree::{BoundaryStamp, ContentToken, leader_origin, resolve_follower_transform};
pub use tree::{LayerNode, LayerTree};
