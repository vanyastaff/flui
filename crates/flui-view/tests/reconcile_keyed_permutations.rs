//! 6-permutation keyed-reorder corpus (plan §U18 / SC-002 / FR-024 (b)).
//!
//! For each of the 6 permutations of `[A, B, C]` (the symmetric group
//! S_3), the test:
//!
//! 1. Mounts a 3-element box-vec of identity-tagged keyed leaves.
//! 2. Captures each element's initial identity-id.
//! 3. Runs `reconcile_children` with the permutation as the new
//!    views.
//! 4. Asserts the SAME identity-ids survive at the permuted slots —
//!    proving the keyed reconciler reuses elements (preserving any
//!    state they hold) across the reorder rather than tearing down
//!    and recreating them.
//! 5. Asserts the `ReconcileEvent` stream (captured via the
//!    `ReconcileEventCollector`) is the expected multiset of
//!    dispositions per permutation. Multiset equality is the
//!    SC-008 contract (Phase 4's HashMap-iteration order on the
//!    keyed middle is not deterministic across rustc versions; the
//!    multiset shape is the part of the spec that locks).
//!
//! Phase 2 covers the DYNAMIC `Vec<BoxedView>` path only; the
//! tuple-static `ViewSeq` path lands in Phase 3 §U25/§U31 once
//! `ViewSeq` exists.

#![cfg(feature = "test-utils")]

use std::any::TypeId;
use std::sync::atomic::{AtomicU64, Ordering};

use flui_foundation::{ElementId, ValueKey, ViewKey};
use flui_view::{
    BuildOwner, ElementBase, View,
    element::Lifecycle,
    reconcile_children,
    tree::{
        ReconcileEventKind,
        test_utils::{CollectedEvent, ReconcileEventCollector},
    },
};
use tracing::dispatcher::Dispatch;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;

// ============================================================================
// Identity-tagged keyed leaf fixture
// ============================================================================

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_identity() -> u64 {
    ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Keyed view tagged with a stable `tag` (the test labels: 'A', 'B',
/// 'C') used to look up the element after the reorder.
#[derive(Clone)]
struct TaggedView {
    tag: char,
    key: ValueKey<char>,
}

impl TaggedView {
    fn new(tag: char) -> Self {
        Self {
            tag,
            key: ValueKey::new(tag),
        }
    }
}

impl View for TaggedView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(TaggedElement {
            tag: self.tag,
            identity_id: next_identity(),
            depth: 0,
            lifecycle: Lifecycle::Initial,
            key: Box::new(self.key.clone()),
        })
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

struct TaggedElement {
    tag: char,
    identity_id: u64,
    depth: usize,
    lifecycle: Lifecycle,
    key: Box<dyn ViewKey>,
}

impl ElementBase for TaggedElement {
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<TaggedView>()
    }
    fn current_key_hash(&self) -> Option<u64> {
        Some(self.key.key_hash())
    }
    fn current_key(&self) -> Option<&dyn ViewKey> {
        Some(&*self.key)
    }
    fn depth(&self) -> usize {
        self.depth
    }
    fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }
    fn mount(
        &mut self,
        _parent: Option<ElementId>,
        slot: usize,
        _owner: &mut flui_view::ElementOwner<'_>,
    ) {
        self.depth = slot;
        self.lifecycle = Lifecycle::Active;
    }
    fn unmount(&mut self, _owner: &mut flui_view::ElementOwner<'_>) {
        self.lifecycle = Lifecycle::Defunct;
    }
    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
    }
    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
    }
    fn update(&mut self, new_view: &dyn View, _owner: &mut flui_view::ElementOwner<'_>) {
        // Re-clone the key from the new view (mirrors production
        // ElementNode::update from §U7).
        if let Some(k) = new_view.key() {
            self.key = k.clone_key();
        }
    }
    fn mark_needs_build(&mut self) {}
    fn perform_build(&mut self, _owner: &mut flui_view::ElementOwner<'_>) {}
    fn visit_children(&self, _visitor: &mut dyn FnMut(ElementId)) {}
}

fn as_tagged(child: &dyn ElementBase) -> &TaggedElement {
    child
        .as_any()
        .downcast_ref::<TaggedElement>()
        .expect("test invariant: every child here is TaggedElement")
}

// ============================================================================
// Test harness
// ============================================================================

fn parent_id() -> ElementId {
    ElementId::new(1)
}

/// Tag-to-identity map captured at the initial mount.
type Identities = [(char, u64); 3];

/// Vector of mounted children — the type used throughout the test
/// harness. Extracted to keep `clippy::type_complexity` from firing
/// on each helper signature.
type Children = Vec<Box<dyn ElementBase>>;

