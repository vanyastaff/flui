//! `RenderFollowerLayer` — positions its subtree relative to whichever
//! [`RenderLeaderLayer`](super::leader::RenderLeaderLayer) currently
//! publishes under the same [`LayerLink`].
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderFollowerLayer`](https://api.flutter.dev/flutter/rendering/RenderFollowerLayer-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart:4550-4753`), backing
//! `CompositedTransformFollower`.
//!
//! # Scope — Tier 1 (structural) + Tier 2 (render-time position); hit-test
//! resolution remains deferred
//!
//! This type makes the `LayerTree` node structurally correct and
//! harness-verifiable — a real `Layer::Follower` with the right `link`/
//! `show_when_unlinked`/`offset`/anchor fields — exactly like the
//! ShaderMask/BackdropFilter precedent's own "structurally correct,
//! visually not yet" scoping (Tier 1). **The on-screen position is now
//! resolved at render time**: `paint` publishes this node's own laid-out
//! size onto the pushed `Layer::Follower` (mirroring how
//! `RenderLeaderLayer` publishes its size), and `flui-engine`'s
//! `render_layer_recursive` resolves the actual pixel offset against the
//! already-fully-built `LayerTree` and a per-frame `LinkRegistry` — see
//! `flui_layer::resolve_follower_offset` (Tier 2).
//!
//! **Hit-testing remains limited to the structural forward only** (see
//! [`RenderBox::hit_test`] below) — resolved-transform-aware hit-testing is
//! a genuine chief-architect ADR question, not a shortcut to invent in this
//! pass. See `docs/research/2026-07-01-render-leader-follower-layer-plan.md`
//! §4/§4.4/§8.
//!
//! # Rust-native shape
//!
//! Not a shared generic with [`RenderLeaderLayer`](super::leader::RenderLeaderLayer)
//! — see that module's doc for why the two land in the "two plain structs"
//! bucket (plan §5).
//!
//! # Divergence from the immediately-preceding ShaderMask/BackdropFilter
//! pair
//!
//! Oracle pushes the `FollowerLayer` and reports `alwaysNeedsCompositing`
//! **unconditionally**, regardless of child presence (`:4656`,
//! `:4708-4721`) — unlike `RenderShaderMask`/`RenderBackdropFilter`, which
//! gate both on `child != null`.

use flui_rendering::layer::LayerLink;
use flui_tree::Single;
use flui_types::{Offset, Size, painting::Alignment};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A render object that positions its subtree relative to a
/// [`RenderLeaderLayer`](super::leader::RenderLeaderLayer) linked via the
/// same [`LayerLink`].
///
/// Zero or one child (a `RenderProxyBox`, oracle `:4564`).
#[derive(Debug, Clone)]
pub struct RenderFollowerLayer {
    link: LayerLink,
    /// Whether to remain visible when no leader currently publishes
    /// under `link`. Default `true` (oracle `:4554`).
    show_when_unlinked: bool,
    /// Oracle's dual-purpose field (`:4555`): feeds BOTH the linked-anchor
    /// gap AND the unlinked-fallback standalone position (resolved at a
    /// later render-time pass, not by this render object — see the
    /// module doc's Tier-2 note).
    offset: Offset,
    /// Anchor point on the leader's rect. Default `TOP_LEFT` (oracle
    /// `:4556`).
    leader_anchor: Alignment,
    /// Anchor point on this follower's own rect. Default `TOP_LEFT`
    /// (oracle `:4557`).
    follower_anchor: Alignment,
    /// Whether a child is attached (tracked for hit testing / layout,
    /// mirroring `RenderClip`'s `has_child`). Does **not** gate paint or
    /// `always_needs_compositing` — see the module doc's divergence note.
    has_child: bool,
}

impl RenderFollowerLayer {
    /// Creates a follower layer targeting `link`, with oracle's defaults:
    /// `show_when_unlinked = true`, zero `offset`, `TOP_LEFT` anchors on
    /// both sides.
    pub fn new(link: LayerLink) -> Self {
        Self {
            link,
            show_when_unlinked: true,
            offset: Offset::ZERO,
            leader_anchor: Alignment::TOP_LEFT,
            follower_anchor: Alignment::TOP_LEFT,
            has_child: false,
        }
    }

