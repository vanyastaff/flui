//! `RenderShaderMask` — applies a GPU shader as a mask over a single child.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderShaderMask`](https://api.flutter.dev/flutter/rendering/RenderShaderMask-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart:1128-1195`).
//!
//! # Rust-native shape
//!
//! Not built on [`super::clip::ClipGeometry`] or shared with
//! [`super::backdrop_filter::RenderBackdropFilter`] via a generic body —
//! the two proxy effects have categorically different config types
//! (an owner-lane shader target plus fallback shader vs. a plain
//! `ImageFilter` value), different default `blend_mode`s, and different
//! gating logic (`RenderBackdropFilter` has an independent `enabled` bypass;
//! `RenderShaderMask` has none). The
//! shared shape — single-child proxy, draws nothing of its own, wraps
//! `paint_child()` in one closure-scoped effect — is about four lines,
//! not worth a generic parameter (see the design research doc,
//! `docs/research/2026-07-01-render-backdrop-filter-shader-mask-plan.md`,
//! §3).
//!
//! # Divergence from the oracle
//!
//! Flutter's `RenderShaderMask` has **zero** `debugFillProperties`
//! entries (grepped the full class body: no override at all). This port
//! deliberately surfaces `blend_mode` anyway — a documented FLUI-side
//! improvement for catalog-wide consistency (every other proxy render
//! object in this crate — `RenderClip`, `RenderOpacity`,
//! `RenderPhysicalModel` — surfaces all of its fields), not a silent
//! divergence.

use std::fmt;

use flui_tree::Single;
use flui_types::{
    Offset, Pixels, Point, Rect, Size,
    painting::{BlendMode, Shader},
};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext, PaintCx},
    hit_testing::{ShaderMaskTarget, resolve_shader_mask_target},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A render object that masks its child with a GPU shader.
///
/// The shader is resolved once per paint. Static masks use the stored fallback
/// [`Shader`]. Bounds-dependent masks store a data-only [`ShaderMaskTarget`]
/// and resolve the executable factory through the active owner interaction
/// lane with the node's LOCAL bounds rect (oracle: `Offset.zero & size`).
/// Draws nothing of its own; see [`RenderBox::paint`] below.
///
/// # Engine note
///
/// The `flui-rendering` wiring this render object drives — a real
/// `Layer::ShaderMask` node in the composed `LayerTree`, correct fields,
/// correct local/global rect handling — is complete and
/// harness-verifiable. The `flui-engine` wgpu backend does **not** yet
/// apply the shader visually: `LayerRender<ShaderMaskLayer>` currently
/// pushes an inert clip-to-bounds save-layer and never reads `shader()`/
/// `blend_mode()` at all (a confirmed, pre-existing gap, not introduced
/// or closed by this type). Do not infer on-screen masking from this
/// type existing.
pub struct RenderShaderMask {
    shader: Shader,
    shader_target: Option<ShaderMaskTarget>,
    /// Blend mode used when compositing the masked result.
    /// Default `BlendMode::Modulate` — oracle `proxy_box.dart:1133`,
    /// **not** `SrcOver` (contrast [`super::backdrop_filter::RenderBackdropFilter`]'s
    /// default).
    blend_mode: BlendMode,
    /// Whether a child is attached (tracked for hit testing / paint /
    /// `always_needs_compositing`, mirroring `RenderClip`'s `has_child`).
    has_child: bool,
}

impl RenderShaderMask {
    /// Creates a shader mask with the given static fallback shader and the
    /// oracle's default blend mode (`BlendMode::Modulate`).
    ///
    /// Bounds-dependent shader factories are registered in the owner runtime
    /// and connected with [`with_shader_target`](Self::with_shader_target);
    /// this render object never stores executable shader callbacks.
    pub fn new(shader: Shader) -> Self {
        Self {
            shader,
            shader_target: None,
            blend_mode: BlendMode::Modulate,
            has_child: false,
        }
    }

    /// Builder: overrides the blend mode.
    #[must_use]
    pub fn with_blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Builder: resolves the shader through an owner-local shader target.
    #[must_use]
    pub fn with_shader_target(mut self, target: ShaderMaskTarget) -> Self {
        self.shader_target = Some(target);
        self
    }

    /// The static fallback shader.
    #[inline]
    pub const fn shader(&self) -> &Shader {
        &self.shader
    }

    /// Replaces the static fallback shader; returns `true` if the value changed.
    pub fn set_shader(&mut self, shader: Shader) -> bool {
        if self.shader == shader {
            return false;
        }
        self.shader = shader;
        true
    }

    /// Owner-lane shader target used for bounds-dependent masks.
    #[inline]
    pub const fn shader_target(&self) -> Option<ShaderMaskTarget> {
        self.shader_target
    }

    /// Replaces the owner-lane shader target; returns `true` if the value
    /// changed.
    pub fn set_shader_target(&mut self, target: Option<ShaderMaskTarget>) -> bool {
        if self.shader_target == target {
            return false;
        }
        self.shader_target = target;
        true
    }

    /// The current blend mode.
    #[inline]
    pub fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    /// Replaces the blend mode; returns `true` if the value changed.
    /// Paint-only — Flutter parity: `markNeedsPaint()`, never a relayout.
    pub fn set_blend_mode(&mut self, blend_mode: BlendMode) -> bool {
        if self.blend_mode == blend_mode {
            return false;
        }
        self.blend_mode = blend_mode;
        true
    }

