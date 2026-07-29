//! Ported from `packages/flutter/test/widgets/framework_test.dart` (tag
//! `3.44.0`), the `'GlobalKey duplication N'` series.
//!
//! ## The two models, and where they actually differ
//!
//! Both frameworks resolve a `GlobalKey` optimistically. Flutter's
//! `Element._retakeInactiveElement` takes the keyed element even when it is
//! still attached to a live parent, and says so in its own comment: the
//! "inactivity" is *forward-looking* — the old parent is assumed to be about
//! to give the child up, and "the only way that assumption could be false is
//! if the global key is being duplicated". FLUI's `reuse_or_mount` →
//! `retake_active_global_key` (`crates/flui-view/src/tree/element_tree.rs`)
//! does the same graft. So the frameworks agree on the optimistic move; the
//! series' cases are about what happens when the optimism turns out to be
//! wrong.
//!
//! - **Same parent, twice — both reject, eagerly.** Flutter throws `'A
//!   GlobalKey was used multiple times inside one widget's child list.'`
//!   straight from `_retakeInactiveElement` when the element's parent *is*
//!   the parent now asking for it; FLUI panics in debug from
//!   `retake_active_global_key` under the same condition. Same verdict, same
//!   timing, different channel.
//! - **Two different parents — Flutter re-checks, FLUI does not.** Flutter
//!   records the loser via
//!   `_debugTrackElementThatWillNeedToBeRebuiltDueToGlobalKeyShenanigans` and
//!   verifies at the end of the frame
//!   (`_debugVerifyGlobalKeyReservation` after `buildScope`,
//!   `_debugVerifyIllFatedPopulation` in `finalizeTree`), raising `'Multiple
//!   widgets used the same GlobalKey.'` if the first parent still claims the
//!   child. It also repairs the tree (`forgetChild` on the losing parent) so
//!   teardown does not cascade. **FLUI has no end-of-frame verification at
//!   all**, so a genuine cross-parent duplicate is never reported: the two
//!   parents simply take turns grafting the one element.
//!
//! What is ported is therefore the *verdict* on each tree shape, not the
//! oracle's message text or its diagnostic machinery. Where FLUI's verdict
//! differs, the test says so and asserts what FLUI actually does — never a
//! narrowed version of the oracle's expectation. The gap is filed in
//! `docs/ROADMAP.md` Cross.H (search `_debugVerifyGlobalKeyReservation`).
//!
//! Structural substitution: the oracle expresses its trees as
//! `Stack`/`Container` hierarchies through `pumpWidget`. These ports drive
//! `ElementTree` directly with the same *shape* (N parents, each given a child
//! carrying the key), because the subject is the element tree's keyed
//! reconciliation, not any widget's layout.

// ADR-0027: the test/prod seam still hands `Arc<RwLock<ElementTree/BuildOwner>>`
// around, and the owner graph is `!Send`. Do not restore `Send + Sync` to
// satisfy clippy — the sibling `global_key.rs` carries the same waiver for the
// same reason, and a future UiRealm/`Rc` migration removes both.
#![allow(clippy::arc_with_non_send_sync)]

use std::sync::Arc;

use flui_foundation::ViewKey;
use flui_view::{
    BuildContext, BuildOwner, ElementId, ElementTree, GlobalKey, IntoView, StatefulView,
    StatelessView, View, ViewExt, ViewState,
};
use parking_lot::RwLock;

// ============================================================================
// Fixtures — mirrors `global_key.rs`'s, kept local so the two files can
// evolve independently.
// ============================================================================

/// A leaf used as a parent slot or filler — the oracle's `Container` with no
/// child.
#[derive(Clone)]
struct Filler;

impl StatelessView for Filler {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for Filler {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

struct KeyedState {
    /// Distinguishes a migrated element from a freshly created one.
    tag: i32,
}

/// A stateful view carrying a `GlobalKey` — the oracle's keyed child.
#[derive(Clone)]
struct Keyed {
    key: GlobalKey<KeyedState>,
    tag: i32,
}

impl StatefulView for Keyed {
    type State = KeyedState;