    /// Builder: overrides `show_when_unlinked`.
    #[must_use]
    pub fn with_show_when_unlinked(mut self, show_when_unlinked: bool) -> Self {
        self.show_when_unlinked = show_when_unlinked;
        self
    }

    /// Builder: overrides `offset`.
    #[must_use]
    pub fn with_offset(mut self, offset: Offset) -> Self {
        self.offset = offset;
        self
    }

    /// Builder: overrides `leader_anchor`.
    #[must_use]
    pub fn with_leader_anchor(mut self, leader_anchor: Alignment) -> Self {
        self.leader_anchor = leader_anchor;
        self
    }

    /// Builder: overrides `follower_anchor`.
    #[must_use]
    pub fn with_follower_anchor(mut self, follower_anchor: Alignment) -> Self {
        self.follower_anchor = follower_anchor;
        self
    }

    /// The current layer link.
    #[inline]
    pub fn link(&self) -> LayerLink {
        self.link
    }

    /// Whether this follower remains visible when unlinked.
    #[inline]
    pub fn show_when_unlinked(&self) -> bool {
        self.show_when_unlinked
    }

    /// The current pixel offset (linked-anchor gap AND unlinked fallback
    /// position).
    #[inline]
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// The current leader-side anchor.
    #[inline]
    pub fn leader_anchor(&self) -> Alignment {
        self.leader_anchor
    }

    /// The current follower-side anchor.
    #[inline]
    pub fn follower_anchor(&self) -> Alignment {
        self.follower_anchor
    }

    /// Replaces the layer link; returns `true` if the value changed.
    pub fn set_link(&mut self, link: LayerLink) -> bool {
        if self.link == link {
            return false;
        }
        self.link = link;
        true
    }

    /// Replaces `show_when_unlinked`; returns `true` if the value changed.
    pub fn set_show_when_unlinked(&mut self, show_when_unlinked: bool) -> bool {
        if self.show_when_unlinked == show_when_unlinked {
            return false;
        }
        self.show_when_unlinked = show_when_unlinked;
        true
    }

    /// Replaces `offset`; returns `true` if the value changed.
    pub fn set_offset(&mut self, offset: Offset) -> bool {
        if self.offset == offset {
            return false;
        }
        self.offset = offset;
        true
    }

    /// Replaces `leader_anchor`; returns `true` if the value changed.
    pub fn set_leader_anchor(&mut self, leader_anchor: Alignment) -> bool {
        if self.leader_anchor == leader_anchor {
            return false;
        }
        self.leader_anchor = leader_anchor;
        true
    }

    /// Replaces `follower_anchor`; returns `true` if the value changed.
    pub fn set_follower_anchor(&mut self, follower_anchor: Alignment) -> bool {
        if self.follower_anchor == follower_anchor {
            return false;
        }
        self.follower_anchor = follower_anchor;
        true
    }
}

impl flui_foundation::Diagnosticable for RenderFollowerLayer {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        // Oracle `:4746-4752` surfaces `link`/`show_when_unlinked`/`offset`
        // plus a derived `current transform matrix`. That last property
        // has no resolved value to show until the Tier-2 render-time
        // resolution lands (module doc) — omitted here rather than
        // fabricated.
        builder.add_enum("link", self.link);
        builder.add_flag(
            "show_when_unlinked",
            self.show_when_unlinked,
            "show when unlinked",
        );
        builder.add("offset", self.offset);
        builder.add_enum("leader_anchor", self.leader_anchor);
        builder.add_enum("follower_anchor", self.follower_anchor);
    }
}

impl RenderBox for RenderFollowerLayer {
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

