//! RenderParagraph — lays out and paints styled text.
//!
//! Wraps [`flui_painting::TextPainter`] (the cosmic-text-backed shaping +
//! metrics authority) the way [`RenderImage`](super::RenderImage) wraps a
//! leaf: the render object owns a painter, drives its layout from box
//! constraints, and forwards intrinsics / baseline / paint to it. Ports the
//! renderable core of Flutter's `RenderParagraph` (`paragraph.dart`): layout,
//! dry layout, the four intrinsics, baseline, and paint — with `softWrap`,
//! `maxLines`, and ellipsis truncation.
//!
//! Out of scope for this object (separable per Flutter's own structure):
//! inline `WidgetSpan` children, text selection, semantics, and the
//! clip/fade `TextOverflow` policies (only `ellipsis` is wired here).

use flui_foundation::Diagnosticable;
use flui_painting::{Invalidation, TextBaseline as PainterBaseline, TextPainter};
use flui_tree::Leaf;
use flui_types::{
    Offset, Size,
    typography::{InlineSpan, TextAlign, TextDirection},
};

use crate::{
    constraints::BoxConstraints,
    context::{BoxDryBaselineCtx, BoxDryLayoutCtx, BoxIntrinsicsCtx, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::{
        HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability, TextBaseline,
    },
};

/// Render object that lays out and paints a styled text span.
#[derive(Debug)]
pub struct RenderParagraph {
    /// The shaping + metrics authority. Owns the text, alignment, scale,
    /// max-lines/ellipsis, and the cached layout.
    painter: TextPainter,
    /// Whether the text wraps at the box's max width. When `false` the text
    /// lays out at unbounded width (single logical line per hard break) and
    /// can overflow — Flutter `RenderParagraph.softWrap`.
    soft_wrap: bool,
}

impl RenderParagraph {
    /// Creates a paragraph for `text` laid out in `direction`.
    pub fn new(text: impl Into<InlineSpan>, direction: TextDirection) -> Self {
        Self {
            painter: TextPainter::new()
                .with_text(text)
                .with_text_direction(direction),
            soft_wrap: true,
        }
    }

    /// Sets the text alignment (builder form).
    #[must_use]
    pub fn with_text_align(mut self, align: TextAlign) -> Self {
        self.painter.set_text_align(align);
        self
    }

    /// Sets the maximum number of lines before truncation (builder form).
    #[must_use]
    pub fn with_max_lines(mut self, max_lines: Option<u32>) -> Self {
        self.painter.set_max_lines(max_lines);
        self
    }

    /// Sets the ellipsis string shown when text is truncated (builder form).
    #[must_use]
    pub fn with_ellipsis(mut self, ellipsis: Option<String>) -> Self {
        self.painter.set_ellipsis(ellipsis);
        self
    }

    /// Sets the accessibility text scale factor (builder form).
    #[must_use]
    pub fn with_text_scale_factor(mut self, factor: f32) -> Self {
        self.painter.set_text_scale_factor(factor);
        self
    }

    /// Disables line wrapping (builder form) — the text lays out at unbounded
    /// width and may overflow the box.
    #[must_use]
    pub fn without_soft_wrap(mut self) -> Self {
        self.soft_wrap = false;
        self
    }

    /// Replaces the text span and returns the invalidation level.
    ///
    /// - [`Invalidation::Layout`] — text content changed; caller must
    ///   mark the node layout-dirty.
    /// - [`Invalidation::Paint`] — only paint attributes changed (color,
    ///   shadow); caller can mark paint-dirty only (cheaper).
    /// - [`Invalidation::None`] — no observable change.
    pub fn set_text(&mut self, text: impl Into<InlineSpan>) -> Invalidation {
        self.painter.set_text(Some(text.into()))
    }

    /// Sets the text alignment. The caller is responsible for marking the node
    /// layout-dirty.
    pub fn set_text_align(&mut self, align: TextAlign) {
        self.painter.set_text_align(align);
    }

    /// Read access to the underlying painter (cursor / selection geometry).
    pub fn painter(&self) -> &TextPainter {
        &self.painter
    }

    /// The width to lay out at for the given constraints. The box width
    /// matters — and the finite max is used — when the text wraps OR an
    /// ellipsis is configured (Flutter `_layoutText`:
    /// `widthMatters = softWrap || overflow == ellipsis`). A no-wrap label
    /// still needs the finite width so its ellipsis truncation can trigger;
    /// only a no-wrap, no-ellipsis paragraph lays out at unbounded width.
    fn layout_max_width(&self, constraints: &BoxConstraints) -> f32 {
        let width_matters = self.soft_wrap || self.painter.ellipsis().is_some();
        let max = constraints.max_width.get();
        if width_matters && max.is_finite() {
            max
        } else {
            f32::INFINITY
        }
    }
}

impl Diagnosticable for RenderParagraph {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_enum("text_align", self.painter.text_align());
        properties.add(
            "text_direction",
            self.painter
                .text_direction()
                .map(|d| format!("{d:?}"))
                .unwrap_or_else(|| "unset".to_string()),
        );
        properties.add_flag("soft_wrap", self.soft_wrap, "soft wrap");
        properties.add(
            "max_lines",
            self.painter
                .max_lines()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unlimited".to_string()),
        );
        properties.add(
            "ellipsis",
            self.painter
                .ellipsis()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "none".to_string()),
        );
    }
}

