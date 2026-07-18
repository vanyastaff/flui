//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/focus_manager_test.dart` and
//! `.../focus_scope_test.dart` (tag `3.44.0`). `focus_traversal_test.dart` was
//! read too — see *Not ported* for why almost none of it lands here.
//!
//! FLUI's `Focus`/`FocusScope`/`ExcludeFocus` widgets
//! (`crates/flui-widgets/src/interaction/focus.rs`) already carry a
//! substantial self-authored unit-test suite (mount/unmount lifecycle,
//! autofocus, `on_focus_change`, rebuild-config-reset, Tab traversal), and
//! `flui-interaction`'s `FocusManager`/`FocusNode`/`FocusScopeNode` carry an
//! even larger one (traversal edge behavior, key-event bubbling, reparenting).
//! This file's job is different, matching this crate's `navigator_test.rs`
//! precedent: every case below is anchored to a **named upstream Flutter
//! test** and asserts the sequence or contrast Flutter itself asserts. Where
//! a case is already fully exercised by an existing self-authored test, the
//! citation was added there instead of duplicating it here (see each
//! function's doc comment in `interaction/focus.rs`).
//!
//! Two styles of case appear, matching each oracle test's own style:
//! - **Widget-mounted** — a `Focus`/`FocusScope`/`ExcludeFocus` tree is built
//!   through this crate's headless [`crate::common::lay_out`] harness, for
//!   cases whose Flutter oracle is itself about widget mount/rebuild/unmount
//!   behavior (the `ViewState` glue, not just the node API).
//! - **Node-level** — `FocusNode`/`FocusScopeNode`/`FocusManager` are
//!   exercised directly, for cases whose Flutter oracle asserts a node-API
//!   contract and only uses a widget to obtain a `BuildContext`/node
//!   reference (FLUI has no `Focus.of`/`FocusScope.of` ambient lookup — see
//!   *Not ported* — so the direct node reference stands in for it).
//!
//! `FocusManager::global()` is an owner-thread (thread-local) singleton
//! (`crates/flui-interaction/src/routing/focus.rs`); nextest's libtest runner
//! reuses OS threads across many `#[test]` functions in this binary, so every
//! case below takes [`FOCUS_TEST_LOCK`] and explicitly `unfocus()`s /
//! detaches whatever it attached, mirroring `flui-interaction`'s own
//! `GLOBAL_FOCUS_LOCK` convention (`routing/focus_scope.rs`) and
//! `flui-widgets`' crate-private `FOCUS_TEST_LOCK`
//! (`src/test_harness.rs`, unreachable from this external integration-test
//! crate).
//!
//! ## Ported cases
//! - `'Can add children to scope and focus'` (focus_manager_test.dart) —
//!   `hasFocus` (any descendant focused) vs `hasPrimaryFocus` (this node
//!   specifically) contrast on a parent/child/child chain, node-level —
//!   [`can_add_children_to_scope_and_focus_contrasts_has_focus_and_has_primary_focus`].
//! - `'Can focus'` (focus_scope_test.dart) — `request_focus` on a mounted
//!   `Focus`'s node yields `has_focus` —
//!   [`can_focus_via_request_focus`].
//! - `'Can unfocus'` (focus_scope_test.dart) — focusing sibling B unfocuses A
//!   (focus gain/loss ordering) — [`can_unfocus_by_focusing_a_sibling`].
//! - `'Can have multiple focused children and they update accordingly'`
//!   (focus_scope_test.dart) — autofocus plus toggling focus back and forth
//!   between two siblings —
//!   [`multiple_focused_children_update_accordingly_as_focus_moves_between_siblings`].
//! - `'Removing focused widget moves focus to next widget'`
//!   (focus_scope_test.dart) — despite the name, the assertion is that focus
//!   is **not** auto-transferred to the survivor; a dispose-while-focused
//!   lifecycle case —
//!   [`removing_the_focused_widget_does_not_transfer_focus_to_the_survivor`].
//! - `"Removing focused widget doesn't move focus to next widget within
//!   FocusScope"` (focus_scope_test.dart) — the scoped variant —
//!   [`removing_the_focused_widget_within_a_scope_does_not_transfer_focus`].
//! - `'Adding a new FocusScope attaches the child to its parent.'`
//!   (focus_scope_test.dart) — FocusScope node capture: a child scope added
//!   on a later rebuild attaches under the parent scope's node —
//!   [`adding_a_new_focus_scope_attaches_its_node_under_the_parent_scope`].
//! - `'Can focus root node.'` (focus_scope_test.dart) — the root scope itself
//!   can hold primary focus. **Adapted**: node-level — the oracle mounts a
//!   widget only to reach `FocusScope.of`, which FLUI does not have —
//!   [`can_focus_the_root_scope_directly`].
//! - `'Focus is ignored when set to not focusable.'` (focus_scope_test.dart)
//!   — `canRequestFocus: false` refuses a `request_focus` call and
//!   `on_focus_change` never fires — [`focus_is_ignored_when_not_focusable`].
//! - `'Focus is lost when set to not focusable.'` (focus_scope_test.dart) —
//!   flipping `canRequestFocus` to `false` on a rebuild releases focus the
//!   node currently holds — exercises the `FocusNode::set_can_request_focus`
//!   fix landed alongside this port (see below) —
//!   [`focus_is_lost_when_set_to_not_focusable_mid_focus`].
//! - `'Child of unfocusable Focus can get focus.'` (focus_scope_test.dart) —
//!   `canRequestFocus: false` gates only the node it is set on, not its
//!   descendants — [`child_of_an_unfocusable_focus_can_still_get_focus`].
//! - `'descendantsAreFocusable works as expected.'` (focus_scope_test.dart) —
//!   the inverse gate: `descendantsAreFocusable: false` blocks every
//!   descendant while leaving the node's own eligibility alone —
//!   [`descendants_are_focusable_gates_descendants_not_the_node_itself`].
//! - `'canRequestFocus causes descendants of scope to be skipped.'`
//!   (focus_scope_test.dart) — **Adapted and split in two**: node-level
//!   against `FocusNode`/`FocusScopeNode` directly (the oracle's widget tree
//!   only supplies node references via `GlobalKey`), and trimmed to one
//!   scope level (the oracle's outer `scope1` layer asserts the identical
//!   scope-level contract `scope2` already covers) at the oracle's own
//!   `focus1`-holds-`focus2` depth. Dropped from the oracle: the redundant
//!   outer `scope1` sub-case; the `unfocus()`-then-refocus round-trip inside
//!   the plain-Focus-ancestor sub-case; and re-mounting a whole widget tree
//!   per state (each kept assertion runs directly against the current node
//!   state instead). Kept: a plain `Focus`'s own `canRequestFocus: false`
//!   does **not** restrict its descendants —
//!   [`can_request_focus_on_a_plain_focus_does_not_restrict_its_descendants`]
//!   — but a **scope**'s does, in both directions the oracle checks:
//!   disabling it before a descendant requests refuses the request, and
//!   disabling it *while* a descendant already holds focus evicts that
//!   focus (the oracle's `pumpTest(allowScope2: false)` step, checked while
//!   `focus2` held focus) — and re-enabling restores descendant focus —
//!   [`can_request_focus_on_a_scope_restricts_its_descendants`].
//! - `'Nodes are removed when all Focuses are removed.'`
//!   (focus_scope_test.dart) — focus node lifecycle: unmounting detaches the
//!   node and releases the focus it held. **Adapted**: the oracle's
//!   `rootScope.descendants, hasLength(2)` counts Flutter's baseline
//!   traversal-group nodes, which FLUI has no equivalent of; ported as
//!   "detached and unfocused" instead of an exact count —
//!   [`nodes_are_removed_when_all_focuses_are_removed`].
//! - `'FocusManager notifies listeners when a widget loses focus because it
//!   was removed.'` (focus_manager_test.dart) — removing the focused
//!   widget's node notifies exactly once, and the survivor does not inherit
//!   focus —
//!   [`removing_a_focused_node_notifies_listeners_exactly_once_without_transferring_focus`].
//! - `'Focus changes notify listeners.'` (focus_manager_test.dart) —
//!   **Adapted**: the oracle's `child1→child2→child1` burst collapses to one
//!   notification because Flutter batches focus changes to end-of-frame;
//!   FLUI's `FocusManager` notifies synchronously per call (module doc,
//!   ADR-0022) — ported as two synchronous notifications instead of one
//!   batched one, with the divergence stated inline —
//!   [`focus_changes_notify_listeners_on_each_synchronous_request`].
//!
//! ## Not ported
//! - `Focus.of`/`Focus.maybeOf`/`FocusScope.of` (ambient lookup) and every
//!   case whose only role for them is obtaining a node reference — already
//!   documented not-ported in `interaction/focus.rs`'s module doc; a direct
//!   node/scope reference stands in wherever the case is otherwise portable
//!   (see *Adapted* notes above).
//! - `'Setting first focus requests focus for the scope properly.'`,
//!   `'Can move focus in and out of FocusScope'`, `'Moving widget from one
//!   scope to another retains focus'`, `'Moving FocusScopeNodes retains
//!   focus'`, and the `isFirstFocus`/pinned-`GlobalKey` cases — Flutter's
//!   `FocusScopeNode.setFirstFocus`/`isFirstFocus` designate which child a
//!   parent scope restores focus to; FLUI's `FocusScopeNode::set_first_focus`
//!   instead focuses the first *traversable* descendant directly and has no
//!   restore-priority concept. The underlying "reparenting preserves primary
//!   focus" invariant these lean on is already covered node-level by
//!   `flui-interaction`'s `adopt_preserves_primary_focus_across_a_reparent`.
//! - `'Autofocus works with global key reparenting'`, `"Doesn't lose focused
//!   child when reparenting if the nearestScope doesn't change."` — FLUI's
//!   reparent path is `did_change_dependencies`-driven (ADR-0022), not
//!   `GlobalKey` cross-parent teleport; same underlying invariant as above.
//! - `'node detached before autofocus is applied'`, `'works when the
//!   previous focused node is detached'` — edge cases in Flutter's two-phase
//!   `FocusAttachment.attach`/`.reparent()` lifecycle, which FLUI's
//!   single-phase `attach_node` does not reproduce.
//! - `'Setting canRequestFocus on focus node causes update.'` — a rebuild
//!   bookkeeping check with no distinct FLUI behavior beyond what
//!   `a_rebuild_resets_dropped_focus_config` (`interaction/focus.rs`) already
//!   exercises.
//! - `'skipTraversal works as expected.'`, `'descendantsAreTraversable works
//!   as expected.'` — `skipTraversal` is already covered
//!   (`a_rebuild_resets_dropped_focus_config`,
//!   `a_skip_traversal_cursor_still_steps_to_the_node_after_it` in
//!   `flui-interaction`); `descendantsAreTraversable` has no FLUI node-layer
//!   flag (documented not-ported in `interaction/focus.rs`).
//! - `'Focus updates the onKey handler...'` (legacy `onKey`) — legacy
//!   `onKey` is not ported. The modern `onKeyEvent` half is covered by
//!   `a_rebuild_resets_dropped_focus_config`.
//! - `'Focus passes changes in attribute values to its focus node'` —
//!   covered by `a_rebuild_resets_dropped_focus_config`.
//! - `'Focus widgets set Semantics information about focus'`, `"Focus
//!   doesn't introduce a Semantics node when includeSemantics is false"`,
//!   `"ExcludeFocus doesn't introduce a Semantics node"`, `'Focus widget
//!   gains input focus when it gains accessibility focus'` — no semantics-
//!   layer integration for `Focus` yet (`includeSemantics` documented
//!   not-ported).
//! - `'Initial highlight mode guesses correctly.'` and the other
//!   `FocusHighlightMode` cases — `FocusHighlightMode` (mouse/keyboard/touch
//!   interaction-mode heuristic) is not implemented in FLUI.
//! - `'Scopes can be focused without sending focus to descendants.'` — no
//!   FLUI scenario distinct from `can_focus_the_root_scope_directly`, which
//!   already proves a scope alone can hold primary focus with no descendant
//!   involved.
//! - `hasAGoodToStringDeep`/`toStringDeep`/`debugDescribeFocusTree`/
//!   `debugFillProperties`/`debugFocusChanges` cases — no `Diagnosticable`
//!   tree-dump equivalent for the focus tree.
//! - `'Ancestors get notified exactly as often as needed if focused child
//!   changes focus.'` — the dedup-on-edges contract this pins is already
//!   covered widget-level by `on_focus_change_reports_gain_and_loss`
//!   (`interaction/focus.rs`).
//! - `'Unfocus with disposition previouslyFocusedChild works properly'`,
//!   `'...disposition scope...'`, `'Unfocus works properly when some nodes
//!   are unfocusable'` — `UnfocusDisposition` has no FLUI equivalent;
//!   `FocusManager::unfocus`/`FocusNode::unfocus` always clear to `None`.
//!   Adding a disposition parameter is a new public-API decision, out of
//!   this test-port's bounds.
//! - `'Removing a FocusScope removes its node from the tree'` — already
//!   fully exercised widget-level by
//!   `a_focus_widget_attaches_under_the_nearest_scope_and_unmount_releases`
//!   (`interaction/focus.rs`), which the citation was added to instead.
//! - `'Autofocus works'` (focus_scope_test.dart) and `'Can autofocus a
//!   node.'` — the mount-time half is already covered by
//!   `a_focus_widget_attaches_under_the_nearest_scope_and_unmount_releases`;
//!   the rebuild-time half (`autofocus` flipping `false` → `true` on an
//!   already-mounted node) exposed a real gap, fixed alongside this port and
//!   pinned by the new `a_rebuild_that_turns_on_autofocus_requests_focus`
//!   unit test (both in `interaction/focus.rs`).
//! - `"Won't autofocus a node if one is already focused."` — already
//!   exercised by `autofocus_yields_to_an_already_focused_scope`
//!   (`interaction/focus.rs`), which the citation was added to instead.
//! - `"Descendants of ExcludeFocus aren't focusable."`, `"ExcludeFocus
//!   doesn't transfer focus to another descendant."` — already exercised by
//!   `exclude_focus_refuses_allows_evicts_idempotently_and_does_not_refocus`
//!   (`interaction/focus.rs`), which the citations were added to instead.
//! - `'Requesting focus before adding to tree results in a request after
//!   adding'` (focus_manager_test.dart) — a real gap (`FocusNode::request_focus`
//!   was an unconditional no-op while unattached), fixed alongside this port
//!   in `flui-interaction` and pinned by
//!   `request_focus_before_attach_is_granted_on_attach` +
//!   `a_pending_request_dropped_by_can_request_focus_does_not_linger`
//!   (`crates/flui-interaction/src/routing/focus_scope.rs`).
//! - `focus_traversal_test.dart` as a whole — its `FocusTraversalGroup` /
//!   per-subtree traversal-**policy**-selection widget does not exist in
//!   FLUI (traversal itself — `FocusManager::focus_next`/`focus_previous`
//!   against `ReadingOrderPolicy` — is implemented and extensively covered
//!   node-level in `flui-interaction` and widget-level by
//!   `tab_traversal_follows_geometry_not_attach_order`,
//!   `interaction/focus.rs`).
//! - `'FocusScope does not crash at zero area'`, `'Focus does not crash at
//!   zero area'` — a physical-size-zero layout/paint edge case orthogonal to
//!   focus semantics, not meaningfully portable without a platform-size
//!   harness.
//! - `"FocusScope doesn't update the focusNode attributes when the widget
//!   updates if withExternalFocusNode is used"` — exercises legacy `onKey`
//!   (not ported) and `descendantsAreTraversable` (no FLUI flag); the
//!   `onKeyEvent`/`descendantsAreFocusable` half is covered by
//!   `a_rebuild_resets_dropped_focus_config`.
//! - `'Focus.of stops at the nearest Focus widget.'`, `'Can traverse Focus
//!   children.'` — `Focus.of` is not ported; the traversal-ordering half of
//!   the latter is covered by `tab_traversal_follows_geometry_not_attach_order`.
//! - `'Can set focus.'` — redundant with `'Can focus'` (ported) plus
//!   `on_focus_change` coverage already in `interaction/focus.rs`.
//!
//! Widget → node mapping: `Focus` → `FocusNode`, `FocusScope` →
//! `FocusScopeNode`, both via `flui-interaction`'s `routing` module.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_interaction::{FocusManager, FocusNode, FocusScopeNode};
use flui_widgets::prelude::*;
use flui_widgets::{Column, Focus, FocusScope, SizedBox};
use parking_lot::Mutex;

