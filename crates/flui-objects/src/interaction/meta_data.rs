//! `RenderMetaData` — single-child proxy that attaches an opaque
//! piece of user data to the hit-test entry it produces.
//!
//! Downstream gesture detectors fish the data back out of the
//! `HitTestEntry`.
//!
//! # Design
//!
//! * Metadata is stored as `Option<Arc<dyn Any + Send + Sync + 'static>>`
//!   — type-erased, but `Arc`-shared so the render
//!   object stays `Clone` without putting executable callbacks in render
//!   storage.
//! * Hit-test policy is the typed [`HitTestBehavior`] enum
//!   (`DeferToChild` / `Opaque` / `Translucent`) rather than two
//!   independent booleans — the helper methods on
//!   `HitTestBehavior::registers_self()` /
//!   `HitTestBehavior::blocks_below()` make the four branches read
//!   like a state table.
//! * Setters return `bool` change-flags so the pipeline can skip
//!   `mark_needs_paint` on no-op writes.

use std::{any::Any, fmt, sync::Arc};

use flui_foundation::Single;
use flui_foundation::geometry::Offset;

use flui_rendering::{
    context::BoxHitTestContext, hit_testing::HitTestBehavior, parent_data::BoxParentData,
    traits::RenderBox,
};

/// Type-erased metadata payload attached to a `RenderMetaData` node.
///
/// `Arc`-shared so cloning the render object is cheap and the
/// gesture system can extract a shared reference from a hit-test
/// entry without copying the payload.
pub type MetaDataPayload = Arc<dyn Any + Send + Sync + 'static>;

/// A render object that attaches opaque metadata to hit-test results.
///
/// Layout and paint are pure pass-throughs; the metadata is irrelevant
/// until a hit-test entry reaches the gesture system, which extracts
/// it for routing.
pub struct RenderMetaData {
    metadata: Option<MetaDataPayload>,
    behavior: HitTestBehavior,
    has_child: bool,
}

impl RenderMetaData {
    /// Creates a metadata render object with no payload and the
    /// default hit-test behavior (`DeferToChild`).
    pub const fn new() -> Self {
        Self {
            metadata: None,
            behavior: HitTestBehavior::DeferToChild,
            has_child: false,
        }
    }

    /// Builder: set the metadata payload.
    #[must_use]
    pub fn with_metadata<T>(mut self, value: T) -> Self
    where
        T: Any + Send + Sync + 'static,
    {
        self.metadata = Some(Arc::new(value));
        self
    }

    /// Builder: set the hit-test behavior.
    #[must_use]
    pub const fn with_behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.behavior = behavior;
        self
    }

    /// Returns the stored metadata payload, if any.
    #[inline]
    pub fn metadata(&self) -> Option<&MetaDataPayload> {
        self.metadata.as_ref()
    }

    /// Attempts to downcast the metadata payload to the requested
    /// concrete type. Returns `None` if no payload is stored or the
    /// payload's type doesn't match.
    pub fn metadata_as<T: Any + Send + Sync + 'static>(&self) -> Option<&T> {
        self.metadata.as_ref()?.downcast_ref::<T>()
    }

    /// Returns the current hit-test behavior.
    #[inline]
    pub fn behavior(&self) -> HitTestBehavior {
        self.behavior
    }

    /// Replaces the metadata payload. Returns `true` if the slot was
    /// changed (a None → Some / Some → None / Some → Some transition).
    /// Same-type, different-value swaps always return `true` because
    /// `dyn Any` cannot be compared structurally.
    pub fn set_metadata<T>(&mut self, value: Option<T>) -> bool
    where
        T: Any + Send + Sync + 'static,
    {
        let had = self.metadata.is_some();
        let has = value.is_some();
        self.metadata = value.map(|v| Arc::new(v) as MetaDataPayload);
        had != has || has
    }

    /// Replaces the payload with an already-shared one.
    ///
    /// The widget-facing setter: a `MetaData` widget holds its payload as an
    /// `Arc` already (it may be rebuilt many times and must not re-wrap on
    /// each), so this takes the `Arc` rather than boxing a fresh one like
    /// [`Self::set_metadata`].
    ///
    /// Returns whether the payload changed identity, the same contract the
    /// other setters give a caller gating work on a change.
    pub fn set_shared_metadata(&mut self, payload: Option<MetaDataPayload>) -> bool {
        let changed = match (&self.metadata, &payload) {
            (Some(current), Some(next)) => !Arc::ptr_eq(current, next),
            (None, None) => false,
            _ => true,
        };
        self.metadata = payload;
        changed
    }

    /// Clears the metadata. Returns `true` if a payload was present.
    pub fn clear_metadata(&mut self) -> bool {
        let had = self.metadata.is_some();
        self.metadata = None;
        had
    }

    /// Updates the hit-test behavior; returns true if the value changed.
    pub fn set_behavior(&mut self, behavior: HitTestBehavior) -> bool {
        if self.behavior == behavior {
            return false;
        }
        self.behavior = behavior;
        true
    }
}

impl Default for RenderMetaData {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for RenderMetaData {
    fn clone(&self) -> Self {
        Self {
            metadata: self.metadata.clone(),
            behavior: self.behavior,
            has_child: self.has_child,
        }
    }
}

impl fmt::Debug for RenderMetaData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderMetaData")
            .field("has_metadata", &self.metadata.is_some())
            .field("behavior", &self.behavior)
            .field("has_child", &self.has_child)
            .finish()
    }
}

impl flui_foundation::Diagnosticable for RenderMetaData {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("has_metadata", self.metadata.is_some(), "has metadata");
        builder.add_enum("behavior", self.behavior);
    }
}

impl RenderBox for RenderMetaData {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    // paint: default pass-through (splices the child in order).

    fn hit_test_behavior(&self) -> HitTestBehavior {
        self.behavior
    }

    /// Hand the payload to every hit that lands here.
    ///
    /// This is what makes the node findable: the searcher walks a hit path it
    /// got from somewhere else entirely and downcasts, never needing to know
    /// this type exists. `DragTarget` discovery rides on exactly this.
    fn metadata(&self) -> Option<MetaDataPayload> {
        self.metadata.clone()
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        // Children are tested FIRST, whatever the behavior --
        // `RenderProxyBoxWithHitTestBehavior.hitTest` is
        // `hitTestChildren(...) || hitTestSelf(...)`, and only `hitTestSelf`
        // consults the behavior. Skipping the descent for `Opaque` (as this
        // did) makes an opaque node a wall: it claims the hit and everything
        // beneath it becomes unreachable, so a tagged subtree inside another
        // tagged subtree is invisible. Nested drop targets are exactly that
        // shape.
        let child_hit = self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO);

        if child_hit {
            return true;
        }

        // The child missed. `Translucent` must still appear in the path
        // WITHOUT blocking what is behind it, and the bool this method
        // returns cannot say both: the bridge reads it as `add_self` AND
        // `blocks_below`, so returning `true` here would make a translucent
        // node opaque to its siblings. `register_self_hit_entry` is the half
        // that adds without blocking -- the same split `RenderListener`
        // makes.
        if self.behavior == HitTestBehavior::Translucent {
            ctx.register_self_hit_entry();
        }
        self.behavior.blocks_below()
    }
}

// ===========================================================================
// Tests
// ===========================================================================
