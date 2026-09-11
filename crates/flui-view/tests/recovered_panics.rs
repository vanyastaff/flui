//! Integration tests for `BuildOwner::take_recovered_panics` (issue #561).
//!
//! `build_or_recover` is the first producer feeding the drain: when a
//! Stateless/Stateful `build()` panics, it substitutes an `ErrorView` —
//! that substitution has always been there — and now also records what it
//! caught, so a later consumer (`WidgetsBinding::take_recovered_panics`,
//! forwarded as a frame-failure report) has something to read besides the
//! `tracing::error!` line.
//!
//! These tests use the default `ErrorView` and never touch the
//! process-global error-view builder, unlike `error_view_recovery.rs`
//! (which mutates it and therefore stays its own `[[test]]` target for
//! process isolation) — so they run as ordinary `view_it` modules.

use std::any::TypeId;

use flui_view::{
    BuildContext, BuildOwner, ElementId, ElementTree, ErrorView, IntoView, LifecycleHook,
    RebuildReason, StatelessView, View, ViewExt,
};

// ============================================================================
// Test fixtures
// ============================================================================

/// A stateless view whose `build()` always panics with a fixed message —
/// the failing subtree the drain must record.
#[derive(Clone)]
struct PanicBuildView {
    message: &'static str,
}

impl StatelessView for PanicBuildView {
    // `!` does not implement `IntoView` — bind the panic through an
    // explicit `Box<dyn View>` so inference uses the `IntoView for
    // Box<dyn View>` shim (same anchor as `error_view_recovery.rs`'s
    // `PanickingView`). The line still panics; the bind exists purely to
    // fix a concrete `impl IntoView`-satisfying type.
    #[expect(
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

impl View for PanicBuildView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// A well-behaved stateless parent hosting a single child — isolates the
/// panic to the child's own build (the parent's build itself never
/// panics), so the recorded element is the child, not the root.
#[derive(Clone)]
struct HostView {
    child: PanicBuildView,
}

impl StatelessView for HostView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone().boxed()
    }
}

impl View for HostView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

fn mount_and_build(view: &dyn View) -> (ElementTree, BuildOwner, ElementId) {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root_id = tree.mount_root(view, &mut owner.element_owner_mut());
    owner.schedule_build_for(root_id, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    (tree, owner, root_id)
}

/// The single child element mounted directly under `parent`.
fn only_child(tree: &ElementTree, parent: ElementId) -> ElementId {
    let children: Vec<ElementId> = tree
        .iter_nodes()
        .filter_map(|(id, node)| (node.parent() == Some(parent)).then_some(id))
        .collect();
    assert_eq!(
        children.len(),
        1,
        "host must mount exactly one child, found {children:?}"
    );
    children[0]
}

// ============================================================================
// A contained build panic is recorded once, with its element and hook
// ============================================================================

#[test]
fn a_contained_build_panic_is_recorded_once_with_its_element_and_hook() {
    let host = HostView {
        child: PanicBuildView {
            message: "injected build panic",
        },
    };

    // build_scope must RETURN — the panic is contained, not propagated.
    let (tree, mut owner, root_id) = mount_and_build(&host);
    let child_id = only_child(&tree, root_id);

    let mut recovered = owner.take_recovered_panics();
    assert_eq!(
        recovered.len(),
        1,
        "exactly one contained panic must be recorded, got {recovered:?}"
    );
    let panic = recovered.remove(0);
    assert_eq!(panic.hook, LifecycleHook::Build);
    assert_eq!(
        panic.element, child_id,
        "the recorded element is the panicking child, not the well-behaved host"
    );
    assert_eq!(panic.view_type_id, TypeId::of::<PanicBuildView>());
    assert!(
        !panic.internal_invariant,
        "an ordinary panic message is not a BUG: internal invariant"
    );
    assert!(
        panic.error.message.contains("injected build panic"),
        "the recorded error must carry the panic payload text, got {:?}",
        panic.error.message
    );

    // The drain consumes — a second take sees nothing new.
    assert!(
        owner.take_recovered_panics().is_empty(),
        "take_recovered_panics must drain, not merely peek"
    );
}

// ============================================================================
// A `BUG:`-prefixed payload is classified as an internal invariant, but
// containment is unaffected — it substitutes an ErrorView exactly the same
// (docs/PANIC-POLICY.md: classification never routes).
// ============================================================================

#[test]
fn a_bug_prefixed_payload_is_classified_internal_invariant_but_still_contained() {
    let host = HostView {
        child: PanicBuildView {
            message: "BUG: injected",
        },
    };

    let (tree, mut owner, root_id) = mount_and_build(&host);
    // `PanicBuildView`'s own element is not replaced — only ITS `build()`
    // panicked, so its element stays mounted; the substituted `ErrorView`
    // is reconciled in as its child (same shape
    // `error_view_recovery.rs::nested_child_build_panic_replaces_only_that_subtree`
    // pins for a non-BUG: panic).
    let child_id = only_child(&tree, root_id);

    let recovered = owner.take_recovered_panics();
    assert_eq!(recovered.len(), 1, "got {recovered:?}");
    assert!(
        recovered[0].internal_invariant,
        "a BUG:-prefixed payload must classify as an internal invariant"
    );

    let error_view_id = only_child(&tree, child_id);
    assert_eq!(
        tree.get(error_view_id)
            .expect("error view slot still present")
            .element()
            .view_type_id(),
        TypeId::of::<ErrorView>(),
        "a contained BUG: panic still substitutes an ErrorView, same as any other"
    );
}