use crate::common::{lay_out, loose};

/// Conservatively serializes this file's focus fixtures on top of
/// `FocusManager::global()`'s owner-thread singleton — see the module doc.
static FOCUS_TEST_LOCK: Mutex<()> = Mutex::new(());

/// A leaf big enough to mount without tripping any zero-size edge case, and
/// otherwise inert — geometry is not what these tests assert about.
fn leaf() -> SizedBox {
    SizedBox::new(10.0, 10.0)
}

// ============================================================================
// Reusable fixtures
// ============================================================================

/// Zero, one, or two `Focus` widgets (each driving its own external node) in
/// a `Column`, with no explicit `FocusScope` wrapper — matching the bare
/// `Column(children: [TestFocus, TestFocus])` shape most `focus_scope_test
/// .dart`/`focus_manager_test.dart` cases mount. Falls back to
/// `FocusManager::global().root_scope()` (`Focus`'s own documented fallback).
///
/// Both `Column` slots are always present — a hidden slot renders an inert
/// placeholder rather than being omitted from the list. `Column`'s
/// reconciliation is positional: omitting a slot would shift the survivor
/// into the removed slot's *position*, reusing that position's existing
/// `Focus` element (and its `FocusState`, which keeps its original node —
/// external nodes are read once, see `Focus::focus_node`'s doc) instead of
/// actually unmounting the node this fixture means to remove.
#[derive(Clone, StatelessView)]
struct TwoFocusHost {
    a: Arc<FocusNode>,
    b: Arc<FocusNode>,
    show_a: bool,
    show_b: bool,
    a_autofocus: bool,
    b_autofocus: bool,
}

