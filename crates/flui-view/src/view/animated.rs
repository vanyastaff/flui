//! AnimatedView - Views that automatically rebuild when animations change.
//!
//! AnimatedViews provide automatic subscription to Animation/Listenable changes,
//! eliminating boilerplate code for animated widgets.

use super::stateful::StatefulView;
use flui_foundation::Listenable;
use std::sync::Arc;

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
///     opacity: Arc<dyn Animation<f32>>,
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
    /// Typically this is an Animation<T>, which implements Listenable.
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
/// using AnimationBehavior for automatic listener management.
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
            fn create_element(&self) -> Box<dyn $crate::ElementBase> {
                use $crate::element::AnimationBehavior;
                Box::new($crate::AnimatedElement::new(self, AnimationBehavior::new(self)))
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::BuildContext;
    use crate::element::{AnimationBehavior, Lifecycle};
    use crate::view::{AnimatedElement, ElementBase, View, ViewState};
    use flui_foundation::ChangeNotifier;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // Test animated view
    #[derive(Clone)]
    struct TestAnimatedView {
        listenable: Arc<ChangeNotifier>,
        value: i32,
    }

    // Test state
    struct TestAnimatedState {
        build_count: Arc<AtomicUsize>,
    }

    // Dummy view for testing
    #[derive(Clone)]
    struct DummyView;

    impl View for DummyView {
        fn create_element(&self) -> Box<dyn super::super::view::ElementBase> {
            // Create a minimal element for testing
            use super::super::stateless::StatelessView;
            use crate::element::StatelessBehavior;
            use crate::view::StatelessElement;

            #[derive(Clone)]
            struct MinimalView;
            impl StatelessView for MinimalView {
                fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
                    Box::new(DummyView)
                }
            }

            Box::new(StatelessElement::new(&MinimalView, StatelessBehavior::new()))
        }
    }

    impl ViewState<TestAnimatedView> for TestAnimatedState {
        fn build(&self, _view: &TestAnimatedView, _ctx: &dyn BuildContext) -> Box<dyn View> {
            self.build_count.fetch_add(1, Ordering::SeqCst);
            // Return a dummy view for testing
            Box::new(DummyView)
        }

        fn dispose(&mut self) {
            // Cleanup
        }
    }

    impl StatefulView for TestAnimatedView {
        type State = TestAnimatedState;

        fn create_state(&self) -> Self::State {
            TestAnimatedState {
                build_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl AnimatedView for TestAnimatedView {
        fn listenable(&self) -> Arc<dyn Listenable> {
            self.listenable.clone() as Arc<dyn Listenable>
        }
    }

    #[test]
    fn test_animated_element_creation() {
        let listenable = Arc::new(ChangeNotifier::new());
        let view = TestAnimatedView {
            listenable: listenable.clone(),
            value: 42,
        };

        let element = AnimatedElement::new(&view, AnimationBehavior::new(&view));
        assert_eq!(element.core().lifecycle(), Lifecycle::Initial);
    }

    #[test]
    fn test_animated_element_listener_subscription() {
        let listenable = Arc::new(ChangeNotifier::new());
        let view = TestAnimatedView {
            listenable: listenable.clone(),
            value: 42,
        };

        let mut element = AnimatedElement::new(&view, AnimationBehavior::new(&view));

        // Before mount, no listener
        assert_eq!(listenable.len(), 0);

        // Mount the element
        element.mount(None, 0);

        // After mount, listener should be subscribed
        assert_eq!(listenable.len(), 1);

        // Unmount
        element.unmount();

        // After unmount, listener should be unsubscribed
        assert_eq!(listenable.len(), 0);
    }

    #[test]
    fn test_animated_element_automatic_rebuild() {
        let listenable = Arc::new(ChangeNotifier::new());
        let view = TestAnimatedView {
            listenable: listenable.clone(),
            value: 42,
        };

        let mut element = AnimatedElement::new(&view, AnimationBehavior::new(&view));

        // Mount the element
        element.mount(None, 0);

        // Element should be dirty after mount
        assert!(element.core().is_dirty());

        // Clear dirty flag
        element.core_mut().clear_dirty();
        assert!(!element.core().is_dirty());

        // Notify listeners (simulating animation change)
        listenable.notify_listeners();

        // Element should be marked dirty again
        assert!(element.core().is_dirty());

        // Unmount
        element.unmount();
    }

    #[test]
    fn test_animated_element_state_access() {
        let listenable = Arc::new(ChangeNotifier::new());
        let view = TestAnimatedView {
            listenable: listenable.clone(),
            value: 42,
        };

        let element = AnimatedElement::new(&view, AnimationBehavior::new(&view));

        // Access state through behavior
        let state = element.behavior().state();
        assert_eq!(state.build_count.load(Ordering::SeqCst), 0);
    }
}
