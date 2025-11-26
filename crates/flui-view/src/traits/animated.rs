//! AnimatedView - Views that rebuild when animation values change
//!
//! For views that subscribe to animation changes and rebuild automatically.

use flui_element::IntoElement;

use flui_element::BuildContext;

/// Listenable - Types that can notify listeners of changes
///
/// Implemented by animation controllers, value notifiers, etc.
pub trait Listenable: Send + Sync + 'static {
    /// Add a listener callback
    fn add_listener(&self, callback: Box<dyn Fn() + Send + Sync>);

    /// Remove a listener callback (by some identifier)
    fn remove_listener(&self);
}

/// AnimatedView - Views that subscribe to animation changes.
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
/// # Example
///
/// ```rust,ignore
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
pub trait AnimatedView<L: Listenable>: Send + Sync + 'static {
    /// Build UI with current animation value.
    ///
    /// Called on every animation frame when listenable notifies.
    fn build(&mut self, ctx: &dyn BuildContext) -> impl IntoElement;

    /// Get the listenable to subscribe to.
    ///
    /// Framework subscribes to this listenable and triggers rebuild on notify.
    fn listenable(&self) -> &L;

    /// Called on each animation tick (optional).
    ///
    /// Override for custom behavior on every animation frame.
    fn on_animation_tick(&mut self, _ctx: &dyn BuildContext) {}

    /// Initialize after element is mounted (optional).
    fn init(&mut self, _ctx: &dyn BuildContext) {}

    /// Called when element is disposed (optional).
    fn dispose(&mut self, _ctx: &dyn BuildContext) {}
}
