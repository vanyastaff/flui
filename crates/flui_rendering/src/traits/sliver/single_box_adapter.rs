//! Single box adapter trait for slivers wrapping box children

use crate::traits::{RenderBox, RenderSliver};

/// Trait for slivers that wrap a single box child
///
/// RenderSliverSingleBoxAdapter is used for slivers that contain a single
/// box render object as their child. The most common example is
/// SliverToBoxAdapter which wraps a box widget in a sliver context.
///
/// # Use Case
///
/// This allows non-scrollable content (boxes) to be inserted into
/// scrollable viewports (slivers):
///
/// ```ignore
/// CustomScrollView(
///     slivers: [
///         SliverToBoxAdapter(  // Sliver wrapping box
///             child: Container(height: 200.0),  // Box widget
///         ),
///         SliverList(...),  // Regular sliver
///     ],
/// )
/// ```
///
/// # Ambassador Support
///
/// ```ignore
/// use ambassador::Delegate;
///
/// #[derive(Delegate)]
/// #[delegate(RenderSliverSingleBoxAdapter, target = "adapter")]
/// struct RenderSliverToBoxAdapter {
///     adapter: /* container type */,
/// }
///
/// impl RenderSliverSingleBoxAdapter for RenderSliverToBoxAdapter {
///     fn child(&self) -> Option<&dyn RenderBox> {
///         self.adapter.child()
///     }
///
///     fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
///         self.adapter.child_mut()
///     }
/// }
/// ```
pub trait RenderSliverSingleBoxAdapter: RenderSliver {
    /// Returns a reference to the box child, if any
    fn child(&self) -> Option<&dyn RenderBox>;

    /// Returns a mutable reference to the box child, if any
    fn child_mut(&mut self) -> Option<&mut dyn RenderBox>;

    /// Returns whether this adapter has a child
    fn has_child(&self) -> bool {
        self.child().is_some()
    }
}
