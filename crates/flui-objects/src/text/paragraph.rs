//! RenderParagraph — lays out and paints styled text.
//!
//! Wraps [`flui_painting::TextPainter`] (the shaping and metrics authority)
//! the way [`RenderImage`](crate::RenderImage) wraps a leaf: the render
//! object owns a painter, drives its layout from box constraints, and
//! forwards intrinsics / baseline / paint to it. Every measurement goes
//! through the UI runtime's text context, lent by the layout, intrinsics and dry
//! contexts as `ctx.text()` (ADR-0092 §10 step 3). It covers
//! the renderable core of a paragraph: layout, dry layout, the four
//! intrinsics, baseline, and paint — with soft wrap, `max_lines`, and
//! ellipsis truncation.
//!
//! Out of scope for this object (separable): inline `WidgetSpan` children, text selection, semantics, and the
//! clip/fade `TextOverflow` policies (only `ellipsis` is wired here).

use flui_foundation::Diagnosticable;
use flui_foundation::Leaf;
use flui_foundation::geometry::{Offset, Size};
use flui_painting::typography::{InlineSpan, TextAlign, TextDirection};
use flui_painting::{Invalidation, TextBaseline as PainterBaseline, TextPainter};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryBaselineCtx, BoxDryLayoutCtx, BoxIntrinsicsCtx, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    semantics::SemanticsConfiguration,
    traits::{RenderBox, TextBaseline},
};

/// Render object that lays out and paints a styled text span.
#[derive(Debug)]
pub struct RenderParagraph {
    /// The shaping + metrics authority. Owns the text, alignment, scale,
    /// max-lines/ellipsis, and the cached layout.
    painter: TextPainter,
    /// Whether the text wraps at the box's max width. When `false` the text
    /// lays out at unbounded width (single logical line per hard break) and
    /// can overflow.
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
    pub fn with_text_scale_factor(mut self, factor: f64) -> Self {
        self.painter.set_text_scale_factor(factor);
        self
    }

