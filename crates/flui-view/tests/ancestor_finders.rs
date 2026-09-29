//! Acceptance + edge-case tests for the ancestor-finder trio and
//! the render-object finder on `BuildContext`:
//!
//! - `find_ancestor_view` / `find_ancestor` (R6) — nearest View match.
//! - `find_ancestor_state` / `find_state` (R7) — nearest State match.
//! - `find_root_ancestor_state` / `find_root_state` (R8) — root-most
//!   State match.
//! - `find_render_object` (R9) — nearest `RenderId` from a
//!   `RenderElement` ancestor.
//!
//! Test fixtures use the same `mount_root` / `insert` shape as
//! `inherited_dependency.rs`. The dependent-tracking concerns of the
//! inherited-lookup APIs are out of scope here: these finders are read-only walks per Flutter
//! parity (`framework.dart:5122-5160` —
//! `findAncestorWidgetOfExactType<T>`,
//! `findAncestorStateOfType<T>`, `findRootAncestorStateOfType<T>`,
//! `findAncestorRenderObjectOfType<T>`).

// ADR-0027: ElementBuildContext's current test/prod seam still takes
// Arc<RwLock<ElementTree/BuildOwner>>. The owner graph is !Send; do not restore
// Send + Sync to satisfy clippy. Future UiRealm/Rc migration should remove this.
#![expect(clippy::arc_with_non_send_sync)]

use std::sync::Arc;

use flui_view::{
    BuildContext, BuildContextExt, BuildOwner, ElementBuildContext, ElementTree, IntoView,
    StatelessView, View, ViewExt,
};
use parking_lot::RwLock;

// ============================================================================
// Test fixtures
// ============================================================================

/// A leaf StatelessView used to anchor the dependent in the tree shape.
#[derive(Clone)]
struct DummyChild;

impl StatelessView for DummyChild {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for DummyChild {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// A second StatelessView type, useful for "intermediate ancestor that
/// should NOT match" scenarios.
#[derive(Clone)]
struct Spacer;

impl StatelessView for Spacer {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for Spacer {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// A StatelessView with a configurable payload, used for R6
/// (find_ancestor_view returns the matched view's data).
#[derive(Clone)]
struct LabeledView {
    value: u32,
}

impl LabeledView {
    fn value(&self) -> u32 {
        self.value
    }
}

impl StatelessView for LabeledView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        DummyChild.boxed()
    }
}

impl View for LabeledView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn create_tree_and_owner() -> (Arc<RwLock<ElementTree>>, Arc<RwLock<BuildOwner>>) {
    let tree = Arc::new(RwLock::new(ElementTree::new()));
    let owner = Arc::new(RwLock::new(BuildOwner::new()));
    (tree, owner)
}

// ============================================================================
// R6: find_ancestor_view returns the nearest matching ancestor
// ============================================================================

#[test]
fn find_ancestor_view_returns_nearest_match() {
    // Tree shape: LabeledView(42) -> Spacer -> DummyChild.
    // From DummyChild, find_ancestor::<LabeledView> should yield 42.
    let (tree, owner) = create_tree_and_owner();

    let labeled = LabeledView { value: 42 };
    let labeled_id = tree
        .write()
        .mount_root(&labeled, &mut owner.write().element_owner_mut());

    let spacer_id = tree.write().insert(
        &Spacer,
        labeled_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let child_id = tree.write().insert(
        &DummyChild,
        spacer_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone()).unwrap();

    let value = ctx.find_ancestor::<LabeledView, u32>(LabeledView::value);
    assert_eq!(
        value,
        Some(42),
        "find_ancestor should return the nearest LabeledView's value"
    );
}

// ============================================================================
// R7: find_ancestor_state returns the nearest matching ancestor's state
// ============================================================================

// ============================================================================
// R8: find_root_ancestor_state returns the ROOT-MOST match (not nearest)
// ============================================================================

// ============================================================================
// Callback contract: closure runs at most once per invocation
// ============================================================================

// ============================================================================
// R9: find_render_object returns the nearest RenderElement ancestor's RenderId
// ============================================================================
