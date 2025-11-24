//! Core View trait with protocol system.
//!
//! Views are immutable descriptions of UI that the framework converts into
//! mutable elements for lifecycle management.

use crate::element::Element;
use crate::view::build_context::BuildContext;
use crate::view::protocol::ViewProtocol;

// ============================================================================
// VIEW TRAIT
// ============================================================================

/// Base View trait with protocol type parameter.
///
/// This is an internal trait. Users should implement one of:
/// - [`StatelessView`] - Simple views without state
/// - [`StatefulView<S>`] - Views with persistent state
/// - [`AnimatedView<L>`] - Views driven by animations
/// - [`ProviderView<T>`] - Views that provide data to descendants
/// - [`ProxyView`] - Views that wrap single child
/// - [`RenderView<P, A>`] - Views that create render objects
///
/// Each specialized trait auto-implements `View<Protocol>`.
///
/// [`StatelessView`]: crate::view::StatelessView
/// [`StatefulView<S>`]: crate::view::StatefulView
/// [`AnimatedView<L>`]: crate::view::AnimatedView
/// [`ProviderView<T>`]: crate::view::ProviderView
/// [`ProxyView`]: crate::view::ProxyView
/// [`RenderView<P, A>`]: crate::view::RenderView
pub trait View<P: ViewProtocol>: Send + 'static {
    /// Internal build method.
    ///
    /// Called by the framework through ViewObject wrappers.
    /// Users should not call this directly.
    fn _build(&mut self, ctx: &BuildContext) -> Element;
}