impl StatelessView for TwoFocusHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let slot_a: BoxedView = if self.show_a {
            Focus::new(leaf())
                .focus_node(Arc::clone(&self.a))
                .autofocus(self.a_autofocus)
                .into_view()
                .boxed()
        } else {
            leaf().into_view().boxed()
        };
        let slot_b: BoxedView = if self.show_b {
            Focus::new(leaf())
                .focus_node(Arc::clone(&self.b))
                .autofocus(self.b_autofocus)
                .into_view()
                .boxed()
        } else {
            leaf().into_view().boxed()
        };
        Column::new(vec![slot_a, slot_b])
    }
}

/// A parent `FocusScope` whose child scope is present only when
/// `show_child` is set — `'Adding a new FocusScope attaches the child to its
/// parent.'`'s shape.
#[derive(Clone, StatelessView)]
struct NestedScopeHost {
    parent: Arc<FocusScopeNode>,
    child: Arc<FocusScopeNode>,
    show_child: bool,
}

impl StatelessView for NestedScopeHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let inner: BoxedView = if self.show_child {
            FocusScope::with_external_node(Arc::clone(&self.child), leaf())
                .into_view()
                .boxed()
        } else {
            leaf().into_view().boxed()
        };
        FocusScope::with_external_node(Arc::clone(&self.parent), inner)
    }
}

