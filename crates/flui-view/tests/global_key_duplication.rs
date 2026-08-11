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
//! `docs/ROADMAP.md` under the foundation-hardening gaps (search
//! `_debugVerifyGlobalKeyReservation`).
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
        Leaf.boxed()
    }
}

/// A terminal leaf — a render view, so building it mounts a render object and
/// stops.
///
/// It exists because [`Filler`] cannot be a build *output*. `Filler::build`
/// returns `self.clone()`, which is harmless for the tests that only call
/// `ElementTree::insert` (nothing ever builds it) and non-terminating the
/// moment a real build pass runs: each Filler builds another Filler, forever.
/// Anything reached through `build_scope` in this file must bottom out here.
#[derive(Clone)]
struct Leaf;

impl flui_view::RenderView for Leaf {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = flui_objects::RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        flui_objects::RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for Leaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
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

/// **Divergence, pinned.** When the first parent claims the key *back* — a
/// tree the oracle rejects outright — FLUI grafts it again and never reports
/// anything.
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
/// with whichever parent asked last. Filed in `docs/ROADMAP.md` (search
/// `_debugVerifyGlobalKeyReservation`).
///
/// What this pins is the graft, not the absent verification — and the two must
/// not be conflated. `retake_active_global_key` unlinks the element from the
/// previous parent before relinking it, so no two parents' child lists ever
/// name it at once, and a bare sequence of inserts crosses no build/finalize
/// boundary. A reservation check modelled on Flutter's records the *declaring*
/// parent during build and verifies at the frame boundary, so it would not
/// necessarily fire on this shape: treat this as a description of today's
/// relocation semantics, not as a canary that goes red when the verification
/// lands.
/// The canary is
/// [`two_parents_declaring_one_key_survive_a_whole_frame_unreported`] below,
/// which drives the frame boundary a reservation check would hook into.
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

/// A parent whose *build output* carries the key — the oracle's `Container`
/// with a keyed child, expressed through the build path rather than by a
/// direct tree insert. This is what makes a reservation check observable:
/// the key is declared by a build, which is where Flutter records it.
#[derive(Clone)]
struct KeyedParent {
    key: GlobalKey<KeyedState>,
    tag: i32,
}

impl StatelessView for KeyedParent {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Keyed {
            key: self.key.clone(),
            tag: self.tag,
        }
        .boxed()
    }
}

impl View for KeyedParent {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

/// **The canary for the missing end-of-frame duplicate-key verification.**
/// Two parents each *build* a child carrying the same
/// key; a whole frame runs — `build_scope` then `finalize_tree` — and nothing
/// is reported. The oracle rejects exactly this tree.
///
/// Flutter parity: `framework_test.dart` `'GlobalKey duplication 1 - double
/// appearance'` and the ordering variants `'7'`–`'10'` (3.44.0), all of which
/// expect a `FlutterError`. Flutter records the losing parent during build
/// (`_debugTrackElementThatWillNeedToBeRebuiltDueToGlobalKeyShenanigans`) and
/// verifies at the frame boundary (`_debugVerifyGlobalKeyReservation` after
/// `buildScope`, `_debugVerifyIllFatedPopulation` in `finalizeTree`).
///
/// Why this shape rather than a sequence of inserts: the graft
/// (`retake_active_global_key`) unlinks the child from its previous parent
/// before relinking it, so raw inserts never leave two parents claiming the
/// key and never cross a frame boundary — a reservation check need not fire on
/// them at all. Here both parents genuinely declare the key in one frame, so a
/// check placed where Flutter places it *must* see the conflict.
///
/// The assertions describe today's behaviour: exactly one keyed element
/// survives, held by whichever parent built last, and the frame completes
/// without a panic. Once the verification lands, the frame will instead report a
/// duplicate and this test goes red — which is the point of keeping it.
#[test]
#[serial_test::serial(global_key_registry)]
fn two_parents_declaring_one_key_survive_a_whole_frame_unreported() {
    let (tree, owner) = fresh_tree();
    flui_view::test_only_set_global_key_registry(&tree, &owner);
    let key = GlobalKey::<KeyedState>::new();

    let root = tree
        .write()
        .mount_root(&Filler, &mut owner.write().element_owner_mut());
    let parent_a = tree.write().insert(
        &KeyedParent {
            key: key.clone(),
            tag: 1,
        },
        root,
        0,
        &mut owner.write().element_owner_mut(),
    );
    let parent_b = tree.write().insert(
        &KeyedParent {
            key: key.clone(),
            tag: 2,
        },
        root,
        1,
        &mut owner.write().element_owner_mut(),
    );

    // Drive the boundary a reservation check would hook into. Both guards must
    // be held across the call — `build_scope` takes `&mut` to each — which is
    // why nothing inside may re-enter these locks.
    {
        let mut owner_guard = owner.write();
        owner_guard.schedule_build_for(parent_a, 1, flui_view::RebuildReason::InitialMount);
        owner_guard.schedule_build_for(parent_b, 1, flui_view::RebuildReason::InitialMount);
        let mut tree_guard = tree.write();
        owner_guard.build_scope(&mut tree_guard);
        owner_guard.finalize_tree(&mut tree_guard);
    }

    let a_children = children_of(&tree, parent_a);
    let b_children = children_of(&tree, parent_b);

    assert_eq!(
        a_children.len() + b_children.len(),
        1,
        "one keyed element for two declarations — the duplicate is real and \
         unreported (a: {a_children:?}, b: {b_children:?})",
    );

    // That count alone would also hold if only ONE parent had ever built, so
    // it cannot be the whole proof. The state settles it: `create_state` runs
    // once, at the first mount, so the surviving state carries the tag of the
    // parent that built FIRST, while the parent now holding the child is the
    // one that built LAST. Different values mean both parents ran and the
    // second took the element from the first — which is the duplicate.
    //
    // Deliberately not asserting *which* parent wins: `DirtyElement::cmp`
    // orders the dirty heap by depth alone, so two siblings at the same depth
    // have no defined build order. Pinning one would be pinning heap
    // incidentals, and would break on a change that leaves this gap exactly as
    // it is.
    let creator_tag = key
        .with_current_state(|state: &KeyedState| state.tag)
        .expect("the surviving element still carries state");
    let holder_tag = if a_children.is_empty() { 2 } else { 1 };
    assert_ne!(
        creator_tag, holder_tag,
        "both parents built: one created the keyed element (tag {creator_tag}) \
         and the other took it over (tag {holder_tag}) — a frame in which only \
         one parent built would leave these equal",
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
