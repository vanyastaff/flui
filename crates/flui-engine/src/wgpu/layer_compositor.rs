//! Opacity/layer save-state machine extracted from `WgpuPainter`.
//!
//! `LayerCompositor` owns the three parallel stacks that track group-opacity
//! layer state between `save_layer` and `restore_layer`:
//!
//! | Field             | Purpose                                         |
//! |-------------------|-------------------------------------------------|
//! | `opacity_stack`   | Vestigial legacy stack (underflow fallback)     |
//! | `current_opacity` | Accumulated opacity for the current subtree    |
//! | `layer_stack`     | Save-state for each open opacity layer          |
//!
//! All GPU emission stays in `GpuReplay::submit` / `GpuReplay::flush_opacity_layer`;
//! draw-record mutation (`draw_order`, `current_segment`) stays on `WgpuPainter`.
//! The compositor only performs the book-keeping half: snapshot on push,
//! restore + branch decision on pop.
//!
//! # Balance assertion
//!
//! `WgpuPainter::reset_frame_state` calls `debug_assert_balanced()` BEFORE
//! `reset()`, mirroring `GpuStateStack`'s frame-boundary pattern exactly.
//! No `Drop` impl: a `Drop`-based assertion would false-positive-panic during
//! unwind and abort the process.

use flui_types::Rect;
use flui_types::geometry::Pixels;
use flui_types::painting::BlendMode;

use super::command_ir::{DrawItem, DrawSegment, ImageFilterSpec, LayerFilterChain, SavedLayer};

/// Outcome returned by [`LayerCompositor::pop_layer`].
///
/// The painter matches on this value to decide which draw-record mutation and
/// GPU-emission path to follow.  All payloads are owned values so the painter
/// can mutate its own fields after the compositor call returns — no aliasing.
// `DrawSegment` / `Vec<DrawItem>` contain `wgpu::TextureView` which is not `Debug`.
#[allow(missing_debug_implementations)]
pub(super) enum RestoreOutcome {
    /// The layer had content AND needs a premultiplied offscreen composite
    /// (opacity ≠ 1.0, non-white tint, or advanced blend mode).  The painter
    /// should finalize the parent segment, then queue a `DrawItem::OpacityLayer`.
    Composite {
        /// Offscreen draw items accumulated inside the layer.
        offscreen_items: Vec<DrawItem>,
        /// Final offscreen segment accumulated after the last draw item.
        offscreen_final_segment: DrawSegment,
        /// Effective layer opacity in `[0, 1]`.
        layer_opacity: f32,
        /// Per-channel RGB tint (`[1, 1, 1]` = no-op chroma).
        tint_rgb: [f32; 3],
        /// Compositing bounds (provided or viewport-derived, pre-resolved by the painter).
        composite_bounds: Rect<Pixels>,
        /// Blend mode to apply when compositing this layer onto its parent.
        ///
        /// `SrcOver` for plain opacity layers; an advanced mode (e.g. Multiply)
        /// for layers opened with an explicit blend mode via `save_layer`.
        layer_blend: BlendMode,
        /// Color-filter chain to apply before compositing.
        ///
        /// Forwarded from [`SavedLayer::filters`] so the painter can pass it into
        /// [`super::command_ir::PendingOpacityLayer`] without coupling the flush path.
        layer_filter: LayerFilterChain,
        /// Image filter (bounds-GROWING) to apply via `DrawItem::Filter` instead of
        /// the normal `DrawItem::OpacityLayer` path.
        ///
        /// When `Some`, the painter emits `DrawItem::Filter` with the offscreen content
        /// and a `Morph` pass derived from this spec, rather than `DrawItem::OpacityLayer`.
        /// Forwarded from [`SavedLayer::image_filter`].
        image_filter: Option<ImageFilterSpec>,
        /// Parent segment saved before `save_layer` — splice back into `current_segment`.
        saved_segment: DrawSegment,
        /// Parent draw order saved before `save_layer` — splice back into `draw_order`.
        saved_draw_order: Vec<DrawItem>,
    },
    /// The layer had content BUT opacity ≈ 1.0 AND tint is white — no offscreen
    /// composite needed; the painter should re-integrate the offscreen items.
    Reintegrate {
        /// Offscreen draw items to merge back into the parent draw order.
        offscreen_items: Vec<DrawItem>,
        /// Final offscreen segment to merge back into the parent draw order.
        offscreen_final_segment: DrawSegment,
        /// Parent segment saved before `save_layer` — splice back into `current_segment`.
        saved_segment: DrawSegment,
        /// Parent draw order saved before `save_layer` — splice back into `draw_order`.
        saved_draw_order: Vec<DrawItem>,
    },
    /// The layer was empty (both `offscreen_items` and `offscreen_final_segment`
    /// had no content).  The painter restores draw-record state but emits nothing.
    Empty {
        /// Parent segment saved before `save_layer` — splice back into `current_segment`.
        saved_segment: DrawSegment,
        /// Parent draw order saved before `save_layer` — splice back into `draw_order`.
        saved_draw_order: Vec<DrawItem>,
    },
    /// `layer_stack` was empty when `pop_layer` was called — compositor fell back
    /// to the legacy `opacity_stack` underflow path (or did nothing).
    ///
    /// The draw records passed to `pop_layer` are returned unchanged so the
    /// painter can restore `current_segment`/`draw_order` to their pre-call
    /// state.  This makes the underflow path byte-identical to the original
    /// painter code, which only performed the `mem::take`s inside the
    /// `if let Some(saved)` block.
    Underflow {
        /// `offscreen_final_segment` parameter handed back unchanged.
        current_segment: DrawSegment,
        /// `offscreen_items` parameter handed back unchanged.
        draw_order: Vec<DrawItem>,
    },
}

