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
pub trait StatefulView: Clone + 'static + Sized {
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
pub trait ViewState<V: StatefulView>: 'static {
    /// Called once after the state is created.
    ///
    /// Use this for one-time initialization that requires BuildContext.
    fn init_state(&mut self, _ctx: &dyn BuildContext) {}

    /// Called when an already-registered `InheritedView` dependency changes.
    ///
    /// **Divergence from Flutter:** Flutter's `State.didChangeDependencies`
    /// is guaranteed to fire once, unconditionally, right after `initState` —
    /// even before any dependency has been registered — precisely so a
    /// widget can use that first call to register one. This implementation
    /// does not provide that guarantee: it fires only when an `InheritedView`
    /// this state has *already* registered as a dependent of (via
    /// `ctx.depend_on()`) later notifies. A state that needs its first
    /// `depend_on`-derived value at mount time must resolve it directly in
    /// `init_state` too (see e.g. `interaction::draggable::DraggableState`,
    /// which resolves `Overlay::maybe_of` in both hooks for exactly this
    /// reason) — relying on this hook alone for the initial value silently
    /// never fires.
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
    /// or is needed (FR-008).
    ///
    /// The framework normalizes the opaque return via
    /// [`IntoView::into_view`] inside the build call site (see
    /// `element/behavior.rs`), boxing the concrete `'static` value
    /// into `Box<dyn View>` before the closure / catch-unwind
    /// boundary. See `StatelessView::build` for the rationale.
    fn build(&self, view: &V, ctx: &dyn BuildContext) -> impl IntoView;

    /// Called when the View configuration changes.
    ///
    /// The Element has just swapped in `new_view` (the current configuration);
    /// `old_view` is the previous one. Compare the two to react to a changed
    /// field — this is Flutter's `didUpdateWidget(oldWidget)`, where `oldWidget`
    /// is the argument and the new widget is `this.widget` (here passed
    /// explicitly as `new_view`, since FLUI state does not hold the view).
    ///
    /// An implicitly-animated widget retargets its controller here: if the
    /// animated property differs between `old_view` and `new_view`, it sets the
    /// tween's `begin` to the current displayed value, its `end` to the new
    /// target, and restarts the controller from `0`.
    fn did_update_view(&mut self, _old_view: &V, _new_view: &V) {}

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

// The legacy `impl_stateful_view!` declarative macro was deleted
// (FR-010 "MUST NOT be two parallel authoring paths").
// Widget authors now write `#[derive(StatefulView)]` from
// `flui-macros` instead; the derive is re-exported from
// `flui_view::prelude` for ergonomic single-import access. See
// `crates/flui-macros/src/derive_stateful.rs` for the generated
// `impl View` block this used to emit by hand-rolled `macro_rules!`.
//
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
        fn build(&self, _view: &TestCounter, _ctx: &dyn BuildContext) -> impl IntoView {
            // In real code, return actual child views
            TestCounter {
                initial: self.count,
            }
        }

        fn dispose(&mut self) {
            self.disposed = true;
        }
    }

    // Implement View for TestCounter
    impl View for TestCounter {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::stateful(self)
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
