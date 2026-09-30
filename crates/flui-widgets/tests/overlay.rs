//! Tests for [`Overlay`] / [`OverlayEntry`].
//!
//! # Scenarios
//!
//! Inserting an entry at the top, below and above another entry (singly and
//! in bulk), `rearrange`, the failure and the `maybe` variants of looking up
//! the overlay from a context, changing an entry's opacity before it is part of
//! an overlay, entries not rebuilding when opacity changes or an opaque entry
//! is added (red by design — see below), `Positioned` inside an entry, and
//! removing an entry twice. Expected values are fixed by the documented
//! contract, not by running this code.
//!
//! # Surface
//!
//! [`Overlay`]/[`OverlayEntry`]/[`OverlayHandle`] and the mutation surface
//! (`insert`/`rearrange`/`InsertPosition`, ADR-0076) are public. The entry
//! list is read back through the temporary test-access probe
//! `flui_widgets::__test_access::OverlayProbe` (ADR-0083 §4). The private
//! onstage plan keeps a unit test in `src/overlay/tests.rs`.

// ADR-0027: these tests capture owner-local handles in shared cells. The
// library carries the same lint expectation; an integration test is a
// separate crate, so it is repeated here.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::ElementId;
use flui_view::prelude::*;
use flui_widgets::__test_access::{OverlayEntryProbe as _, OverlayProbe as _};
use flui_widgets::{InsertPosition, Overlay, OverlayEntry, OverlayHandle, SizedBox};

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

// ============================================================================
// TESTS
// ============================================================================

/// `rearrange` reorders, and the keyed reconciler reuses each layer's element —
/// so subtree state survives the move.
///
/// **This is the load-bearing precondition for not keying entries with a
/// `GlobalKey`.** If it fails, the `GlobalKey` must come back and the lock hazard must be resolved for real.
///
/// Red-check: delete `OverlayEntryView::key`. Reconciliation then matches by
/// index and type, so element ids stay put while the *views* swap — A's element
/// would silently host B's entry, and `did_update_view`'s `debug_assert` fires.
pub(crate) fn overlay_rearrange_reorders_and_preserves_entry_state() {
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

/// The build loop stops adding onstage children once an opaque
/// entry is reached, and an entry below it without `maintainState` is not added
/// at all — it never enters the view tree.
///
/// Replaces the earlier `overlay_deferred_opaque_builds_every_entry`, which
/// pinned the not-yet-implemented behavior and is red by design now.
pub(crate) fn overlay_opaque_top_entry_drops_lower_entries_entirely() {
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

// ============================================================================
// ADR-0021 S8 — `Positioned` inside an overlay entry
// ============================================================================

// ============================================================================
// ADR-0076 — `Overlay::of` / `Overlay::maybe_of`
// ============================================================================
