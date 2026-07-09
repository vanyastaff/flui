//! ADR-0019 U1 tests for [`Overlay`] / [`OverlayEntry`].
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/test/widgets/overlay_test.dart` — `'insert top'`,
//! `'insert below'`, `'insert above'`, `'insertAll top'`, `'rearrange'`.
//! Expected values are read from `overlay.dart`, not from running this code.
//!
//! # Why an in-crate harness
//!
//! [`Overlay`] is `pub(crate)` until ADR-0019 U4, so an integration test in
//! `tests/` cannot name it. `tests/common::lay_out` is an integration-test module
//! and is unreachable from `src/`. [`mount`] below is the trimmed equivalent: it
//! keeps `lay_out`'s load-bearing ordering — **binding first, so the async driver
//! is installed before the mount `build_scope`** (ADR-0018 U6) — and drops the
//! geometry helpers this unit does not need.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_binding::HeadlessBinding;
use flui_foundation::ElementId;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::prelude::*;
use flui_view::{BuildOwner, ElementTree};
use parking_lot::RwLock;

use super::{InsertPosition, Overlay, OverlayEntry, OverlayHandle};
use crate::SizedBox;

// ============================================================================
// HARNESS
// ============================================================================

struct Harness {
    binding: HeadlessBinding,
    root_element: ElementId,
}

/// Mount `root` as the render-tree root and drive one frame.
fn mount(root: impl View) -> Harness {
    let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
    let mut build_owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    let mut binding = HeadlessBinding::new();
    build_owner.set_async_driver(binding.scheduler().async_driver().clone());

    let root_element = tree.mount_root_with_pipeline_owner(
        &root,
        Some(Arc::clone(&pipeline_owner)),
        &mut build_owner.element_owner_mut(),
    );

    build_owner.schedule_build_for(root_element, 0);
    build_owner.build_scope(&mut tree);

    let root_render = {
        let owner = pipeline_owner.read();
        let render_tree = owner.render_tree();
        render_tree
            .iter()
            .map(|(id, _)| id)
            .find(|id| render_tree.parent(*id).is_none())
            .expect("the mounted subtree should have a render root")
    };
    {
        let mut guard = pipeline_owner.write();
        guard.set_root_id(Some(root_render));
        guard.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(800.0), px(600.0)))));
    }
    build_owner
        .run_frame_with_layout_builders(&mut tree, &pipeline_owner)
        .expect("headless frame should succeed");

    binding.bind_tree(build_owner, tree, pipeline_owner);

    Harness {
        binding,
        root_element,
    }
}

impl Harness {
    /// Drive a frame without dirtying the root — so only what an
    /// `OverlayHandle`/`OverlayEntry` scheduled through its `RebuildHandle`
    /// rebuilds. Every rebuild assertion below depends on this: a `pump`-style
    /// root-dirtying tick would rebuild the whole tree and prove nothing.
    fn tick(&mut self) {
        self.binding.pump_frame(Duration::ZERO);
    }

    /// Replace the root view and settle — used to unmount the overlay.
    fn swap_root(&mut self, new_root: impl View) {
        self.binding.swap_root_view(self.root_element, &new_root);
        self.binding.pump_frame(Duration::ZERO);
    }

    /// The ordered children of `parent`, read through the public `ElementNode`
    /// surface (`parent()` + `slot()`); `child_ids()` is crate-private.
    fn children_of(&mut self, parent: ElementId) -> Vec<ElementId> {
        let mut kids: Vec<(usize, ElementId)> = self
            .binding
            .tree_mut()
            .iter_nodes()
            .filter(|(_, node)| node.parent() == Some(parent))
            .map(|(id, node)| (node.slot(), id))
            .collect();
        kids.sort_unstable();
        kids.into_iter().map(|(_, id)| id).collect()
    }

    /// The `Stack` element the overlay builds (the overlay element's only child).
    fn stack_element(&mut self) -> ElementId {
        let kids = self.children_of(self.root_element);
        assert_eq!(kids.len(), 1, "the overlay builds exactly one Stack");
        kids[0]
    }

    /// The overlay's layer elements, bottom → top.
    fn layer_elements(&mut self) -> Vec<ElementId> {
        let stack = self.stack_element();
        self.children_of(stack)
    }

    fn layer_count(&mut self) -> usize {
        self.layer_elements().len()
    }
}

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
    assert_eq!(harness.layer_count(), 1, "one layer element");
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
    let layers = harness.layer_elements();
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
    assert_eq!(harness.layer_count(), 5);
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
    assert_eq!(harness.layer_count(), 2);

    entry_a.remove();
    assert_eq!(handle.len(), 1, "the list is mutated eagerly");
    harness.tick();

    assert_eq!(harness.layer_count(), 1, "A's layer element is gone");
    assert_eq!(
        harness.layer_elements(),
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
    assert_eq!(harness.layer_count(), 1);

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

/// `rearrange` reorders, and the keyed reconciler reuses each layer's element —
/// so subtree state survives the move.
///
/// **This is ADR-0019 §3.2's `UNVERIFIED` precondition for dropping Flutter's
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
    assert_eq!(harness.layer_elements(), vec![element_a, element_b]);
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
        harness.layer_elements(),
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
    assert_eq!(harness.layer_count(), 3);
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

/// **`opaque` / `maintainState` are deferred, not implemented.** Every entry is
/// built every frame, even one fully covered by another.
///
/// This pins the *current* behavior so that implementing Flutter's
/// `OverlayState.build` skipping loop (`overlay.dart:888-918`) turns it red —
/// which is the point. It is not a parity claim; ADR-0019 §6 records the cost.
#[test]
fn overlay_deferred_opaque_builds_every_entry() {
    let (bottom, top) = (Calls::default(), Calls::default());
    let (entry_a, entry_b) = (counting_entry(&bottom), counting_entry(&top));
    let (_handle, overlay) = overlay_with(&[entry_a, entry_b]);
    let mut harness = mount(overlay);

    assert_eq!(
        (bottom.get(), top.get()),
        (1, 1),
        "the covered entry is still built — opaque skipping is not implemented"
    );
    assert_eq!(harness.layer_count(), 2);
}
