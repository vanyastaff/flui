//! `RenderBackdropFilter` — filters whatever was already painted behind a
//! single child before painting that child on top.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderBackdropFilter`](https://api.flutter.dev/flutter/rendering/RenderBackdropFilter-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart:1201-1364`), scoped
//! to the classic, still-stable surface: `filter: ImageFilter`,
//! `blend_mode: BlendMode`, `enabled: bool`.
//!
//! # Scope decision — not transcribed verbatim
//!
//! Current `.flutter/` master has grown a `filterConfig: ImageFilterConfig`
//! (a layout-aware filter blueprint with bounded-blur/tile-mode/compose
//! support) plus a `backdropKey: BackdropKey?` for `BackdropGroup`-shared
//! backdrop sampling. Neither has any FLUI-side backing today —
//! `flui_types::painting::ImageFilter` has no bounded/tile-mode blur
//! variant, and `flui-layer`'s `BackdropFilterLayer` has no
//! `backdrop_key` field at all. Adding either speculatively would be dead
//! plumbing with zero consumers. This port targets the classic
//! `filter`/`blendMode`/`enabled` surface (the deprecated oracle `filter`
//! getter/setter this already IS, modulo naming) and documents the
//! newer surface as deferred — see the design research doc,
//! `docs/research/2026-07-01-render-backdrop-filter-shader-mask-plan.md`,
//! §1.3 and §6.
//!
//! # Rust-native shape
//!
//! Not shared with [`super::shader_mask::RenderShaderMask`] via a generic
//! body — see that module's doc for why the two proxy effects land in
//! the "two plain structs" bucket rather than a shared generic.
//!
//! # Engine note
//!
//! `flui-engine`'s Dual Kawase GPU blur path only covers
//! `ImageFilter::Blur`; every other `ImageFilter` variant (`Dilate`,
//! `Erode`, `Matrix`, `ColorAdjust`, `Compose`) currently degrades to
//! "children only, no backdrop effect" with a `tracing::warn!`. The
//! `filter` field here accepts any `ImageFilter` variant — this port
//! does not claim full-variant GPU coverage, only that the render-object
//! and `LayerTree` wiring is variant-agnostic and correct.

use flui_tree::Single;
use flui_types::{
    Offset, Size,
    painting::{BlendMode, ImageFilter},
};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A render object that filters the backdrop behind its child.
///
/// Two **independent** gates control `paint` (oracle
/// `proxy_box.dart:1328-1353`; see [`RenderBox::paint`] below):
/// `enabled = false` bypasses the filter machinery entirely and paints
/// the child unfiltered (or nothing, if there is no child); `enabled =
/// true` with no child paints nothing at all. Collapsing these into one
/// `enabled && has_child` condition is a behavior bug — see the module
/// doc's design research citation, trap §4.4.
#[derive(Debug, Clone)]
pub struct RenderBackdropFilter {
    /// The image filter applied to the backdrop.
    filter: ImageFilter,
    /// Blend mode used when compositing the filtered backdrop with the
    /// child painted on top. Default `BlendMode::SrcOver` — oracle
    /// `proxy_box.dart:1212`, **not** `Modulate` (contrast
    /// [`super::shader_mask::RenderShaderMask`]'s default).
    blend_mode: BlendMode,
    /// Gate 1 — bypasses the filter entirely when `false`. Default `true`
    /// (oracle `:1213`).
    enabled: bool,
    /// Whether a child is attached (tracked for hit testing / paint /
    /// `always_needs_compositing`, mirroring `RenderClip`'s `has_child`).
    has_child: bool,
}

impl RenderBackdropFilter {
    /// Creates a backdrop filter with the given `filter`, `enabled =
    /// true`, and the oracle's default blend mode (`BlendMode::SrcOver`).
    pub fn new(filter: ImageFilter) -> Self {
        Self {
            filter,
            blend_mode: BlendMode::SrcOver,
            enabled: true,
            has_child: false,
        }
    }

    /// Builder: overrides the blend mode.
    #[must_use]
    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Builder: overrides whether the filter is enabled.
    #[must_use]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The current image filter.
    #[inline]
    pub fn filter(&self) -> &ImageFilter {
        &self.filter
    }

    /// The current blend mode.
    #[inline]
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// Whether the filter is currently enabled.
    #[inline]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Replaces the image filter; returns `true` if the value changed.
    /// Paint-only — Flutter parity: `markNeedsPaint()`, never a relayout.
    pub fn set_filter(&mut self, filter: ImageFilter) -> bool {
        if self.filter == filter {
            return false;
        }
        self.filter = filter;
        true
    }