    fn resolve_shader(&self, bounds: Rect<Pixels>) -> Shader {
        if let Some(target) = self.shader_target {
            match resolve_shader_mask_target(target, bounds) {
                Ok(shader) => return shader,
                Err(error) => {
                    tracing::warn!(
                        ?error,
                        "RenderShaderMask shader target resolution failed; using fallback shader"
                    );
                }
            }
        }
        self.shader.clone()
    }
}

impl Clone for RenderShaderMask {
    fn clone(&self) -> Self {
        Self {
            shader: self.shader.clone(),
            shader_target: self.shader_target,
            blend_mode: self.blend_mode,
            has_child: self.has_child,
        }
    }
}

impl fmt::Debug for RenderShaderMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderShaderMask")
            .field("shader", &self.shader)
            .field("has_shader_target", &self.shader_target.is_some())
            .field("blend_mode", &self.blend_mode)
            .field("has_child", &self.has_child)
            .finish_non_exhaustive()
    }
}

impl flui_foundation::Diagnosticable for RenderShaderMask {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        // Oracle has zero diagnostics for this class (see module doc) —
        // `blend_mode` is a deliberate FLUI-side addition, not a
        // transcription.
        builder.add_enum("blend_mode", self.blend_mode);
        builder.add_flag(
            "shader_target",
            self.shader_target.is_some(),
            "shader_target",
        );
    }
}

impl RenderBox for RenderShaderMask {
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

    // Oracle `:1174-1175` — `alwaysNeedsCompositing => child != null`,
    // data-dependent (not an unconditional `true`). This trait default
    // (`false`) is live, consumed infrastructure in this pipeline (see
    // design research plan §2.7), so the override matters.
    fn always_needs_compositing(&self) -> bool {
        self.has_child
    }

    // Closure is load-bearing: `PaintCx::paint_child` is ambiguous as a method path
    // (Single's zero-arg overload vs the indexed variant on other arities), so the
    // closure cannot be replaced by a method reference.
    #[allow(clippy::redundant_closure_for_method_calls)]
    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        // Oracle `:1191-1193` — no child means nothing at all is drawn
        // (not even an empty mask layer).
        if ctx.child_count() == 0 {
            return;
        }
        // LOCAL rect (oracle `Offset.zero & size`) — the shader callback
        // must see LOCAL coordinates, and so must the scope's recorded
        // `bounds`; the composer applies the origin shift for us (see
        // `PaintCx::with_shader_mask`'s doc and the design research
        // plan's trap §4.3).
        let bounds = Rect::from_origin_size(Point::ZERO, ctx.size());
        let shader = self.resolve_shader(bounds);
        ctx.with_shader_mask(shader, self.blend_mode, |ctx| ctx.paint_child());
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // RenderBox's trait default is LEAF-shaped (bounds check only,
        // no child recursion) — RenderShaderMask MUST override to
        // forward, mirroring oracle's `RenderProxyBoxMixin.hitTestChildren`
        // (`:127`). No shape gate: the mask is purely visual, oracle
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
    use flui_types::styling::Color;

    use super::*;

    fn solid_shader() -> Shader {
        Shader::solid(Color::WHITE)
    }

    #[test]
    fn default_blend_mode_is_modulate() {
        let node = RenderShaderMask::new(solid_shader());
        assert_eq!(node.blend_mode(), BlendMode::Modulate);
    }

    #[test]
    fn with_blend_mode_overrides_default() {
        let node = RenderShaderMask::new(solid_shader()).with_blend_mode(BlendMode::Multiply);
        assert_eq!(node.blend_mode(), BlendMode::Multiply);
    }

    #[test]
    fn set_blend_mode_returns_change_flag() {
        let mut node = RenderShaderMask::new(solid_shader());
        assert!(node.set_blend_mode(BlendMode::Screen));
        assert!(!node.set_blend_mode(BlendMode::Screen));
    }

    #[test]
    fn set_shader_returns_change_flag() {
        let mut node = RenderShaderMask::new(solid_shader());

        assert!(!node.set_shader(solid_shader()));
        assert!(node.set_shader(Shader::solid(Color::BLACK)));
        assert!(!node.set_shader(Shader::solid(Color::BLACK)));
    }

    #[test]
    fn debug_format_hides_closure_internals() {
        let node = RenderShaderMask::new(solid_shader());
        let dbg = format!("{node:?}");
        assert!(dbg.contains("RenderShaderMask"));
        assert!(dbg.contains("shader"));
        assert!(dbg.contains("has_shader_target"));
        assert!(dbg.contains("blend_mode"));
        assert!(dbg.contains("has_child"));
    }

    #[test]
    fn always_needs_compositing_tracks_has_child() {
        let mut node = RenderShaderMask::new(solid_shader());
        assert!(!node.always_needs_compositing(), "no child yet");
        node.has_child = true;
        assert!(node.always_needs_compositing(), "oracle: child != null");
        node.has_child = false;
        assert!(!node.always_needs_compositing());
    }

    #[test]
    fn debug_fill_properties_surfaces_blend_mode() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderShaderMask::new(solid_shader()).with_blend_mode(BlendMode::Screen);
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "blend_mode"));
    }
}
