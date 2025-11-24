//! Animated view trait.
//!
//! For views that rebuild when animation values change.

use crate::element::IntoElement;
use crate::foundation::Listenable;
use crate::view::{Animated, BuildContext, View};

/// Animated view - views that subscribe to animation changes.
///
/// Similar to Flutter's `AnimatedWidget`. Automatically rebuilds when
/// the listenable (animation, controller, stream) notifies.
///
/// # Architecture
///
/// ```text
/// AnimatedView → subscribes to → Listenable
///                                    ↓ notify()
///                             rebuild scheduled
/// ```
///
/// # Lifecycle
///
/// 1. **Created**: View instantiated with listenable
/// 2. **Mounted**: Subscribe to listenable
/// 3. **Notification**: Listenable notifies → rebuild
/// 4. **Build**: `build()` called with current value
/// 5. **Updated**: If listenable changes, resubscribe
/// 6. **Disposed**: Unsubscribe from listenable
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone)]
/// struct FadeTransition {
///     opacity: Animation<f32>,
///     child: Element,
/// }
///
/// impl AnimatedView<Animation<f32>> for FadeTransition {
///     fn listenable(&self) -> &Animation<f32> {
///         &self.opacity
///     }
///
///     fn build(&mut self, _ctx: &BuildContext) -> impl IntoElement {
///         Opacity::new(self.opacity.value())
///             .child(self.child.clone())
///     }
/// }
/// ```
///
/// # When to Use
///
/// - Widget driven by animation
/// - Rebuild on every animation frame
/// - Multiple widgets sharing same animation
///
/// # When NOT to Use
///
/// - Animations need internal state → Use `StatefulView` with controller
/// - Implicit animations → Use `ImplicitlyAnimatedView` (future)
/// - No animation → Use `StatelessView` or `StatefulView`
///
/// # Comparison to Flutter
///
/// | Flutter | FLUI |
/// |---------|------|
/// | `AnimatedWidget` | `AnimatedView` |
/// | `listenable` field | `listenable()` method |
/// | Auto-rebuild on notify | Auto-rebuild on notify |
pub trait AnimatedView<L: Listenable>: Clone + Send + 'static {
    /// Build UI with current animation value.
    ///
    /// Called on every animation frame when listenable notifies.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn build(&mut self, _ctx: &BuildContext) -> impl IntoElement {
    ///     let scale = self.scale_animation.value();
    ///     Transform::scale(scale).child(self.child.clone())
    /// }
    /// ```
    fn build(&mut self, ctx: &BuildContext) -> impl IntoElement;

    /// Get the listenable to subscribe to.
    ///
    /// Framework subscribes to this listenable and triggers rebuild on notify.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn listenable(&self) -> &Animation<f32> {
    ///     &self.opacity
    /// }
    /// ```
    fn listenable(&self) -> &L;

    /// Called on each animation tick (optional).
    ///
    /// Override for custom behavior on every animation frame.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn on_animation_tick(&mut self, ctx: &BuildContext) {
    ///     // Log animation progress
    ///     tracing::trace!("Animation value: {}", self.listenable().value());
    /// }
    /// ```
    fn on_animation_tick(&mut self, _ctx: &BuildContext) {}
}

/// Auto-implement `View<Animated<L>>` for all `AnimatedView<L>`.
///
/// This allows `AnimatedView` to integrate with the internal protocol system.
impl<V, L> View<Animated<L>> for V
where
    V: AnimatedView<L>,
    L: Listenable,
{
    fn _build(&mut self, ctx: &BuildContext) -> crate::element::Element {
        self.build(ctx).into_element()
    }
}
