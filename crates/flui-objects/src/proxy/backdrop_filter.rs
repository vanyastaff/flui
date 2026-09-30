//! `RenderBackdropFilter` — filters whatever was already painted behind a
//! single child before painting that child on top.
//!
//! # Scope
//!
//! The surface is `filter: ImageFilter`, `blend_mode: BlendMode`,
//! `enabled: bool`. A layout-aware filter blueprint (bounded-blur/tile-mode/
//! compose support) and a shared backdrop key for grouped backdrop sampling
//! have no FLUI-side backing today —
//! `flui_painting::paint::ImageFilter` has no bounded/tile-mode blur
//! variant, and `flui-layer`'s `BackdropFilterLayer` has no
//! `backdrop_key` field at all. Adding either speculatively would be dead
//! plumbing with zero consumers, so they are deferred — see the
//! [design research](https://github.com/vanyastaff/flui/blob/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/docs/research/2026-07-01-render-backdrop-filter-shader-mask-plan.md),
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
//! `filter` field here accepts any `ImageFilter` variant — this does
//! not claim full-variant GPU coverage, only that the render-object
//! and `LayerTree` wiring is variant-agnostic and correct.

use flui_foundation::Single;
use flui_foundation::geometry::Offset;
use flui_painting::paint::{BlendMode, ImageFilter};

use flui_rendering::{
    context::{BoxHitTestContext, PaintCx},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A render object that filters the backdrop behind its child.
///
/// Two **independent** gates control `paint` (see [`RenderBox::paint`] below):
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
    /// child painted on top. Default `BlendMode::SrcOver`, **not** `Modulate`
    /// (contrast [`super::shader_mask::RenderShaderMask`]'s default).
    blend_mode: BlendMode,
    /// Gate 1 — bypasses the filter entirely when `false`. Default `true`.
    enabled: bool,
    /// Whether a child is attached (tracked for hit testing / paint /
    /// `always_needs_compositing`, mirroring `RenderClip`'s `has_child`).
    has_child: bool,
}

impl RenderBackdropFilter {
    /// Creates a backdrop filter with the given `filter`, `enabled =
    /// true`, and the default blend mode (`BlendMode::SrcOver`).
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

    /// Replaces the image filter and returns the exact pipeline impact.
    /// Paint-only — never a relayout.
    pub fn set_filter(&mut self, filter: ImageFilter) -> flui_rendering::RenderUpdateImpact {
        if self.filter == filter {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.filter = filter;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// Replaces the blend mode and returns the exact pipeline impact.
    pub fn set_blend_mode(&mut self, blend_mode: BlendMode) -> flui_rendering::RenderUpdateImpact {
        if self.blend_mode == blend_mode {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.blend_mode = blend_mode;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// Replaces the enabled flag and returns the exact pipeline impact.
    pub fn set_enabled(&mut self, enabled: bool) -> flui_rendering::RenderUpdateImpact {
        if self.enabled == enabled {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.enabled = enabled;
        flui_rendering::RenderUpdateImpact::PAINT
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

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    // Compositing is needed iff there is a child: data-dependent (not an
    // unconditional `true`). This trait default
    // (`false`) is live, consumed infrastructure in this pipeline (see
    // design research plan §2.7), so the override matters.
    fn always_needs_compositing(&self) -> bool {
        self.has_child
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        // Gate 1 — bypasses the filter machinery
        // ENTIRELY, independent of gate 2 below. A child, if present,
        // still paints unfiltered; `paint_child()` is itself a no-op
        // when there is no child.
        if !self.enabled {
            ctx.paint_child();
            return;
        }
        // Gate 2 — only reachable when enabled.
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
        // forward. No shape gate: the filter is purely visual, so there is
        // no hit-test restriction beyond the child's own bounds.
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
