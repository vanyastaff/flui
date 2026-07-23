//! `RenderMetaData` — single-child proxy that attaches an opaque
//! piece of user data to the hit-test entry it produces.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderMetaData`](https://api.flutter.dev/flutter/rendering/RenderMetaData-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`). Flutter
//! stores `metaData: Object?`; downstream gesture detectors fish it
//! back out of `HitTestEntry`.
//!
//! # Rust-native improvements
//!
//! * Metadata is stored as `Option<Arc<dyn Any + Send + Sync + 'static>>`
//!   — type-erased like Flutter, but `Arc`-shared so the render
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

use flui_tree::Single;
use flui_types::{Offset, Size};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext},
    hit_testing::HitTestBehavior,
    parent_data::BoxParentData,
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

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            child_size
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    // paint: default pass-through (splices the child in order).

    fn hit_test_behavior(&self) -> HitTestBehavior {
        self.behavior
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        // Test the child first when the behavior allows deferral.
        let child_hit = if matches!(
            self.behavior,
            HitTestBehavior::DeferToChild | HitTestBehavior::Translucent
        ) && self.has_child
        {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        };

        if child_hit {
            return true;
        }

        // TODO(core.1): once the gesture system threads a target id
        // through hit-test contexts, register `metadata` against the
        // hit-test entry via `ctx.add_self(id)`. For now self-hits
        // for `Opaque` / `Translucent` just return `true` so the
        // upstream router treats this node as the target.
        self.behavior.registers_self()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Tag(&'static str);

    #[test]
    fn defaults_have_no_metadata_and_defer_to_child() {
        let node = RenderMetaData::default();
        assert!(node.metadata().is_none());
        assert_eq!(node.behavior(), HitTestBehavior::DeferToChild);
    }

    #[test]
    fn with_metadata_round_trips_via_downcast() {
        let node = RenderMetaData::new().with_metadata(Tag("button-1"));
        let tag = node.metadata_as::<Tag>().expect("payload present");
        assert_eq!(tag, &Tag("button-1"));
    }

    #[test]
    fn metadata_as_returns_none_on_type_mismatch() {
        let node = RenderMetaData::new().with_metadata(42_u32);
        assert!(node.metadata_as::<String>().is_none());
        assert_eq!(node.metadata_as::<u32>(), Some(&42));
    }

    #[test]
    fn set_behavior_returns_change_flag() {
        let mut node = RenderMetaData::new();
        assert!(node.set_behavior(HitTestBehavior::Opaque));
        assert!(!node.set_behavior(HitTestBehavior::Opaque));
        assert!(node.set_behavior(HitTestBehavior::Translucent));
    }

    #[test]
    fn set_metadata_none_to_some_reports_change() {
        let mut node = RenderMetaData::new();
        assert!(node.set_metadata(Some(Tag("a"))));
        assert!(node.metadata().is_some());
    }

    #[test]
    fn set_metadata_some_to_none_reports_change() {
        let mut node = RenderMetaData::new().with_metadata(Tag("a"));
        assert!(node.set_metadata::<Tag>(None));
        assert!(node.metadata().is_none());
    }

    #[test]
    fn clear_metadata_returns_whether_payload_was_present() {
        let mut node = RenderMetaData::new().with_metadata(Tag("a"));
        assert!(node.clear_metadata());
        assert!(!node.clear_metadata());
    }

    #[test]
    fn clone_shares_metadata_arc() {
        let original = RenderMetaData::new().with_metadata(Tag("a"));
        let cloned = original.clone();
        assert!(cloned.metadata_as::<Tag>().is_some());
    }

    #[test]
    fn hit_test_behavior_exposes_field() {
        let node = RenderMetaData::new().with_behavior(HitTestBehavior::Opaque);
        // Trait-level method on RenderBox; verify it returns the
        // stored behavior so the pipeline reads the right value.
        assert_eq!(RenderBox::hit_test_behavior(&node), HitTestBehavior::Opaque);
    }

    #[test]
    fn debug_fill_properties_lists_state() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderMetaData::new().with_metadata(Tag("a"));
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        for required in ["has_metadata", "behavior"] {
            assert!(
                names.iter().any(|n| n == required),
                "missing diagnostic field: {required}"
            );
        }
    }
}
