//! `StatefulViewWrapper` - Wrapper that holds a `StatefulView` and its state
//!
//! Implements `ViewObject` for `StatefulView` types.

use std::any::Any;

use crate::traits::StatefulView;
use crate::{BuildContext, IntoView, ViewMode, ViewObject};

/// Wrapper for `StatefulView` that implements `ViewObject`
///
/// Stores both the view configuration and the mutable state.
pub struct StatefulViewWrapper<V: StatefulView> {
    /// The view configuration
    view: V,

    /// The mutable state (created lazily)
    state: Option<V::State>,
}

impl<V: StatefulView> StatefulViewWrapper<V> {
    /// Create a new wrapper
    pub fn new(view: V) -> Self {
        Self { view, state: None }
    }

    /// Get reference to view
    pub fn view(&self) -> &V {
        &self.view
    }

    /// Get reference to state (if initialized)
    pub fn state(&self) -> Option<&V::State> {
        self.state.as_ref()
    }

    /// Get mutable reference to state (if initialized)
    pub fn state_mut(&mut self) -> Option<&mut V::State> {
        self.state.as_mut()
    }
}

impl<V: StatefulView> std::fmt::Debug for StatefulViewWrapper<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatefulViewWrapper")
            .field("has_state", &self.state.is_some())
            .finish()
    }
}

impl<V: StatefulView> ViewObject for StatefulViewWrapper<V> {
    fn mode(&self) -> ViewMode {
        ViewMode::Stateful
    }

    fn build(&mut self, ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        // Initialize state on first build
        if self.state.is_none() {
            self.state = Some(self.view.create_state());
        }

        // Build with state
        if let Some(ref mut state) = self.state {
            Some(self.view.build(state, ctx).into_view())
        } else {
            None
        }
    }

    fn init(&mut self, ctx: &dyn BuildContext) {
        // Call init_state after state is created and element is mounted
        if let Some(ref mut state) = self.state {
            self.view.init_state(state, ctx);
        }
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        // Delegate to StatefulView
        if let Some(ref mut state) = self.state {
            self.view.did_change_dependencies(state, ctx);
        }
    }

    fn did_update(&mut self, old_view: &dyn Any, _ctx: &dyn BuildContext) {
        // Notify view of configuration change
        if let Some(old) = old_view.downcast_ref::<V>() {
            if let Some(ref mut state) = self.state {
                self.view.did_update_view(state, old);
            }
        }
    }

    fn deactivate(&mut self, ctx: &dyn BuildContext) {
        // Delegate to StatefulView - called when element is temporarily removed
        if let Some(ref mut state) = self.state {
            self.view.deactivate(state, ctx);
        }
    }

    fn activate(&mut self, ctx: &dyn BuildContext) {
        // Delegate to StatefulView - called when element is reactivated
        if let Some(ref mut state) = self.state {
            self.view.activate(state, ctx);
        }
    }

