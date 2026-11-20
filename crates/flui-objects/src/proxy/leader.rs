//! `RenderLeaderLayer` — publishes an anchor position/size that
//! [`RenderFollowerLayer`](super::follower::RenderFollowerLayer) instances
//! can position themselves relative to.
//!
//! Backs `CompositedTransformTarget`.
//!
//! # Scope
//!
//! The object pushes a `Layer::Leader` carrying its `link` and `size`.
//! Rendering that layer belongs to the engine.
//!
//! # Rust-native shape
//!
//! Not a shared generic with
//! [`RenderFollowerLayer`](super::follower::RenderFollowerLayer) — Leader's
//! hit test only forwards to its child within its own bounds, while Follower
//! has a different, non-trivial hit-test path and configuration fields (offset,
//! anchors, clip) with no Leader analogue.
//!
//! # Difference from ShaderMask/BackdropFilter
//!
//! This object pushes the `LeaderLayer` and reports
//! `always_needs_compositing` **unconditionally**, regardless of child
//! presence — unlike `RenderShaderMask`/`RenderBackdropFilter`, which gate
//! both on having a child. A childless leader is a coordinate anchor,
//! not a visual effect, so it still needs its own compositor layer.

use flui_foundation::Single;
use flui_foundation::geometry::Offset;
use flui_layer::LayerLink;

use flui_rendering::{
    context::{BoxHitTestContext, PaintContext},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A render object that publishes its own position/size under a
/// [`LayerLink`] so linked [`RenderFollowerLayer`](super::follower::RenderFollowerLayer)
/// instances can anchor to it.
///
/// Draws nothing of its own — see [`RenderBox::paint`] below. Zero or one
/// child.
#[derive(Debug, Clone)]
pub struct RenderLeaderLayer {
    link: LayerLink,
    /// Whether a child is attached (tracked for hit testing / layout,
    /// mirroring `RenderClip`'s `has_child`). Does **not** gate paint or
    /// `always_needs_compositing` — see the module doc's divergence note.
    has_child: bool,
}

impl RenderLeaderLayer {
    /// Creates a leader layer publishing under `link`.
    pub fn new(link: LayerLink) -> Self {
        Self {
            link,
            has_child: false,
        }
    }

    /// The current layer link.
    #[inline]
    pub fn link(&self) -> LayerLink {
        self.link
    }

    /// Replaces the layer link and returns the exact pipeline impact.
    /// Paint-only, never a relayout (there is no per-link size field to
    /// migrate between links, so the swap is a plain overwrite).
    pub fn set_link(&mut self, link: LayerLink) -> flui_rendering::RenderUpdateImpact {
        if self.link == link {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.link = link;
        flui_rendering::RenderUpdateImpact::PAINT
    }
}

impl flui_foundation::inspect::Inspect for RenderLeaderLayer {
    fn kind(&self) -> &'static str {
        "RenderLeaderLayer"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        // Only `link` is surfaced.
        out.field("link", &self.link.id());
    }
}

impl RenderBox for RenderLeaderLayer {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    // UNCONDITIONAL, unlike ShaderMask/BackdropFilter's
    // `self.has_child`-gated version: a leader with no child still needs
    // its own compositor layer (it's a coordinate anchor, not a visual
    // effect).
    fn always_needs_compositing(&self) -> bool {
        true
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
        // Pushes the LeaderLayer regardless of child presence; the children
        // paint (nothing, when childless) inside the scope either way.
        let size = ctx.size();
        ctx.with_leader(self.link, size, PaintContext::paint_children_in_order);
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Plain forward, no shape gate beyond the child's own bounds.
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }
}

// =============================================================================
// Tests
// =============================================================================