// ============================================================================
// Node-level cases (focus_manager_test.dart style)
// ============================================================================

/// Oracle: `'Can add children to scope and focus'` (focus_manager_test.dart).
///
/// `hasFocus` (this node or any descendant) vs `hasPrimaryFocus` (this node
/// specifically) on a `scope > parent > {child1, child2}` chain, switching
/// which child holds focus.
#[test]
fn can_add_children_to_scope_and_focus_contrasts_has_focus_and_has_primary_focus() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let scope = FocusScopeNode::with_debug_label("add-children-scope");
    manager.root_scope().attach_node(scope.as_focus_node());
    let parent = FocusNode::with_debug_label("parent");
    scope.attach_node(&parent);
    let child1 = FocusNode::with_debug_label("child1");
    parent.attach_node(&child1);
    let child2 = FocusNode::with_debug_label("child2");
    parent.attach_node(&child2);

    child1.request_focus();
    assert_eq!(scope.focused_child(), Some(child1.id()));
    assert!(
        parent.has_focus(),
        "an ancestor of the focused node reports hasFocus"
    );
    assert!(
        !parent.has_primary_focus(),
        "but not hasPrimaryFocus — it isn't the focused node itself"
    );
    assert!(child1.has_focus());
    assert!(child1.has_primary_focus());
    assert!(!child2.has_focus());
    assert!(!child2.has_primary_focus());

    child2.request_focus();
    assert_eq!(scope.focused_child(), Some(child2.id()));
    assert!(parent.has_focus());
    assert!(!parent.has_primary_focus());
    assert!(!child1.has_focus());
    assert!(!child1.has_primary_focus());
    assert!(child2.has_focus());
    assert!(child2.has_primary_focus());

    manager.unfocus();
    manager.root_scope().detach_node(scope.as_focus_node().id());
}

/// Oracle: `'Can focus root node.'` (focus_scope_test.dart).
///
/// Adapted: node-level — the oracle mounts a `Focus` widget only to reach
/// `FocusScope.of(element)`; FLUI has no ambient lookup (module doc), so the
/// root scope's own backing node is exercised directly.
#[test]
fn can_focus_the_root_scope_directly() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let root = manager.root_scope().as_focus_node();
    root.request_focus();

    assert!(root.has_primary_focus(), "the root scope itself can focus");

    manager.unfocus();
}