    /// Updates accessibility sizing and invalidates paragraph geometry.
    pub fn set_text_scale_factor(&mut self, factor: f64) -> flui_rendering::RenderUpdateImpact {
        let previous = self.painter.text_scale_factor();
        self.painter.set_text_scale_factor(factor);
        if self.painter.text_scale_factor() == previous {
            flui_rendering::RenderUpdateImpact::NONE
        } else {
            flui_rendering::RenderUpdateImpact::LAYOUT
                | flui_rendering::RenderUpdateImpact::SEMANTICS
        }
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
    pub fn set_text(&mut self, text: impl Into<InlineSpan>) -> flui_rendering::RenderUpdateImpact {
        text_invalidation_impact(self.painter.set_text(Some(text.into())), true)
    }

    /// Sets the text alignment. The caller is responsible for marking the node
    /// layout-dirty.
    pub fn set_text_align(&mut self, align: TextAlign) -> flui_rendering::RenderUpdateImpact {
        text_invalidation_impact(self.painter.set_text_align(align), false)
    }

    /// Updates the resolved text direction.
    pub fn set_text_direction(
        &mut self,
        direction: TextDirection,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.painter.text_direction() == Some(direction) {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.painter.set_text_direction(Some(direction));
        flui_rendering::RenderUpdateImpact::LAYOUT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Updates the maximum line count.
    pub fn set_max_lines(&mut self, max_lines: Option<u32>) -> flui_rendering::RenderUpdateImpact {
        if self.painter.max_lines() == max_lines {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.painter.set_max_lines(max_lines);
        flui_rendering::RenderUpdateImpact::LAYOUT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Read access to the underlying painter (cursor / selection geometry).
    pub fn painter(&self) -> &TextPainter {
        &self.painter
    }

    /// The width to lay out at for the given constraints. The box width
    /// matters — and the finite max is used — when the text wraps OR an
    /// ellipsis is configured. A no-wrap label
    /// still needs the finite width so its ellipsis truncation can trigger;
    /// only a no-wrap, no-ellipsis paragraph lays out at unbounded width.
    fn layout_max_width(&self, constraints: &BoxConstraints) -> f64 {
        let width_matters = self.soft_wrap || self.painter.ellipsis().is_some();
        let max = constraints.max_width;
        if width_matters && max.is_finite() {
            max
        } else {
            f64::INFINITY
        }
    }
}

fn text_invalidation_impact(
    invalidation: Invalidation,
    semantics_may_change: bool,
) -> flui_rendering::RenderUpdateImpact {
    match invalidation {
        Invalidation::None => flui_rendering::RenderUpdateImpact::NONE,
        Invalidation::Paint => flui_rendering::RenderUpdateImpact::PAINT,
        Invalidation::Layout => {
            let impact = flui_rendering::RenderUpdateImpact::LAYOUT;
            if semantics_may_change {
                impact | flui_rendering::RenderUpdateImpact::SEMANTICS
            } else {
                impact
            }
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
                .map_or_else(|| "unset".to_string(), |d| format!("{d:?}")),
        );
        properties.add_flag("soft_wrap", self.soft_wrap, "soft wrap");
        properties.add(
            "max_lines",
            self.painter
                .max_lines()
                .map_or_else(|| "unlimited".to_string(), |n| n.to_string()),
        );
        properties.add(
            "ellipsis",
            self.painter
                .ellipsis()
                .map_or_else(|| "none".to_string(), ToString::to_string),
        );
        // Emit the plain text so diagnostics dumps and test finders can match on
        // content without inspecting the full `InlineSpan` tree.
        if let Some(span) = self.painter.text() {
            properties.add("text", span.to_plain_text());
            // Lets a mounted-widget test read a resolved text color back out
            // (e.g. a themed/cascaded color reaching the real paragraph a
            // build produced) without downcasting or inspecting paint
            // output — `TextStyle: Debug` is already required for the type
            // to be usable at all, so this is a pure diagnostics read, not
            // a new capability.
            properties.add("style", format!("{:?}", span.style()));
        }
    }
}

impl RenderBox for RenderParagraph {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        let max_width = self.layout_max_width(&constraints);
        self.painter
            .layout(&mut ctx.text(), constraints.min_width, max_width);
        // The text's own size, then clamped into the box constraints.
        constraints.constrain(self.painter.size())
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        let max_width = self.layout_max_width(&constraints);
        let text_size = self
            .painter
            .dry_size(&mut ctx.text(), constraints.min_width, max_width);
        constraints.constrain(text_size)
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f64> {
        let max_width = self.layout_max_width(&constraints);
        let painter_baseline = match baseline {
            TextBaseline::Alphabetic => PainterBaseline::Alphabetic,
            TextBaseline::Ideographic => PainterBaseline::Ideographic,
        };
        self.painter.dry_baseline(
            &mut ctx.text(),
            constraints.min_width,
            max_width,
            painter_baseline,
        )
    }

    // Width intrinsics ignore the height extent (text width does not depend on
    // available height); height intrinsics lay the text out at the given width.

    fn compute_min_intrinsic_width(&self, _height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.painter.min_intrinsic_width(&mut ctx.text())
    }

    fn compute_max_intrinsic_width(&self, _height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.painter.max_intrinsic_width(&mut ctx.text())
    }

    fn compute_min_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.painter.intrinsic_height(&mut ctx.text(), width)
    }

    fn compute_max_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.painter.intrinsic_height(&mut ctx.text(), width)
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f64> {
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

    /// Covers the plain-text case this object supports (no `WidgetSpan`
    /// children, no gesture recognizers on the span — see the module doc's
    /// "Out of scope" list): the label is the plain text and the text
    /// direction is the paragraph's.
    ///
    /// Mapping decision (see `crates/flui-objects/ARCHITECTURE.md` "Mapping
    /// decisions"): an EMPTY plain-text span sets neither `label` nor
    /// `text_direction`, so the paragraph stays un-annotated and contributes
    /// no semantics node of its own. `SemanticsConfiguration::set_text_direction`
    /// alone marks the configuration annotated (see that
    /// setter's doc comment), which would otherwise publish an empty,
    /// unlabelled node for every text-less paragraph in the tree.
    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        let Some(span) = self.painter.text() else {
            return;
        };
        let text = span.to_plain_text();
        if text.is_empty() {
            return;
        }
        config.set_label(text);
        if let Some(direction) = self.painter.text_direction() {
            config.set_text_direction(semantics_text_direction(direction));
        }
    }
}

/// Maps the painting-side [`TextDirection`] onto `flui-semantics`'s own
/// parallel enum of the same name (two definitions of the same concept).
fn semantics_text_direction(direction: TextDirection) -> flui_rendering::semantics::TextDirection {
    match direction {
        TextDirection::Ltr => flui_rendering::semantics::TextDirection::Ltr,
        TextDirection::Rtl => flui_rendering::semantics::TextDirection::Rtl,
    }
}