impl RenderBox for RenderParagraph {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        let max_width = self.layout_max_width(&constraints);
        self.painter.layout(constraints.min_width.get(), max_width);
        // The text's own size, then clamped into the box constraints
        // (Flutter `size = constraints.constrain(textPainter.size)`).
        constraints.constrain(self.painter.size())
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        let max_width = self.layout_max_width(&constraints);
        let text_size = self
            .painter
            .dry_size(constraints.min_width.get(), max_width);
        constraints.constrain(text_size)
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        _ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        let max_width = self.layout_max_width(&constraints);
        let painter_baseline = match baseline {
            TextBaseline::Alphabetic => PainterBaseline::Alphabetic,
            TextBaseline::Ideographic => PainterBaseline::Ideographic,
        };
        self.painter
            .dry_baseline(constraints.min_width.get(), max_width, painter_baseline)
    }

    // Width intrinsics ignore the height extent (text width does not depend on
    // available height); height intrinsics lay the text out at the given width.

    fn compute_min_intrinsic_width(&self, _height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.painter.min_intrinsic_width()
    }

    fn compute_max_intrinsic_width(&self, _height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.painter.max_intrinsic_width()
    }

    fn compute_min_intrinsic_height(&self, width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.painter.intrinsic_height(width)
    }

    fn compute_max_intrinsic_height(&self, width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.painter.intrinsic_height(width)
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        // Map the render-side baseline enum onto the painting-side one (two
        // parallel definitions, consolidation tracked). Valid only after
        // `perform_layout` populated the painter's cache; the baseline phase
        // always runs after layout, but guard so a stray pre-layout query
        // returns None instead of panicking.
        let painter_baseline = match baseline {
            TextBaseline::Alphabetic => PainterBaseline::Alphabetic,
            TextBaseline::Ideographic => PainterBaseline::Ideographic,
        };
        self.painter.has_layout().then(|| {
            self.painter
                .compute_distance_to_actual_baseline(painter_baseline)
        })
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
        // The recorder pre-translates the canvas to this node's origin, so the
        // text paints in local coordinates. Skipped before layout (no cache).
        if self.painter.has_layout() {
            self.painter.paint(ctx.canvas(), Offset::ZERO);
        }
    }
}

impl PaintEffectsCapability for RenderParagraph {}
impl SemanticsCapability for RenderParagraph {}
impl HotReloadCapability for RenderParagraph {}

#[cfg(test)]
mod tests {
    use flui_types::{geometry::px, typography::TextSpan};