/// Opacity/layer save-state machine.
///
/// Owns `opacity_stack`, `current_opacity`, and `layer_stack`.  Performs pure
/// book-keeping: snapshot into `SavedLayer` on push, restore + branch decision
/// on pop.  Never touches GPU, `draw_order`, or `current_segment` directly.
// `Vec<SavedLayer>` contains `DrawSegment` → `wgpu::TextureView` which is not `Debug`.
#[allow(missing_debug_implementations)]
pub(super) struct LayerCompositor {
    /// Vestigial legacy opacity stack.
    ///
    /// Never pushed in the current save-layer code path — only
    /// `mem::take`-saved and restored around each layer.  The underflow branch
    /// of `pop_layer` pops from it as a last-resort fallback.  Retained as-is
    /// for byte-identical behavior with pre-extraction code.
    opacity_stack: Vec<f32>,

    /// Current accumulated opacity for the active subtree (`1.0` = fully opaque).
    ///
    /// Draw methods read this via `current_opacity()` to modulate per-primitive
    /// alpha within a layer.  Reset to `1.0` when a new layer starts (children
    /// draw at full opacity; group opacity is applied at composite time).
    current_opacity: f32,

    /// Stack of saved render state for each open opacity layer.
    ///
    /// Each entry captures draw state at `save_layer` time so the subtree can
    /// be rendered to an offscreen texture and composited with group opacity.
    layer_stack: Vec<SavedLayer>,
}

impl LayerCompositor {
    /// Create a compositor at identity state: `current_opacity = 1.0`, empty stacks.
    pub(super) fn new() -> Self {
        Self {
            opacity_stack: Vec::new(),
            current_opacity: 1.0,
            layer_stack: Vec::new(),
        }
    }

    /// Reset to identity state.
    ///
    /// Called by `WgpuPainter::reset_frame_state` AFTER `debug_assert_balanced`.
    /// Clears both stacks and restores `current_opacity` to `1.0`.
    pub(super) fn reset(&mut self) {
        self.layer_stack.clear();
        self.opacity_stack.clear();
        self.current_opacity = 1.0;
    }

    /// Assert that the layer stack is empty — every `save_layer` was balanced
    /// by a matching `restore_layer`.
    ///
    /// Called at the frame boundary BEFORE `reset()`.  Implemented as a
    /// `debug_assert!` (compiles away in release).  No `Drop` impl — a
    /// `Drop`-based assertion would false-positive-panic during unwind and abort.
    pub(super) fn debug_assert_balanced(&self) {
        debug_assert!(
            self.layer_stack.is_empty(),
            "LayerCompositor: unbalanced save_layer/restore_layer — {} layer(s) still open at \
             frame boundary",
            self.layer_stack.len()
        );
    }

