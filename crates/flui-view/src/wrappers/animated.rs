//! `AnimatedViewWrapper` - Wrapper that holds an `AnimatedView`
//!
//! Implements `ViewObject` for `AnimatedView` types.

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use flui_foundation::ListenerId;

use crate::handle::ViewConfig;
use crate::traits::{AnimatedView, Listenable};
use crate::{BuildContext, IntoView, IntoViewConfig, ViewMode, ViewObject};

/// Type alias for rebuild callback (wrapped in Arc for clonability)
type RebuildCallback = Arc<dyn Fn() + Send + Sync>;

/// Wrapper for `AnimatedView` that implements `ViewObject`
///
/// Animated views subscribe to a Listenable and rebuild automatically
/// when the listenable notifies (e.g., animation tick).
///
/// # Rebuild Mechanism
///
/// During `init()`, this wrapper obtains a rebuild callback from `BuildContext`
/// using `create_rebuild_callback()`. When the animation ticks, this callback
/// is invoked to schedule a rebuild of the element.
///
/// The callback is thread-safe and can be called from any thread, making it
/// suitable for async animation controllers.
///
/// # Performance
///
/// Uses `Arc<AtomicBool>` for lock-free listener state management and caches
/// the rebuild callback for O(1) rebuild scheduling on each animation tick.
pub struct AnimatedViewWrapper<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    /// The animated view
    view: V,

    /// Flag indicating listener is attached
    listening: Arc<AtomicBool>,

    /// The listener ID for removal
    listener_id: Option<ListenerId>,

    /// Callback to trigger rebuild (captured from BuildContext during init)
    rebuild_callback: Option<RebuildCallback>,

    /// Type marker for the listenable
    _marker: std::marker::PhantomData<L>,
}

impl<V, L> AnimatedViewWrapper<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    /// Create a new wrapper
    pub fn new(view: V) -> Self {
        Self {
            view,
            listening: Arc::new(AtomicBool::new(false)),
            listener_id: None,
            rebuild_callback: None,
            _marker: std::marker::PhantomData,
        }
    }

    /// Get reference to view
    pub fn view(&self) -> &V {
        &self.view
    }

    /// Get mutable reference to view
    pub fn view_mut(&mut self) -> &mut V {
        &mut self.view
    }

    /// Get the listenable
    pub fn listenable(&self) -> &L {
        self.view.listenable()
    }

    /// Check if currently listening to animation
    pub fn is_listening(&self) -> bool {
        self.listening.load(Ordering::Relaxed)
    }

    /// Extract the inner view, consuming the wrapper.
    pub fn into_inner(self) -> V
    where
        V: Clone,
    {
        self.view.clone()
    }

    /// Create a rebuild callback from BuildContext
    ///
    /// Uses the `BuildContext::create_rebuild_callback()` method to obtain
    /// a callback that can trigger rebuilds from async contexts.
    #[inline]
    fn create_rebuild_callback(ctx: &dyn BuildContext) -> Option<RebuildCallback> {
        // Wrap the Box in Arc for clonability
        Some(Arc::from(ctx.create_rebuild_callback()))
    }

    /// Start listening to the listenable
    fn start_listening(&mut self) {
        if self.listening.swap(true, Ordering::SeqCst) {
            return; // Already listening
        }

        // Subscribe to changes
        let listening = self.listening.clone();
        let rebuild_callback = self.rebuild_callback.clone();

        let id = self.view.listenable().add_listener(Box::new(move || {
            if listening.load(Ordering::Relaxed) {
                // Trigger rebuild if callback is available
                if let Some(ref callback) = rebuild_callback {
                    tracing::trace!("Animation tick - scheduling rebuild");
                    callback();
                } else {
                    tracing::trace!(
                        "Animation tick - no rebuild callback available (test context?)"
                    );
                }
            }
        }));
        self.listener_id = Some(id);
    }

    /// Stop listening to the listenable
    fn stop_listening(&mut self) {
        if !self.listening.swap(false, Ordering::SeqCst) {
            return; // Not listening
        }

        if let Some(id) = self.listener_id.take() {
            self.view.listenable().remove_listener(id);
        }
    }
}

impl<V, L> std::fmt::Debug for AnimatedViewWrapper<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimatedViewWrapper")
            .field("listening", &self.is_listening())
            .finish()
    }
}

impl<V, L> ViewObject for AnimatedViewWrapper<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    #[inline]
    fn mode(&self) -> ViewMode {
        ViewMode::Animated
    }

    #[inline]
    fn build(&mut self, ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        // Call animation tick hook
        self.view.on_animation_tick(ctx);

        // Build the child
        Some(self.view.build(ctx).into_view())
    }

    fn init(&mut self, ctx: &dyn BuildContext) {
        self.view.init(ctx);

        // Try to set up rebuild callback by accessing PipelineBuildContext
        // This requires flui-pipeline to be available at runtime
        self.rebuild_callback = Self::create_rebuild_callback(ctx);

        self.start_listening();
    }

    #[inline]
    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        self.view.did_change_dependencies(ctx);
    }

    #[inline]
    fn deactivate(&mut self, ctx: &dyn BuildContext) {
        self.stop_listening();
        self.view.deactivate(ctx);
    }

    #[inline]
    fn activate(&mut self, ctx: &dyn BuildContext) {
        self.view.activate(ctx);
        self.start_listening();
    }

    #[inline]
    fn dispose(&mut self, ctx: &dyn BuildContext) {
        self.stop_listening();
        self.view.dispose(ctx);
    }

    #[inline]
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<V>()
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl<V, L> Drop for AnimatedViewWrapper<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    fn drop(&mut self) {
        self.stop_listening();
    }
}

