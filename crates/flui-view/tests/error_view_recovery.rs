//! Integration tests for build-panic recovery (plan §U7, origin R9).
//!
//! When a user `build()` panics, `Element::perform_build` must catch the
//! unwind and substitute the registered `ErrorView` instead of letting
//! the panic abort the frame — mirroring Flutter's
//! `ComponentElement.performRebuild` dual try/catch
//! (`framework.dart:5810-5859`).
//!
//! These tests deliberately panic inside `build()`. `catch_unwind` still
//! prints the panic's backtrace to stderr even when the unwind is
//! caught — that stderr noise is expected; the test process must NOT
//! abort and the assertions below must hold.

use std::{
    any::TypeId,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use flui_view::{
    BuildContext, BuildOwner, ElementBase, ErrorView, FlutterError, IntoView, Lifecycle,
    StatefulBehavior, StatefulElement, StatefulView, StatelessBehavior, StatelessElement,
    StatelessView, View, ViewExt, ViewState, clear_error_view_builder, set_error_view_builder,
};

/// Serializes the tests in this file.
///
/// `set_error_view_builder` / `clear_error_view_builder` mutate a
/// process-global `RwLock<Option<ErrorViewBuilder>>` in `flui-view`.
/// `cargo test` runs integration tests in parallel by default, so two
/// tests racing on the global builder produce cross-talk — a test that
/// expects the default-fallback path can see a builder set by a
/// neighbouring test. Holding this mutex across each test gives the
/// global builder cell a single writer at a time.
///
/// Poison-tolerant: if one test fails its assertions and panics, the
/// remaining tests still run a useful body — we extract the inner
/// guard via `into_inner` either way.
static GLOBAL_BUILDER_GUARD: Mutex<()> = Mutex::new(());

fn acquire_builder_guard() -> std::sync::MutexGuard<'static, ()> {
    match GLOBAL_BUILDER_GUARD.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// ============================================================================
// Test fixtures
// ============================================================================

/// A stateless view whose `build()` always panics.
#[derive(Clone)]
struct PanickingView {
    message: &'static str,
}

impl StatelessView for PanickingView {
    // `!` does not implement `IntoView` — bind the panic through an
    // explicit `Box<dyn View>` so the inference uses the
    // `IntoView for Box<dyn View>` shim. The line still panics (the
    // assignment never completes); the bind exists purely to anchor
    // a concrete `impl IntoView`-satisfying type.
    #[allow(
        unreachable_code,
        unused_variables,
        clippy::diverging_sub_expression,
        reason = "panic body — see comment above"
    )]
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let v: Box<dyn View> = panic!("{}", self.message);
        v
    }
}

impl View for PanickingView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }
}

/// A stateless view that wraps a single child view of any type — lets a
/// test build a parent whose child subtree panics while the parent
/// itself does not.
#[derive(Clone)]
struct WrapperView<C: View + Clone> {
    child: C,
}

impl<C: View + Clone + 'static> StatelessView for WrapperView<C> {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone().boxed()
    }
}

impl<C: View + Clone + 'static> View for WrapperView<C> {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }
}

/// Stateful counterpart: `ViewState::build` panics.
#[derive(Clone)]
struct PanickingStatefulView;

struct PanickingStatefulState;

impl StatefulView for PanickingStatefulView {
    type State = PanickingStatefulState;

    fn create_state(&self) -> Self::State {
        PanickingStatefulState
    }
}

impl ViewState<PanickingStatefulView> for PanickingStatefulState {
    // See `PanickingView::build` — anchor `!` through `Box<dyn View>` so
    // the `impl IntoView`-satisfying type is fixed.
    #[allow(
        unreachable_code,
        unused_variables,
        clippy::diverging_sub_expression,
        reason = "panic body — see comment above"
    )]
    fn build(&self, _view: &PanickingStatefulView, _ctx: &dyn BuildContext) -> impl IntoView {
        let v: Box<dyn View> = panic!("stateful build exploded");
        v
    }
}

impl View for PanickingStatefulView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatefulElement::new(self, StatefulBehavior::new(self)))
    }
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

/// Count of children whose `view_type_id()` is `ErrorView`.
fn count_error_children(element: &StatelessElement<impl StatelessView>) -> usize {
    let mut count = 0;
    element.core().visit_children(|child| {
        if child.view_type_id() == TypeId::of::<ErrorView>() {
            count += 1;
        }
    });
    count
}

/// Count of children whose `view_type_id()` is `ErrorView` for a
/// stateful element.
fn count_error_children_stateful(element: &StatefulElement<impl StatefulView>) -> usize {
    let mut count = 0;
    element.core().visit_children(|child| {
        if child.view_type_id() == TypeId::of::<ErrorView>() {
            count += 1;
        }
    });
    count
}

// ============================================================================
// Happy path — registered builder, stateless build panics
// ============================================================================