/// Oracle: `'Focus changes notify listeners.'` (focus_manager_test.dart).
///
/// Adapted: the oracle's `child1.requestFocus(); child2.requestFocus();
/// child1.requestFocus();` burst collapses to one `notifyCount` because
/// Flutter batches focus changes to end-of-frame (`applyFocusChangesIfNeeded`).
/// FLUI's `FocusManager` notifies synchronously per call (module doc,
/// ADR-0022) — ported as two synchronous notifications for the two real
/// changes, not Flutter's one batched notification.
#[test]
fn focus_changes_notify_listeners_on_each_synchronous_request() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let scope = FocusScopeNode::with_debug_label("notify-scope");
    manager.root_scope().attach_node(scope.as_focus_node());
    let child1 = FocusNode::with_debug_label("child1");
    scope.attach_node(&child1);
    let child2 = FocusNode::with_debug_label("child2");
    scope.attach_node(&child2);

    let notify_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&notify_count);
    let listener_id = manager.add_listener(Rc::new(move |_previous, _new| {
        counter.fetch_add(1, Ordering::SeqCst);
    }));

    child1.request_focus();
    assert_eq!(notify_count.load(Ordering::SeqCst), 1);

    notify_count.store(0, Ordering::SeqCst);
    child2.request_focus();
    child1.request_focus();
    assert_eq!(
        notify_count.load(Ordering::SeqCst),
        2,
        "two real focus changes, notified synchronously per change — not \
         batched into Flutter's single end-of-frame notification"
    );

    notify_count.store(0, Ordering::SeqCst);
    child1.unfocus();
    assert_eq!(notify_count.load(Ordering::SeqCst), 1);

    manager.remove_listener(listener_id);
    manager.unfocus();
    manager.root_scope().detach_node(scope.as_focus_node().id());
}

/// Oracle: `'canRequestFocus causes descendants of scope to be skipped.'`
/// (focus_scope_test.dart), the plain-`Focus` half.
///
/// Adapted: node-level, trimmed to one `Focus` level (the oracle nests a
/// second, `focus2`, under `focus1`; one level already shows the contrast).
/// A plain `FocusNode`'s own `canRequestFocus: false` gates only itself.
#[test]
fn can_request_focus_on_a_plain_focus_does_not_restrict_its_descendants() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let scope = FocusScopeNode::with_debug_label("plain-gate-scope");
    manager.root_scope().attach_node(scope.as_focus_node());
    let focus1 = FocusNode::with_debug_label("focus1");
    scope.attach_node(&focus1);
    let focus2 = FocusNode::with_debug_label("focus2");
    focus1.attach_node(&focus2);

    focus1.set_can_request_focus(false);
    focus2.request_focus();

    assert!(
        focus2.has_primary_focus(),
        "an ancestor Focus's canRequestFocus(false) does not restrict a descendant"
    );

    manager.unfocus();
    manager.root_scope().detach_node(scope.as_focus_node().id());
}

/// Oracle: `'canRequestFocus causes descendants of scope to be skipped.'`
/// (focus_scope_test.dart), the scope half — see the module doc's *Adapted*
/// note for what was trimmed and kept.
///
/// A **scope**'s own `canRequestFocus: false` gates every descendant, not
/// just itself — the contrast with the plain-`Focus` case above — in both
/// directions the oracle checks: refusing a fresh request while disabled,
/// AND evicting a descendant's *already-held* focus the moment the scope is
/// disabled (the oracle's `pumpTest(allowScope2: false)` step, checked while
/// `focus2` held focus — `FocusNode.canRequestFocus`'s setter,
/// `focus_manager.dart`, checks `hasFocus`, not `hasPrimaryFocus`, so a
/// scope's own eviction reaches a focused descendant, not just itself).
///
/// Red-check (verified): before `FocusNode::set_can_request_focus` also
/// checked `is_scope() && has_focus()`, the mid-focus disable left `focus2`
/// focused and the eviction assertion failed.
#[test]
fn can_request_focus_on_a_scope_restricts_its_descendants() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let scope = FocusScopeNode::with_debug_label("scope-gate-scope");
    manager.root_scope().attach_node(scope.as_focus_node());
    let focus1 = FocusNode::with_debug_label("focus1");
    scope.attach_node(&focus1);
    let focus2 = FocusNode::with_debug_label("focus2");
    focus1.attach_node(&focus2);

    // `focus2` already holds focus when the scope is disabled — the
    // mid-focus eviction case.
    focus2.request_focus();
    assert!(
        focus2.has_primary_focus(),
        "sanity: focus2 is focused first"
    );

    scope.as_focus_node().set_can_request_focus(false);

    assert!(
        !focus2.has_focus(),
        "disabling the scope evicts the descendant's already-held focus"
    );
    assert_eq!(manager.primary_focus(), None);

    // While still disabled, a fresh request is refused too.
    focus2.request_focus();
    assert!(
        !focus2.has_primary_focus(),
        "the scope's own canRequestFocus(false) blocks every descendant"
    );
    assert_eq!(manager.primary_focus(), None);

    scope.as_focus_node().set_can_request_focus(true);
    focus2.request_focus();

    assert!(
        focus2.has_primary_focus(),
        "re-enabling the scope restores descendant focus"
    );

    manager.unfocus();
    manager.root_scope().detach_node(scope.as_focus_node().id());
}

// ============================================================================
// Widget-mounted cases (focus_scope_test.dart / focus_manager_test.dart style)
// ============================================================================