/// Mount a fresh 3-element `[A, B, C]` tree and return the elements
/// plus a (tag -> initial_identity) map.
fn build_initial(owner: &mut BuildOwner) -> (Children, Identities) {
    let tags = ['A', 'B', 'C'];
    let children: Children = tags
        .iter()
        .enumerate()
        .map(|(slot, &tag)| {
            let v = TaggedView::new(tag);
            let mut el = v.create_element();
            el.mount(None, slot, &mut owner.element_owner_mut());
            el
        })
        .collect();

    let mut identities = [('?', 0); 3];
    for (i, child) in children.iter().enumerate() {
        let t = as_tagged(&**child);
        identities[i] = (t.tag, t.identity_id);
    }
    (children, identities)
}

/// Capture a closure's emitted ReconcileEvents.
fn capture<F: FnOnce()>(body: F) -> Vec<CollectedEvent> {
    let collector = ReconcileEventCollector::new();
    let subscriber = Registry::default().with(collector.layer());
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
    collector.events()
}

/// Run one permutation case: rebuild with `permutation` (e.g.
/// `['C','A','B']`) and return (final children, captured events).
fn run_permutation(permutation: [char; 3]) -> (Children, Identities, Vec<CollectedEvent>) {
    let mut owner = BuildOwner::new();
    let (mut children, initial_identities) = build_initial(&mut owner);

    let new_views: Vec<TaggedView> = permutation.iter().map(|&t| TaggedView::new(t)).collect();
    let view_refs: Vec<&dyn View> = new_views.iter().map(|v| v as &dyn View).collect();

    let events = capture(|| {
        reconcile_children(
            parent_id(),
            &mut children,
            &view_refs,
            &mut owner.element_owner_mut(),
        );
    });

    (children, initial_identities, events)
}

/// Assert that each tag in `permutation` ends up at its permuted
/// slot with the SAME identity-id it started with — proves keyed
/// reuse preserves element identity (and therefore any state the
/// element holds).
fn assert_identity_preserved(
    permutation: [char; 3],
    children: &[Box<dyn ElementBase>],
    initial: &Identities,
) {
    for (new_slot, &tag) in permutation.iter().enumerate() {
        let initial_id = initial
            .iter()
            .find_map(|&(t, id)| if t == tag { Some(id) } else { None })
            .unwrap_or_else(|| panic!("tag {tag} missing from initial identities"));
        let actual_id = as_tagged(&*children[new_slot]).identity_id;
        let actual_tag = as_tagged(&*children[new_slot]).tag;
        assert_eq!(
            actual_id, initial_id,
            "permutation {permutation:?}: slot {new_slot} should hold the original element for tag {tag} \
             (id={initial_id}), got tag {actual_tag} (id={actual_id})",
        );
    }
}

/// Assert the captured events form the EXPECTED multiset of
/// (kind, slot, child_key_hash) triples. The order of the Phase 4
/// keyed-middle dispositions is not deterministic across HashMap
/// iteration orders, so multiset equality is the SC-008 contract.
fn assert_event_multiset(
    permutation: [char; 3],
    events: &[CollectedEvent],
    expected: &[(ReconcileEventKind, u64)],
) {
    // Vacuous-pass guard: positive count BEFORE any per-event
    // assertion.
    assert_eq!(
        events.len(),
        expected.len(),
        "permutation {permutation:?}: expected {} events, observed {}; events={events:?}",
        expected.len(),
        events.len(),
    );

    // Convert to comparable multisets keyed by (kind, slot). The
    // key hash field is implied by slot+tag mapping in TaggedView
    // (every slot's hash is the tag's ValueKey hash), so the
    // multiset on (kind, slot) is sufficient — adding hash would
    // over-specify a deterministic field.
    let mut actual: Vec<(ReconcileEventKind, u64)> =
        events.iter().map(|e| (e.kind, e.slot)).collect();
    let mut expected_sorted: Vec<(ReconcileEventKind, u64)> = expected.to_vec();
    actual.sort_by_key(|(k, s)| (*k as u8, *s));
    expected_sorted.sort_by_key(|(k, s)| (*k as u8, *s));
    assert_eq!(
        actual, expected_sorted,
        "permutation {permutation:?}: event multiset mismatch\n  expected: {expected_sorted:?}\n  actual:   {actual:?}\n  full events: {events:?}",
    );

    // Every event MUST carry the real parent id stamped by §U15
    // (parent_id() == ElementId::new(1) here because the test uses
    // a synthetic parent without going through ElementTree).
    let expected_parent = parent_id().as_u64();
    for e in events {
        assert_eq!(
            e.parent, expected_parent,
            "event {e:?}: parent id must match the threaded parent",
        );
    }
}

