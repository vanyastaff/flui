//! Tests for [`Overlay`] / [`OverlayEntry`].
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/test/widgets/overlay_test.dart` (tag `3.44.0`) —
//! `'insert top'`, `'insert below'`, `'insert above'`, `'insertAll top'`,
//! `'insertAll below'`, `'insertAll above'`, `'rearrange'`,
//! `'OverlayState.of() throws when called if an Overlay does not exist'`,
//! `'OverlayState.maybeOf() works when an Overlay does and doesn't exist'`,
//! `'OverlayEntry.opaque can be changed when OverlayEntry is not part of an
//! Overlay (yet)'`, `'OverlayEntries do not rebuild when opaqueness changes'`,
//! `'OverlayEntries do not rebuild when opaque entry is added'`, `'Can use
//! Positioned within OverlayEntry'`. Expected values are read from
//! `overlay.dart`, not from running this code. The mutation-surface,
//! opaque/maintainState, and lookup cases above are everything this suite
//! reasonably ports; `tests/parity/overlay_test.rs` documents the rest of the
//! ~30-case oracle file as out of scope, with reasons, since almost none of it
//! is reachable through the crate's public API at all (see that file's module
//! docs).
//!
//! # Why an in-crate harness
//!
//! [`Overlay`]/[`OverlayEntry`]/[`OverlayHandle`] are `pub` since ADR-0036, but
//! the mutation surface these tests exercise directly (`insert`/`rearrange`/
//! `InsertPosition`/`entry_ids`/…), plus [`OverlayScope`] and the
//! `Theater`/`OverlayState` view machinery, stay `pub(crate)` — so an
//! integration test in `tests/` still cannot drive this suite.
//! `tests/common::lay_out` is an integration-test module and is unreachable
//! from `src/`. [`mount`] below is the trimmed equivalent: it keeps `lay_out`'s
//! load-bearing ordering — **binding first, so the async driver is installed
//! before the mount `build_scope`** — and drops the geometry helpers this
//! unit does not need.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::ElementId;
use flui_types::geometry::px;
use flui_view::InheritedView;
use flui_view::prelude::*;
use parking_lot::Mutex;

use super::{
    InsertPosition, OnstagePlan, Overlay, OverlayEntry, OverlayHandle, OverlayScope, onstage_plan,
};
use crate::SizedBox;
use crate::test_harness::{Harness, mount};

// ============================================================================
// PROBES
// ============================================================================

/// Counts how many times an entry's builder closure ran.
#[derive(Clone, Default)]
struct Calls(Arc<AtomicUsize>);