    /// Replaces the blend mode; returns `true` if the value changed.
    pub fn set_blend_mode(&mut self, blend_mode: BlendMode) -> bool {
        if self.blend_mode == blend_mode {
            return false;
        }
        self.blend_mode = blend_mode;
        true
    }

    /// Replaces the enabled flag; returns `true` if the value changed.
    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        if self.enabled == enabled {
            return false;
        }
        self.enabled = enabled;
        true
    }
}

impl flui_foundation::Diagnosticable for RenderBackdropFilter {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_enum("filter", &self.filter);
        builder.add_enum("blend_mode", self.blend_mode);
        builder.add_flag("enabled", self.enabled, "enabled");
    }
}

impl RenderBox for RenderBackdropFilter {
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

    // Oracle `:1325-1326` — `alwaysNeedsCompositing => child != null`,
    // data-dependent (not an unconditional `true`). This trait default
    // (`false`) is live, consumed infrastructure in this pipeline (see
    // design research plan §2.7), so the override matters.
    fn always_needs_compositing(&self) -> bool {
        self.has_child
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        // Gate 1 (oracle `:1330-1333`) — bypasses the filter machinery
        // ENTIRELY, independent of gate 2 below. A child, if present,
        // still paints unfiltered; `paint_child()` is itself a no-op
        // when there is no child.
        if !self.enabled {
            ctx.paint_child();
            return;
        }
        // Gate 2 (oracle `:1350-1352`) — only reachable when enabled.
        // No child means nothing at all is painted (not even a filtered,
        // childless backdrop).
        if ctx.child_count() == 0 {
            return;
        }
        // `with_backdrop_filter` computes the LOCAL bounds rect itself
        // (`Rect::from_origin_size(Point::ZERO, ctx.size())`, matching
        // every other `with_*` scope method) and the composer shifts it
        // to global space — no manual bounds arithmetic needed here.
        ctx.with_backdrop_filter(self.filter.clone(), self.blend_mode, |ctx| {
            ctx.paint_child();
        });
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // RenderBox's trait default is LEAF-shaped (bounds check only,
        // no child recursion) — RenderBackdropFilter MUST override to
        // forward, mirroring oracle's `RenderProxyBoxMixin.hitTestChildren`
        // (`:127`). No shape gate: the filter is purely visual, oracle
        // imposes no hit-test restriction beyond the child's own bounds.
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

    fn blur() -> ImageFilter {
        ImageFilter::blur(5.0)
    }

    #[test]
    fn default_blend_mode_is_src_over() {
        let node = RenderBackdropFilter::new(blur());
        assert_eq!(node.blend_mode(), BlendMode::SrcOver);
    }

    #[test]
    fn default_enabled_is_true() {
        let node = RenderBackdropFilter::new(blur());
        assert!(node.enabled());
    }

    #[test]
    fn with_blend_mode_overrides_default() {
        let node = RenderBackdropFilter::new(blur()).with_blend_mode(BlendMode::Multiply);
        assert_eq!(node.blend_mode(), BlendMode::Multiply);
    }

    #[test]
    fn with_enabled_overrides_default() {
        let node = RenderBackdropFilter::new(blur()).with_enabled(false);
        assert!(!node.enabled());
    }

    #[test]
    fn set_filter_returns_change_flag() {
        let mut node = RenderBackdropFilter::new(ImageFilter::blur(1.0));
        assert!(node.set_filter(ImageFilter::blur(2.0)));
        assert!(!node.set_filter(ImageFilter::blur(2.0)));
    }

    #[test]
    fn set_blend_mode_returns_change_flag() {
        let mut node = RenderBackdropFilter::new(blur());
        assert!(node.set_blend_mode(BlendMode::Screen));
        assert!(!node.set_blend_mode(BlendMode::Screen));
    }

    #[test]
    fn set_enabled_returns_change_flag() {
        let mut node = RenderBackdropFilter::new(blur());
        assert!(node.set_enabled(false));
        assert!(!node.set_enabled(false));
    }

    #[test]
    fn always_needs_compositing_tracks_has_child() {
        let mut node = RenderBackdropFilter::new(blur());
        assert!(!node.always_needs_compositing(), "no child yet");
        node.has_child = true;
        assert!(node.always_needs_compositing(), "oracle: child != null");
        node.has_child = false;
        assert!(!node.always_needs_compositing());
    }

    #[test]
    fn debug_fill_properties_lists_all_fields() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderBackdropFilter::new(blur());
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "filter"));
        assert!(names.iter().any(|n| n == "blend_mode"));
        assert!(names.iter().any(|n| n == "enabled"));
    }
}
