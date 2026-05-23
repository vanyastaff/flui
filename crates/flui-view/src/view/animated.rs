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
            fn create_element(&self) -> Box<dyn $crate::ElementBase> {
                use $crate::element::AnimatedBehavior;
                Box::new($crate::AnimatedElement::new(
                    self,
                    AnimatedBehavior::new(self),
                ))
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use flui_foundation::ChangeNotifier;

    use super::*;
    use crate::{
        context::BuildContext,
        element::{AnimatedBehavior, Lifecycle},
        view::{AnimatedElement, ElementBase, IntoView, View, ViewExt, ViewState},
    };

    // Test animated view
    #[derive(Clone)]
    struct TestAnimatedView {
        listenable: Arc<ChangeNotifier>,
        #[expect(dead_code, reason = "exercised only by the derived Clone impl")]
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
            use crate::{element::StatelessBehavior, view::StatelessElement};

            #[derive(Clone)]
            struct MinimalView;
            impl StatelessView for MinimalView {
                fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
                    DummyView.boxed()
                }
            }
            impl View for MinimalView {
                fn create_element(&self) -> Box<dyn super::super::view::ElementBase> {
                    Box::new(StatelessElement::new(self, StatelessBehavior::new()))
                }
            }

            Box::new(StatelessElement::new(
                &MinimalView,
                StatelessBehavior::new(),
            ))
        }
    }

    impl ViewState<TestAnimatedView> for TestAnimatedState {
        fn build(&self, _view: &TestAnimatedView, _ctx: &dyn BuildContext) -> impl IntoView {
            self.build_count.fetch_add(1, Ordering::SeqCst);
            // Return a dummy view for testing
            DummyView.boxed()
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

    // Every view-trait type also implements `View` in real code (the
    // `<Kind>View` traits do not require it structurally, but every
    // concrete view supplies it). The unified `Element`'s `ElementBase`
    // impl now demands `V: View`, so this fixture must spell it too.
    impl View for TestAnimatedView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(AnimatedElement::new(self, AnimatedBehavior::new(self)))
        }
    }

    #[test]
    fn test_animated_element_creation() {
        let listenable = Arc::new(ChangeNotifier::new());
        let view = TestAnimatedView {
            listenable: listenable.clone(),
            value: 42,
        };

        let element = AnimatedElement::new(&view, AnimatedBehavior::new(&view));
        assert_eq!(element.core().lifecycle(), Lifecycle::Initial);
    }

    #[test]
    fn test_animated_element_listener_subscription() {
        let listenable = Arc::new(ChangeNotifier::new());
        let view = TestAnimatedView {
            listenable: listenable.clone(),
            value: 42,
        };

        let mut element = AnimatedElement::new(&view, AnimatedBehavior::new(&view));

        // Before mount, no listener
        assert_eq!(listenable.len(), 0);

        // Mount the element
        let mut owner = crate::BuildOwner::new();
        element.mount(None, 0, &mut owner.element_owner_mut());

        // After mount, listener should be subscribed
        assert_eq!(listenable.len(), 1);

        // Unmount
        element.unmount(&mut owner.element_owner_mut());

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

        let mut element = AnimatedElement::new(&view, AnimatedBehavior::new(&view));

        // Mount the element
        let mut owner = crate::BuildOwner::new();
        element.mount(None, 0, &mut owner.element_owner_mut());

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
        element.unmount(&mut owner.element_owner_mut());
    }

    #[test]
    fn test_animated_element_state_access() {
        let listenable = Arc::new(ChangeNotifier::new());
        let view = TestAnimatedView {
            listenable: listenable.clone(),
            value: 42,
        };

        let element = AnimatedElement::new(&view, AnimatedBehavior::new(&view));

        // Access state through behavior
        let state = element.behavior().state();
        assert_eq!(state.build_count.load(Ordering::SeqCst), 0);
    }
}