/// Oracle: `'Can focus'` (focus_scope_test.dart).
#[test]
fn can_focus_via_request_focus() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let a = FocusNode::with_debug_label("a");
    let b = FocusNode::with_debug_label("b");
    let _laid = lay_out(
        TwoFocusHost {
            a: Arc::clone(&a),
            b: Arc::clone(&b),
            show_a: true,
            show_b: false,
            a_autofocus: false,
            b_autofocus: false,
        },
        loose(200.0),
    );

    assert!(!a.has_focus(), "not focused before the request");
    a.request_focus();

    assert!(a.has_focus());

    manager.unfocus();
    manager.root_scope().detach_node(a.id());
}

/// Oracle: `'Can unfocus'` (focus_scope_test.dart) — focus gain/loss
/// ordering: focusing sibling B unfocuses sibling A.
#[test]
fn can_unfocus_by_focusing_a_sibling() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let a = FocusNode::with_debug_label("a");
    let b = FocusNode::with_debug_label("b");
    let _laid = lay_out(
        TwoFocusHost {
            a: Arc::clone(&a),
            b: Arc::clone(&b),
            show_a: true,
            show_b: true,
            a_autofocus: false,
            b_autofocus: false,
        },
        loose(200.0),
    );

    a.request_focus();
    assert!(a.has_focus());
    assert!(!b.has_focus());

    b.request_focus();
    assert!(!a.has_focus(), "focusing b unfocuses a");
    assert!(b.has_focus());

    manager.unfocus();
    manager.root_scope().detach_node(a.id());
    manager.root_scope().detach_node(b.id());
}

/// Oracle: `'Can have multiple focused children and they update accordingly'`
/// (focus_scope_test.dart) — autofocus on mount, then focus toggles back and
/// forth between the two siblings.
#[test]
fn multiple_focused_children_update_accordingly_as_focus_moves_between_siblings() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let a = FocusNode::with_debug_label("a");
    let b = FocusNode::with_debug_label("b");
    let _laid = lay_out(
        TwoFocusHost {
            a: Arc::clone(&a),
            b: Arc::clone(&b),
            show_a: true,
            show_b: true,
            a_autofocus: true,
            b_autofocus: false,
        },
        loose(200.0),
    );

    assert!(a.has_focus(), "autofocus landed on mount");
    assert!(!b.has_focus());

    b.request_focus();
    assert!(!a.has_focus());
    assert!(b.has_focus());

    a.request_focus();
    assert!(a.has_focus());
    assert!(!b.has_focus());

    manager.unfocus();
    manager.root_scope().detach_node(a.id());
    manager.root_scope().detach_node(b.id());
}

/// Oracle: `'Removing focused widget moves focus to next widget'`
/// (focus_scope_test.dart). Despite the name, the assertion is that the
/// survivor does **not** inherit focus — a dispose-while-focused lifecycle
/// case.
#[test]
fn removing_the_focused_widget_does_not_transfer_focus_to_the_survivor() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let a = FocusNode::with_debug_label("a");
    let b = FocusNode::with_debug_label("b");
    let mut laid = lay_out(
        TwoFocusHost {
            a: Arc::clone(&a),
            b: Arc::clone(&b),
            show_a: true,
            show_b: true,
            a_autofocus: false,
            b_autofocus: false,
        },
        loose(200.0),
    );

    a.request_focus();
    assert!(a.has_focus());
    assert!(!b.has_focus());

    laid.pump_widget(TwoFocusHost {
        a: Arc::clone(&a),
        b: Arc::clone(&b),
        show_a: false,
        show_b: true,
        a_autofocus: false,
        b_autofocus: false,
    });

    assert!(!a.is_attached(), "a's widget was removed");
    assert!(
        !b.has_focus(),
        "b does not inherit the focus a's removal released"
    );

    manager.unfocus();
    manager.root_scope().detach_node(b.id());
}

/// Oracle: `"Removing focused widget doesn't move focus to next widget
/// within FocusScope"` (focus_scope_test.dart) — the explicit-`FocusScope`
/// variant of the case above.
#[test]
fn removing_the_focused_widget_within_a_scope_does_not_transfer_focus() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let scope = FocusScopeNode::with_debug_label("removal-scope");
    let a = FocusNode::with_debug_label("a");
    let b = FocusNode::with_debug_label("b");

    // Both `Column` slots stay present (see `TwoFocusHost`'s doc for why an
    // omitted slot would let the survivor reuse the removed slot's element).
    #[derive(Clone, StatelessView)]
    struct ScopedTwoFocusHost {
        scope: Arc<FocusScopeNode>,
        a: Arc<FocusNode>,
        b: Arc<FocusNode>,
        show_a: bool,
    }
    impl StatelessView for ScopedTwoFocusHost {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            let slot_a: BoxedView = if self.show_a {
                Focus::new(leaf())
                    .focus_node(Arc::clone(&self.a))
                    .into_view()
                    .boxed()
            } else {
                leaf().into_view().boxed()
            };
            let slot_b: BoxedView = Focus::new(leaf())
                .focus_node(Arc::clone(&self.b))
                .into_view()
                .boxed();
            FocusScope::with_external_node(
                Arc::clone(&self.scope),
                Column::new(vec![slot_a, slot_b]),
            )
        }
    }

    let mut laid = lay_out(
        ScopedTwoFocusHost {
            scope: Arc::clone(&scope),
            a: Arc::clone(&a),
            b: Arc::clone(&b),
            show_a: true,
        },
        loose(200.0),
    );

    a.request_focus();
    manager.set_active_scope(Some(Arc::clone(&scope)));
    assert!(a.has_focus());

    laid.pump_widget(ScopedTwoFocusHost {
        scope: Arc::clone(&scope),
        a: Arc::clone(&a),
        b: Arc::clone(&b),
        show_a: false,
    });

    assert!(!a.is_attached());
    assert!(
        !b.has_focus(),
        "b does not inherit the focus a's removal released, even inside the scope"
    );

    manager.unfocus();
    manager.set_active_scope(None);
    manager.root_scope().detach_node(scope.as_focus_node().id());
}