    // Oracle `:4656` — UNCONDITIONAL, same as `RenderLeaderLayer`, and
    // unlike ShaderMask/BackdropFilter's `self.has_child`-gated version.
    fn always_needs_compositing(&self) -> bool {
        true
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        // Oracle `:4708-4721` — pushes the FollowerLayer regardless of
        // child presence; the no-leader/hidden decision resolves at a
        // later render-time pass (module doc), not here. This node's own
        // paint-time size is published the same way `RenderLeaderLayer`
        // publishes its size — the Tier-2 render-time resolution needs it
        // for `FollowerLayer::calculate_offset`'s `follower_size` param.
        let size = ctx.size();
        ctx.with_follower(
            self.link,
            size,
            self.offset,
            self.show_when_unlinked,
            self.leader_anchor,
            self.follower_anchor,
            |ctx| ctx.paint_children_in_order(),
        );
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Oracle `:4672-4694`: Follower never adds itself as a hit
        // target, only forwards, gated on `link.leader == null &&
        // !show_when_unlinked`, wrapped in the CURRENT resolved
        // transform. This Tier-1 body implements ONLY the structural
        // forward half — has a child, forward the hit at its own
        // layout-relative offset; no child, miss — deliberately NOT the
        // resolved-transform-aware half (module doc; a self-cached
        // `Cell<Offset>` here would be silently wrong whenever this
        // node's own paint ran before its leader's in the same pass, so
        // it is not a shortcut taken in this pass — see the design
        // research plan §4.4/§7.5/§8's ADR deferral).
        if !self.has_child {
            return false;
        }
        ctx.hit_test_child_at_offset(0, Offset::ZERO)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_oracle() {
        let node = RenderFollowerLayer::new(LayerLink::new());
        assert!(node.show_when_unlinked());
        assert_eq!(node.offset(), Offset::ZERO);
        assert_eq!(node.leader_anchor(), Alignment::TOP_LEFT);
        assert_eq!(node.follower_anchor(), Alignment::TOP_LEFT);
    }

    #[test]
    fn builders_override_defaults() {
        let node = RenderFollowerLayer::new(LayerLink::new())
            .with_show_when_unlinked(false)
            .with_offset(Offset::new(
                flui_types::geometry::px(4.0),
                flui_types::geometry::px(6.0),
            ))
            .with_leader_anchor(Alignment::BOTTOM_CENTER)
            .with_follower_anchor(Alignment::TOP_CENTER);

        assert!(!node.show_when_unlinked());
        assert_eq!(
            node.offset(),
            Offset::new(flui_types::geometry::px(4.0), flui_types::geometry::px(6.0))
        );
        assert_eq!(node.leader_anchor(), Alignment::BOTTOM_CENTER);
        assert_eq!(node.follower_anchor(), Alignment::TOP_CENTER);
    }

    #[test]
    fn setters_return_change_flag() {
        let link_a = LayerLink::new();
        let link_b = LayerLink::new();
        let mut node = RenderFollowerLayer::new(link_a);

        assert!(node.set_link(link_b));
        assert!(!node.set_link(link_b));

        assert!(node.set_show_when_unlinked(false));
        assert!(!node.set_show_when_unlinked(false));

        let offset = Offset::new(flui_types::geometry::px(1.0), flui_types::geometry::px(2.0));
        assert!(node.set_offset(offset));
        assert!(!node.set_offset(offset));

        assert!(node.set_leader_anchor(Alignment::CENTER));
        assert!(!node.set_leader_anchor(Alignment::CENTER));

        assert!(node.set_follower_anchor(Alignment::CENTER));
        assert!(!node.set_follower_anchor(Alignment::CENTER));
    }

    #[test]
    fn always_needs_compositing_is_unconditional() {
        // Regression: contrasts with ShaderMask/BackdropFilter's
        // `self.has_child`-gated version (oracle `:4656`).
        let mut node = RenderFollowerLayer::new(LayerLink::new());
        assert!(node.always_needs_compositing(), "no child yet");
        node.has_child = true;
        assert!(node.always_needs_compositing());
    }

    #[test]
    fn debug_fill_properties_surfaces_all_fields_except_resolved_transform() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderFollowerLayer::new(LayerLink::new());
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        for required in [
            "link",
            "show_when_unlinked",
            "offset",
            "leader_anchor",
            "follower_anchor",
        ] {
            assert!(names.iter().any(|n| n == required), "missing {required}");
        }
        // No fabricated "current transform matrix" — see module doc.
        assert!(!names.iter().any(|n| n.contains("transform")));
    }
}
