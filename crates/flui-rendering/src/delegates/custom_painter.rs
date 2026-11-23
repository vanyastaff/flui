//! Custom painter delegate for custom painting on a canvas.
//!
//! [`CustomPainter`] allows users to implement custom painting behavior
//! without creating a new render object. It provides methods for painting,
//! hit testing, and accessibility.

use std::{any::Any, fmt::Debug};

use flui_foundation::Listenable;
use flui_foundation::geometry::{Offset, Size};
use flui_semantics::SemanticsRegion;

use super::CustomPaintContext;

/// A delegate that provides custom painting behavior.
///
/// Implement this trait to define custom painting on a canvas. The delegate
/// is used by `RenderCustomPaint` to paint content before or after its child.
///
/// Flutter's counterpart is `CustomPainter.paint(Canvas, Size)`, whose painter
/// lays a `TextPainter` out in paint against the process's font system. FLUI
/// has none: the UI runtime lends its text context per phase, as a layout
/// context's `text()` does. So [`Self::paint`] receives a
/// [`CustomPaintContext`], in the shape `RenderBox::paint` takes its
/// `PaintContext`: the canvas, the size, and a text loan on demand. A painter
/// that calls [`CustomPaintContext::text`] is recorded with its node, so
/// registering a font paints it again; one that never asks is not recorded.
///
/// # Example
///
/// A bar with its label, shaped through the lent text context:
///
/// ```
/// use std::any::Any;
/// use flui_foundation::TextDirection;
/// use flui_foundation::geometry::{Offset, Point, Rect, Size};
/// use flui_painting::{TextPainter, paint::Paint, styling::Color, typography::TextSpan};
/// use flui_rendering::delegates::{CustomPaintContext, CustomPainter};
///
/// #[derive(Debug)]
/// struct BarPainter {
///     fraction: f64,
///     label: String,
/// }
///
/// impl CustomPainter for BarPainter {
///     fn paint(&self, ctx: &mut CustomPaintContext<'_>) {
///         let size = ctx.size();
///         let bar = Size::new(size.width * self.fraction, size.height);
///         ctx.canvas().draw_rect(
///             Rect::from_origin_size(Point::ZERO, bar),
///             &Paint::fill(Color::rgb(30, 90, 200)),
///         );
///         let mut label = TextPainter::new()
///             .with_text(TextSpan::new(self.label.clone()))
///             .with_text_direction(TextDirection::Ltr);
///         // Shape while the context is lent, then draw once the loan ends.
///         let shaped = ctx.text().is_ok_and(|mut text| {
///             text.measurement().layout(&mut label, 0.0, size.width).is_ok()
///         });
///         if shaped {
///             label.paint(ctx.canvas(), Offset::ZERO);
///         }
///     }
///
///     fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
///         (old_delegate as &dyn Any)
///             .downcast_ref::<Self>()
///             .is_none_or(|old| old.fraction != self.fraction || old.label != self.label)
///     }
/// }
/// ```
pub trait CustomPainter: Any + Debug {
    /// Paint custom content through `ctx`.
    ///
    /// The canvas coordinate space is configured such that the origin is at
    /// the top left of the box. The area of the box is
    /// [`ctx.size()`](CustomPaintContext::size).
    ///
    /// Paint operations should remain inside the given area. Text is shaped
    /// through [`ctx.text()`](CustomPaintContext::text), which records the
    /// node so that registering a font repaints it.
    fn paint(&self, ctx: &mut CustomPaintContext<'_>);

    /// Whether this painter should repaint when replaced with a new delegate.
    ///
    /// Called when a new instance of the delegate is provided, to check if
    /// the new instance represents different information that requires
    /// repainting.
    ///
    /// # Arguments
    ///
    /// * `old_delegate` - The previous painter delegate
    ///
    /// # Returns
    ///
    /// `true` if the painter should repaint, `false` otherwise.
    fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool;

    /// An optional repaint [`Listenable`]: when it notifies, the hosting
    /// `RenderCustomPaint` requests paint and semantics together, so changed
    /// authored regions update their geometry, attributes and actions without
    /// a widget rebuild. Stable region keys retain their identities.
    ///
    /// The host retains the returned source with its registration and removes
    /// that exact registration on detach, replacement or owner release. It
    /// does not call this getter again to discover the cancellation target.
    /// A changed return value takes effect on the next attachment or replacement.
    /// Defaults to `None` (a static painter that never self-invalidates).
    fn repaint(&self) -> Option<std::rc::Rc<dyn Listenable>> {
        None
    }

    /// Hit test at the given position.
    ///
    /// The given position is relative to the same coordinate space as the
    /// last [`Self::paint`] call. Return `true` if the position is a "hit",
    /// `false` if it is a miss, and `None` to use the caller's default
    /// behavior.
    ///
    /// The default implementation returns `None`. `RenderCustomPaint`
    /// resolves the tri-state by role: a *background* painter
    /// defaults to hit (`None` → `true`) and a *foreground* painter defaults
    /// to miss (`None` → `false`) — the trait itself is direction-agnostic,
    /// so it cannot pick a single default for both roles.
    fn hit_test(&self, _position: Offset) -> Option<bool> {
        None
    }

    /// Describes accessible parts of the content using the committed paint size.
    ///
    /// Keys must be unique within this painter's contribution and stable across
    /// reordering. Each region receives independent geometry and action routing.
    /// The host orders background regions, actual child content, then foreground
    /// regions. Nesting follows the real render tree and its merge/exclude policy.
    fn semantics_regions(&self, _size: Size) -> Vec<SemanticsRegion> {
        Vec::new()
    }

    /// Whether to rebuild semantics when the delegate changes.
    ///
    /// Called when a new instance of the delegate is provided, to check if
    /// the semantics information needs to be rebuilt.
    fn should_rebuild_semantics(&self, _old_delegate: &dyn CustomPainter) -> bool {
        true
    }
}