/// Oracle: `'Adding a new FocusScope attaches the child to its parent.'`
/// (focus_scope_test.dart) — FocusScope node capture: a child scope added on
/// a later rebuild attaches under the parent scope's node.
#[test]
fn adding_a_new_focus_scope_attaches_its_node_under_the_parent_scope() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let parent_scope = FocusScopeNode::with_debug_label("parent-scope");
    let child_scope = FocusScopeNode::with_debug_label("child-scope");
    let mut laid = lay_out(
        NestedScopeHost {
            parent: Arc::clone(&parent_scope),
            child: Arc::clone(&child_scope),
            show_child: false,
        },
        loose(200.0),
    );

    assert!(
        !child_scope.as_focus_node().is_attached(),
        "the child scope is not yet in the tree"
    );

    laid.pump_widget(NestedScopeHost {
        parent: Arc::clone(&parent_scope),
        child: Arc::clone(&child_scope),
        show_child: true,
    });

    assert_eq!(
        child_scope.as_focus_node().parent().map(|node| node.id()),
        Some(parent_scope.as_focus_node().id()),
        "the child scope now hangs under the parent scope's node"
    );

    manager.unfocus();
    manager
        .root_scope()
        .detach_node(parent_scope.as_focus_node().id());
}

/// Oracle: `'Focus is ignored when set to not focusable.'`
/// (focus_scope_test.dart).
#[test]
fn focus_is_ignored_when_not_focusable() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let node = FocusNode::with_debug_label("unfocusable");
    let got_focus = Arc::new(Mutex::new(Vec::<bool>::new()));
    let recorded = Arc::clone(&got_focus);
    let root = Focus::new(leaf())
        .focus_node(Arc::clone(&node))
        .can_request_focus(false)
        .on_focus_change(move |focused| recorded.lock().push(focused));
    let _laid = lay_out(root, loose(200.0));

    node.request_focus();

    assert!(got_focus.lock().is_empty(), "on_focus_change never fires");
    assert!(!node.has_focus());

    manager.unfocus();
    manager.root_scope().detach_node(node.id());
}

/// Oracle: `'Focus is lost when set to not focusable.'`
/// (focus_scope_test.dart) — exercises the `FocusNode::set_can_request_focus`
/// fix landed alongside this port (see this crate's module doc): flipping
/// `canRequestFocus` to `false` on a rebuild now releases focus the node
/// currently holds, matching Flutter's setter semantics.
#[test]
fn focus_is_lost_when_set_to_not_focusable_mid_focus() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    #[derive(Clone, StatelessView)]
    struct Host {
        node: Arc<FocusNode>,
        can_request_focus: bool,
        on_focus_change: Rc<dyn Fn(bool)>,
    }
    impl StatelessView for Host {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            let handler = Rc::clone(&self.on_focus_change);
            Focus::new(leaf())
                .focus_node(Arc::clone(&self.node))
                .autofocus(true)
                .can_request_focus(self.can_request_focus)
                .on_focus_change(move |focused| handler(focused))
        }
    }

    let node = FocusNode::with_debug_label("was-focusable");
    let got_focus = Arc::new(Mutex::new(Vec::<bool>::new()));
    let recorded = Arc::clone(&got_focus);
    let mut laid = lay_out(
        Host {
            node: Arc::clone(&node),
            can_request_focus: true,
            on_focus_change: Rc::new(move |focused| recorded.lock().push(focused)),
        },
        loose(200.0),
    );

    assert!(node.has_focus(), "autofocus landed on mount");
    assert_eq!(got_focus.lock().as_slice(), [true]);
    got_focus.lock().clear();

    let recorded = Arc::clone(&got_focus);
    laid.pump_widget(Host {
        node: Arc::clone(&node),
        can_request_focus: false,
        on_focus_change: Rc::new(move |focused| recorded.lock().push(focused)),
    });

    assert!(
        !node.has_focus(),
        "flipping canRequestFocus false released the focus this node held"
    );
    assert_eq!(
        got_focus.lock().as_slice(),
        [false],
        "on_focus_change reports the loss, matching the oracle's `expect(gotFocus, false)`"
    );

    manager.unfocus();
    manager.root_scope().detach_node(node.id());
}

/// Oracle: `'Child of unfocusable Focus can get focus.'`
/// (focus_scope_test.dart) — `canRequestFocus: false` gates only the node it
/// is set on, not its descendants.
#[test]
fn child_of_an_unfocusable_focus_can_still_get_focus() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let outer = FocusNode::with_debug_label("outer-unfocusable");
    let inner = FocusNode::with_debug_label("inner");
    let root = Focus::new(Focus::new(leaf()).focus_node(Arc::clone(&inner)))
        .focus_node(Arc::clone(&outer))
        .can_request_focus(false);
    let _laid = lay_out(root, loose(200.0));

    outer.request_focus();
    assert!(!outer.has_focus(), "the unfocusable outer refuses");

    inner.request_focus();
    assert!(
        inner.has_focus(),
        "its child is not gated by the parent's canRequestFocus"
    );
    assert!(
        outer.has_focus(),
        "the outer now reports hasFocus transitively, through the focused child"
    );

    manager.unfocus();
    manager.root_scope().detach_node(outer.id());
}

