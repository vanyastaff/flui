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
    use std::any::TypeId;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use flui_foundation::{ChangeNotifier, ElementId};

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

    // A true leaf view (its element builds NO children), so a tree-driven
    // build terminates — unlike `DummyView`, whose `MinimalView` rebuilds
    // `DummyView` forever (fine for the standalone tests, fatal under a real
    // `build_scope` drain).
    #[derive(Clone)]
    struct LeafView;

    impl View for LeafView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(LeafElement)
        }
    }

    struct LeafElement;

    impl ElementBase for LeafElement {
        fn view_type_id(&self) -> TypeId {
            TypeId::of::<LeafView>()
        }
        fn lifecycle(&self) -> Lifecycle {
            Lifecycle::Active
        }
        fn update(&mut self, _: &dyn View, _: &mut crate::ElementOwner<'_>) {}
        fn mark_needs_build(&mut self) {}
        fn build_into_views(&mut self, _: &mut crate::ElementOwner<'_>) -> Vec<Box<dyn View>> {
            Vec::new()
        }
        fn mount(&mut self, _: Option<ElementId>, _: usize, _: &mut crate::ElementOwner<'_>) {}
        fn deactivate(&mut self) {}
        fn activate(&mut self) {}
        fn unmount(&mut self, _: &mut crate::ElementOwner<'_>) {}
        fn depth(&self) -> usize {
            0
        }
    }

    // An AnimatedView whose state's build count is observable from the test
    // (the shared `Arc<AtomicUsize>` is threaded in by the view, not minted in
    // `create_state`), so a tree-driven rebuild is detectable.
    #[derive(Clone)]
    struct CountingAnimatedView {
        listenable: Arc<ChangeNotifier>,
        build_count: Arc<AtomicUsize>,
    }

    struct CountingAnimatedState {
        build_count: Arc<AtomicUsize>,
    }

    impl ViewState<CountingAnimatedView> for CountingAnimatedState {
        fn build(&self, _view: &CountingAnimatedView, _ctx: &dyn BuildContext) -> impl IntoView {
            self.build_count.fetch_add(1, Ordering::SeqCst);
            LeafView.boxed()
        }
    }

    impl StatefulView for CountingAnimatedView {
        type State = CountingAnimatedState;

        fn create_state(&self) -> Self::State {
            CountingAnimatedState {
                build_count: Arc::clone(&self.build_count),
            }
        }
    }

    impl AnimatedView for CountingAnimatedView {
        fn listenable(&self) -> Arc<dyn Listenable> {
            self.listenable.clone() as Arc<dyn Listenable>
        }
    }

    impl View for CountingAnimatedView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(AnimatedElement::new(self, AnimatedBehavior::new(self)))
        }
    }

    // A `StatelessView` wrapper so the animated view can be mounted at tree
    // depth >= 1 (its own `ElementCore::depth` field is the sibling slot, 0).
    #[derive(Clone)]
    struct Wrapper {
        child: CountingAnimatedView,
    }

    impl crate::view::StatelessView for Wrapper {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.child.clone()
        }
    }

    impl View for Wrapper {
        fn create_element(&self) -> Box<dyn ElementBase> {
            use crate::{element::StatelessBehavior, view::StatelessElement};
            Box::new(StatelessElement::new(self, StatelessBehavior::new()))
        }
    }

    /// End-to-end: a listenable change (an animation tick) on a tree-mounted
    /// `AnimatedView` must schedule a rebuild that the NEXT `build_scope`
    /// actually runs — not merely flip the element's dirty flag.
    ///
    /// Before the external-build-inbox wiring, the mark-dirty callback only set
    /// the `Arc<AtomicBool>` dirty flag; the element was never pushed onto the
    /// heap `build_scope` drains, so its `ViewState::build` never re-ran. This
    /// test is RED without that wiring (`build_count` would not advance on
    /// notify).
    #[test]
    fn animation_notify_schedules_rebuild_through_build_scope() {
        let listenable = Arc::new(ChangeNotifier::new());
        let build_count = Arc::new(AtomicUsize::new(0));
        let view = CountingAnimatedView {
            listenable: listenable.clone(),
            build_count: Arc::clone(&build_count),
        };

        let mut tree = crate::ElementTree::new();
        let mut owner = crate::BuildOwner::new();
        let root = tree.mount_root(&view, &mut owner.element_owner_mut());

        // Initial build.
        owner.schedule_build_for(root, 0);
        owner.build_scope(&mut tree);
        let after_initial = build_count.load(Ordering::SeqCst);
        assert!(
            after_initial >= 1,
            "the animated view should build at least once on mount",
        );

        // A listenable change between frames (an animation tick) fires the
        // mark-dirty callback, which must enqueue this element for the next
        // build_scope.
        listenable.notify_listeners();
        owner.build_scope(&mut tree);

        let after_notify = build_count.load(Ordering::SeqCst);
        assert!(
            after_notify > after_initial,
            "notify_listeners must schedule a rebuild that build_scope runs \
             (before={after_initial}, after={after_notify})",
        );
    }

    /// The same end-to-end rebuild, but with the `AnimatedView` mounted at tree
    /// depth >= 1 (under a `Wrapper`). The dirty-heap depth key must be the
    /// element's TREE depth, looked up from its node at drain time — NOT the
    /// `ElementCore::depth` slot index (always 0 for a single child), which
    /// would mis-order the nested element as the root. This guards against a
    /// regression to capturing the slot in the mark-dirty callback.
    #[test]
    fn nested_animation_notify_reschedules_at_correct_tree_depth() {
        let listenable = Arc::new(ChangeNotifier::new());
        let build_count = Arc::new(AtomicUsize::new(0));
        let view = Wrapper {
            child: CountingAnimatedView {
                listenable: listenable.clone(),
                build_count: Arc::clone(&build_count),
            },
        };

        let mut tree = crate::ElementTree::new();
        let mut owner = crate::BuildOwner::new();
        let root = tree.mount_root(&view, &mut owner.element_owner_mut());

        owner.schedule_build_for(root, 0);
        owner.build_scope(&mut tree);
        let after_initial = build_count.load(Ordering::SeqCst);
        assert!(
            after_initial >= 1,
            "the nested animated view builds on mount"
        );

        listenable.notify_listeners();
        owner.build_scope(&mut tree);

        let after_notify = build_count.load(Ordering::SeqCst);
        assert!(
            after_notify > after_initial,
            "a tick on a depth>=1 animated view must reschedule it \
             (before={after_initial}, after={after_notify})",
        );
    }
}