    use super::*;
    use crate::context::intrinsics_test_support::{
        leaf_dry_baseline, leaf_dry_layout, leaf_intrinsics,
    };

    fn para(text: &str) -> RenderParagraph {
        RenderParagraph::new(TextSpan::new(text), TextDirection::Ltr)
    }

    #[test]
    fn baseline_is_none_before_layout() {
        let p = para("hello");
        assert_eq!(
            p.compute_distance_to_actual_baseline(TextBaseline::Alphabetic),
            None,
            "baseline is unavailable until perform_layout runs",
        );
    }

    #[test]
    fn max_intrinsic_width_bounds_min_intrinsic_width() {
        let p = para("hello world wrapping example");
        let max = leaf_intrinsics(|c| p.compute_max_intrinsic_width(f32::INFINITY, c));
        let min = leaf_intrinsics(|c| p.compute_min_intrinsic_width(f32::INFINITY, c));
        assert!(max > 0.0, "single-line width must be positive, got {max}");
        assert!(
            min > 0.0 && min <= max,
            "min-content {min} must be in (0, max-content {max}]",
        );
    }

    #[test]
    fn narrow_constraints_wrap_taller_and_no_wider_than_single_line() {
        let p = para("a b c d e f g h i j k l m n");
        let wide = leaf_dry_layout(|c| {
            p.compute_dry_layout(
                BoxConstraints::new(px(0.0), px(10_000.0), px(0.0), px(10_000.0)),
                c,
            )
        });
        let narrow = leaf_dry_layout(|c| {
            p.compute_dry_layout(
                BoxConstraints::new(px(0.0), px(30.0), px(0.0), px(10_000.0)),
                c,
            )
        });
        assert!(
            narrow.height > wide.height,
            "wrapping at 30px ({narrow:?}) must be taller than a single line ({wide:?})",
        );
        assert!(
            narrow.width <= wide.width,
            "wrapped width {:?} cannot exceed the single-line width {:?}",
            narrow.width,
            wide.width,
        );
    }

    #[test]
    fn dry_baseline_is_available_without_layout() {
        let p = para("hello");
        let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
        let dry =
            leaf_dry_baseline(|c| p.compute_dry_baseline(constraints, TextBaseline::Alphabetic, c));
        assert!(
            dry.is_some_and(|baseline| baseline > 0.0),
            "text dry baseline must be computable before perform_layout",
        );
    }

    #[test]
    fn intrinsic_height_is_positive_for_text() {
        let p = para("hello");
        let h = leaf_intrinsics(|c| p.compute_min_intrinsic_height(200.0, c));
        assert!(h > 0.0, "laid-out text has positive height, got {h}");
    }

    #[test]
    fn intrinsic_height_is_finite_at_infinite_width() {
        let p = para("hello world");
        let h = leaf_intrinsics(|c| p.compute_max_intrinsic_height(f32::INFINITY, c));
        assert!(
            h.is_finite() && h > 0.0,
            "height at unbounded width must be finite, got {h}",
        );
    }

    #[test]
    fn no_wrap_ellipsis_truncates_under_finite_constraints() {
        // A single-line label with an ellipsis must still honor the finite
        // parent width (so truncation triggers) even though wrapping is off.
        let p = RenderParagraph::new(
            TextSpan::new("a very long single line of text that must be ellipsized"),
            TextDirection::Ltr,
        )
        .without_soft_wrap()
        .with_max_lines(Some(1))
        .with_ellipsis(Some("…".to_string()));

        let full = leaf_intrinsics(|c| p.compute_max_intrinsic_width(f32::INFINITY, c));
        let dry = leaf_dry_layout(|c| {
            p.compute_dry_layout(
                BoxConstraints::new(px(0.0), px(60.0), px(0.0), px(10_000.0)),
                c,
            )
        });
        assert!(
            dry.width.get() < full,
            "ellipsized width {} must be less than the untruncated single-line width {full} \
             (the finite max width must reach the painter despite soft_wrap=false)",
            dry.width.get(),
        );
    }
}
