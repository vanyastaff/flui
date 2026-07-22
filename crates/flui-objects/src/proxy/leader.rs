//! `RenderLeaderLayer` — publishes an anchor position/size that
//! [`RenderFollowerLayer`](super::follower::RenderFollowerLayer) instances
//! can position themselves relative to.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderLeaderLayer`](https://api.flutter.dev/flutter/rendering/RenderLeaderLayer-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart:4475-4535`), backing
//! `CompositedTransformTarget`.
//!
//! # Scope — Tier 1 of a two-tier plan
//!
//! This type makes the `LayerTree` node structurally correct and
//! harness-verifiable — a real `Layer::Leader` with the right `link`/`size`/
//! `offset` fields, matching the ShaderMask/BackdropFilter precedent's own
//! "structurally correct, wiring not yet consumed" scoping. `Layer::Leader`'s
//! own GPU rendering (`LayerRender<LeaderLayer>`,
//! `crates/flui-engine/src/wgpu/layer_render.rs`) is already complete and
//! self-contained — it needs no further engine work once this node pushes
//! the layer. See `docs/research/2026-07-01-render-leader-follower-layer-plan.md`.
//!
//! # Rust-native shape
//!
//! Not a shared generic with
//! [`RenderFollowerLayer`](super::follower::RenderFollowerLayer) — Leader has
//! **zero** hit-test/`applyPaintTransform` override in oracle at all (relies
//! entirely on inherited `RenderProxyBoxMixin` defaults), while Follower has
//! a materially different, non-trivial custom `hitTest` override and three
//! extra fields with no Leader analogue (plan §5).
//!
//! # Divergence from the immediately-preceding ShaderMask/BackdropFilter pair
//!
//! Oracle pushes the `LeaderLayer` and reports `alwaysNeedsCompositing`
//! **unconditionally**, regardless of child presence (`:4498-4499`,
//! `:4513-4528`) — unlike `RenderShaderMask`/`RenderBackdropFilter`, which
//! gate both on `child != null`. A childless leader is a coordinate anchor,
//! not a visual effect, so it still needs its own compositor layer.

use flui_rendering::layer::LayerLink;
use flui_tree::Single;
use flui_types::{Offset, Size};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A render object that publishes its own position/size under a
/// [`LayerLink`] so linked [`RenderFollowerLayer`](super::follower::RenderFollowerLayer)
/// instances can anchor to it.
///
/// Draws nothing of its own — see [`RenderBox::paint`] below. Zero or one
/// child (a `RenderProxyBox`, oracle `:4477`).
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

    /// Replaces the layer link; returns `true` if the value changed.
    /// Paint-only — Flutter parity: `markNeedsPaint()`, never a relayout
    /// (oracle `:4486-4496`; FLUI has no embedded-mutable-`LayerLink`
    /// `leaderSize` field to migrate between links, so the swap is a
    /// plain overwrite).
    pub fn set_link(&mut self, link: LayerLink) -> bool {
        if self.link == link {
            return false;
        }
        self.link = link;
        true
    }
}

impl flui_foundation::Diagnosticable for RenderLeaderLayer {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        // Oracle surfaces `link` only (`:4531-4534`).
        builder.add_enum("link", self.link);
    }
}

impl RenderBox for RenderLeaderLayer {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            size
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    // Oracle `:4498-4499` — UNCONDITIONAL, unlike ShaderMask/BackdropFilter's
    // `self.has_child`-gated version: a leader with no child still needs
    // its own compositor layer (it's a coordinate anchor, not a visual
    // effect).
    fn always_needs_compositing(&self) -> bool {
        true
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        // Oracle `:4513-4528` — pushes the LeaderLayer regardless of
        // child presence; `super.paint` (paints nothing when childless)
        // runs inside the scope either way.
        let size = ctx.size();
        ctx.with_leader(self.link, size, PaintCx::paint_children_in_order);
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // No override at all in oracle — relies on `RenderProxyBoxMixin`
        // defaults (plain forward, no shape gate beyond the child's own
        // bounds, `:129-132`).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_link_returns_change_flag() {
        let link_a = LayerLink::new();
        let link_b = LayerLink::new();
        let mut node = RenderLeaderLayer::new(link_a);
        assert!(node.set_link(link_b));
        assert!(!node.set_link(link_b));
    }

    #[test]
    fn always_needs_compositing_is_unconditional() {
        // Regression: contrasts with ShaderMask/BackdropFilter's
        // `self.has_child`-gated version (oracle `:4498-4499`).
        let mut node = RenderLeaderLayer::new(LayerLink::new());
        assert!(node.always_needs_compositing(), "no child yet");
        node.has_child = true;
        assert!(node.always_needs_compositing());
    }

    #[test]
    fn debug_fill_properties_surfaces_link_only() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderLeaderLayer::new(LayerLink::new());
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert_eq!(names, vec!["link"]);
    }
}