// ============================================================================
// IntoView IMPLEMENTATION
// ============================================================================

/// Helper struct to disambiguate `AnimatedView` from other view types
///
/// Use `Animated(my_view)` to create an animated view object.
#[derive(Debug)]
pub struct Animated<V, L>(pub V, std::marker::PhantomData<L>)
where
    V: AnimatedView<L>,
    L: Listenable;

impl<V, L> Animated<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    /// Create a new Animated wrapper
    pub fn new(view: V) -> Self {
        Self(view, std::marker::PhantomData)
    }
}

impl<V, L> IntoView for Animated<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(AnimatedViewWrapper::<V, L>::new(self.0))
    }
}

// ============================================================================
// IntoViewConfig IMPLEMENTATION
// ============================================================================

/// Implementation for `AnimatedViewWrapper`.
///
/// This allows animated views to be converted to `ViewConfig` when wrapped:
///
/// ```rust,ignore
/// use flui_view::{Animated, AnimatedView, IntoViewConfig};
///
/// let config = Animated::new(MyAnimatedView { ... }).into_view_config();
/// ```
impl<V, L> IntoViewConfig for AnimatedViewWrapper<V, L>
where
    V: AnimatedView<L> + Clone + Send + Sync + 'static,
    L: Listenable,
{
    fn into_view_config(self) -> ViewConfig {
        let view = self.view.clone();
        ViewConfig::new_with_factory(view, |v: &V| {
            Box::new(AnimatedViewWrapper::<V, L>::new(v.clone()))
        })
    }
}

/// Implementation for `Animated` helper.
///
/// ```rust,ignore
/// use flui_view::{Animated, IntoViewConfig};
///
/// let config = Animated::new(MyAnimatedView { ... }).into_view_config();
/// ```
impl<V, L> IntoViewConfig for Animated<V, L>
where
    V: AnimatedView<L> + Clone + Send + Sync + 'static,
    L: Listenable,
{
    fn into_view_config(self) -> ViewConfig {
        ViewConfig::new_with_factory(self.0, |v: &V| {
            Box::new(AnimatedViewWrapper::<V, L>::new(v.clone()))
        })
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MockBuildContext;
    use flui_foundation::{ElementId, ListenerId};
    use parking_lot::Mutex;

    // Helper for tests - represents an empty view
    struct EmptyIntoView;

    impl IntoView for EmptyIntoView {
        fn into_view(self) -> Box<dyn ViewObject> {
            Box::new(EmptyViewObject)
        }
    }

    struct EmptyViewObject;

    impl ViewObject for EmptyViewObject {
        fn mode(&self) -> ViewMode {
            ViewMode::Stateless
        }

        fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
            None
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    /// Simple test animation controller
    struct TestAnimation {
        _value: f32,
        listeners: Mutex<Vec<(ListenerId, Box<dyn Fn() + Send + Sync>)>>,
        next_id: Mutex<usize>,
    }

    impl TestAnimation {
        fn new(value: f32) -> Self {
            Self {
                _value: value,
                listeners: Mutex::new(Vec::new()),
                next_id: Mutex::new(0),
            }
        }
    }

    impl Listenable for TestAnimation {
        fn add_listener(&self, callback: Box<dyn Fn() + Send + Sync>) -> ListenerId {
            let mut next_id = self.next_id.lock();
            let id = ListenerId::new(*next_id);
            *next_id += 1;
            self.listeners.lock().push((id, callback));
            id
        }

        fn remove_listener(&self, id: ListenerId) {
            self.listeners.lock().retain(|(lid, _)| *lid != id);
        }
    }

    struct TestAnimatedView {
        animation: TestAnimation,
    }

    impl AnimatedView<TestAnimation> for TestAnimatedView {
        fn build(&mut self, _ctx: &dyn BuildContext) -> impl IntoView {
            // Would use animation.value() to build UI
            EmptyIntoView
        }

        fn listenable(&self) -> &TestAnimation {
            &self.animation
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = AnimatedViewWrapper::new(TestAnimatedView {
            animation: TestAnimation::new(0.5),
        });
        assert_eq!(wrapper.mode(), ViewMode::Animated);
        assert!(!wrapper.is_listening());
    }

    #[test]
    fn test_into_view() {
        let view = TestAnimatedView {
            animation: TestAnimation::new(0.5),
        };
        let view_obj = Animated::new(view).into_view();
        assert_eq!(view_obj.mode(), ViewMode::Animated);
    }

    #[test]
    fn test_build() {
        let mut wrapper = AnimatedViewWrapper::new(TestAnimatedView {
            animation: TestAnimation::new(0.5),
        });
        let ctx = MockBuildContext::new(ElementId::new(1));

        let result = wrapper.build(&ctx);
        assert!(result.is_some());
    }
}
