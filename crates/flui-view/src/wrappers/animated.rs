//! AnimatedViewWrapper - Wrapper that holds an AnimatedView
//!
//! Implements ViewObject for AnimatedView types.

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use flui_element::{Element, IntoElement};
use flui_foundation::RenderStateAccessor;

use crate::context::BuildContext;
use crate::object::ViewObject;
use crate::protocol::ViewMode;
use crate::traits::{AnimatedView, Listenable};

/// Wrapper for AnimatedView that implements ViewObject
///
/// Animated views subscribe to a Listenable and rebuild automatically
/// when the listenable notifies (e.g., animation tick).
pub struct AnimatedViewWrapper<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    /// The animated view
    view: V,

    /// Flag indicating listener is attached
    listening: Arc<AtomicBool>,

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

    /// Start listening to the listenable
    fn start_listening(&self) {
        if self.listening.swap(true, Ordering::SeqCst) {
            return; // Already listening
        }

        // Subscribe to changes
        // Note: In a real implementation, we'd need to capture the element ID
        // and schedule a rebuild when the listenable notifies
        let listening = self.listening.clone();
        self.view.listenable().add_listener(Box::new(move || {
            if listening.load(Ordering::Relaxed) {
                // TODO: Schedule rebuild via BuildContext or BuildOwner
                tracing::trace!("Animation tick - would schedule rebuild");
            }
        }));
    }

    /// Stop listening to the listenable
    fn stop_listening(&self) {
        if !self.listening.swap(false, Ordering::SeqCst) {
            return; // Not listening
        }

        self.view.listenable().remove_listener();
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
    fn mode(&self) -> ViewMode {
        ViewMode::Animated
    }

    fn build(&mut self, ctx: &dyn BuildContext) -> Element {
        // Call animation tick hook
        self.view.on_animation_tick(ctx);

        // Build the child
        self.view.build(ctx).into_element()
    }

    fn init(&mut self, ctx: &dyn BuildContext) {
        self.view.init(ctx);
        self.start_listening();
    }

    fn deactivate(&mut self, _ctx: &dyn BuildContext) {
        self.stop_listening();
    }

    fn dispose(&mut self, ctx: &dyn BuildContext) {
        self.stop_listening();
        self.view.dispose(ctx);
    }

    fn debug_name(&self) -> &'static str {
        std::any::type_name::<V>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// RenderStateAccessor - Non-render wrapper uses defaults (returns None)
impl<V, L> RenderStateAccessor for AnimatedViewWrapper<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

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
// IntoElement IMPLEMENTATION
// ============================================================================

/// Helper struct to disambiguate AnimatedView from other view types
///
/// Use `Animated(my_view)` to create an animated element.
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

impl<V, L> IntoElement for Animated<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    fn into_element(self) -> Element {
        let wrapper = AnimatedViewWrapper::<V, L>::new(self.0);
        Element::with_mode(wrapper, ViewMode::Animated)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    /// Simple test animation controller
    struct TestAnimation {
        value: f32,
        callback: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    }

    impl TestAnimation {
        fn new(value: f32) -> Self {
            Self {
                value,
                callback: Mutex::new(None),
            }
        }

        fn value(&self) -> f32 {
            self.value
        }
    }

    impl Listenable for TestAnimation {
        fn add_listener(&self, callback: Box<dyn Fn() + Send + Sync>) {
            *self.callback.lock() = Some(callback);
        }

        fn remove_listener(&self) {
            *self.callback.lock() = None;
        }
    }

    struct TestAnimatedView {
        animation: TestAnimation,
    }

    impl AnimatedView<TestAnimation> for TestAnimatedView {
        fn build(&mut self, _ctx: &dyn BuildContext) -> impl IntoElement {
            // Would use animation.value() to build UI
            ()
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
    fn test_into_element() {
        let view = TestAnimatedView {
            animation: TestAnimation::new(0.5),
        };
        let element = Animated::new(view).into_element();
        assert!(element.has_view_object());
    }
}
