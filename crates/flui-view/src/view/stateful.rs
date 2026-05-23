//! StatefulView - Views with persistent mutable state.
//!
//! StatefulViews maintain state that persists across rebuilds.
//! The state is held by the Element, not the View itself.

use super::into_view::IntoView;
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
///     fn build(&self, view: &Counter, ctx: &dyn BuildContext) -> impl IntoView {
///         Text::new(format!("Count: {}", self.count))
///     }
/// }
/// ```
pub trait StatefulView: Clone + Send + Sync + 'static + Sized {
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
    ///
    /// # Object safety
    ///
    /// `ViewState::build` returns `impl IntoView` (return-position
    /// `impl Trait` in trait, stabilized in Rust 1.75). This makes
    /// `ViewState` **non-object-safe** — no `dyn ViewState` use exists
    /// or is needed (Phase 3 §U22, FR-008).
    ///
    /// The `+ use<Self, V>` precise-capture clause (Rust 1.82+) declares
    /// that the opaque return depends only on `Self` and `V`, not on
    /// the elided lifetimes of `&self` / `&V` / `&dyn BuildContext`.
    /// See [`StatelessView::build`] for the rationale (E0515 when a
    /// `move || state.build(&view, &ctx)` closure owns the args).
    fn build(&self, view: &V, ctx: &dyn BuildContext) -> impl IntoView + use<Self, V>;

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
///
/// # Deprecation
///
/// Phase 3 §U24 deletes this macro in favor of `#[derive(StatefulView)]`
/// from `flui-macros`. The macro stays during the §U22→§U24 transition
/// so existing call sites continue to compile (FR-009 / FR-010).
#[macro_export]
macro_rules! impl_stateful_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> Box<dyn $crate::ElementBase> {
                use $crate::element::StatefulBehavior;
                Box::new($crate::StatefulElement::new(
                    self,
                    StatefulBehavior::new(self),
                ))
            }
        }
    };
}

// NOTE: StatefulElement implementation has been moved to unified Element
// architecture. See crates/flui-view/src/element/unified.rs and
// element/behavior.rs The type alias is exported from element/mod.rs:
//   pub type StatefulElement<V> = Element<V, Single, StatefulBehavior<V>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        StatefulElement,
        element::{Lifecycle, StatefulBehavior},
        view::{ElementBase, View},
    };

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
            Box::new(StatefulElement::new(self, StatefulBehavior::new(self)))
        }
    }

    #[test]
    fn test_stateful_element_creation() {
        let view = TestCounter { initial: 10 };
        let element = StatefulElement::new(&view, StatefulBehavior::new(&view));
        assert_eq!(element.state().count, 10);
        assert_eq!(element.lifecycle(), Lifecycle::Initial);
    }

    #[test]
    fn test_stateful_element_set_state() {
        let view = TestCounter { initial: 10 };
        let mut element = StatefulElement::new(&view, StatefulBehavior::new(&view));

        element.set_state(|state| {
            state.count += 1;
        });

        assert_eq!(element.state().count, 11);
        // Element is marked dirty after set_state
    }

    #[test]
    fn test_stateful_element_dispose() {
        let view = TestCounter { initial: 10 };
        let mut element = StatefulElement::new(&view, StatefulBehavior::new(&view));
        let mut owner = crate::BuildOwner::new();
        element.mount(None, 0, &mut owner.element_owner_mut());
        element.unmount(&mut owner.element_owner_mut());

        assert!(element.state().disposed);
        assert_eq!(element.lifecycle(), Lifecycle::Defunct);
    }
}