    fn create_state(&self) -> Self::State {
        KeyedState { tag: self.tag }
    }
}

impl ViewState<Keyed> for KeyedState {
    fn build(&self, _view: &Keyed, _ctx: &dyn BuildContext) -> impl IntoView {
        Filler.boxed()
    }
}

impl View for Keyed {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

fn fresh_tree() -> (Arc<RwLock<ElementTree>>, Arc<RwLock<BuildOwner>>) {
    (
        Arc::new(RwLock::new(ElementTree::new())),
        Arc::new(RwLock::new(BuildOwner::new())),
    )
}

/// Mounts a root with `parent_count` `Filler` parents beneath it and returns
/// their ids — the oracle's `Stack` with N `Container` children.
fn tree_with_parents(
    tree: &Arc<RwLock<ElementTree>>,
    owner: &Arc<RwLock<BuildOwner>>,
    parent_count: usize,
) -> Vec<ElementId> {
    let root = tree
        .write()
        .mount_root(&Filler, &mut owner.write().element_owner_mut());
    (0..parent_count)
        .map(|slot| {
            tree.write()
                .insert(&Filler, root, slot, &mut owner.write().element_owner_mut())
        })
        .collect()
}

/// Attaches a child carrying `key` under `parent`.
fn attach_keyed(
    tree: &Arc<RwLock<ElementTree>>,
    owner: &Arc<RwLock<BuildOwner>>,
    parent: ElementId,
    key: &GlobalKey<KeyedState>,
    tag: i32,
) -> ElementId {
    let view = Keyed {
        key: key.clone(),
        tag,
    };
    tree.write()
        .insert(&view, parent, 0, &mut owner.write().element_owner_mut())
}

/// Children of `parent` in slot order, as the tree currently holds them.
fn children_of(tree: &Arc<RwLock<ElementTree>>, parent: ElementId) -> Vec<ElementId> {
    let tree = tree.read();
    let mut children: Vec<_> = tree
        .iter_nodes()
        .filter(|(_, node)| node.parent() == Some(parent))
        .map(|(id, node)| (node.slot(), id))
        .collect();
    children.sort_by_key(|(slot, _)| *slot);
    children.into_iter().map(|(_, id)| id).collect()
}

// ============================================================================
// Cases 1, 7, 8, 9, 10 — the key appears under two DIFFERENT parents
// ============================================================================

/// The optimistic graft itself matches the oracle: a second parent claiming
/// the key takes the *same* element, rather than a second one being created.
///
/// Flutter parity: `framework_test.dart` `'GlobalKey duplication 1 - double
/// appearance'` (3.44.0) builds exactly this shape. Flutter performs the same
/// graft — `_retakeInactiveElement` takes the element from its live parent —
/// and only *afterwards*, at end of frame, decides the tree was illegal. This
/// test pins the graft half, which the two frameworks agree on; the half they
/// disagree on is pinned by the tug-of-war test below.
#[test]
#[serial_test::serial(global_key_registry)]
fn a_second_parent_grafts_the_same_element_rather_than_creating_another() {
    let (tree, owner) = fresh_tree();
    // `GlobalKey::current_element` reads a realm-scoped registry that is
    // inactive in unit tests driving `ElementTree` directly.
    flui_view::test_only_set_global_key_registry(&tree, &owner);
    let parents = tree_with_parents(&tree, &owner, 2);
    let key = GlobalKey::<KeyedState>::new();

    let first = attach_keyed(&tree, &owner, parents[0], &key, 1);
    assert_eq!(
        children_of(&tree, parents[0]),
        vec![first],
        "the first parent holds the keyed child",
    );

    let second = attach_keyed(&tree, &owner, parents[1], &key, 2);

    assert_eq!(
        second, first,
        "FLUI reuses the element rather than mounting a second one — the \
         element identity is preserved across the relocation",
    );
    assert_eq!(
        children_of(&tree, parents[1]),
        vec![first],
        "the element ends up under the second parent",
    );
    assert!(
        children_of(&tree, parents[0]).is_empty(),
        "and is gone from the first — Flutter's graft leaves the same hole, \
         and repairs it at end of frame; see this file's module doc",
    );
    assert_eq!(
        key.current_element(),
        Some(first),
        "the registry points at the single surviving element",
    );

    flui_view::test_only_clear_global_key_registry();
}

/// The relocation verdict does not depend on the order the two parents are
/// visited in — the oracle's cases 7–10 differ only in that ordering.
///
/// Flutter parity: `framework_test.dart` `'GlobalKey duplication 7 -
/// appearing later'` / `'8 - appearing earlier'` / `'9 - moving and appearing
/// later'` / `'10 - moving and appearing earlier'` (3.44.0), all four of which
/// expect a `FlutterError`. FLUI relocates in every ordering; this pins that
/// the outcome is order-independent, so a future deferred check has one
/// behaviour to replace rather than four.
#[test]
#[serial_test::serial(global_key_registry)]
fn the_relocation_verdict_is_the_same_whichever_parent_claims_the_key_first() {
    for (claim_first, claim_second) in [(0usize, 2usize), (2usize, 0usize)] {
        let (tree, owner) = fresh_tree();
        let parents = tree_with_parents(&tree, &owner, 3);
        let key = GlobalKey::<KeyedState>::new();

        let first = attach_keyed(&tree, &owner, parents[claim_first], &key, 1);
        let second = attach_keyed(&tree, &owner, parents[claim_second], &key, 2);

        assert_eq!(second, first, "the element is relocated, not duplicated");
        assert!(
            children_of(&tree, parents[claim_first]).is_empty(),
            "the earlier claimant loses the child",
        );
        assert_eq!(
            children_of(&tree, parents[claim_second]),
            vec![first],
            "the later claimant holds it",
        );

        flui_view::test_only_clear_global_key_registry();
    }
}

/// **Divergence, pinned.** When the first parent claims the key *back* — the
/// shape that proves the optimistic graft was wrong — FLUI grafts it again and
/// never reports anything. The oracle rejects this tree.
///
/// Flutter parity: `framework_test.dart` `'GlobalKey duplication 1 - double
/// appearance'` and the ordering variants `'7 - appearing later'`, `'8 -
/// appearing earlier'`, `'9 - moving and appearing later'`, `'10 - moving and
/// appearing earlier'` (3.44.0). All five build a tree in which two parents
/// hold the key at once and all five expect a `FlutterError`. Flutter reaches
/// that verdict at end of frame: the loser was recorded by
/// `_debugTrackElementThatWillNeedToBeRebuiltDueToGlobalKeyShenanigans`, and
/// `_debugVerifyGlobalKeyReservation` sees both parents reserving the same key.
///
/// FLUI has no end-of-frame verification, so each claim is just another graft:
/// the element ping-pongs between the parents and the tree is silently left
/// with whichever parent asked last. Filed in `docs/ROADMAP.md` Cross.H
/// (search `_debugVerifyGlobalKeyReservation`).
///
/// This asserts the current behaviour, so it fails the day the verification
/// lands — which is the intent.
#[test]
#[serial_test::serial(global_key_registry)]
fn two_parents_claiming_the_key_in_turn_are_never_reported_as_a_duplicate() {
    let (tree, owner) = fresh_tree();
    let parents = tree_with_parents(&tree, &owner, 2);
    let key = GlobalKey::<KeyedState>::new();

    let original = attach_keyed(&tree, &owner, parents[0], &key, 1);
    attach_keyed(&tree, &owner, parents[1], &key, 2);
    // The first parent asks for it back — under the oracle this is the moment
    // the tree is provably illegal, because both parents now want the key.
    let back = attach_keyed(&tree, &owner, parents[0], &key, 3);

    assert_eq!(
        back, original,
        "the same element is grafted a second time, with no complaint",
    );
    assert_eq!(
        children_of(&tree, parents[0]),
        vec![original],
        "the last claimant holds it",
    );
    assert!(
        children_of(&tree, parents[1]).is_empty(),
        "and the other is left empty — no duplicate is ever reported",
    );

    flui_view::test_only_clear_global_key_registry();
}

/// The relocated element keeps its state — this is a move, not a remount.
///
/// The oracle has no counterpart (it rejects the tree outright), but the
/// property is what makes FLUI's verdict a *relocation* rather than a silent
/// drop-and-recreate: without it, the divergence above would be losing user
/// state as well as diverging on the error.
#[test]
#[serial_test::serial(global_key_registry)]
fn the_relocated_element_keeps_the_state_it_was_created_with() {
    let (tree, owner) = fresh_tree();
    flui_view::test_only_set_global_key_registry(&tree, &owner);
    let parents = tree_with_parents(&tree, &owner, 2);
    let key = GlobalKey::<KeyedState>::new();

    attach_keyed(&tree, &owner, parents[0], &key, 41);
    attach_keyed(&tree, &owner, parents[1], &key, 99);

    let tag = key
        .with_current_state(|state: &KeyedState| state.tag)
        .expect("the surviving element still carries state");
    assert_eq!(
        tag, 41,
        "the state belongs to the element created at the first attachment — \
         the second view's tag (99) would mean a fresh state was built",
    );

    flui_view::test_only_clear_global_key_registry();
}

// ============================================================================
// Case 11 — the key appears twice under the SAME parent
// ============================================================================

/// **Matches the oracle.** Two children with the same `GlobalKey` under one
/// parent is rejected.
///
/// Flutter parity: `framework_test.dart` `'GlobalKey duplication 11 - double
/// sibling appearance'` (3.44.0), which expects a `FlutterError`. FLUI rejects
/// it too, though as a debug panic from `retake_active_global_key` rather than
/// an error object — the verdict is the same, the channel differs (see this
/// file's module doc).
///
/// This is the one shape FLUI's eager check *can* recognise: the second
/// attachment names the parent that already holds the element, so it is
/// provably not a relocation.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "duplicate GlobalKey children are not allowed")]
#[serial_test::serial(global_key_registry)]
fn one_key_twice_under_the_same_parent_is_rejected() {
    let (tree, owner) = fresh_tree();
    let parents = tree_with_parents(&tree, &owner, 1);
    let key = GlobalKey::<KeyedState>::new();

    attach_keyed(&tree, &owner, parents[0], &key, 1);
    attach_keyed(&tree, &owner, parents[0], &key, 2);
}
