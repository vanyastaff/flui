//! Acceptance + edge-case tests for the U11 ancestor-finder trio on
//! `BuildContext`:
//!
//! - `find_ancestor_view` / `find_ancestor` (R6) — nearest View match.
//! - `find_ancestor_state` / `find_state` (R7) — nearest State match.
//! - `find_root_ancestor_state` / `find_root_state` (R8) — root-most
//!   State match.
//!
//! Test fixtures use the same `mount_root` / `insert` shape as
//! `inherited_dependency.rs`. The dependent-tracking concerns of U9/U10
//! are out of scope here: these finders are read-only walks per Flutter
//! parity (`framework.dart:5122-5160` —
//! `findAncestorWidgetOfExactType<T>`,
//! `findAncestorStateOfType<T>`, `findRootAncestorStateOfType<T>`).

use std::sync::Arc;

use flui_view::{
    BuildContext, BuildContextExt, BuildOwner, ElementBase, ElementBuildContext, ElementTree,
    StatefulBehavior, StatefulElement, StatefulView, StatelessBehavior, StatelessElement,
    StatelessView, View, ViewState,
};
use parking_lot::RwLock;

// ============================================================================
// Test fixtures
// ============================================================================

/// A leaf StatelessView used to anchor the dependent in the tree shape.
#[derive(Clone)]
struct DummyChild;

impl StatelessView for DummyChild {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for DummyChild {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }
}

/// A second StatelessView type, useful for "intermediate ancestor that
/// should NOT match" scenarios.
#[derive(Clone)]
struct Spacer;

impl StatelessView for Spacer {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for Spacer {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self, StatelessBehavior))
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
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(DummyChild)
    }
}

impl View for LabeledView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }
}

/// A StatefulView carrying an integer payload, used for R7/R8 (find
/// ancestor state).
#[derive(Clone)]
struct CounterView {
    initial: i32,
}

struct CounterState {
    count: i32,
}

impl CounterState {
    fn snapshot(&self) -> i32 {
        self.count
    }
}

impl StatefulView for CounterView {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            count: self.initial,
        }
    }
}

impl ViewState<CounterView> for CounterState {
    fn build(&self, _view: &CounterView, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(DummyChild)
    }
}

impl View for CounterView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatefulElement::new(self, StatefulBehavior::new(self)))
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

    let value = ctx.find_ancestor::<LabeledView, u32>(|v| v.value());
    assert_eq!(
        value,
        Some(42),
        "find_ancestor should return the nearest LabeledView's value"
    );
}

#[test]
fn find_ancestor_view_picks_nearest_when_multiple_match() {
    // Tree shape: LabeledView(1) [outer] -> LabeledView(2) [inner] -> DummyChild.
    // The nearest LabeledView (value=2) wins per Flutter parity
    // `framework.dart:5122` — `findAncestorWidgetOfExactType` walks
    // _parent and stops at the first match.
    let (tree, owner) = create_tree_and_owner();

    let outer = LabeledView { value: 1 };
    let outer_id = tree
        .write()
        .mount_root(&outer, &mut owner.write().element_owner_mut());

    let inner = LabeledView { value: 2 };
    let inner_id = tree
        .write()
        .insert(&inner, outer_id, 0, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        inner_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree.clone(), owner.clone()).unwrap();

    let value = ctx.find_ancestor::<LabeledView, u32>(|v| v.value());
    assert_eq!(
        value,
        Some(2),
        "find_ancestor returns the nearest match, not the outer one"
    );
}