    /// Returns the current accumulated opacity by **copy**.
    ///
    /// Always returned by value — never by reference — so draw-method callers
    /// can read it while simultaneously holding a mutable borrow on
    /// `current_segment`.
    #[inline]
    pub(super) fn current_opacity(&self) -> f32 {
        self.current_opacity
    }

    /// Compute the effective layer opacity: `current_opacity × paint_alpha`.
    ///
    /// Shared helper used by both `save_layer` (white-chroma path) and
    /// `save_layer_with_tint` (chroma path) to avoid duplicating the multiply.
    #[inline]
    pub(super) fn effective_layer_opacity(&self, paint_alpha: f32) -> f32 {
        self.current_opacity * paint_alpha
    }

    /// Snapshot current opacity state and push a new opacity layer.
    ///
    /// The painter takes `saved_draw_order` and `saved_segment` from its own
    /// fields via `mem::take`/`mem::replace` before calling this, then passes
    /// the owned values in so the compositor can store them in the `SavedLayer`.
    ///
    /// `layer_blend` is `SrcOver` for plain opacity layers and an advanced mode
    /// (e.g. Multiply) for `saveLayer` calls with an explicit blend mode.
    ///
    /// After this call `current_opacity` is `1.0`; children inside the layer
    /// draw at full opacity and group opacity is applied during compositing.
    #[allow(
        clippy::too_many_arguments,
        reason = "all arguments are load-bearing: draw-order snapshot, opacity, tint, blend, \
                  bounds, and filter must all be stored on the SavedLayer; the alternative \
                  (a builder struct) adds complexity without a semantic boundary"
    )]
    pub(super) fn push_layer(
        &mut self,
        saved_draw_order: Vec<DrawItem>,
        saved_segment: DrawSegment,
        layer_opacity: f32,
        layer_tint_rgb: [f32; 3],
        layer_blend: BlendMode,
        bounds: Option<[f32; 4]>,
        filters: LayerFilterChain,
    ) {
        let saved = SavedLayer {
            saved_draw_order,
            saved_segment,
            saved_opacity_stack: std::mem::take(&mut self.opacity_stack),
            saved_opacity: self.current_opacity,
            layer_opacity,
            layer_tint_rgb,
            layer_blend,
            bounds,
            filters,
            image_filter: None, // set by save_layer_with_image_filter after push
        };
        self.layer_stack.push(saved);

        // Children inside the layer draw at full opacity; group opacity is
        // applied at composite time by GpuReplay::flush_opacity_layer.
        self.current_opacity = 1.0;
    }

    /// Return the `bounds` field of the top-of-stack `SavedLayer` without popping.
    ///
    /// Used by the painter to resolve `composite_bounds` (applying the viewport
    /// fallback) before calling `pop_layer`, since the painter owns `size`.
    /// Returns `None` when the stack is empty (underflow is handled by `pop_layer`).
    pub(super) fn peek_layer_bounds(&self) -> Option<[f32; 4]> {
        self.layer_stack.last().and_then(|saved| saved.bounds)
    }

    /// Mark the top-of-stack `SavedLayer` with an image filter spec.
    ///
    /// Called by `WgpuPainter::save_layer_with_image_filter` immediately after
    /// `save_layer_impl` pushes the new entry.  Panics in debug mode if the stack
    /// is empty (which would indicate a caller ordering bug — `push_layer` must
    /// precede this call).
    pub(super) fn set_top_image_filter(&mut self, spec: ImageFilterSpec) {
        debug_assert!(
            !self.layer_stack.is_empty(),
            "LayerCompositor::set_top_image_filter called on empty layer_stack"
        );
        if let Some(top) = self.layer_stack.last_mut() {
            top.image_filter = Some(spec);
        }
    }

    /// Pop the top layer, restore parent opacity, and decide the compositing branch.
    ///
    /// Returns a [`RestoreOutcome`] describing which path the painter should
    /// follow.  The painter then performs all draw-record mutations
    /// (`draw_order`, `current_segment`) and GPU emission calls per the matched
    /// variant.
    ///
    /// `offscreen_final_segment` and `offscreen_items` are the draw records the
    /// painter captured from `current_segment`/`draw_order` via `mem::replace`
    /// for the offscreen subtree.
    ///
    /// `composite_bounds` is the fully-resolved compositing rect — either from
    /// `SavedLayer::bounds` or the viewport fallback — computed by the painter
    /// before calling this.
    pub(super) fn pop_layer(
        &mut self,
        offscreen_final_segment: DrawSegment,
        offscreen_items: Vec<DrawItem>,
        composite_bounds: Rect<Pixels>,
    ) -> RestoreOutcome {
        let Some(saved) = self.layer_stack.pop() else {
            tracing::warn!("LayerCompositor::pop_layer: layer_stack underflow");

            // Legacy opacity_stack fallback for callers that bypassed push_layer.
            if let Some(prev_opacity) = self.opacity_stack.pop() {
                self.current_opacity = prev_opacity;
            }
            // Hand the captured records straight back so the painter can restore
            // current_segment/draw_order to their pre-call state unchanged.
            return RestoreOutcome::Underflow {
                current_segment: offscreen_final_segment,
                draw_order: offscreen_items,
            };
        };

        // Restore parent opacity state.
        self.opacity_stack = saved.saved_opacity_stack;
        self.current_opacity = saved.saved_opacity;

        let has_offscreen_content =
            !offscreen_final_segment.is_empty() || !offscreen_items.is_empty();

        if !has_offscreen_content {
            return RestoreOutcome::Empty {
                saved_segment: saved.saved_segment,
                saved_draw_order: saved.saved_draw_order,
            };
        }

        // A non-white tint carries ColorFilter chroma that the fast reintegrate
        // path cannot apply (it splices children into the parent draw order
        // unchanged).  A hue-only filter at effective_alpha == 1.0 MUST still go
        // through the offscreen composite path so the premultiplied tint shifts
        // chroma — otherwise the hue shift is silently dropped.  White tint
        // (plain opacity) at ~1.0 AND SrcOver blend keeps the cheap reintegrate path.
        //
        // An advanced blend mode (Multiply, Screen, …) ALWAYS forces the composite
        // path even at opacity=1.0 and white tint, because the reintegrate path
        // splices children unchanged into the parent draw order — silently dropping
        // the blend.
        //
        // A LayerFilter ALWAYS forces the composite path: the filter must run on
        // the rendered offscreen before pixels reach the parent, which the
        // reintegrate path cannot do (it inserts content directly into the parent
        // draw order without an offscreen round-trip).
        let has_chroma = (saved.layer_tint_rgb[0] - 1.0).abs() > f32::EPSILON
            || (saved.layer_tint_rgb[1] - 1.0).abs() > f32::EPSILON
            || (saved.layer_tint_rgb[2] - 1.0).abs() > f32::EPSILON;
        // G3 guardrail: an image_filter ALWAYS forces the composite path so the
        // Morph (or future Blur) pass can run on the rendered offscreen before
        // pixels reach the parent.  The Reintegrate fast-path splices children
        // directly into the parent draw order and CANNOT apply an image filter.
        let needs_composite = (1.0 - saved.layer_opacity).abs() > f32::EPSILON
            || has_chroma
            || saved.layer_blend.is_advanced()
            || !saved.filters.is_empty()
            || saved.image_filter.is_some();

        if needs_composite {
            RestoreOutcome::Composite {
                offscreen_items,
                offscreen_final_segment,
                layer_opacity: saved.layer_opacity,
                tint_rgb: saved.layer_tint_rgb,
                composite_bounds,
                layer_blend: saved.layer_blend,
                layer_filter: saved.filters,
                image_filter: saved.image_filter,
                saved_segment: saved.saved_segment,
                saved_draw_order: saved.saved_draw_order,
            }
        } else {
            RestoreOutcome::Reintegrate {
                offscreen_items,
                offscreen_final_segment,
                saved_segment: saved.saved_segment,
                saved_draw_order: saved.saved_draw_order,
            }
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;
    use flui_types::{Color, Rect};

    use super::super::instancing::RectInstance;
    use super::*;

    fn rect_bounds_100() -> Rect<Pixels> {
        Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0))
    }

    fn segment_with_one_rect() -> DrawSegment {
        let mut seg = DrawSegment::new();
        let instance = RectInstance::rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(10.0), px(10.0)),
            Color::rgba(255, 0, 0, 255),
        );
        let _ = seg.rect_batch.add(instance);
        seg
    }

    /// A `debug_assert_balanced` call on an unbalanced compositor must panic.
    ///
    /// Mirrors the equivalent test for `GpuStateStack` (T7 pattern).
    #[test]
    #[should_panic(expected = "unbalanced save_layer/restore_layer")]
    fn debug_assert_balanced_panics_when_layer_stack_is_not_empty() {
        let mut compositor = LayerCompositor::new();
        // Push a layer without popping — simulates a mismatched save_layer/restore_layer.
        compositor.push_layer(
            Vec::new(),
            DrawSegment::new(),
            1.0,
            [1.0, 1.0, 1.0],
            BlendMode::SrcOver,
            None,
            LayerFilterChain::new(), // no filter
        );
        compositor.debug_assert_balanced();
    }

    #[test]
    fn new_compositor_is_balanced_and_at_full_opacity() {
        let compositor = LayerCompositor::new();
        compositor.debug_assert_balanced(); // must not panic
        assert!((compositor.current_opacity() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn reset_restores_identity_after_open_layer() {
        let mut compositor = LayerCompositor::new();
        compositor.push_layer(
            Vec::new(),
            DrawSegment::new(),
            0.5,
            [1.0, 1.0, 1.0],
            BlendMode::SrcOver,
            None,
            LayerFilterChain::new(), // no filter
        );
        // Inside the layer, children draw at full opacity.
        assert!((compositor.current_opacity() - 1.0).abs() < f32::EPSILON);
        compositor.reset();
        compositor.debug_assert_balanced(); // must not panic after reset
        assert!((compositor.current_opacity() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn push_layer_sets_current_opacity_to_full() {
        let mut compositor = LayerCompositor::new();
        compositor.current_opacity = 0.5;
        compositor.push_layer(
            Vec::new(),
            DrawSegment::new(),
            0.5,
            [1.0, 1.0, 1.0],
            BlendMode::SrcOver,
            None,
            LayerFilterChain::new(), // no filter
        );
        // Children inside the layer draw at full opacity.
        assert!((compositor.current_opacity() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn pop_layer_restores_parent_opacity() {
        let mut compositor = LayerCompositor::new();
        compositor.current_opacity = 0.8;
        compositor.push_layer(
            Vec::new(),
            DrawSegment::new(),
            0.8,
            [1.0, 1.0, 1.0],
            BlendMode::SrcOver,
            None,
            LayerFilterChain::new(), // no filter
        );
        let _ = compositor.pop_layer(DrawSegment::new(), Vec::new(), rect_bounds_100());
        assert!((compositor.current_opacity() - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn pop_layer_empty_when_no_offscreen_content() {
        let mut compositor = LayerCompositor::new();
        compositor.push_layer(
            Vec::new(),
            DrawSegment::new(),
            0.5,
            [1.0, 1.0, 1.0],
            BlendMode::SrcOver,
            None,
            LayerFilterChain::new(), // no filter
        );
        let outcome = compositor.pop_layer(DrawSegment::new(), Vec::new(), rect_bounds_100());
        assert!(matches!(outcome, RestoreOutcome::Empty { .. }));
    }

    #[test]
    fn pop_layer_composite_when_opacity_not_one() {
        let mut compositor = LayerCompositor::new();
        compositor.push_layer(
            Vec::new(),
            DrawSegment::new(),
            0.5, // not 1.0 → must composite
            [1.0, 1.0, 1.0],
            BlendMode::SrcOver,
            None,
            LayerFilterChain::new(), // no filter
        );
        let outcome = compositor.pop_layer(segment_with_one_rect(), Vec::new(), rect_bounds_100());
        assert!(matches!(outcome, RestoreOutcome::Composite { .. }));
    }

    #[test]
    fn pop_layer_composite_when_chroma_tint_even_at_full_opacity() {
        let mut compositor = LayerCompositor::new();
        compositor.push_layer(
            Vec::new(),
            DrawSegment::new(),
            1.0,             // opacity ~1.0
            [0.0, 0.0, 1.0], // blue tint → has_chroma
            BlendMode::SrcOver,
            None,
            LayerFilterChain::new(), // no filter
        );
        let outcome = compositor.pop_layer(segment_with_one_rect(), Vec::new(), rect_bounds_100());
        assert!(
            matches!(outcome, RestoreOutcome::Composite { .. }),
            "chroma tint at full opacity must still go through the composite path"
        );
    }

    #[test]
    fn pop_layer_reintegrate_when_full_opacity_white_tint_with_content() {
        let mut compositor = LayerCompositor::new();
        compositor.push_layer(
            Vec::new(),
            DrawSegment::new(),
            1.0,             // opacity ~1.0
            [1.0, 1.0, 1.0], // white tint → no chroma
            BlendMode::SrcOver,
            None,
            LayerFilterChain::new(), // no filter
        );
        let outcome = compositor.pop_layer(segment_with_one_rect(), Vec::new(), rect_bounds_100());
        assert!(
            matches!(outcome, RestoreOutcome::Reintegrate { .. }),
            "full opacity + white tint must use the cheap reintegrate path"
        );
    }

    #[test]
    fn pop_layer_underflow_returns_underflow_outcome() {
        let mut compositor = LayerCompositor::new();

        // Build recognizable non-empty records to pass in.
        // DrawItem::Segment wraps a DrawSegment and requires no GPU resources.
        let sentinel_segment = DrawSegment::new();
        let sentinel_items: Vec<DrawItem> = vec![DrawItem::Segment(DrawSegment::new())];

        // Pop from empty stack — must not panic, must return Underflow and carry
        // the records back unchanged so the painter can restore them.
        let outcome = compositor.pop_layer(sentinel_segment, sentinel_items, rect_bounds_100());

        let RestoreOutcome::Underflow {
            current_segment,
            draw_order,
        } = outcome
        else {
            panic!("expected Underflow, got a different outcome variant");
        };

        // The segment was empty going in — it must come back empty.
        assert!(
            current_segment.is_empty(),
            "underflow must return current_segment intact"
        );
        // The single sentinel DrawItem must be returned, not dropped.
        assert_eq!(
            draw_order.len(),
            1,
            "underflow must return draw_order intact (expected 1 item, got {})",
            draw_order.len()
        );
    }

    #[test]
    fn effective_layer_opacity_multiplies_parent_and_paint_alpha() {
        let mut compositor = LayerCompositor::new();
        compositor.current_opacity = 0.5;
        let effective = compositor.effective_layer_opacity(0.4);
        assert!((effective - 0.2).abs() < 1e-6);
    }

    /// G3 guardrail: a layer with `image_filter: Some(_)` MUST route through
    /// `RestoreOutcome::Composite`, not `Reintegrate`, even when opacity≈1.0 and
    /// tint is white (conditions that would otherwise pick Reintegrate).
    ///
    /// The Reintegrate fast-path splices children directly into the parent draw
    /// order without an offscreen round-trip — it CANNOT apply an image filter.
    /// If this test fails, the Morph pass is silently dropped.
    #[test]
    fn pop_layer_composite_when_image_filter_set_even_at_full_opacity_white_tint() {
        use super::super::command_ir::{ImageFilterSpec, MorphOp};

        let mut compositor = LayerCompositor::new();
        // Push a layer that would normally Reintegrate (opacity=1.0, white tint,
        // SrcOver, no color-filters) — but we mark it with an image filter.
        compositor.push_layer(
            Vec::new(),
            DrawSegment::new(),
            1.0,             // opacity ~1.0 — would ordinarily Reintegrate
            [1.0, 1.0, 1.0], // white tint — no chroma
            BlendMode::SrcOver,
            None,
            LayerFilterChain::new(),
        );
        // Set the image filter on the top-of-stack entry.
        compositor.set_top_image_filter(ImageFilterSpec::Morph {
            radius: 2.0,
            op: MorphOp::Dilate,
        });

        // Pop with actual content so we don't fall through to Empty.
        let outcome = compositor.pop_layer(segment_with_one_rect(), Vec::new(), rect_bounds_100());
        assert!(
            matches!(outcome, RestoreOutcome::Composite { .. }),
            "image_filter=Some(_) must force Composite outcome (G3 guardrail)"
        );
    }
}