/// Oracle: `'descendantsAreFocusable works as expected.'`
/// (focus_scope_test.dart) — the inverse gate from the case above:
/// `descendantsAreFocusable: false` blocks every descendant while leaving
/// the node's own eligibility alone.
#[test]
fn descendants_are_focusable_gates_descendants_not_the_node_itself() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let outer = FocusNode::with_debug_label("blocks-descendants");
    let inner = FocusNode::with_debug_label("blocked-descendant");
    let root = Focus::new(Focus::new(leaf()).focus_node(Arc::clone(&inner)))
        .focus_node(Arc::clone(&outer))
        .descendants_are_focusable(false);
    let _laid = lay_out(root, loose(200.0));

    inner.request_focus();
    assert!(
        !inner.has_focus(),
        "descendantsAreFocusable(false) blocks the descendant's request"
    );

    outer.request_focus();
    assert!(
        outer.has_focus(),
        "the node's own eligibility is untouched by its descendants-are-focusable flag"
    );

    manager.unfocus();
    manager.root_scope().detach_node(outer.id());
}

/// Oracle: `'Nodes are removed when all Focuses are removed.'`
/// (focus_scope_test.dart) — focus node lifecycle: unmounting detaches the
/// node and releases the focus it held.
///
/// Adapted: the oracle's `rootScope.descendants, hasLength(2)` counts
/// Flutter's baseline traversal-group/view-scope nodes, which FLUI's
/// simpler, single-phase attach model has no equivalent of; ported as
/// "detached and unfocused" rather than an exact descendant count.
#[test]
fn nodes_are_removed_when_all_focuses_are_removed() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    // A stable root TYPE that toggles its shape internally — `pump_widget`
    // dispatches by `TypeId`, so swapping between two *different* concrete
    // root widget types is not the supported way to unmount a subtree (see
    // `LaidOut::pump_widget`'s doc); `show` is.
    #[derive(Clone, StatelessView)]
    struct SoloFocusHost {
        scope: Arc<FocusScopeNode>,
        node: Arc<FocusNode>,
        on_focus_change: Rc<dyn Fn(bool)>,
        show: bool,
    }
    impl StatelessView for SoloFocusHost {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            if !self.show {
                return leaf().into_view().boxed();
            }
            let handler = Rc::clone(&self.on_focus_change);
            FocusScope::with_external_node(
                Arc::clone(&self.scope),
                Focus::new(leaf())
                    .focus_node(Arc::clone(&self.node))
                    .on_focus_change(move |focused| handler(focused)),
            )
            .into_view()
            .boxed()
        }
    }

    let node = FocusNode::with_debug_label("solo");
    let scope = FocusScopeNode::with_debug_label("solo-scope");
    let got_focus = Arc::new(Mutex::new(Vec::<bool>::new()));
    let recorded = Arc::clone(&got_focus);
    let mut laid = lay_out(
        SoloFocusHost {
            scope: Arc::clone(&scope),
            node: Arc::clone(&node),
            on_focus_change: Rc::new(move |focused| recorded.lock().push(focused)),
            show: true,
        },
        loose(200.0),
    );

    node.request_focus();
    laid.tick();
    assert!(node.has_focus());
    assert_eq!(got_focus.lock().as_slice(), [true]);

    laid.pump_widget(SoloFocusHost {
        scope: Arc::clone(&scope),
        node: Arc::clone(&node),
        on_focus_change: Rc::new(|_focused| {}),
        show: false,
    });

    assert!(!node.is_attached(), "unmounting detached the node");
    assert_eq!(
        manager.primary_focus(),
        None,
        "a disposed focused widget releases the primary focus"
    );

    manager.unfocus();
}

/// Oracle: `'FocusManager notifies listeners when a widget loses focus
/// because it was removed.'` (focus_manager_test.dart) — removing the
/// focused widget's node notifies exactly once, and the survivor does not
/// inherit focus.
#[test]
fn removing_a_focused_node_notifies_listeners_exactly_once_without_transferring_focus() {
    let _guard = FOCUS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let a = FocusNode::with_debug_label("a");
    let b = FocusNode::with_debug_label("b");
    let mut laid = lay_out(
        TwoFocusHost {
            a: Arc::clone(&a),
            b: Arc::clone(&b),
            show_a: true,
            show_b: true,
            a_autofocus: false,
            b_autofocus: false,
        },
        loose(200.0),
    );

    a.request_focus();
    assert!(a.has_focus());

    let notify_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&notify_count);
    let listener_id = manager.add_listener(Rc::new(move |_previous, _new| {
        counter.fetch_add(1, Ordering::SeqCst);
    }));

    laid.pump_widget(TwoFocusHost {
        a: Arc::clone(&a),
        b: Arc::clone(&b),
        show_a: false,
        show_b: true,
        a_autofocus: false,
        b_autofocus: false,
    });

    assert_eq!(
        notify_count.load(Ordering::SeqCst),
        1,
        "removing the focused node's widget notifies exactly once"
    );
    assert!(!a.has_focus());
    assert!(
        !b.has_focus(),
        "the survivor does not inherit the released focus"
    );

    manager.remove_listener(listener_id);
    manager.unfocus();
    manager.root_scope().detach_node(b.id());
}