impl Calls {
    fn get(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }
    fn bump(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

/// An entry whose builder counts invocations and builds a leaf.
fn counting_entry(calls: &Calls) -> OverlayEntry {
    let calls = calls.clone();
    OverlayEntry::new(move |_ctx| {
        calls.bump();
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
}

/// A stateful leaf whose `create_state` is counted: if the element is reused
/// across a reorder, its state is **not** recreated.
#[derive(Clone)]
struct Probe {
    creations: Arc<AtomicUsize>,
}

impl View for Probe {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for Probe {
    type State = ProbeState;

    fn create_state(&self) -> Self::State {
        self.creations.fetch_add(1, Ordering::Relaxed);
        ProbeState
    }
}

struct ProbeState;

impl ViewState<Probe> for ProbeState {
    fn build(&self, _view: &Probe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// An entry whose subtree is a state-creation-counting [`Probe`].
fn probe_entry(creations: &Arc<AtomicUsize>) -> OverlayEntry {
    let creations = Arc::clone(creations);
    OverlayEntry::new(move |_ctx| {
        Probe {
            creations: Arc::clone(&creations),
        }
        .into_view()
        .boxed()
    })
}

/// The `Theater` element the overlay builds (the overlay element's only child).
fn stack_element(harness: &mut Harness, overlay_element: ElementId) -> ElementId {
    harness.only_child(overlay_element)
}

/// The overlay's layer elements, bottom → top.
fn layer_elements(harness: &mut Harness) -> Vec<ElementId> {
    let root = harness.root();
    let stack = stack_element(harness, root);
    harness.children_of(stack)
}

fn layer_count(harness: &mut Harness) -> usize {
    layer_elements(harness).len()
}

/// An overlay pre-loaded with `entries`, bottom → top.
fn overlay_with(entries: &[OverlayEntry]) -> (OverlayHandle, Overlay) {
    let handle = OverlayHandle::new();
    handle.insert_all(entries, &InsertPosition::Top);
    let overlay = Overlay::new(handle.clone());
    (handle, overlay)
}

/// A root that can build the overlay or drop it.
///
/// `Harness::swap_root` goes through `ElementTree::update`, whose dispatch is
/// keyed by `TypeId`, so the root's *type* must not change between frames.
/// Toggling a field on one root type is how a subtree gets unmounted.
#[derive(Clone)]
struct Host {
    show_overlay: bool,
    handle: OverlayHandle,
}

impl View for Host {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for Host {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if self.show_overlay {
            Overlay::new(self.handle.clone()).into_view().boxed()
        } else {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

/// A single entry is built and mounted on the first frame.
#[test]
fn overlay_first_entry_builds() {
    let calls = Calls::default();
    let (handle, overlay) = overlay_with(&[counting_entry(&calls)]);

    let mut harness = mount(overlay);

    assert_eq!(handle.len(), 1);
    assert_eq!(layer_count(&mut harness), 1, "one layer element");
    assert_eq!(calls.get(), 1, "the entry's builder ran exactly once");
}

/// `_entries` is bottom → top and the last entry paints on top
/// (`overlay.dart:894`, `:916`, `:1157-1161`). Insertion order is child order.
///
/// Red-check: reverse the `children` vec in `OverlayState::build`.
#[test]
fn overlay_entries_preserve_insertion_order() {
    let (bottom, top) = (Calls::default(), Calls::default());
    let (entry_a, entry_b) = (counting_entry(&bottom), counting_entry(&top));
    let (handle, overlay) = overlay_with(&[entry_a.clone(), entry_b.clone()]);

    let mut harness = mount(overlay);

    assert_eq!(handle.entry_ids(), vec![entry_a.id(), entry_b.id()]);
    let layers = layer_elements(&mut harness);
    assert_eq!(layers.len(), 2);
    assert_eq!(
        layers,
        vec![
            entry_a.element_id().expect("A mounted"),
            entry_b.element_id().expect("B mounted"),
        ],
        "A is the first (bottom) child, B the last (top) — last paints on top"
    );
}

/// `_insertionIndex` (`overlay.dart:660-669`): `above` → `index + 1`,
/// `below` → `index`, neither → append.
///
/// Red-check: swap the `Above`/`Below` arms of `insertion_index`.
#[test]
fn overlay_insert_above_and_below_place_entries_exactly() {
    let calls = Calls::default();
    let (entry_a, entry_b) = (counting_entry(&calls), counting_entry(&calls));
    let (handle, overlay) = overlay_with(&[entry_a.clone(), entry_b.clone()]);
    let mut harness = mount(overlay);

    // [A, B] → insert C below B → [A, C, B]
    let entry_c = counting_entry(&calls);
    handle.insert(&entry_c, &InsertPosition::Below(entry_b.clone()));
    harness.tick();
    assert_eq!(
        handle.entry_ids(),
        vec![entry_a.id(), entry_c.id(), entry_b.id()]
    );

    // [A, C, B] → insert D above A → [A, D, C, B]
    let entry_d = counting_entry(&calls);
    handle.insert(&entry_d, &InsertPosition::Above(entry_a.clone()));
    harness.tick();
    assert_eq!(
        handle.entry_ids(),
        vec![entry_a.id(), entry_d.id(), entry_c.id(), entry_b.id()]
    );

    // Top appends.
    let entry_e = counting_entry(&calls);
    handle.insert(&entry_e, &InsertPosition::Top);
    harness.tick();
    assert_eq!(
        handle.entry_ids(),
        vec![
            entry_a.id(),
            entry_d.id(),
            entry_c.id(),
            entry_b.id(),
            entry_e.id()
        ]
    );
    assert_eq!(layer_count(&mut harness), 5);
}

/// `insertAll` places the group contiguously, preserving relative order
/// (`overlay.dart:758-771`), and early-returns on an empty group (`:767`).
#[test]
fn overlay_insert_all_keeps_the_group_contiguous() {
    let calls = Calls::default();
    let (entry_a, entry_b) = (counting_entry(&calls), counting_entry(&calls));
    let (handle, overlay) = overlay_with(&[entry_a.clone(), entry_b.clone()]);
    let mut harness = mount(overlay);

    let (entry_c, entry_d) = (counting_entry(&calls), counting_entry(&calls));
    handle.insert_all(
        &[entry_c.clone(), entry_d.clone()],
        &InsertPosition::Below(entry_b.clone()),
    );
    harness.tick();
    assert_eq!(
        handle.entry_ids(),
        vec![entry_a.id(), entry_c.id(), entry_d.id(), entry_b.id()]
    );

    handle.insert_all(&[], &InsertPosition::Top);
    assert_eq!(handle.len(), 4, "an empty insert_all is a no-op");
}

/// `insertAll` at [`InsertPosition::Top`] appends the whole group, in order,
/// after every existing entry.
///
/// Flutter parity: `'insertAll top'` (`overlay_test.dart`, tag `3.44.0`).
#[test]
fn overlay_insert_all_top_appends_the_group_in_order() {
    let calls = Calls::default();
    let entry_a = counting_entry(&calls);
    let (handle, overlay) = overlay_with(std::slice::from_ref(&entry_a));
    let mut harness = mount(overlay);
    assert_eq!(calls.get(), 1);

    let (entry_b, entry_c) = (counting_entry(&calls), counting_entry(&calls));
    handle.insert_all(&[entry_b.clone(), entry_c.clone()], &InsertPosition::Top);
    harness.tick();

    assert_eq!(
        handle.entry_ids(),
        vec![entry_a.id(), entry_b.id(), entry_c.id()]
    );
    assert_eq!(layer_count(&mut harness), 3);
}

/// `insertAll` at [`InsertPosition::Above`] places the whole group directly
/// after the reference entry, preserving the group's relative order.
///
/// Flutter parity: `'insertAll above'` (`overlay_test.dart`, tag `3.44.0`).
#[test]
fn overlay_insert_all_above_places_the_group_after_the_reference() {
    let calls = Calls::default();
    let (entry_a, entry_c) = (counting_entry(&calls), counting_entry(&calls));
    let (handle, overlay) = overlay_with(&[entry_a.clone(), entry_c.clone()]);
    let mut harness = mount(overlay);

    let (entry_b1, entry_b2) = (counting_entry(&calls), counting_entry(&calls));
    handle.insert_all(
        &[entry_b1.clone(), entry_b2.clone()],
        &InsertPosition::Above(entry_a.clone()),
    );
    harness.tick();

    assert_eq!(
        handle.entry_ids(),
        vec![entry_a.id(), entry_b1.id(), entry_b2.id(), entry_c.id()]
    );
    assert_eq!(layer_count(&mut harness), 4);
}

/// Removing an entry rebuilds the overlay without it, on the next frame — via
/// the `RebuildHandle` alone, since [`Harness::tick`] never dirties the root.
///
/// Red-check: drop the `schedule_rebuild()` call in `OverlayEntry::remove`; the
/// layer count stays 2.
#[test]
fn overlay_remove_entry_rebuilds() {
    let calls = Calls::default();
    let (entry_a, entry_b) = (counting_entry(&calls), counting_entry(&calls));
    let (handle, overlay) = overlay_with(&[entry_a.clone(), entry_b.clone()]);
    let mut harness = mount(overlay);
    assert_eq!(layer_count(&mut harness), 2);

    entry_a.remove();
    assert_eq!(handle.len(), 1, "the list is mutated eagerly");
    harness.tick();

    assert_eq!(layer_count(&mut harness), 1, "A's layer element is gone");
    assert_eq!(
        layer_elements(&mut harness),
        vec![entry_b.element_id().expect("B still mounted")]
    );
    assert!(!entry_a.is_mounted(), "A's state was disposed");
    assert!(!entry_a.is_attached());
}

/// `markNeedsBuild` (`overlay.dart:250`) rebuilds **one** entry, not the overlay.
///
/// Red-check: route `mark_needs_build` through `OverlayShared::schedule_rebuild`
/// instead of the entry's own handle — then B's builder reruns too.
#[test]
fn overlay_mark_needs_build_rebuilds_only_that_entry() {
    let (calls_a, calls_b) = (Calls::default(), Calls::default());
    let (entry_a, entry_b) = (counting_entry(&calls_a), counting_entry(&calls_b));
    let (_handle, overlay) = overlay_with(&[entry_a.clone(), entry_b.clone()]);
    let mut harness = mount(overlay);
    assert_eq!((calls_a.get(), calls_b.get()), (1, 1));

    assert!(
        entry_a.is_mounted(),
        "a mounted entry's handle is not inert"
    );
    entry_a.mark_needs_build();
    harness.tick();

    assert_eq!(calls_a.get(), 2, "A rebuilt");
    assert_eq!(calls_b.get(), 1, "B did not rebuild");
}

/// A removed entry is inert: a second `remove()` evicts nobody, and
/// `mark_needs_build` cannot resurrect or rebuild it.
///
/// Note what is *not* asserted: that B stays un-rebuilt. Removing an entry marks
/// the **overlay** dirty, so `OverlayState::build` reruns and every surviving
/// layer rebuilds. Flutter does exactly the same — `_markDirty` → `setState` →
/// a fresh `_OverlayEntryWidget` per entry, each wrapping a fresh
/// `Builder(builder: widget.entry.builder)` (`overlay.dart:424-427`). Only
/// [`OverlayEntry::mark_needs_build`] is targeted; a structural change is not.
///
/// What actually makes a removed entry inert is [`OverlayEntry::remove`] *taking*
/// the overlay back-reference: a second `remove()` then finds nothing and cannot
/// schedule another overlay rebuild. Red-check: make `detach` clone instead of
/// `take`; the trailing `entry_a.remove()` dirties the overlay and B's builder runs a
/// third time.
///
/// A `removed` flag guarding `mark_needs_build` was also written, and deleted: a
/// red-check proved it unreachable. The overlay's rebuild unmounts A's element
/// before the drained dirty id is processed, and `RebuildHandle::schedule`
/// already treats a vanished element as a no-op.
#[test]
fn removed_entry_cannot_reinsert_or_rebuild_silently() {
    let (calls_a, calls_b) = (Calls::default(), Calls::default());
    let (entry_a, entry_b) = (counting_entry(&calls_a), counting_entry(&calls_b));
    let (handle, overlay) = overlay_with(&[entry_a.clone(), entry_b.clone()]);
    let mut harness = mount(overlay);
    assert_eq!((calls_a.get(), calls_b.get()), (1, 1));

    entry_a.remove();
    // A's element is still mounted right now — it unmounts on the next frame.
    entry_a.mark_needs_build();

    // Removing twice must not evict B, which now occupies A's old index.
    entry_a.remove();
    assert_eq!(
        handle.entry_ids(),
        vec![entry_b.id()],
        "B survived the double remove"
    );

    harness.tick();
    assert_eq!(calls_a.get(), 1, "the removed entry never rebuilt");
    assert_eq!(calls_b.get(), 2, "the overlay rebuild reran B's builder");
    assert_eq!(layer_count(&mut harness), 1);

    // Still inert once unmounted, and it dirties nothing.
    entry_a.mark_needs_build();
    entry_a.remove();
    harness.tick();
    assert_eq!(
        (calls_a.get(), calls_b.get()),
        (1, 2),
        "no further rebuilds"
    );
}

/// A handle outliving its overlay is harmless: no panic, no resurrection.
///
/// Flutter's `OverlayEntry.remove` returns early on `!overlay.mounted`
/// (`overlay.dart:233`) and `_markDirty` is `if (mounted)` (`:849`).
///
/// Red-check: drop `OverlayState::dispose`; `is_mounted()` stays true and the
/// stale `RebuildHandle` schedules a dead element every frame.
#[test]
fn stale_overlay_handle_is_harmless() {
    let calls = Calls::default();
    let entry = counting_entry(&calls);
    let handle = OverlayHandle::new();
    handle.insert(&entry, &InsertPosition::Top);

    let mut harness = mount(Host {
        show_overlay: true,
        handle: handle.clone(),
    });
    assert!(handle.is_mounted());

    // Unmount the overlay: same root type, different child.
    harness.swap_root(Host {
        show_overlay: false,
        handle: handle.clone(),
    });
    assert!(
        !handle.is_mounted(),
        "the overlay's rebuild handle was revoked"
    );
    assert!(!entry.is_mounted());

    // Every mutation on the stale handle is a silent no-op, not a panic.
    let late = counting_entry(&calls);
    handle.insert(&late, &InsertPosition::Top);
    handle.rearrange(std::slice::from_ref(&late));
    entry.mark_needs_build();
    entry.remove();
    harness.tick();

    assert_eq!(calls.get(), 1, "nothing was rebuilt after unmount");
}

/// `OverlayEntry::remove` on an **unmounted** overlay detaches the entry but leaves
/// the overlay's entry list untouched — Flutter's `if (!overlay.mounted) return;`
/// (`overlay.dart:231-233`), which sits *before* `overlay._entries.remove(this)`.
///
/// Found by a parity re-check against Flutter: FLUI mutated the list regardless.
///
/// Red-check: delete the `if !shared.is_mounted() { return; }` guard in
/// `OverlayEntry::remove`; the list drops to 0.
#[test]
fn overlay_entry_remove_leaves_an_unmounted_overlays_list_alone() {
    let calls = Calls::default();
    let entry = counting_entry(&calls);
    let handle = OverlayHandle::new();
    handle.insert(&entry, &InsertPosition::Top);

    let mut harness = mount(Host {
        show_overlay: true,
        handle: handle.clone(),
    });
    assert_eq!(handle.len(), 1);

    harness.swap_root(Host {
        show_overlay: false,
        handle: handle.clone(),
    });
    assert!(!handle.is_mounted());

    entry.remove();

    assert!(!entry.is_attached(), "the entry detached from the overlay");
    assert_eq!(
        handle.len(),
        1,
        "but an unmounted overlay's entry list is left alone"
    );
}

/// `rearrange` reorders, and the keyed reconciler reuses each layer's element —
/// so subtree state survives the move.
///
/// **This is the load-bearing precondition for dropping Flutter's
/// `GlobalKey<_OverlayEntryWidgetState>`** (`overlay.dart:214`). If it fails, the
/// `GlobalKey` must come back and the lock hazard must be resolved for real.
///
/// Red-check: delete `OverlayEntryView::key`. Reconciliation then matches by
/// index and type, so element ids stay put while the *views* swap — A's element
/// would silently host B's entry, and `did_update_view`'s `debug_assert` fires.
#[test]
fn overlay_rearrange_reorders_and_preserves_entry_state() {
    let (creations_a, creations_b) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (entry_a, entry_b) = (probe_entry(&creations_a), probe_entry(&creations_b));
    let (handle, overlay) = overlay_with(&[entry_a.clone(), entry_b.clone()]);
    let mut harness = mount(overlay);

    let (element_a, element_b) = (
        entry_a.element_id().expect("A mounted"),
        entry_b.element_id().expect("B mounted"),
    );
    assert_eq!(layer_elements(&mut harness), vec![element_a, element_b]);
    assert_eq!(creations_a.load(Ordering::Relaxed), 1);
    assert_eq!(creations_b.load(Ordering::Relaxed), 1);

    handle.rearrange(&[entry_b.clone(), entry_a.clone()]);
    harness.tick();

    assert_eq!(
        handle.entry_ids(),
        vec![entry_b.id(), entry_a.id()],
        "order swapped"
    );
    assert_eq!(
        layer_elements(&mut harness),
        vec![element_b, element_a],
        "the same elements moved; they were not recreated in place"
    );
    assert_eq!(
        entry_a.element_id(),
        Some(element_a),
        "A's layer element survived the reorder"
    );
    assert_eq!(
        creations_a.load(Ordering::Relaxed),
        1,
        "A's subtree state was preserved across the reorder"
    );
    assert_eq!(creations_b.load(Ordering::Relaxed), 1);
}

/// The `listEquals` short-circuit (`overlay.dart:833`): rearranging to the order
/// already held mutates nothing and schedules no rebuild.
///
/// Red-check: delete the short-circuit; the builders rerun.
#[test]
fn overlay_rearrange_to_the_same_order_is_a_noop() {
    let calls = Calls::default();
    let (entry_a, entry_b) = (counting_entry(&calls), counting_entry(&calls));
    let (handle, overlay) = overlay_with(&[entry_a.clone(), entry_b.clone()]);
    let mut harness = mount(overlay);
    assert_eq!(calls.get(), 2);

    handle.rearrange(&[entry_a.clone(), entry_b.clone()]);
    harness.tick();

    assert_eq!(calls.get(), 2, "no rebuild for a no-op rearrange");
    assert_eq!(handle.entry_ids(), vec![entry_a.id(), entry_b.id()]);
}

/// Entries the overlay holds but `rearrange` does not name stay, as a group, on
/// top of the named ones (`overlay.dart:798-811`, `:845`, with no `above`/`below`).
#[test]
fn overlay_rearrange_leaves_unmentioned_entries_on_top() {
    let calls = Calls::default();
    let (entry_a, entry_b, entry_c) = (
        counting_entry(&calls),
        counting_entry(&calls),
        counting_entry(&calls),
    );
    let (handle, overlay) = overlay_with(&[entry_a.clone(), entry_b.clone(), entry_c.clone()]);
    let mut harness = mount(overlay);

    // Name only C then A; B is unmentioned and floats to the top.
    handle.rearrange(&[entry_c.clone(), entry_a.clone()]);
    harness.tick();

    assert_eq!(
        handle.entry_ids(),
        vec![entry_c.id(), entry_a.id(), entry_b.id()]
    );
    assert_eq!(layer_count(&mut harness), 3);
}

/// `rearrange` inserts entries it names that the overlay does not hold
/// (`overlay.dart:798`).
#[test]
fn overlay_rearrange_inserts_unknown_entries() {
    let calls = Calls::default();
    let entry_a = counting_entry(&calls);
    let (handle, overlay) = overlay_with(std::slice::from_ref(&entry_a));
    let mut harness = mount(overlay);

    let entry_b = counting_entry(&calls);
    handle.rearrange(&[entry_b.clone(), entry_a.clone()]);
    harness.tick();

    assert_eq!(handle.entry_ids(), vec![entry_b.id(), entry_a.id()]);
    assert!(entry_b.is_mounted(), "the newly named entry was mounted");
    assert!(entry_b.is_attached());
}

// ============================================================================
// opaque / maintainState / skipCount
// ============================================================================

/// `overlay.dart:890-897`: the loop stops adding onstage children once an opaque
/// entry is reached, and an entry below it without `maintainState` is not added
/// at all — it never enters the view tree.
///
/// Replaces the earlier `overlay_deferred_opaque_builds_every_entry`, which
/// pinned the not-yet-implemented behavior and is red by design now.
#[test]
fn overlay_opaque_top_entry_drops_lower_entries_entirely() {
    let (bottom, top) = (Calls::default(), Calls::default());
    let entry_a = counting_entry(&bottom);
    let entry_b = counting_entry(&top).with_opaque(true);
    let (_handle, overlay) = overlay_with(&[entry_a.clone(), entry_b]);
    let mut harness = mount(overlay);

    assert_eq!(
        (bottom.get(), top.get()),
        (0, 1),
        "the covered entry must not be built at all"
    );
    assert_eq!(layer_count(&mut harness), 1);
    assert!(
        !entry_a.is_mounted(),
        "a covered entry without maintain_state has no mounted subtree"
    );
}

/// `overlay.dart:898-905`: `maintainState` keeps a covered entry in the tree.
/// It is then one of the theater's leading `skipCount` children.
#[test]
fn overlay_maintain_state_keeps_covered_entry_built() {
    let (bottom, top) = (Calls::default(), Calls::default());
    let entry_a = counting_entry(&bottom).with_maintain_state(true);
    let entry_b = counting_entry(&top).with_opaque(true);
    let (_handle, overlay) = overlay_with(&[entry_a.clone(), entry_b]);
    let mut harness = mount(overlay);

    assert_eq!(
        (bottom.get(), top.get()),
        (1, 1),
        "a maintain_state entry is built even when covered"
    );
    assert_eq!(layer_count(&mut harness), 2);
    assert!(entry_a.is_mounted());
}

/// `opaque == false` is the default and skips nothing — the original
/// behavior, which must survive.
#[test]
fn overlay_non_opaque_top_entry_skips_nothing() {
    let (bottom, top) = (Calls::default(), Calls::default());
    let (entry_a, entry_b) = (counting_entry(&bottom), counting_entry(&top));
    let (_handle, overlay) = overlay_with(&[entry_a, entry_b]);
    let mut harness = mount(overlay);

    assert_eq!((bottom.get(), top.get()), (1, 1));
    assert_eq!(layer_count(&mut harness), 2);
}

/// Only the entries *below* the topmost opaque one are covered. Flutter adds the
/// opaque entry itself before flipping `onstage` (`overlay.dart:892-896`).
#[test]
fn overlay_opaque_entry_below_another_still_builds_the_top() {
    let (bottom, middle, top) = (Calls::default(), Calls::default(), Calls::default());
    let entry_a = counting_entry(&bottom);
    let entry_b = counting_entry(&middle).with_opaque(true);
    let entry_c = counting_entry(&top);
    let (_handle, overlay) = overlay_with(&[entry_a, entry_b, entry_c]);
    let mut harness = mount(overlay);

    assert_eq!(
        (bottom.get(), middle.get(), top.get()),
        (0, 1, 1),
        "the opaque entry and everything above it build; only below is dropped"
    );
    assert_eq!(layer_count(&mut harness), 2);
}

/// The `opaque` setter rebuilds the **overlay** — Flutter's
/// `_didChangeEntryOpacity` is `setState` on `OverlayState` (`overlay.dart:879`).
/// Clearing it brings the covered entry back, with fresh state.
#[test]
fn overlay_toggling_opaque_rebuilds_and_restores_the_covered_entry() {
    let creations = Arc::new(AtomicUsize::new(0));
    let entry_a = probe_entry(&creations);
    let entry_b = OverlayEntry::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed());
    let (_handle, overlay) = overlay_with(&[entry_a.clone(), entry_b.clone()]);
    let mut harness = mount(overlay);

    assert_eq!(creations.load(Ordering::Relaxed), 1);
    assert_eq!(layer_count(&mut harness), 2);

    entry_b.set_opaque(true);
    harness.tick();
    assert_eq!(layer_count(&mut harness), 1, "covered entry left the tree");
    assert!(!entry_a.is_mounted());

    entry_b.set_opaque(false);
    harness.tick();
    assert_eq!(layer_count(&mut harness), 2);
    assert_eq!(
        creations.load(Ordering::Relaxed),
        2,
        "an uncovered entry's state is created fresh — its old state was disposed"
    );
}

/// A `set_opaque` that does not change the value must not rebuild — Flutter's
/// `if (_opaque == value) return;` (`overlay.dart:140-142`).
#[test]
fn overlay_setting_opaque_to_the_same_value_is_a_noop() {
    let calls = Calls::default();
    let entry = counting_entry(&calls);
    let (_handle, overlay) = overlay_with(std::slice::from_ref(&entry));
    let mut harness = mount(overlay);
    assert_eq!(calls.get(), 1);

    entry.set_opaque(false);
    harness.tick();
    assert_eq!(calls.get(), 1, "no rebuild for an unchanged flag");

    entry.set_opaque(true);
    harness.tick();
    assert_eq!(calls.get(), 2, "a real change rebuilds the overlay");
}

/// `set_maintain_state` goes through the same `_didChangeEntryOpacity` path, so
/// turning it on under an opaque entry brings the covered entry back.
#[test]
fn overlay_setting_maintain_state_rebuilds_the_overlay() {
    let calls = Calls::default();
    let entry_a = counting_entry(&calls);
    let entry_b =
        OverlayEntry::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()).with_opaque(true);
    let (_handle, overlay) = overlay_with(&[entry_a.clone(), entry_b]);
    let mut harness = mount(overlay);
    assert_eq!(calls.get(), 0);

    entry_a.set_maintain_state(true);
    harness.tick();
    assert_eq!(calls.get(), 1, "the covered entry is built once maintained");
    assert_eq!(layer_count(&mut harness), 2);
}

/// `set_opaque` on an entry attached to no overlay is legal — no rebuild to
/// schedule, no panic — and the value is honored once the entry is inserted.
///
/// Flutter parity: `'OverlayEntry.opaque can be changed when OverlayEntry is
/// not part of an Overlay (yet)'` (`overlay_test.dart`, tag `3.44.0`).
#[test]
fn overlay_entry_opaque_set_before_attachment_is_honored_on_insert() {
    let root_calls = Calls::default();
    let root_entry = counting_entry(&root_calls);
    let (handle, overlay) = overlay_with(std::slice::from_ref(&root_entry));
    let mut harness = mount(overlay);
    assert_eq!(layer_count(&mut harness), 1);

    let top_calls = Calls::default();
    let top_entry = counting_entry(&top_calls);
    assert!(!top_entry.opaque(), "opaque defaults to false");
    assert!(!top_entry.is_attached(), "not yet part of any overlay");
    top_entry.set_opaque(true);
    assert!(top_entry.opaque(), "the flag is stored even while detached");

    handle.insert(&top_entry, &InsertPosition::Top);
    harness.tick();

    assert_eq!(
        layer_count(&mut harness),
        1,
        "root is now covered by the pre-set opaque entry and dropped"
    );
    assert!(!root_entry.is_mounted());
    assert_eq!(top_calls.get(), 1);
}

/// Ported, but **red by design against the oracle's actual regression
/// guard** — the oracle test is `'OverlayEntries do not rebuild when
/// opaqueness changes'` (`overlay_test.dart`, tag `3.44.0`), Flutter's own
/// regression test for flutter/flutter#45797: a covered `maintainState`
/// entry stays mounted (already pinned by
/// `overlay_maintain_state_keeps_covered_entry_built`) *and its builder must
/// not rerun* just because the overlay above it changed.
///
/// FLUI does not have that second half. `OverlayState::build` reconciles a
/// fresh (but key-equal) `OverlayEntryView` for every survivor on every
/// overlay rebuild, and `OverlayEntryView` does not override
/// [`View::should_skip_rebuild`](flui_view::View::should_skip_rebuild) (nor
/// wrap itself in [`flui_view::view::Memo`], the opt-in that would) — so the
/// framework's documented safe default (`flui_view::view::memo`'s module
/// docs: always rebuild unless a view opts out) applies, and every survivor's
/// content rebuilds on every overlay-level structural change, not just the
/// one that actually triggered it. This is a real, previously-undocumented
/// gap, not a test-porting artifact — filed as `docs/ROADMAP.md`'s Cross.H
/// "Overlay entries always rebuild on any overlay-level change" entry.
///
/// This assertion is deliberately the oracle's own expectation, not FLUI's
/// current behavior: weakening it to `(2, 2, 2)` would launder a real gap as
/// a passing parity test. Left `#[ignore]`, not deleted, so the fix (opting
/// `OverlayEntryView` into `Memo`, once `OverlayEntry`/`OverlayHandle` gain
/// the `PartialEq` that requires) has a pinned target to turn green.
#[test]
#[ignore = "known gap, docs/ROADMAP.md Cross.H — OverlayEntryView does not opt into Memo, so surviving entries rebuild on every overlay-level change; see this test's doc comment"]
fn overlay_maintain_state_entries_are_not_rebuilt_when_opaqueness_changes() {
    let (bottom, middle, top) = (Calls::default(), Calls::default(), Calls::default());
    let entry_a = counting_entry(&bottom).with_maintain_state(true);
    let entry_b = counting_entry(&middle).with_maintain_state(true);
    let entry_c = counting_entry(&top).with_maintain_state(true);
    let (_handle, overlay) = overlay_with(&[entry_a.clone(), entry_b.clone(), entry_c.clone()]);
    let mut harness = mount(overlay);
    assert_eq!((bottom.get(), middle.get(), top.get()), (1, 1, 1));

    entry_b.set_opaque(true);
    harness.tick();

    assert_eq!(
        layer_count(&mut harness),
        3,
        "bottom stays in the tree (maintain_state)"
    );
    assert_eq!(
        (bottom.get(), middle.get(), top.get()),
        (1, 1, 1),
        "no entry's builder reran — the overlay reconciled its survivors, it did not rebuild them"
    );
}

/// The same known gap as
/// [`overlay_maintain_state_entries_are_not_rebuilt_when_opaqueness_changes`],
/// exercised from the other direction that oracle test `'OverlayEntries do
/// not rebuild when opaque entry is added'` (`overlay_test.dart`, tag
/// `3.44.0`) covers: `rearrange` inserting a *new* opaque entry between two
/// already-mounted `maintainState` entries should not rebuild either of them.
/// See the sibling test's doc comment for the root cause and the filed gap.
#[test]
#[ignore = "known gap, docs/ROADMAP.md Cross.H — same root cause as overlay_maintain_state_entries_are_not_rebuilt_when_opaqueness_changes"]
fn overlay_maintain_state_entries_are_not_rebuilt_when_an_opaque_entry_is_added() {
    let (bottom, top) = (Calls::default(), Calls::default());
    let entry_a = counting_entry(&bottom).with_maintain_state(true);
    let entry_c = counting_entry(&top).with_maintain_state(true);
    let (handle, overlay) = overlay_with(&[entry_a.clone(), entry_c.clone()]);
    let mut harness = mount(overlay);
    assert_eq!((bottom.get(), top.get()), (1, 1));

    let middle = Calls::default();
    let entry_b = counting_entry(&middle).with_opaque(true);
    handle.rearrange(&[entry_a.clone(), entry_b.clone(), entry_c.clone()]);
    harness.tick();

    assert_eq!(layer_count(&mut harness), 3);
    assert_eq!(
        (bottom.get(), top.get()),
        (1, 1),
        "the pre-existing entries were reconciled, not rebuilt, by the rearrange"
    );
    assert_eq!(
        middle.get(),
        1,
        "the newly inserted entry built exactly once"
    );
}

/// A `rearrange` that reorders entries under an opaque top must still preserve
/// the surviving keyed entries' subtree state — the same keyed-reorder contract
/// verified above, now with `skipCount` in play.
#[test]
fn overlay_rearrange_with_opaque_preserves_surviving_entry_state() {
    let creations = Arc::new(AtomicUsize::new(0));
    let maintained = probe_entry(&creations).with_maintain_state(true);
    let filler = OverlayEntry::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed())
        .with_maintain_state(true);
    let opaque =
        OverlayEntry::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()).with_opaque(true);

    let (handle, overlay) = overlay_with(&[maintained.clone(), filler.clone(), opaque.clone()]);
    let mut harness = mount(overlay);

    assert_eq!(creations.load(Ordering::Relaxed), 1);
    let before = maintained.element_id();
    assert!(before.is_some());

    handle.rearrange(&[filler, maintained.clone(), opaque]);
    harness.tick();

    assert_eq!(
        creations.load(Ordering::Relaxed),
        1,
        "the keyed reorder must move the element, not recreate its state"
    );
    assert_eq!(maintained.element_id(), before);
    assert_eq!(layer_count(&mut harness), 3);
}

/// The `skipCount` handed to the theater is the number of *covered but
/// maintained* entries, and they are always the leading children — the property
/// [`RenderTheater`](flui_objects::RenderTheater) relies on.
///
/// Asserted on [`onstage_plan`] directly: the element tree cannot observe
/// `skip_count`, and `harness_theater_*` in `flui-objects` covers what the render
/// object then does with it.
#[test]
fn overlay_build_plan_matches_flutters_onstage_loop() {
    let plain = || OverlayEntry::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed());
    let opaque = || plain().with_opaque(true);
    let maintained = || plain().with_maintain_state(true);
    let maintained_opaque = || plain().with_opaque(true).with_maintain_state(true);

    let plan = |entries: &[OverlayEntry]| onstage_plan(entries);

    assert_eq!(
        plan(&[plain(), plain()]),
        OnstagePlan {
            build: vec![0, 1],
            skip_count: 0
        },
        "nothing opaque: every entry onstage, a plain expanding stack"
    );
    assert_eq!(
        plan(&[plain(), opaque()]),
        OnstagePlan {
            build: vec![1],
            skip_count: 0
        },
        "the covered entry is dropped, not skipped — it never enters the tree"
    );
    assert_eq!(
        plan(&[maintained(), opaque()]),
        OnstagePlan {
            build: vec![0, 1],
            skip_count: 1
        },
        "a maintained covered entry is built and then skipped by the theater"
    );
    assert_eq!(
        plan(&[maintained(), plain(), opaque(), plain()]),
        OnstagePlan {
            build: vec![0, 2, 3],
            skip_count: 1
        },
        "only entries below the topmost opaque one are covered; the \
         non-maintained one among them is dropped"
    );
    assert_eq!(
        plan(&[maintained(), maintained_opaque(), opaque()]),
        OnstagePlan {
            build: vec![0, 1, 2],
            skip_count: 2
        },
        "an opaque entry that is itself covered is still skipped, not dropped"
    );
    assert_eq!(
        plan(&[]),
        OnstagePlan {
            build: vec![],
            skip_count: 0
        },
    );
}

// ============================================================================
// ADR-0021 S8 — `Positioned` inside an overlay entry
// ============================================================================

/// **S8's verification.** ADR-0021 S8 argues, on paper, that a `Positioned` at the
/// root of an `OverlayEntry` builder must be wrapped in its own `Stack`, because
/// [`RenderTheater`] deliberately does not run `RenderStack`'s positioned split
/// (`theater.rs` module docs) — so a bare `Positioned` would have its
/// `StackParentData` silently dropped and be laid out at the origin.
///
/// Flutter's hero flight entry *is* a `Positioned` (`heroes.dart:588`). If this
/// paper argument were wrong in either direction, the flight entry would land in the
/// wrong place, so it is checked here before anything relies on it.
///
/// Both halves are asserted, because only the pair distinguishes "the inner `Stack`
/// is doing the work" from "everything positions things anyway":
///
/// * inside an inner `Stack`, the `Positioned` is honoured;
/// * as the entry's direct child, it is **not** — it sits at the origin.
///
/// Red-check: remove the inner `Stack`; if it still passes, the theater is
/// honouring `Positioned` and this test's premise is wrong. Dropping the
/// `Stack` from case 1 makes its assertion fail — which is the whole content of S8.
///
/// Case 2 is the converse, and has no mutation short of giving `RenderTheater` the
/// full `RenderStack` positioned split; if that ever lands, this test and S8 must be
/// rewritten rather than the theater reverted.
#[test]
fn positioned_inside_an_overlay_entry_is_laid_out_by_an_inner_stack() {
    use crate::{Positioned, Stack, StackFit};
    use flui_rendering::pipeline::PipelineOwner;
    use flui_types::Point;

    /// The offset of the one `RenderConstrainedBox` (a `SizedBox`) in the tree,
    /// relative to the render root.
    fn sized_box_origin(owner: &PipelineOwner) -> Point {
        let target = owner
            .render_tree()
            .iter()
            .find(|(_, node)| node.debug_name().ends_with("RenderConstrainedBox"))
            .map(|(id, _)| id)
            .expect("the entry built a SizedBox");
        owner
            .local_to_global(target, Point::ZERO, None)
            .expect("committed layout")
    }

    // 1. Wrapped in an inner Stack — S8's proposed shape.
    let overlay = OverlayHandle::new();
    let inner_stack = OverlayEntry::new(|_ctx| {
        Stack::new(vec![
            Positioned::new(SizedBox::new(20.0, 10.0))
                .left(40.0)
                .top(25.0)
                .into_view()
                .boxed(),
        ])
        .fit(StackFit::Expand)
        .into_view()
        .boxed()
    });
    overlay.insert(&inner_stack, &InsertPosition::Top);
    let harness = mount(Overlay::new(overlay.clone()));
    let positioned = sized_box_origin(&harness.pipeline_owner().read());

    assert_eq!(
        positioned,
        Point::new(px(40.0), px(25.0)),
        "an inner Stack runs the positioned split, so the entry lands where it asked"
    );

    // 2. The same `Positioned` as the entry's direct child, under the theater.
    let bare_overlay = OverlayHandle::new();
    let bare = OverlayEntry::new(|_ctx| {
        Positioned::new(SizedBox::new(20.0, 10.0))
            .left(40.0)
            .top(25.0)
            .into_view()
            .boxed()
    });
    bare_overlay.insert(&bare, &InsertPosition::Top);
    let bare_harness = mount(Overlay::new(bare_overlay.clone()));
    let dropped = sized_box_origin(&bare_harness.pipeline_owner().read());

    assert_eq!(
        dropped,
        Point::ZERO,
        "RenderTheater ignores positioned children, so a bare \
         Positioned is silently dropped to the origin — this is why S8 requires \
         the inner Stack, and it is now a fact rather than a paper argument"
    );
}

// ============================================================================
// ADR-0036 — `Overlay::of` / `Overlay::maybe_of`
// ============================================================================

/// A stateless leaf that runs `on_build` every time it builds — a generic
/// hook for capturing whatever a `BuildContext`-driven lookup returns,
/// without a bespoke probe type per test.
#[derive(Clone)]
struct Peek<F: Fn(&dyn BuildContext) + Clone + 'static>(F);

impl<F: Fn(&dyn BuildContext) + Clone + 'static> View for Peek<F> {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl<F: Fn(&dyn BuildContext) + Clone + 'static> StatelessView for Peek<F> {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        (self.0)(ctx);
        SizedBox::new(1.0, 1.0)
    }
}

/// Without an ancestor `Overlay`, `maybe_of` must return `None`.
#[test]
fn overlay_maybe_of_is_none_without_an_overlay_ancestor() {
    // Seeded `Some` so a probe that silently never ran would not be mistaken
    // for a correct `None`.
    let found = Arc::new(Mutex::new(Some(OverlayHandle::new())));
    let found_for_probe = Arc::clone(&found);
    let probe = Peek(move |ctx: &dyn BuildContext| {
        *found_for_probe.lock() = Overlay::maybe_of(ctx);
    });

    let _harness = mount(probe);

    assert!(
        found.lock().is_none(),
        "maybe_of must return None with no Overlay ancestor in the tree"
    );
}

/// `Overlay::of` panics with a message that names the type and hints at the
/// fix, matching the `MediaQuery::of`/`ScaffoldScope::of` precedent.
///
/// The panic is caught with `catch_unwind` **inside** the probe's own build,
/// rather than expecting it to unwind through `mount`: the framework's own
/// `build_or_recover` catches a `build()` panic to keep one bad widget from
/// taking down the whole test process, so asserting on the message has to
/// happen before that outer catch, not after.
#[test]
fn overlay_of_panics_with_a_helpful_message_without_an_overlay_ancestor() {
    let message = Arc::new(Mutex::new(None));
    let message_for_probe = Arc::clone(&message);
    let probe = Peek(move |ctx: &dyn BuildContext| {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Overlay::of(ctx)));
        if let Err(payload) = outcome {
            let text = payload
                .downcast_ref::<&str>() // PORT-CHECK-OK-DOWNCAST: test-only extraction of a caught panic's message, not V-type smuggling
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned()) // PORT-CHECK-OK-DOWNCAST: same panic-message extraction, the `String`-payload case
                .unwrap_or_default();
            *message_for_probe.lock() = Some(text);
        }
    });

    let _harness = mount(probe);

    let text = message
        .lock()
        .clone()
        .expect("Overlay::of must panic without an Overlay ancestor");
    assert!(
        text.contains("Overlay::of") && text.contains("no Overlay ancestor"),
        "panic message must name the failing call and the missing ancestor, got: {text:?}"
    );
    assert!(
        text.contains("Navigator") || text.contains("Overlay::maybe_of"),
        "panic message must hint at the fix (wrap in a Navigator/Overlay, or \
         use maybe_of), got: {text:?}"
    );
}

/// A nested `Overlay`'s own entries resolve **the nearest** enclosing overlay,
/// not an outer one — falling out of the ordinary inherited-map nearest-wins
/// shadowing, with no extra code in `OverlayScope` itself.
///
/// The lookup runs from `Peek`'s **own** `build`, not from the entry
/// builder's top-level closure: an `OverlayEntry`'s builder closure receives
/// the *same* `BuildContext` as the `OverlayEntryViewState` that calls it
/// (Flutter's own `_OverlayEntryWidgetState.build` passes its own `context`
/// into `widget.entry.builder(context)` the identical way), which is an
/// ancestor of, not a descendant of, the `OverlayScope` that build wraps the
/// returned content in — so a lookup made with *that* context can never see
/// its own entry's marker, only an enclosing one's, in Flutter too. A real
/// consumer (a nested widget's own `build`/`did_change_dependencies`, as
/// `DraggableState` will be) always has its own distinct, properly-nested
/// context, which is what `Peek` supplies here.
#[test]
fn overlay_maybe_of_resolves_the_nearest_enclosing_overlay() {
    let found: Arc<Mutex<Option<OverlayHandle>>> = Arc::new(Mutex::new(None));
    let found_for_entry = Arc::clone(&found);

    let inner_handle = OverlayHandle::new();
    let inner_entry = OverlayEntry::new(move |_ctx| {
        let found_for_peek = Arc::clone(&found_for_entry);
        Peek(move |ctx: &dyn BuildContext| {
            *found_for_peek.lock() = Overlay::maybe_of(ctx);
        })
        .into_view()
        .boxed()
    });
    inner_handle.insert(&inner_entry, &InsertPosition::Top);
    let inner_overlay = Overlay::new(inner_handle.clone());

    let outer_handle = OverlayHandle::new();
    let outer_entry = OverlayEntry::new(move |_ctx| inner_overlay.clone().into_view().boxed());
    outer_handle.insert(&outer_entry, &InsertPosition::Top);

    let _harness = mount(Overlay::new(outer_handle.clone()));

    let resolved = found
        .lock()
        .clone()
        .expect("the inner entry's Overlay::maybe_of found an ancestor overlay");
    assert!(
        resolved.is_same(&inner_handle),
        "a nested Overlay's own entry must resolve the nearest ancestor \
         overlay (the inner one), not the outer one"
    );
    assert!(
        !resolved.is_same(&outer_handle),
        "the resolved handle must not be the outer overlay's"
    );
}

/// `OverlayScope::update_should_notify` is handle-**identity**, not
/// structural equality — the same handle (even a fresh clone of it) must not
/// notify, a different handle must.
///
/// This is a direct call, not a mounted-tree test: an `OverlayEntryView`
/// element is reconciled in place across ordinary rebuilds of the *same*
/// mounted entry, and its `overlay` field never changes across that entry's
/// lifetime, so there is no reachable production path that ever hands the
/// same mount point two different overlay identities to compare. The
/// `InheritedView` contract is still real and still worth pinning directly —
/// exactly the precedent `GestureArenaScope`'s own
/// `update_should_notify_is_always_false` test and `view/inherited.rs`'s
/// `test_inherited_element_update_should_notify` set.
///
/// Mutation-RUN: hardcode `update_should_notify` to always return `false` —
/// the second assertion below fails (`scope_b` must notify against `scope_a1`
/// but the stub reports it must not).
#[test]
fn overlay_scope_update_should_notify_is_true_only_on_handle_identity_change() {
    let handle_a = OverlayHandle::new();
    let handle_b = OverlayHandle::new();
    let scope_a1 = OverlayScope::new(handle_a.clone(), SizedBox::shrink());
    let scope_a2 = OverlayScope::new(handle_a.clone(), SizedBox::shrink());
    let scope_b = OverlayScope::new(handle_b, SizedBox::shrink());

    assert!(
        !scope_a2.update_should_notify(&scope_a1),
        "the same overlay handle identity must not notify dependents, even \
         across two separately constructed OverlayScope values"
    );
    assert!(
        scope_b.update_should_notify(&scope_a1),
        "a different overlay handle identity must notify dependents"
    );
}
