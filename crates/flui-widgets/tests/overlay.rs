//! Tests for [`Overlay`] / [`OverlayEntry`].
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/test/widgets/overlay_test.dart` (tag `3.44.0`,
//! 43 cases total — `grep -cE '^\s*(testWidgets|test)\('`) —
//! `'insert top'`, `'insert below'`, `'insert above'`, `'insertAll top'`,
//! `'insertAll below'`, `'insertAll above'`, `'rearrange'`,
//! `'OverlayState.of() throws when called if an Overlay does not exist'`,
//! `'OverlayState.maybeOf() works when an Overlay does and doesn't exist'`,
//! `'OverlayEntry.opaque can be changed when OverlayEntry is not part of an
//! Overlay (yet)'`, `'OverlayEntries do not rebuild when opaqueness changes'`
//! (red by design — see below), `'OverlayEntries do not rebuild when opaque
//! entry is added'` (same), `'Can use Positioned within OverlayEntry'`,
//! `'asserts when remove is called twice'`. Expected values are read from
//! `overlay.dart`, not from running this code. These 14 of the 43 oracle
//! cases are everything this suite reasonably ports; `tests/parity/
//! overlay_test.rs`'s module doc carries the full 43-case accounting —
//! every case ported, cited, or named out of scope with a reason — since
//! almost none of the remaining 29 are reachable through the crate's public
//! API at all.
//!
//! # Surface
//!
//! [`Overlay`]/[`OverlayEntry`]/[`OverlayHandle`] and the mutation surface
//! (`insert`/`rearrange`/`InsertPosition`, ADR-0076) are public. The entry
//! list is read back through the temporary test-access probe
//! `flui_widgets::__test_access::OverlayProbe` (ADR-0083 §4). The private
//! onstage plan and `OverlayScope`'s notification predicate keep unit tests in
//! `src/overlay/tests.rs`.

// ADR-0027: these tests capture owner-local handles in shared cells. The
// library carries the same lint expectation; an integration test is a
// separate crate, so it is repeated here.
#![expect(clippy::arc_with_non_send_sync)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::ElementId;
use flui_foundation::panic::payload_text;
use flui_view::prelude::*;
use flui_widgets::__test_access::{OverlayEntryProbe as _, OverlayProbe as _};
use flui_widgets::{InsertPosition, Overlay, OverlayEntry, OverlayHandle, SizedBox};
use parking_lot::Mutex;

use crate::common::harness::{Harness, mount};

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

/// Two overlays sharing one handle, the second optional.
#[derive(Clone)]
struct TwoOverlays {
    handle: OverlayHandle,
    second: bool,
}

impl View for TwoOverlays {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for TwoOverlays {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let mut children = vec![Overlay::new(self.handle.clone()).into_view().boxed()];
        if self.second {
            children.push(Overlay::new(self.handle.clone()).into_view().boxed());
        }
        flui_widgets::Column::new(children)
    }
}

/// Two overlays at different depths: `deep` sits two `Column`s below `shallow`,
/// so a frame that dirties both rebuilds `shallow` first.
#[derive(Clone)]
struct ShallowAndDeep {
    shallow: OverlayHandle,
    deep: OverlayHandle,
}

impl View for ShallowAndDeep {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for ShallowAndDeep {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let deep = flui_widgets::Column::new(vec![
            flui_widgets::Column::new(vec![Overlay::new(self.deep.clone()).into_view().boxed()])
                .into_view()
                .boxed(),
        ]);
        flui_widgets::Column::new(vec![
            Overlay::new(self.shallow.clone()).into_view().boxed(),
            deep.into_view().boxed(),
        ])
    }
}

// ============================================================================
// TESTS
// ============================================================================

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
/// schedule another overlay rebuild. The surviving B entry retains its element
/// and does not rerun its builder merely because the overlay list changed.
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
    assert_eq!(calls_b.get(), 1, "the surviving entry does not rebuild");
    assert_eq!(layer_count(&mut harness), 1);

    // Still inert once unmounted, and it dirties nothing.
    entry_a.mark_needs_build();
    entry_a.remove();
    harness.tick();
    assert_eq!(
        (calls_a.get(), calls_b.get()),
        (1, 1),
        "no further rebuilds"
    );
}

