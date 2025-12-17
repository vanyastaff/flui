//! StatefulView - Views with persistent mutable state.
//!
//! StatefulViews maintain state that persists across rebuilds.
//! The state is held by the Element, not the View itself.

use super::view::{ElementBase, View};
use crate::context::BuildContext;

/// A View that has persistent mutable state.
///
/// StatefulViews separate configuration (the View) from mutable state
/// (the `ViewState`). The View is immutable and recreated each build,
/// while the State persists in the Element.
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `StatefulWidget` + `State<T>`:
///
/// ```dart
/// class Counter extends StatefulWidget {
///   final int initial;
///   Counter({required this.initial});
///
///   @override
///   State<Counter> createState() => _CounterState();
/// }
///
/// class _CounterState extends State<Counter> {
///   late int count;
///
///   @override
///   void initState() {
///     super.initState();
///     count = widget.initial;
///   }
///
///   @override
///   Widget build(BuildContext context) {
///     return Text('Count: $count');
///   }
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{StatefulView, ViewState, BuildContext, IntoView};
///
/// struct Counter {
///     initial: i32,
/// }
///
/// struct CounterState {
///     count: i32,
/// }
///
/// impl StatefulView for Counter {
///     type State = CounterState;
///
///     fn create_state(&self) -> Self::State {
///         CounterState { count: self.initial }
///     }
/// }
///
/// impl ViewState<Counter> for CounterState {
///     fn build(&self, view: &Counter, ctx: &dyn BuildContext) -> Box<dyn View> {
///         Text::new(format!("Count: {}", self.count)).boxed()
///     }
/// }
/// ```
pub trait StatefulView: Send + Sync + 'static + Sized {
    /// The State type for this View.
    type State: ViewState<Self>;

    /// Create the initial state.
    ///
    /// Called once when the Element is first created.
    /// The returned state will persist across View rebuilds.
    fn create_state(&self) -> Self::State;
}

/// Persistent state for a StatefulView.
///
/// ViewState holds mutable data that persists across rebuilds.
/// It receives lifecycle callbacks and builds the child View tree.
///
/// # Lifecycle
///
/// 1. `init_state()` - Called once after creation
/// 2. `did_change_dependencies()` - Called when inherited data changes
/// 3. `build()` - Called to create child Views (may be called many times)
/// 4. `did_update_view()` - Called when parent provides new View config
/// 5. `deactivate()` - Called when temporarily removed
/// 6. `activate()` - Called when re-inserted
/// 7. `dispose()` - Called before permanent removal
pub trait ViewState<V: StatefulView>: Send + Sync + 'static {
    /// Called once after the state is created.
    ///
    /// Use this for one-time initialization that requires BuildContext.
    fn init_state(&mut self, _ctx: &dyn BuildContext) {}

    /// Called when an InheritedView dependency changes.
    ///
    /// This is called after `init_state()` and whenever an InheritedView
    /// that this state depends on (via `ctx.depend_on()`) notifies.
    fn did_change_dependencies(&mut self, _ctx: &dyn BuildContext) {}

    /// Build the child View tree.
    ///
    /// Called whenever the UI needs to be rendered. Can be called many times.
    ///
    /// # Arguments
    ///
    /// * `view` - The current View configuration
    /// * `ctx` - The build context
    fn build(&self, view: &V, ctx: &dyn BuildContext) -> Box<dyn View>;

    /// Called when the View configuration changes.
    ///
    /// The Element receives a new View instance (with potentially different
    /// field values). Use this to react to configuration changes.
    fn did_update_view(&mut self, _old_view: &V) {}

    /// Called when the Element is temporarily removed from the tree.
    ///
    /// The state may be reactivated later.
    fn deactivate(&mut self) {}

    /// Called when the Element is re-inserted after deactivation.
    fn activate(&mut self) {}

    /// Called before the Element is permanently removed.
    ///
    /// Release any resources here. After this, the state will be dropped.
    fn dispose(&mut self) {}
}

/// Implement View for a StatefulView type.
///
/// This macro creates the View implementation for a StatefulView type.
/// Use it after implementing StatefulView:
///
/// ```rust,ignore
/// impl StatefulView for MyCounter {
///     type State = MyCounterState;
///     fn create_state(&self) -> Self::State { ... }
/// }
/// impl_stateful_view!(MyCounter);
/// ```
#[macro_export]
macro_rules! impl_stateful_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> Box<dyn $crate::ElementBase> {
                Box::new($crate::StatefulElement::new(self))
            }
        }
    };
}

// ============================================================================
// StatefulElement
// ============================================================================

use crate::element::Lifecycle;
use flui_foundation::ElementId;
use std::any::TypeId;

