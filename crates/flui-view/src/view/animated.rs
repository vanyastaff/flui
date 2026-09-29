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

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use flui_foundation::ChangeNotifier;
    use flui_objects::RenderSizedBox;
    use flui_rendering::protocol::BoxProtocol;

    use super::*;
    use crate::{
        context::BuildContext,
        view::{IntoView, View, ViewExt, ViewState},
    };

    // A true leaf view (its element builds NO children), so a tree-driven
    // build terminates.
    #[derive(Clone)]
    struct LeafView;

    impl crate::RenderView for LeafView {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for LeafView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
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
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::animated(self)
        }
    }

    // A `StatelessView` wrapper so the animated view can be mounted at tree
    // depth >= 1 while its sibling slot stays 0.
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
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateless(self)
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
        // Production shape (the bootstrap idiom): a render root carries the
        // animated view, so `LeafView`'s render object mounts with a render
        // parent instead of orphaning under a render-less owner-carrying root.
        let render_root = crate::view::RootRenderView::new(view, 800.0, 600.0);
        let root = tree.mount_root_with_pipeline_owner(
            &render_root,
            Some(flui_rendering::pipeline::PipelineCell::new(
                flui_rendering::pipeline::PipelineOwner::new(),
            )),
            &mut owner.element_owner_mut(),
        );

        // Initial build.
        owner.schedule_build_for(root, 0, crate::RebuildReason::InitialMount);
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
    /// element's TREE depth, looked up from its node at drain time — NOT its
    /// sibling slot (always 0 for a single child), which would mis-order the
    /// nested element as the root. This guards against a regression to
    /// capturing the slot in the mark-dirty callback.
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
        // Same bootstrap idiom as the direct-mount case above: the render root
        // carries the wrapper, so the nested animated view's render child
        // mounts with a render parent. The animated element now sits at tree
        // depth 2 (render root → Wrapper → animated) — still >= 1, which is
        // what this test's depth-key guard needs.
        let render_root = crate::view::RootRenderView::new(view, 800.0, 600.0);
        let root = tree.mount_root_with_pipeline_owner(
            &render_root,
            Some(flui_rendering::pipeline::PipelineCell::new(
                flui_rendering::pipeline::PipelineOwner::new(),
            )),
            &mut owner.element_owner_mut(),
        );

        owner.schedule_build_for(root, 0, crate::RebuildReason::InitialMount);
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