/// `insert` on an **unmounted** overlay changes the list and rebuilds nothing;
/// the entry is built when an `Overlay` mounts with the same handle again.
///
/// This is the public contract ADR-0076 records: the handle, not the mounted
/// view, owns the list, so a subtree that unmounts and remounts its overlay
/// (a route shown again, a conditional branch toggled back) keeps what was
/// inserted meanwhile. A no-op would silently drop it.
///
/// Red-check: make `insert_all` return early when `!self.is_mounted()`. The
/// test fails at its first assertion, with `(0, 0)`: the no-op reading drops
/// the entry inserted before the first mount too, which is the same contract
/// seen from the other side.
#[test]
fn insert_on_an_unmounted_overlay_waits_for_the_next_mount() {
    let (calls_a, calls_b) = (Calls::default(), Calls::default());
    let (entry_a, entry_b) = (counting_entry(&calls_a), counting_entry(&calls_b));
    let handle = OverlayHandle::new();
    handle.insert(&entry_a, &InsertPosition::Top);

    let mut harness = mount(Host {
        show_overlay: true,
        handle: handle.clone(),
    });
    assert_eq!((calls_a.get(), calls_b.get()), (1, 0));

    harness.swap_root(Host {
        show_overlay: false,
        handle: handle.clone(),
    });
    assert!(!handle.is_mounted());

    handle.insert(&entry_b, &InsertPosition::Top);
    harness.tick();
    assert!(entry_b.is_attached(), "the late entry joined the list");
    assert_eq!(handle.entry_ids(), vec![entry_a.id(), entry_b.id()]);
    assert_eq!(calls_b.get(), 0, "nothing is built while unmounted");

    harness.swap_root(Host {
        show_overlay: true,
        handle: handle.clone(),
    });
    assert!(handle.is_mounted());
    assert_eq!(
        calls_b.get(),
        1,
        "the remounted overlay builds the late entry"
    );
    assert_eq!(
        calls_a.get(),
        2,
        "and rebuilds the earlier one in a fresh state"
    );
    assert!(
        entry_a.is_mounted() && entry_b.is_mounted(),
        "both layers are mounted"
    );
}

/// Two `Overlay`s mounted with one handle: the first serves it, the second
/// builds nothing, and disposing the second leaves the first mounted.
///
/// Red-check: make `claim_rebuild` always take the slot; the test fails (the
/// second overlay also serves the handle).
#[test]
fn one_handle_serves_one_mounted_overlay() {
    let calls = Calls::default();
    let entry = counting_entry(&calls);
    let (handle, _) = overlay_with(std::slice::from_ref(&entry));

    let mut harness = mount(TwoOverlays {
        handle: handle.clone(),
        second: true,
    });
    assert!(handle.is_mounted());
    assert_eq!(calls.get(), 1, "only the first overlay builds the entry");

    harness.swap_root(TwoOverlays {
        handle: handle.clone(),
        second: false,
    });
    assert!(
        handle.is_mounted(),
        "disposing the refused second keeps the first"
    );

    entry.mark_needs_build();
    harness.tick();
    assert_eq!(calls.get(), 2, "the first overlay still rebuilds the entry");
}

/// Moving an entry from a deeper overlay to a shallower one within one frame:
/// the shallower overlay rebuilds first and publishes the entry's new rebuild
/// capability, then the deeper one disposes the old view. The old view must
/// not revoke the new one, or `mark_needs_build` goes inert on an entry that
/// is on screen.
///
/// Red-check: make `OverlayEntry::clear_rebuild` take the slot
/// unconditionally; the final `mark_needs_build` rebuilds nothing.
#[test]
fn an_entry_moved_between_overlays_in_one_frame_keeps_rebuilding() {
    let calls = Calls::default();
    let entry = counting_entry(&calls);
    let (deep, _) = overlay_with(std::slice::from_ref(&entry));
    let shallow = OverlayHandle::new();

    let mut harness = mount(ShallowAndDeep {
        shallow: shallow.clone(),
        deep: deep.clone(),
    });
    assert_eq!(calls.get(), 1, "built once, in the deep overlay");

    entry.remove();
    shallow.insert(&entry, &InsertPosition::Top);
    harness.tick();
    assert_eq!(shallow.entry_ids(), vec![entry.id()]);
    assert_eq!(calls.get(), 2, "built again, now in the shallow overlay");
    assert!(
        entry.is_mounted(),
        "the new layer's capability survived the old one's dispose"
    );

    entry.mark_needs_build();
    harness.tick();
    assert_eq!(
        calls.get(),
        3,
        "mark_needs_build still rebuilds the moved entry"
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
    use flui_foundation::geometry::Point;
    use flui_rendering::pipeline::PipelineOwner;
    use flui_widgets::{Positioned, Stack, StackFit};

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
    let positioned = harness.pipeline_owner().with(sized_box_origin);

    assert_eq!(
        positioned,
        Point::new(40.0, 25.0),
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
    let dropped = bare_harness.pipeline_owner().with(sized_box_origin);

    assert_eq!(
        dropped,
        Point::ZERO,
        "RenderTheater ignores positioned children, so a bare \
         Positioned is silently dropped to the origin — this is why S8 requires \
         the inner Stack, and it is now a fact rather than a paper argument"
    );
}

// ============================================================================
// ADR-0076 — `Overlay::of` / `Overlay::maybe_of`
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
        let _prev = std::mem::replace(&mut *found_for_probe.lock(), Overlay::maybe_of(ctx));
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
            // `Some("")` for an opaque payload keeps "did not panic" (`None`)
            // distinguishable from "panicked with a non-string payload".
            let text = payload_text(payload.as_ref())
                .unwrap_or_default()
                .to_owned();
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
