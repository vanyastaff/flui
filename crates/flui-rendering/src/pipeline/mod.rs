//! Pipeline management for render tree.
//!
//! The pipeline coordinates the rendering phases:
//! 1. Layout - compute sizes and positions
//! 2. Compositing bits - determine layer requirements
//! 3. Paint - generate display lists
//! 4. Semantics - build accessibility tree

mod dirty;
pub(crate) mod handle;
pub mod hit_test_probe;
mod local_invalidation;
mod notifier;
mod owner;
pub mod phase;
pub(super) mod scheduler;
mod text_context;

pub(crate) use dirty::PaintKind;
pub use handle::{RenderInvalidationHandle, SendError as DirtySendError};
pub use local_invalidation::LocalRenderInvalidationHandle;
pub use notifier::DeferredVisualUpdate;
pub use owner::{
    AttachRenderSubtreesError, AttachRenderSubtreesFailure, DetachRenderSubtreesError,
    DetachedRenderSubtrees, FramePass, LayoutPremise, LeaderGeometry, LeaderWatch, ParentDataSlot,
    PipelineCell, PipelineCounters, PipelineOwner, PresentedPlacement,
    ReleaseDetachedRenderSubtreesError, ReleaseDetachedRenderSubtreesFailure, RenderChildrenEdit,
    WeakPipelineCell,
};
#[cfg(any(test, feature = "testing"))]
pub use owner::{PresentedKind, PresentedNode};
pub use phase::{Compositing, Layout, PaintPhase, PipelinePhase, Semantics};
pub(crate) use text_context::lend as lend_text;
pub use text_context::{TextContextHandle, TextContextMut, TextSource};
pub(crate) use text_context::{TextLender, TextMeasurers};
