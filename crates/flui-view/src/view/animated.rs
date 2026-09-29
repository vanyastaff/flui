// PORT-TARGET: flui-widgets animated widgets, flui-animation
//! AnimatedView - Views that automatically rebuild when animations change.
//!
//! AnimatedViews provide automatic subscription to Animation/Listenable
//! changes, eliminating boilerplate code for animated widgets.

use std::sync::Arc;

use flui_foundation::Listenable;

use super::stateful::StatefulView;

/// A View that automatically rebuilds when an animation changes.
///
/// AnimatedViews combine StatefulView with automatic Listenable subscription,
/// similar to Flutter's AnimatedWidget. When the listenable changes, the
/// element is automatically marked dirty and rebuilt.
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `AnimatedWidget`:
///
/// ```dart
/// abstract class AnimatedWidget extends StatefulWidget {
///   const AnimatedWidget({required this.listenable});
///   final Listenable listenable;
///
///   @override
///   State<AnimatedWidget> createState() => _AnimatedState();
/// }
///
/// class _AnimatedState extends State<AnimatedWidget> {
///   @override
///   void initState() {
///     super.initState();
///     widget.listenable.addListener(_handleChange);
///   }
///
///   void _handleChange() {
///     setState(() {});  // Rebuild when listenable changes
///   }
///
///   @override
///   void dispose() {
///     widget.listenable.removeListener(_handleChange);
///     super.dispose();
///   }
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{AnimatedView, ViewState, BuildContext, IntoView};
/// use flui_animation::Animation;
/// use std::sync::Arc;
///
/// #[derive(Clone)]
/// struct FadeTransition {
///     opacity: Arc<dyn Animation<f64>>,
///     child: Box<dyn View>,
/// }
///
/// impl AnimatedView for FadeTransition {
///     type State = FadeTransitionState;
///
///     fn listenable(&self) -> Arc<dyn Listenable> {
///         // Return the animation as a Listenable
///         // (Animation<T> extends Listenable)
///         self.opacity.clone() as Arc<dyn Listenable>
///     }
///
///     fn create_state(&self) -> Self::State {
///         FadeTransitionState
///     }
/// }
///
/// struct FadeTransitionState;
///
/// impl ViewState for FadeTransitionState {
///     type View = FadeTransition;
///
///     fn build(&mut self, view: &FadeTransition, ctx: &dyn BuildContext) -> impl IntoView {
///         let opacity = view.opacity.value();
///         Container::new()
///             .opacity(opacity)
///             .child(view.child.clone())
///     }
/// }
/// ```
pub trait AnimatedView: StatefulView {
    /// Get the Listenable to subscribe to.
    ///
    /// Typically this is an `Animation<T>`, which implements Listenable.
    /// When the listenable changes, the element is automatically marked
    /// dirty and rebuilt.
    ///
    /// # Returns
    ///
    /// A Listenable that should trigger rebuilds when it changes.
    fn listenable(&self) -> Arc<dyn Listenable>;
}

/// Implement View for an AnimatedView type.
///
/// This macro creates the View implementation for an AnimatedView type,
/// using AnimatedBehavior for automatic listener management.
///
/// ```rust,ignore
/// impl AnimatedView for MyFadeTransition {
///     type State = FadeTransitionState;
///     // ...
/// }
/// impl_animated_view!(MyFadeTransition);
/// ```
#[macro_export]
macro_rules! impl_animated_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> $crate::element::ElementKind {
                $crate::element::ElementKind::animated(self)
            }
        }
    };
}
