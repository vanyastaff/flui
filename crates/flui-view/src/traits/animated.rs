//! `AnimatedView` - Views that rebuild when animation values change
//!
//! For views that subscribe to animation changes and rebuild automatically.
//!
//! # Lifecycle
//!
//! `AnimatedView` follows Flutter-like lifecycle:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     LIFECYCLE DIAGRAM                        │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                              │
//! │  ┌──────────────┐                                           │
//! │  │     init     │ ← Called once when element is mounted     │
//! │  └──────┬───────┘   (subscribe to listenable here)          │
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌─────────────────────────┐                                │
//! │  │ did_change_dependencies │ ← Called when dependencies     │
//! │  └──────┬──────────────────┘   change (InheritedWidget)     │
//! │         │                                                    │
//! │         ▼                                                    │
//! │  ┌──────────────┐                                           │
//! │  │    build     │◄──────────────────┐                       │
//! │  └──────┬───────┘                   │                       │
//! │         │                           │                       │
//! │         ▼                           │ (listenable.notify()) │
//! │  ┌───────────────────┐              │                       │
//! │  │ on_animation_tick │──────────────┘                       │
//! │  └──────┬────────────┘                                      │
//! │         │                                                    │
//! │         ▼ (element unmounted)                               │
//! │  ┌──────────────┐                                           │
//! │  │   dispose    │ ← Unsubscribe from listenable             │
//! │  └──────────────┘                                           │
//! │                                                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use flui_foundation::ListenerId;

use crate::{BuildContext, IntoView};

/// Listenable - Types that can notify listeners of changes.
///
/// This trait uses interior mutability pattern (via `&self`) to allow
/// subscribing from immutable references, which is common in UI frameworks.
///
/// Implemented by animation controllers, value notifiers, etc.
///
/// # Note
///
/// This differs from `flui_foundation::Listenable` which uses `&mut self`.
/// The interior mutability version is more convenient for UI patterns.
pub trait Listenable: Send + Sync + 'static {
    /// Add a listener callback. Returns an ID for later removal.
    fn add_listener(&self, callback: Box<dyn Fn() + Send + Sync>) -> ListenerId;

    /// Remove a listener by its ID.
    fn remove_listener(&self, id: ListenerId);
}

/// `AnimatedView` - Views that subscribe to animation changes.
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
///     child: Box<dyn ViewObject>,
/// }
///
/// impl AnimatedView<Animation<f32>> for FadeTransition {
///     fn listenable(&self) -> &Animation<f32> {
///         &self.opacity
///     }
///
///     fn build(&mut self, _ctx: &dyn BuildContext) -> impl IntoView {
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
    fn build(&mut self, ctx: &dyn BuildContext) -> impl IntoView;

    /// Get the listenable to subscribe to.
    ///
    /// Framework subscribes to this listenable and triggers rebuild on notify.
    fn listenable(&self) -> &L;

    // ========== LIFECYCLE METHODS ==========

    /// Initialize after element is mounted.
    ///
    /// Called once after the element has been inserted into the tree.
    /// The framework automatically subscribes to the listenable after this.
    ///
    /// **Flutter equivalent:** `State.initState()`
    #[allow(unused_variables)]
    fn init(&mut self, ctx: &dyn BuildContext) {}

    /// Called when an inherited widget dependency changes.
    ///
    /// **Flutter equivalent:** `State.didChangeDependencies()`
    #[allow(unused_variables)]
    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {}

    /// Called on each animation tick.
    ///
    /// Override for custom behavior on every animation frame.
    /// Called after listenable notifies but before rebuild.
    #[allow(unused_variables)]
    fn on_animation_tick(&mut self, ctx: &dyn BuildContext) {}

    /// Called when the element is temporarily removed from the tree.
    ///
    /// **Flutter equivalent:** `State.deactivate()`
    #[allow(unused_variables)]
    fn deactivate(&mut self, ctx: &dyn BuildContext) {}

    /// Called when the element is reinserted after being deactivated.
    ///
    /// **Flutter equivalent:** `State.activate()`
    #[allow(unused_variables)]
    fn activate(&mut self, ctx: &dyn BuildContext) {}

    /// Called when element is permanently removed.
    ///
    /// The framework automatically unsubscribes from the listenable before this.
    /// Clean up any additional resources here.
    ///
    /// **Flutter equivalent:** `State.dispose()`
    #[allow(unused_variables)]
    fn dispose(&mut self, ctx: &dyn BuildContext) {}
}
