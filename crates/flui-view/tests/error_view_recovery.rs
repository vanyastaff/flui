//! Integration tests for build-panic recovery (plan §U7, origin R9).
//!
//! When a user `build()` panics, `ElementBase::build_into_views` must
//! catch the unwind and substitute the registered `ErrorView` instead of
//! letting the panic abort the frame — mirroring Flutter's
//! `ComponentElement.performRebuild` dual try/catch
//! (`framework.dart:5810-5859`).
//!
//! E3 + H3: the element no longer owns its children, and component
//! builds require the live tree-backed `BuildCtx` supplied by
//! `BuildOwner::build_scope`. These tests drive the production
//! mount/schedule/build-scope path and inspect the materialized slab
//! children created by the id-reconciler.
//!
//! These tests deliberately panic inside `build()`. `catch_unwind` still
//! prints the panic's backtrace to stderr even when the unwind is
//! caught — that stderr noise is expected; the test process must NOT
//! abort and the assertions below must hold.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![allow(clippy::unwrap_used)]

use std::{
    any::TypeId,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use flui_view::{
    BuildContext, BuildOwner, ElementTree, ErrorView, FlutterError, IntoView, Lifecycle,
    StatefulView, StatelessView, View, ViewExt, ViewState, clear_error_view_builder,
    set_error_view_builder,
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
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
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
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
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
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

fn mount_and_build(view: &dyn View) -> (ElementTree, BuildOwner, flui_view::ElementId) {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root_id = tree.mount_root(view, &mut owner.element_owner_mut());
    owner.schedule_build_for(root_id, 0);
    owner.build_scope(&mut tree);
    (tree, owner, root_id)
}

fn count_error_child_elements(tree: &ElementTree, parent: flui_view::ElementId) -> usize {
    child_ids(tree, parent)
        .into_iter()
        .filter(|&child_id| {
            tree.get(child_id).unwrap().element().view_type_id() == TypeId::of::<ErrorView>()
        })
        .count()
}

fn child_ids(tree: &ElementTree, parent: flui_view::ElementId) -> Vec<flui_view::ElementId> {
    tree.iter_nodes()
        .filter_map(|(id, node)| (node.parent() == Some(parent)).then_some(id))
        .collect()
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
    // The frame must NOT unwind here — build_scope catches the panic and the
    // reconciler materializes the substituted ErrorView as a child element.
    let (tree, _owner, root_id) = mount_and_build(&view);

    assert_eq!(
        BUILDER_HITS.load(Ordering::SeqCst),
        1,
        "the registered error-view builder must run exactly once"
    );
    assert_eq!(
        count_error_child_elements(&tree, root_id),
        1,
        "the panicked subtree must be replaced by exactly one ErrorView child element"
    );
    assert_eq!(
        tree.get(root_id).unwrap().element().lifecycle(),
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
    let (tree, _owner, root_id) = mount_and_build(&view);

    assert_eq!(
        count_error_child_elements(&tree, root_id),
        1,
        "with no builder the default ErrorView must still substitute"
    );
    assert_eq!(
        tree.get(root_id).unwrap().element().lifecycle(),
        Lifecycle::Active
    );
}

// ============================================================================
// Stateful — ViewState::build panic is caught too
// ============================================================================

#[test]
fn stateful_build_panic_substitutes_error_view() {
    let _guard = acquire_builder_guard();
    clear_error_view_builder();

    let view = PanickingStatefulView;
    let (tree, _owner, root_id) = mount_and_build(&view);

    assert_eq!(
        count_error_child_elements(&tree, root_id),
        1,
        "a panicking ViewState::build must be caught and substituted"
    );
    assert_eq!(
        tree.get(root_id).unwrap().element().lifecycle(),
        Lifecycle::Active
    );
}

// ============================================================================
// Edge — nested child panic: only the panicking subtree is replaced
// ============================================================================

#[test]
fn nested_child_build_panic_replaces_only_that_subtree() {
    let _guard = acquire_builder_guard();
    clear_error_view_builder();

    // Parent is a well-behaved wrapper; its child is a PanickingView.
    // E3: the parent's build returns its child view, and the slab
    // id-reconciler builds that child as its own drain entry, where the
    // child's panic is caught.
    let view = WrapperView {
        child: PanickingView {
            message: "boom in nested child",
        },
    };
    let (tree, _owner, root_id) = mount_and_build(&view);

    // The parent did NOT panic — it built fine. Its direct child remains the
    // PanickingView element; only that child's own subtree is replaced.
    assert_eq!(
        tree.get(root_id).unwrap().element().lifecycle(),
        Lifecycle::Active
    );
    let child_ids = child_ids(&tree, root_id);
    assert_eq!(child_ids.len(), 1, "parent materializes exactly one child");
    let child_id = child_ids[0];
    assert_eq!(
        tree.get(child_id).unwrap().element().view_type_id(),
        TypeId::of::<PanickingView>(),
        "the well-behaved parent's direct child must not be replaced"
    );
    assert_eq!(
        count_error_child_elements(&tree, child_id),
        1,
        "the panicking child subtree must be replaced by an ErrorView child"
    );
    assert_eq!(
        count_error_child_elements(&tree, root_id),
        0,
        "the well-behaved parent's direct child must NOT be an ErrorView"
    );
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
    let (tree, mut owner, root_id) = mount_and_build(&view);

    // The element's own dirty flag must be cleared (the build-half tail
    // runs even on the recovery path) and nothing must be left queued on
    // the BuildOwner's dirty heap.
    assert!(
        !tree.get(root_id).unwrap().element().is_dirty(),
        "build_scope must clear the element's dirty flag on the recovery path"
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
    let (mut tree, mut owner, root_id) = mount_and_build(&view);
    assert_eq!(count_error_child_elements(&tree, root_id), 1);

    // Force a rebuild and run build_scope again: still exactly one ErrorView
    // child element, no extra nodes leaked, no unwind.
    tree.get_mut(root_id)
        .unwrap()
        .element_mut()
        .mark_needs_build();
    owner.schedule_build_for(root_id, 0);
    owner.build_scope(&mut tree);

    assert_eq!(
        count_error_child_elements(&tree, root_id),
        1,
        "a second build after recovery must still yield exactly one ErrorView child"
    );
    assert_eq!(
        child_ids(&tree, root_id).len(),
        1,
        "no extra child element leaked on rebuild"
    );
    assert_eq!(
        tree.get(root_id).unwrap().element().lifecycle(),
        Lifecycle::Active
    );
}
