//! Custom painter delegate for custom painting on a canvas.
//!
//! [`CustomPainter`] allows users to implement custom painting behavior
//! without creating a new render object. It provides methods for painting,
//! hit testing, and accessibility.

use std::{any::Any, fmt::Debug};

use flui_foundation::Listenable;
use flui_foundation::geometry::{Offset, Size};
use flui_painting::Canvas;
use flui_semantics::SemanticsRegion;

/// A delegate that provides custom painting behavior.
///
/// Implement this trait to define custom painting on a canvas. The delegate
/// is used by `RenderCustomPaint` to paint content before or after its child.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::delegates::CustomPainter;
/// use flui_rendering::pipeline::Canvas;
/// use flui_foundation::geometry::Size;
///
/// #[derive(Debug)]
/// struct CheckerboardPainter {
///     cell_size: f64,
/// }
///
/// impl CustomPainter for CheckerboardPainter {
///     fn paint(&self, canvas: &mut Canvas, size: Size) {
///         let cols = (size.width / self.cell_size).ceil() as i32;
///         let rows = (size.height / self.cell_size).ceil() as i32;
///         // Draw checkerboard pattern...
///     }
///
///     fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
///         if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
///             self.cell_size != old.cell_size
///         } else {
///             true
///         }
///     }
/// }
/// ```
pub trait CustomPainter: Debug {
    /// Paint custom content on the canvas.
    ///
    /// The canvas coordinate space is configured such that the origin is at
    /// the top left of the box. The area of the box is the size argument.
    ///
    /// Paint operations should remain inside the given area.
    fn paint(&self, canvas: &mut Canvas, size: Size);

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

    /// Returns self as `Any` for downcasting.
    ///
    /// This enables comparing delegates of the same concrete type in
    /// `should_repaint` and `should_rebuild_semantics`.
    fn as_any(&self) -> &dyn Any;
}