    fn dispose(&mut self, ctx: &dyn BuildContext) {
        // Call dispose on StatefulView before cleaning up state
        if let Some(ref mut state) = self.state {
            self.view.dispose(state, ctx);
        }
        // Clean up state
        self.state = None;
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

// ============================================================================
// IntoView IMPLEMENTATION
// ============================================================================

/// Helper struct to disambiguate `StatefulView` from `StatelessView`
///
/// Use `Stateful(my_view)` to create a stateful view object.
#[derive(Debug)]
pub struct Stateful<V: StatefulView>(pub V);

impl<V: StatefulView> IntoView for Stateful<V> {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(StatefulViewWrapper::new(self.0))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MockBuildContext;
    use flui_foundation::ElementId;

    struct TestStatefulView {
        initial: i32,
    }

    struct TestState {
        count: i32,
    }

    // Note: ViewState is automatically implemented via blanket impl

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

    impl StatefulView for TestStatefulView {
        type State = TestState;

        fn create_state(&self) -> Self::State {
            TestState {
                count: self.initial,
            }
        }

        fn build(&self, state: &mut Self::State, _ctx: &dyn BuildContext) -> impl IntoView {
            state.count += 1;
            EmptyIntoView
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = StatefulViewWrapper::new(TestStatefulView { initial: 10 });
        assert!(wrapper.state.is_none()); // State not created until build
        assert_eq!(wrapper.mode(), ViewMode::Stateful);
    }

    #[test]
    fn test_into_view() {
        let view = TestStatefulView { initial: 10 };
        let view_obj = Stateful(view).into_view();
        assert_eq!(view_obj.mode(), ViewMode::Stateful);
    }

    #[test]
    fn test_state_persistence() {
        let mut wrapper = StatefulViewWrapper::new(TestStatefulView { initial: 10 });
        let ctx = MockBuildContext::new(ElementId::new(1));

        // First build creates state
        wrapper.build(&ctx);
        assert!(wrapper.state.is_some());
        assert_eq!(wrapper.state().unwrap().count, 11);

        // Second build reuses state
        wrapper.build(&ctx);
        assert_eq!(wrapper.state().unwrap().count, 12);
    }

    // ========== LIFECYCLE TESTS ==========

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// State that tracks lifecycle method calls
    struct LifecycleTrackingState {
        init_count: Arc<AtomicUsize>,
        dependencies_count: Arc<AtomicUsize>,
        deactivate_count: Arc<AtomicUsize>,
        activate_count: Arc<AtomicUsize>,
        dispose_count: Arc<AtomicUsize>,
    }

    struct LifecycleTrackingView {
        init_count: Arc<AtomicUsize>,
        dependencies_count: Arc<AtomicUsize>,
        deactivate_count: Arc<AtomicUsize>,
        activate_count: Arc<AtomicUsize>,
        dispose_count: Arc<AtomicUsize>,
    }

    impl LifecycleTrackingView {
        fn new() -> Self {
            Self {
                init_count: Arc::new(AtomicUsize::new(0)),
                dependencies_count: Arc::new(AtomicUsize::new(0)),
                deactivate_count: Arc::new(AtomicUsize::new(0)),
                activate_count: Arc::new(AtomicUsize::new(0)),
                dispose_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl StatefulView for LifecycleTrackingView {
        type State = LifecycleTrackingState;

        fn create_state(&self) -> Self::State {
            LifecycleTrackingState {
                init_count: Arc::clone(&self.init_count),
                dependencies_count: Arc::clone(&self.dependencies_count),
                deactivate_count: Arc::clone(&self.deactivate_count),
                activate_count: Arc::clone(&self.activate_count),
                dispose_count: Arc::clone(&self.dispose_count),
            }
        }

        fn init_state(&self, state: &mut Self::State, _ctx: &dyn BuildContext) {
            state.init_count.fetch_add(1, Ordering::SeqCst);
        }

        fn did_change_dependencies(&self, state: &mut Self::State, _ctx: &dyn BuildContext) {
            state.dependencies_count.fetch_add(1, Ordering::SeqCst);
        }

        fn build(&self, _state: &mut Self::State, _ctx: &dyn BuildContext) -> impl IntoView {
            EmptyIntoView
        }

        fn deactivate(&self, state: &mut Self::State, _ctx: &dyn BuildContext) {
            state.deactivate_count.fetch_add(1, Ordering::SeqCst);
        }

        fn activate(&self, state: &mut Self::State, _ctx: &dyn BuildContext) {
            state.activate_count.fetch_add(1, Ordering::SeqCst);
        }

        fn dispose(&self, state: &mut Self::State, _ctx: &dyn BuildContext) {
            state.dispose_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_lifecycle_init_state() {
        let view = LifecycleTrackingView::new();
        let init_count = Arc::clone(&view.init_count);

        let mut wrapper = StatefulViewWrapper::new(view);
        let ctx = MockBuildContext::new(ElementId::new(1));

        // Build creates state
        wrapper.build(&ctx);
        assert_eq!(init_count.load(Ordering::SeqCst), 0);

        // init() calls init_state
        wrapper.init(&ctx);
        assert_eq!(init_count.load(Ordering::SeqCst), 1);

        // Calling init again increments counter (framework should only call once)
        wrapper.init(&ctx);
        assert_eq!(init_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_lifecycle_did_change_dependencies() {
        let view = LifecycleTrackingView::new();
        let deps_count = Arc::clone(&view.dependencies_count);

        let mut wrapper = StatefulViewWrapper::new(view);
        let ctx = MockBuildContext::new(ElementId::new(1));

        wrapper.build(&ctx);
        assert_eq!(deps_count.load(Ordering::SeqCst), 0);

        wrapper.did_change_dependencies(&ctx);
        assert_eq!(deps_count.load(Ordering::SeqCst), 1);

        wrapper.did_change_dependencies(&ctx);
        assert_eq!(deps_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_lifecycle_deactivate_activate() {
        let view = LifecycleTrackingView::new();
        let deactivate_count = Arc::clone(&view.deactivate_count);
        let activate_count = Arc::clone(&view.activate_count);

        let mut wrapper = StatefulViewWrapper::new(view);
        let ctx = MockBuildContext::new(ElementId::new(1));

        wrapper.build(&ctx);

        // Deactivate
        wrapper.deactivate(&ctx);
        assert_eq!(deactivate_count.load(Ordering::SeqCst), 1);
        assert_eq!(activate_count.load(Ordering::SeqCst), 0);

        // Activate
        wrapper.activate(&ctx);
        assert_eq!(deactivate_count.load(Ordering::SeqCst), 1);
        assert_eq!(activate_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_lifecycle_dispose() {
        let view = LifecycleTrackingView::new();
        let dispose_count = Arc::clone(&view.dispose_count);

        let mut wrapper = StatefulViewWrapper::new(view);
        let ctx = MockBuildContext::new(ElementId::new(1));

        wrapper.build(&ctx);
        assert!(wrapper.state().is_some());

        // Dispose calls dispose on view and clears state
        wrapper.dispose(&ctx);
        assert_eq!(dispose_count.load(Ordering::SeqCst), 1);
        assert!(wrapper.state().is_none());
    }

    #[test]
    fn test_full_lifecycle_sequence() {
        // Test the full Flutter-like lifecycle sequence
        let view = LifecycleTrackingView::new();
        let init_count = Arc::clone(&view.init_count);
        let deps_count = Arc::clone(&view.dependencies_count);
        let deactivate_count = Arc::clone(&view.deactivate_count);
        let activate_count = Arc::clone(&view.activate_count);
        let dispose_count = Arc::clone(&view.dispose_count);

        let mut wrapper = StatefulViewWrapper::new(view);
        let ctx = MockBuildContext::new(ElementId::new(1));

        // 1. create_state (via build)
        wrapper.build(&ctx);

        // 2. initState
        wrapper.init(&ctx);
        assert_eq!(init_count.load(Ordering::SeqCst), 1);

        // 3. didChangeDependencies (first call after initState)
        wrapper.did_change_dependencies(&ctx);
        assert_eq!(deps_count.load(Ordering::SeqCst), 1);

        // 4. build again (on state change)
        wrapper.build(&ctx);

        // 5. deactivate (temporarily removed)
        wrapper.deactivate(&ctx);
        assert_eq!(deactivate_count.load(Ordering::SeqCst), 1);

        // 6. activate (reinserted)
        wrapper.activate(&ctx);
        assert_eq!(activate_count.load(Ordering::SeqCst), 1);

        // 7. dispose (permanently removed)
        wrapper.dispose(&ctx);
        assert_eq!(dispose_count.load(Ordering::SeqCst), 1);
    }
}