#[test]
fn stateless_build_panic_substitutes_registered_error_view() {
    let _guard = acquire_builder_guard();
    // A custom builder records that it ran and returns a plain ErrorView
    // with a "custom: " prefix so we can identify which builder produced
    // the rendered child even under parallel cargo-test runs.
    static BUILDER_HITS: AtomicUsize = AtomicUsize::new(0);
    BUILDER_HITS.store(0, Ordering::SeqCst);
    fn builder(error: &FlutterError) -> Box<dyn View> {
        BUILDER_HITS.fetch_add(1, Ordering::SeqCst);
        Box::new(ErrorView::new(format!("custom: {}", error.message)))
    }
    set_error_view_builder(builder);

    let view = PanickingView {
        message: "boom in stateless build",
    };
    let mut element = StatelessElement::new(&view, StatelessBehavior);
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    // The frame must NOT unwind here — perform_build catches the panic.
    element.perform_build(&mut owner.element_owner_mut());

    assert_eq!(
        BUILDER_HITS.load(Ordering::SeqCst),
        1,
        "the registered error-view builder must run exactly once"
    );
    assert_eq!(
        count_error_children(&element),
        1,
        "the panicked subtree must be replaced by exactly one ErrorView child"
    );
    assert_eq!(
        element.lifecycle(),
        Lifecycle::Active,
        "the element itself stays Active after recovering"
    );

    clear_error_view_builder();
}

// ============================================================================
// Edge — no builder registered, default error view renders
// ============================================================================

#[test]
fn stateless_build_panic_falls_back_to_default_error_view() {
    let _guard = acquire_builder_guard();
    // No builder registered — the built-in default must still produce an
    // ErrorView and the frame must not unwind.
    clear_error_view_builder();

    let view = PanickingView {
        message: "boom with no builder",
    };
    let mut element = StatelessElement::new(&view, StatelessBehavior);
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    element.perform_build(&mut owner.element_owner_mut());

    assert_eq!(
        count_error_children(&element),
        1,
        "with no builder the default ErrorView must still substitute"
    );
    assert_eq!(element.lifecycle(), Lifecycle::Active);
}

// ============================================================================
// Stateful — ViewState::build panic is caught too
// ============================================================================

#[test]
fn stateful_build_panic_substitutes_error_view() {
    let _guard = acquire_builder_guard();
    clear_error_view_builder();

    let view = PanickingStatefulView;
    let mut element = StatefulElement::new(&view, StatefulBehavior::new(&view));
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    element.perform_build(&mut owner.element_owner_mut());

    assert_eq!(
        count_error_children_stateful(&element),
        1,
        "a panicking ViewState::build must be caught and substituted"
    );
    assert_eq!(element.lifecycle(), Lifecycle::Active);
}

// ============================================================================
// Edge — nested child panic: only the panicking subtree is replaced
// ============================================================================

#[test]
fn nested_child_build_panic_replaces_only_that_subtree() {
    let _guard = acquire_builder_guard();
    clear_error_view_builder();

    // Parent is a well-behaved wrapper; its child is a PanickingView.
    // perform_build recurses: the parent builds fine, the child panics.
    let view = WrapperView {
        child: PanickingView {
            message: "boom in nested child",
        },
    };
    let mut element = StatelessElement::new(&view, StatelessBehavior);
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    element.perform_build(&mut owner.element_owner_mut());

    // The parent did NOT panic — its own child slot holds the (recovered)
    // child element, which itself is the StatelessElement<PanickingView>.
    // That child must have caught its own build panic and now hold an
    // ErrorView grandchild. The parent element stays Active.
    assert_eq!(element.lifecycle(), Lifecycle::Active);

    let mut child_count = 0;
    element.core().visit_children(|child| {
        child_count += 1;
        // The child is not itself an ErrorView (the parent built fine).
        assert_ne!(
            child.view_type_id(),
            TypeId::of::<ErrorView>(),
            "the well-behaved parent's child must NOT be replaced"
        );
    });

    assert_eq!(child_count, 1, "parent keeps exactly one child");
}

// ============================================================================
// Error path — no dangling dirty-list state after a caught panic
// ============================================================================

#[test]
fn caught_panic_leaves_no_dangling_dirty_state() {
    let _guard = acquire_builder_guard();
    clear_error_view_builder();

    let view = PanickingView {
        message: "boom — check dirty heap",
    };
    let mut element = StatelessElement::new(&view, StatelessBehavior);
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    element.perform_build(&mut owner.element_owner_mut());

    // The element's own dirty flag must be cleared (perform_build's tail
    // runs even on the recovery path) and nothing must be left queued on
    // the BuildOwner's dirty heap.
    assert!(
        !element.core().is_dirty(),
        "perform_build must clear the element's dirty flag on the recovery path"
    );
    assert_eq!(
        owner.element_owner_mut().dirty_count(),
        0,
        "a caught build panic must not leave a dangling entry on the dirty heap"
    );
}

// ============================================================================
// Recovery is repeatable — a second build does not double-panic / leak
// ============================================================================

#[test]
fn repeated_build_after_panic_stays_stable() {
    let _guard = acquire_builder_guard();
    clear_error_view_builder();

    let view = PanickingView {
        message: "boom — repeated",
    };
    let mut element = StatelessElement::new(&view, StatelessBehavior);
    let mut owner = BuildOwner::new();
    element.mount(None, 0, &mut owner.element_owner_mut());

    element.perform_build(&mut owner.element_owner_mut());
    assert_eq!(count_error_children(&element), 1);

    // Force a rebuild and run perform_build again: still exactly one
    // ErrorView child, no second child leaked, no unwind.
    element.mark_needs_build();
    element.perform_build(&mut owner.element_owner_mut());

    assert_eq!(
        count_error_children(&element),
        1,
        "a second build after recovery must still hold exactly one ErrorView"
    );
    assert_eq!(element.lifecycle(), Lifecycle::Active);
}