/// Element for StatefulViews.
///
/// Manages the lifecycle of a StatefulView and holds its State.
pub struct StatefulElement<V: StatefulView> {
    /// The current View configuration.
    view: V,
    /// The persistent state.
    state: V::State,
    /// Current lifecycle state.
    lifecycle: Lifecycle,
    /// Depth in tree.
    depth: usize,
    /// Child element (built from state.build()).
    child: Option<Box<dyn ElementBase>>,
    /// Whether we need to rebuild.
    dirty: bool,
    /// Whether init_state has been called.
    initialized: bool,
}

impl<V: StatefulView> StatefulElement<V> {
    /// Create a new StatefulElement for the given View.
    pub fn new(view: &V) -> Self
    where
        V: Clone,
    {
        let state = view.create_state();
        Self {
            view: view.clone(),
            state,
            lifecycle: Lifecycle::Initial,
            depth: 0,
            child: None,
            dirty: true,
            initialized: false,
        }
    }

    /// Get a reference to the state.
    pub fn state(&self) -> &V::State {
        &self.state
    }

    /// Get a mutable reference to the state.
    pub fn state_mut(&mut self) -> &mut V::State {
        &mut self.state
    }

    /// Mark as needing rebuild (like Flutter's setState).
    pub fn set_state<F>(&mut self, f: F)
    where
        F: FnOnce(&mut V::State),
    {
        f(&mut self.state);
        self.dirty = true;
    }
}

impl<V: StatefulView> std::fmt::Debug for StatefulElement<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatefulElement")
            .field("lifecycle", &self.lifecycle)
            .field("depth", &self.depth)
            .field("dirty", &self.dirty)
            .field("initialized", &self.initialized)
            .finish_non_exhaustive()
    }
}

impl<V: StatefulView + Clone> ElementBase for StatefulElement<V> {
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<V>()
    }

    fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    fn update(&mut self, new_view: &dyn View) {
        // Use View::as_any() for safe downcasting
        if let Some(v) = new_view.as_any().downcast_ref::<V>() {
            let old_view = std::mem::replace(&mut self.view, v.clone());
            self.state.did_update_view(&old_view);
            self.dirty = true;
        }
    }

    fn mark_needs_build(&mut self) {
        self.dirty = true;
    }

    fn perform_build(&mut self) {
        if !self.dirty || !self.lifecycle.can_build() {
            return;
        }

        // TODO: Create proper BuildContext from element
        // For now, we'll defer the actual build to when we have BuildOwner
        self.dirty = false;
    }

    fn mount(&mut self, _parent: Option<ElementId>, _slot: usize) {
        self.lifecycle = Lifecycle::Active;
        // TODO: Call init_state with proper BuildContext
        self.initialized = true;
        self.dirty = true;
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
        self.state.deactivate();
        if let Some(child) = &mut self.child {
            child.deactivate();
        }
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
        self.state.activate();
        if let Some(child) = &mut self.child {
            child.activate();
        }
    }

    fn unmount(&mut self) {
        self.lifecycle = Lifecycle::Defunct;
        self.state.dispose();
        if let Some(child) = &mut self.child {
            child.unmount();
        }
        self.child = None;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
        let _ = visitor;
    }

    fn depth(&self) -> usize {
        self.depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestCounter {
        initial: i32,
    }

    struct TestCounterState {
        count: i32,
        disposed: bool,
    }

    impl StatefulView for TestCounter {
        type State = TestCounterState;

        fn create_state(&self) -> Self::State {
            TestCounterState {
                count: self.initial,
                disposed: false,
            }
        }
    }

    impl ViewState<TestCounter> for TestCounterState {
        fn build(&self, _view: &TestCounter, _ctx: &dyn BuildContext) -> Box<dyn View> {
            // In real code, return actual child views
            Box::new(TestCounter {
                initial: self.count,
            })
        }

        fn dispose(&mut self) {
            self.disposed = true;
        }
    }

    // Implement View for TestCounter
    impl View for TestCounter {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(StatefulElement::new(self))
        }
    }

    #[test]
    fn test_stateful_element_creation() {
        let view = TestCounter { initial: 10 };
        let element = StatefulElement::new(&view);
        assert_eq!(element.state().count, 10);
        assert_eq!(element.lifecycle(), Lifecycle::Initial);
    }

    #[test]
    fn test_stateful_element_set_state() {
        let view = TestCounter { initial: 10 };
        let mut element = StatefulElement::new(&view);

        element.set_state(|state| {
            state.count += 1;
        });

        assert_eq!(element.state().count, 11);
        assert!(element.dirty);
    }

    #[test]
    fn test_stateful_element_dispose() {
        let view = TestCounter { initial: 10 };
        let mut element = StatefulElement::new(&view);
        element.mount(None, 0);
        element.unmount();

        assert!(element.state().disposed);
        assert_eq!(element.lifecycle(), Lifecycle::Defunct);
    }
}