// ============================================================================
// 6 permutation tests — Covers SC-002 (dynamic-path side)
// ============================================================================

/// Identity permutation: no reorder, all three slots stay put. Three
/// `Reuse` events expected.
#[test]
fn covers_sc002_permutation_abc_identity() {
    let (children, initial, events) = run_permutation(['A', 'B', 'C']);
    assert_identity_preserved(['A', 'B', 'C'], &children, &initial);
    assert_event_multiset(
        ['A', 'B', 'C'],
        &events,
        &[
            (ReconcileEventKind::Reuse, 0),
            (ReconcileEventKind::Reuse, 1),
            (ReconcileEventKind::Reuse, 2),
        ],
    );
}

/// Swap-last-two: A stays, B<->C swap.
#[test]
fn covers_sc002_permutation_acb() {
    let (children, initial, events) = run_permutation(['A', 'C', 'B']);
    assert_identity_preserved(['A', 'C', 'B'], &children, &initial);
    // Prefix scan matches A. Bottom scan: old[2]=C vs new[2]=B, no
    // match → terminates. Middle: B and C indexed. New walk: C goes
    // to slot 1 (was slot 2 → Reorder), B goes to slot 2 (was slot 1
    // → Reorder).
    assert_event_multiset(
        ['A', 'C', 'B'],
        &events,
        &[
            (ReconcileEventKind::Reuse, 0),
            (ReconcileEventKind::Reorder, 1),
            (ReconcileEventKind::Reorder, 2),
        ],
    );
}

/// Swap-first-two: A<->B swap, C stays.
#[test]
fn covers_sc002_permutation_bac() {
    let (children, initial, events) = run_permutation(['B', 'A', 'C']);
    assert_identity_preserved(['B', 'A', 'C'], &children, &initial);
    // Prefix scan: old[0]=A vs new[0]=B, no match → terminates.
    // Bottom scan: old[2]=C vs new[2]=C → match → record (sync in
    // phase 5a). Middle: A and B indexed. New walk: B at slot 0
    // (Reorder), A at slot 1 (Reorder). Bottom 5a: C at slot 2
    // (Reuse).
    assert_event_multiset(
        ['B', 'A', 'C'],
        &events,
        &[
            (ReconcileEventKind::Reorder, 0),
            (ReconcileEventKind::Reorder, 1),
            (ReconcileEventKind::Reuse, 2),
        ],
    );
}

/// Left rotation: A → end.
#[test]
fn covers_sc002_permutation_bca() {
    let (children, initial, events) = run_permutation(['B', 'C', 'A']);
    assert_identity_preserved(['B', 'C', 'A'], &children, &initial);
    // Prefix: no match. Bottom: old[2]=C vs new[2]=A, no match.
    // Middle has all three indexed. New walk: B at slot 0 (Reorder
    // from old 1), C at slot 1 (Reorder from old 2), A at slot 2
    // (Reorder from old 0).
    assert_event_multiset(
        ['B', 'C', 'A'],
        &events,
        &[
            (ReconcileEventKind::Reorder, 0),
            (ReconcileEventKind::Reorder, 1),
            (ReconcileEventKind::Reorder, 2),
        ],
    );
}

/// Right rotation: C → start.
#[test]
fn covers_sc002_permutation_cab() {
    let (children, initial, events) = run_permutation(['C', 'A', 'B']);
    assert_identity_preserved(['C', 'A', 'B'], &children, &initial);
    // Same family as BCA — all three Reorder.
    assert_event_multiset(
        ['C', 'A', 'B'],
        &events,
        &[
            (ReconcileEventKind::Reorder, 0),
            (ReconcileEventKind::Reorder, 1),
            (ReconcileEventKind::Reorder, 2),
        ],
    );
}

/// Full reverse: [A,B,C] → [C,B,A]. B stays at slot 1.
#[test]
fn covers_sc002_permutation_cba_full_reverse() {
    let (children, initial, events) = run_permutation(['C', 'B', 'A']);
    assert_identity_preserved(['C', 'B', 'A'], &children, &initial);
    // Prefix: no match. Bottom: no match. Middle: all three indexed.
    // New walk: C at slot 0 (Reorder from old 2), B at slot 1
    // (matched from old 1 → old_idx == new_slot → Reuse), A at slot 2
    // (Reorder from old 0).
    assert_event_multiset(
        ['C', 'B', 'A'],
        &events,
        &[
            (ReconcileEventKind::Reorder, 0),
            (ReconcileEventKind::Reuse, 1),
            (ReconcileEventKind::Reorder, 2),
        ],
    );
}