#[test]
fn find_ancestor_view_returns_none_when_no_match() {
    // Tree shape: Spacer -> DummyChild. No LabeledView anywhere.
    let (tree, owner) = create_tree_and_owner();

    let spacer_id = tree
        .write()
        .mount_root(&Spacer, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        spacer_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    let value = ctx.find_ancestor::<LabeledView, u32>(|v| v.value());
    assert_eq!(value, None, "no LabeledView ancestor -> None");
}

#[test]
fn find_ancestor_view_excludes_self() {
    // Tree shape: LabeledView(42) [root, also the build context's element].
    // find_ancestor walks STRICT ancestors (parent and up) — self is NOT
    // a match. Flutter parity: `framework.dart:5122` starts with
    // `Element ancestor = _parent;`.
    let (tree, owner) = create_tree_and_owner();

    let labeled = LabeledView { value: 42 };
    let labeled_id = tree
        .write()
        .mount_root(&labeled, &mut owner.write().element_owner_mut());

    let ctx = ElementBuildContext::for_element(labeled_id, tree, owner).unwrap();

    let value = ctx.find_ancestor::<LabeledView, u32>(|v| v.value());
    assert_eq!(
        value, None,
        "self is excluded from strict-ancestor walk per Flutter parity"
    );
}

// ============================================================================
// R7: find_ancestor_state returns the nearest matching ancestor's state
// ============================================================================

#[test]
fn find_ancestor_state_returns_nearest_match() {
    // Tree shape: CounterView(initial=10) -> Spacer -> DummyChild.
    // From DummyChild, find_state::<CounterState> should yield 10.
    let (tree, owner) = create_tree_and_owner();

    let counter = CounterView { initial: 10 };
    let counter_id = tree
        .write()
        .mount_root(&counter, &mut owner.write().element_owner_mut());

    let spacer_id = tree.write().insert(
        &Spacer,
        counter_id,
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

    let count = ctx.find_state::<CounterState, i32>(|s| s.snapshot());
    assert_eq!(
        count,
        Some(10),
        "find_state should return the nearest CounterState's snapshot"
    );
}

#[test]
fn find_ancestor_state_picks_nearest_when_multiple_match() {
    // Tree: Counter(outer=1) -> Counter(inner=2) -> Spacer -> DummyChild.
    // Nearest match wins (inner snapshot = 2).
    let (tree, owner) = create_tree_and_owner();

    let outer = CounterView { initial: 1 };
    let outer_id = tree
        .write()
        .mount_root(&outer, &mut owner.write().element_owner_mut());

    let inner = CounterView { initial: 2 };
    let inner_id = tree
        .write()
        .insert(&inner, outer_id, 0, &mut owner.write().element_owner_mut());

    let spacer_id =
        tree.write()
            .insert(&Spacer, inner_id, 0, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        spacer_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    let count = ctx.find_state::<CounterState, i32>(|s| s.snapshot());
    assert_eq!(
        count,
        Some(2),
        "find_state returns the nearest CounterState (initial=2), not the outer"
    );
}

#[test]
fn find_ancestor_state_returns_none_when_no_match() {
    // Tree shape: Spacer -> DummyChild. No CounterView anywhere.
    let (tree, owner) = create_tree_and_owner();

    let spacer_id = tree
        .write()
        .mount_root(&Spacer, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        spacer_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    let count = ctx.find_state::<CounterState, i32>(|s| s.snapshot());
    assert_eq!(count, None, "no CounterState ancestor -> None");
}

#[test]
fn find_ancestor_state_excludes_stateless_ancestors() {
    // Tree: Spacer (stateless) -> DummyChild. Stateless ancestors should
    // never expose a `state_as_any`, so find_state must skip them
    // cleanly without false positives.
    let (tree, owner) = create_tree_and_owner();

    let spacer_id = tree
        .write()
        .mount_root(&Spacer, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        spacer_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    // Probe by Spacer's type as if it were a state — stateless
    // elements return None from state_as_any, so the lookup must
    // return None even though Spacer is in the ancestor chain.
    let probe = ctx.find_state::<Spacer, ()>(|_| ());
    assert_eq!(
        probe, None,
        "Stateless ancestors must not match state lookup"
    );
}

// ============================================================================
// R8: find_root_ancestor_state returns the ROOT-MOST match (not nearest)
// ============================================================================

#[test]
fn find_root_ancestor_state_returns_root_most_match() {
    // Tree: Counter(outer=100) -> Counter(inner=200) -> Spacer -> DummyChild.
    // find_root_state must walk all the way to root and return the
    // OUTER (root-most) Counter's state — snapshot = 100.
    // This is the load-bearing assertion that distinguishes R8 from R7.
    let (tree, owner) = create_tree_and_owner();

    let outer = CounterView { initial: 100 };
    let outer_id = tree
        .write()
        .mount_root(&outer, &mut owner.write().element_owner_mut());

    let inner = CounterView { initial: 200 };
    let inner_id = tree
        .write()
        .insert(&inner, outer_id, 0, &mut owner.write().element_owner_mut());

    let spacer_id =
        tree.write()
            .insert(&Spacer, inner_id, 0, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        spacer_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    let count = ctx.find_root_state::<CounterState, i32>(|s| s.snapshot());
    assert_eq!(
        count,
        Some(100),
        "find_root_state returns the OUTER Counter (root-most), \
         not the inner one — Flutter parity framework.dart:5146"
    );
}

#[test]
fn find_root_ancestor_state_single_match_works() {
    // Tree: Counter(initial=7) -> Spacer -> DummyChild.
    // Only one matching ancestor — root-most == nearest == that one.
    let (tree, owner) = create_tree_and_owner();

    let counter = CounterView { initial: 7 };
    let counter_id = tree
        .write()
        .mount_root(&counter, &mut owner.write().element_owner_mut());

    let spacer_id = tree.write().insert(
        &Spacer,
        counter_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let child_id = tree.write().insert(
        &DummyChild,
        spacer_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    let count = ctx.find_root_state::<CounterState, i32>(|s| s.snapshot());
    assert_eq!(
        count,
        Some(7),
        "single-match case: root-most == nearest, returns 7"
    );
}

#[test]
fn find_root_ancestor_state_returns_none_when_no_match() {
    // Tree: Spacer -> DummyChild. No Counter anywhere.
    let (tree, owner) = create_tree_and_owner();

    let spacer_id = tree
        .write()
        .mount_root(&Spacer, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        spacer_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    let count = ctx.find_root_state::<CounterState, i32>(|s| s.snapshot());
    assert_eq!(count, None, "no Counter ancestor -> None");
}

#[test]
fn find_root_ancestor_state_with_non_matching_intermediate() {
    // Tree: Counter(outer=1) -> Spacer -> Counter(inner=2) -> Spacer -> DummyChild.
    // Spacer in the middle MUST be skipped without breaking the
    // root-most logic. Result must be 1 (outer/root-most), not 2.
    let (tree, owner) = create_tree_and_owner();

    let outer = CounterView { initial: 1 };
    let outer_id = tree
        .write()
        .mount_root(&outer, &mut owner.write().element_owner_mut());

    let spacer1_id =
        tree.write()
            .insert(&Spacer, outer_id, 0, &mut owner.write().element_owner_mut());

    let inner = CounterView { initial: 2 };
    let inner_id = tree.write().insert(
        &inner,
        spacer1_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let spacer2_id =
        tree.write()
            .insert(&Spacer, inner_id, 0, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        spacer2_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    let count = ctx.find_root_state::<CounterState, i32>(|s| s.snapshot());
    assert_eq!(
        count,
        Some(1),
        "non-matching intermediate must not interrupt root-most walk"
    );
}

// ============================================================================
// Callback contract: closure runs at most once per invocation
// ============================================================================

#[test]
fn find_ancestor_view_callback_runs_at_most_once() {
    // Even when multiple ancestors of the same type exist, the typed
    // wrapper consumes the FnOnce on the FIRST match and tracks that
    // via `Option::take()`. The closure body must run exactly once.
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (tree, owner) = create_tree_and_owner();

    let outer = LabeledView { value: 1 };
    let outer_id = tree
        .write()
        .mount_root(&outer, &mut owner.write().element_owner_mut());

    let inner = LabeledView { value: 2 };
    let inner_id = tree
        .write()
        .insert(&inner, outer_id, 0, &mut owner.write().element_owner_mut());

    let child_id = tree.write().insert(
        &DummyChild,
        inner_id,
        0,
        &mut owner.write().element_owner_mut(),
    );

    let ctx = ElementBuildContext::for_element(child_id, tree, owner).unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_clone = Arc::clone(&calls);
    let _value = ctx.find_ancestor::<LabeledView, u32>(|v| {
        calls_clone.fetch_add(1, Ordering::Relaxed);
        v.value()
    });

    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "find_ancestor must invoke the typed closure exactly once on a match"
    );
}
