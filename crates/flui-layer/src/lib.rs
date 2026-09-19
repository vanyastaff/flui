//! # flui-layer — the compositor layer tree
//!
//! The fourth of FLUI's five trees (View → Element → Render → **Layer** →
//! Semantics). The paint walk in `flui-rendering` emits one [`LayerTree`] per
//! frame; it is frozen into a [`Scene`], stamped into a [`SceneSnapshot`],
//! moved by value to the raster side, and lowered to GPU work by
//! `flui-engine`.
//!
//! ```text
//! RenderObject::paint  ──►  LayerTree  ──►  Scene  ──►  SceneSnapshot  ──►  flui-engine
//!   (flui-rendering)       (this crate)                 (raster boundary)     (wgpu)
//! ```
//!
//! ## What is here
//!
//! - [`Layer`]: the closed vocabulary of compositor primitives, one variant
//!   per leaf/clip/transform/effect/link kind, each with its payload type
//!   (`PictureLayer`, `ClipRectLayer`, `OpacityLayer`, …).
//! - [`LayerTree`] / [`LayerNode`]: an append-only arena born with its root
//!   ([`LayerTree::new`]) and grown pre-order through
//!   [`LayerTree::push_child`]; the
//!   tree also indexes every [`LeaderLayer`] by its [`LayerLink`].
//! - [`resolve_follower_offset`]: the leader/follower chain math over
//!   [`Layer::local_translation`], the one place the set of translating
//!   variants is written down.
//! - [`SceneBuilder`]: push/pop construction for hand-authored scenes.
//! - [`Scene`] and [`SceneSnapshot`] / [`DamageRegion`]: the frozen tree and
//!   the per-frame package that crosses the raster boundary.
//! - `testing::inspect` (feature `testing`): structural walkers for tests.
//!
//! ## Design
//!
//! A closed `enum` (the GPU backend's exhaustive `match` is the contract); a
//! per-frame arena keyed by 1-based [`LayerId`] with no removal; a leader
//! index on the tree; and a `Scene` that is a frozen tree, not an engine
//! handle. The reference-parity accounting is in this crate's
//! `ARCHITECTURE.md`.
//!
//! ```rust
//! use flui_layer::{Layer, LayerTree, OffsetLayer, PictureLayer, Scene};
//!
//! let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
//! tree.push_child(tree.root(), Layer::from(PictureLayer::default()));
//!
//! let scene = Scene::new(tree);
//! assert_eq!(scene.tree().len(), 2);
//! ```

// Lint levels come from `[workspace.lints]`. Every public item is documented,
// and a consuming builder that returns `Self` is `#[must_use]`.
#![deny(missing_docs)]
#![warn(clippy::return_self_not_must_use)]

mod compositor;
mod layer;
mod link;
mod scene;
mod scene_snapshot;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod tree;

pub use compositor::SceneBuilder;
pub use flui_foundation::LayerId;
pub use layer::{
    AnnotatedRegionLayer, AnnotationValue, BackdropFilterLayer, CanvasLayer, ClipPathLayer,
    ClipRRectLayer, ClipRectLayer, ClipSuperellipseLayer, ColorFilterLayer, FollowerLayer,
    ImageFilterLayer, Layer, LeaderLayer, OffsetLayer, OpacityLayer, PerformanceOverlayLayer,
    PerformanceOverlayOption, PictureLayer, PlatformViewHitTestBehavior, PlatformViewId,
    PlatformViewLayer, SemanticLabel, ShaderMaskLayer, SystemUiOverlayStyle, TextureLayer,
    TransformLayer,
};
pub use link::{LayerLink, resolve_follower_offset};
pub use scene::Scene;
pub use scene_snapshot::{DamageRegion, SceneSnapshot};
pub use tree::{LayerNode, LayerTree};
