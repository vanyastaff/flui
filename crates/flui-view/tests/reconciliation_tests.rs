//! Integration tests for the O(N) keyed child reconciliation algorithm.
//!
//! These tests exercise [`reconcile_children`] on the live box-vec model
//! (`Vec<Box<dyn ElementBase>>`) — the structure `VariableChildStorage`
//! actually owns. The algorithm matches old child elements to new views
//! by `View::key()` (keyed children) or by position (un-keyed children),
//! preserving element state across reorders.
//!
//! State preservation is proven via a `StatefulView` whose `ViewState`
//! captures a process-unique `generation` id at `create_state()` time.
//! A *reused* element keeps its original state (and thus its generation);
//! a *recreated* element gets a fresh `create_state()` call and a new
//! generation. Comparing generations before/after a rebuild proves
//! whether an element was moved or rebuilt-by-index.

use std::{
    any::TypeId,
    sync::atomic::{AtomicU64, Ordering},
};

use flui_view::{
    BuildContext, BuildOwner, ElementBase, ElementOwner, Lifecycle, StatefulBehavior,
    StatefulElement, StatefulView, ValueKey, View, ViewState, reconcile_children,
};

// ============================================================================
// Test infrastructure
// ============================================================================

/// Process-wide counter handing out a fresh `generation` to every
/// `KeyedState` created. Lets a test tell "moved element" (same
/// generation) from "recreated element" (new generation) apart.
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// A stateful test view carrying an optional `ValueKey<u32>`.
///
/// Its state captures a generation id at creation; that id survives an
/// `update` (element reuse) but is replaced on `create_element` (a fresh
/// element). The view's `payload` is configuration that *does* change
/// between builds — used to confirm `update` actually re-threaded the
/// new config into a reused element.
///
/// The `ValueKey<u32>` is stored inline (`ValueKey` is `Clone`), so
/// `View::key()` can hand back a borrow that lives as long as `self`.
#[derive(Clone)]
struct KeyedView {
    /// Reconciliation key. `None` => positional matching.
    key: Option<ValueKey<u32>>,
    /// Per-build configuration payload (changes across rebuilds).
    payload: u32,
}

impl KeyedView {
    fn keyed(key: u32, payload: u32) -> Self {
        Self {
            key: Some(ValueKey::new(key)),
            payload,
        }
    }

    fn unkeyed(payload: u32) -> Self {
        Self { key: None, payload }
    }
}

/// Persistent state for [`KeyedView`].
///
/// Carries only the `generation` id — element identity is proven by
/// generation equality. The view's per-build `payload` config is read
/// straight off the live view via `view_as_any`, so the state does not
/// need to mirror it.
struct KeyedState {
    /// Process-unique id stamped at `create_state()` time. Survives an
    /// `update` (element reuse); a fresh `create_state()` mints a new
    /// one (element recreation).
    generation: u64,
}

impl StatefulView for KeyedView {
    type State = KeyedState;

    fn create_state(&self) -> Self::State {
        KeyedState {
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
        }
    }
}

impl ViewState<KeyedView> for KeyedState {
    fn build(&self, _view: &KeyedView, _ctx: &dyn BuildContext) -> Box<dyn View> {
        // A genuine leaf — terminates the recursive build chain.
        Box::new(InertView)
    }
}

impl View for KeyedView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatefulElement::new(self, StatefulBehavior::new(self)))
    }

    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        // `ValueKey<u32>` is stored inline; hand back a borrow that
        // lives as long as `self` — exactly what `View::key()` wants.
        self.key
            .as_ref()
            .map(|k| k as &dyn flui_foundation::ViewKey)
    }
}

/// A genuine leaf view used as `KeyedView`'s built child.
///
/// Its element ([`LeafElement`]) creates no children, so the recursive
/// `perform_build` chain terminates. A `StatelessView` whose `build`
/// returned `self` would describe an infinitely deep tree and overflow
/// the stack — see commit `8a627786`.
#[derive(Clone)]
struct InertView;

impl View for InertView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(LeafElement::new())
    }
}

/// A minimal hand-rolled leaf element: no children, no render object.
/// Just enough `ElementBase` surface to mount/build/unmount as the
/// terminal node of a build chain.
struct LeafElement {
    depth: usize,
    lifecycle: Lifecycle,
}

impl LeafElement {
    fn new() -> Self {
        Self {
            depth: 0,
            lifecycle: Lifecycle::Initial,
        }
    }
}

impl ElementBase for LeafElement {
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<InertView>()
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    fn mount(
        &mut self,
        _parent: Option<flui_foundation::ElementId>,
        slot: usize,
        _: &mut ElementOwner<'_>,
    ) {
        self.depth = slot;
        self.lifecycle = Lifecycle::Active;
    }

    fn unmount(&mut self, _: &mut ElementOwner<'_>) {
        self.lifecycle = Lifecycle::Defunct;
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
    }

    fn update(&mut self, _new_view: &dyn View, _: &mut ElementOwner<'_>) {
        // Leaf — nothing to re-thread.
    }

    fn mark_needs_build(&mut self) {}

    fn perform_build(&mut self, _: &mut ElementOwner<'_>) {
        // Leaf — no children to build. This is what terminates the
        // recursive build chain.
    }

    fn visit_children(&self, _visitor: &mut dyn FnMut(flui_foundation::ElementId)) {}
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

/// Build the initial old-children box-vec from a list of views,
/// mounting each so it reaches the `Active` lifecycle (reconciliation
/// asserts/relies on children being mounted).
fn mount_children(views: &[KeyedView], owner: &mut BuildOwner) -> Vec<Box<dyn ElementBase>> {
    views
        .iter()
        .enumerate()
        .map(|(slot, v)| {
            let mut element = v.create_element();
            element.mount(None, slot, &mut owner.element_owner_mut());
            element.perform_build(&mut owner.element_owner_mut());
            element
        })
        .collect()
}

/// Box up a slice of `KeyedView` as `Vec<Box<dyn View>>` — the shape
/// `VariableChildStorage::update_with_views` receives.
fn boxed_views(views: &[KeyedView]) -> Vec<Box<dyn View>> {
    views
        .iter()
        .map(|v| Box::new(v.clone()) as Box<dyn View>)
        .collect()
}

/// Read the `generation` of the `KeyedState` held by a child element.
///
/// Returns `None` if the element is not a `KeyedView` element (should
/// not happen in these tests).
fn generation_of(element: &dyn ElementBase) -> Option<u64> {
    element
        .state_as_any()
        .and_then(|s| s.downcast_ref::<KeyedState>())
        .map(|s| s.generation)
}

/// Read the live `payload` config off a child element's current view.
fn payload_of(element: &dyn ElementBase) -> Option<u32> {
    element
        .view_as_any()
        .and_then(|v| v.downcast_ref::<KeyedView>())
        .map(|v| v.payload)
}

// ============================================================================
// Happy path — keyed reorder preserves element state (covers AE4)
// ============================================================================

#[test]
fn keyed_children_reordered_preserve_state() {
    let mut owner = BuildOwner::new();

    // Old: [key=1, key=2, key=3]
    let old_views = [
        KeyedView::keyed(1, 100),
        KeyedView::keyed(2, 200),
        KeyedView::keyed(3, 300),
    ];
    let mut children = mount_children(&old_views, &mut owner);

    // Capture generations keyed by their reconciliation key.
    let gen_for_key_1 = generation_of(children[0].as_ref()).unwrap();
    let gen_for_key_2 = generation_of(children[1].as_ref()).unwrap();
    let gen_for_key_3 = generation_of(children[2].as_ref()).unwrap();

    // New: [key=3, key=1, key=2] — a strict reorder, new payloads.
    let new_views = boxed_views(&[
        KeyedView::keyed(3, 333),
        KeyedView::keyed(1, 111),
        KeyedView::keyed(2, 222),
    ]);

    let view_refs: Vec<&dyn View> = new_views.iter().map(AsRef::as_ref).collect();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 3);

    // Each slot now holds the element whose key matches — and crucially
    // the SAME element (same generation), proving state was preserved
    // and the element was moved, not rebuilt by index.
    assert_eq!(
        generation_of(children[0].as_ref()).unwrap(),
        gen_for_key_3,
        "slot 0 should hold the moved key=3 element"
    );
    assert_eq!(
        generation_of(children[1].as_ref()).unwrap(),
        gen_for_key_1,
        "slot 1 should hold the moved key=1 element"
    );
    assert_eq!(
        generation_of(children[2].as_ref()).unwrap(),
        gen_for_key_2,
        "slot 2 should hold the moved key=2 element"
    );

    // The reused elements received the new config payloads via `update`.
    assert_eq!(payload_of(children[0].as_ref()), Some(333));
    assert_eq!(payload_of(children[1].as_ref()), Some(111));
    assert_eq!(payload_of(children[2].as_ref()), Some(222));
}

// ============================================================================
// Happy path — un-keyed children fall back to positional matching
// ============================================================================

#[test]
fn unkeyed_children_match_positionally() {
    let mut owner = BuildOwner::new();

    let old_views = [
        KeyedView::unkeyed(10),
        KeyedView::unkeyed(20),
        KeyedView::unkeyed(30),
    ];
    let mut children = mount_children(&old_views, &mut owner);

    let gen_slot_0 = generation_of(children[0].as_ref()).unwrap();
    let gen_slot_1 = generation_of(children[1].as_ref()).unwrap();
    let gen_slot_2 = generation_of(children[2].as_ref()).unwrap();

    // Same length, all un-keyed, different payloads → positional reuse.
    let new_views = boxed_views(&[
        KeyedView::unkeyed(11),
        KeyedView::unkeyed(22),
        KeyedView::unkeyed(33),
    ]);
    let view_refs: Vec<&dyn View> = new_views.iter().map(AsRef::as_ref).collect();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 3);
    // Positional matching: each slot keeps the element that was there.
    assert_eq!(generation_of(children[0].as_ref()).unwrap(), gen_slot_0);
    assert_eq!(generation_of(children[1].as_ref()).unwrap(), gen_slot_1);
    assert_eq!(generation_of(children[2].as_ref()).unwrap(), gen_slot_2);
    // Config payloads updated in place.
    assert_eq!(payload_of(children[0].as_ref()), Some(11));
    assert_eq!(payload_of(children[1].as_ref()), Some(22));
    assert_eq!(payload_of(children[2].as_ref()), Some(33));
}

// ============================================================================
// Edge cases — empty / single-element lists
// ============================================================================

#[test]
fn empty_to_empty_is_noop() {
    let mut owner = BuildOwner::new();
    let mut children: Vec<Box<dyn ElementBase>> = Vec::new();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &[],
        &mut owner.element_owner_mut(),
    );
    assert!(children.is_empty());
}

#[test]
fn empty_old_creates_all() {
    let mut owner = BuildOwner::new();
    let mut children: Vec<Box<dyn ElementBase>> = Vec::new();

    let new_views = boxed_views(&[
        KeyedView::keyed(1, 1),
        KeyedView::keyed(2, 2),
        KeyedView::unkeyed(3),
    ]);
    let view_refs: Vec<&dyn View> = new_views.iter().map(AsRef::as_ref).collect();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 3);
    assert_eq!(payload_of(children[0].as_ref()), Some(1));
    assert_eq!(payload_of(children[1].as_ref()), Some(2));
    assert_eq!(payload_of(children[2].as_ref()), Some(3));
}

#[test]
fn empty_new_removes_all() {
    let mut owner = BuildOwner::new();
    let old_views = [KeyedView::keyed(1, 1), KeyedView::keyed(2, 2)];
    let mut children = mount_children(&old_views, &mut owner);

    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &[],
        &mut owner.element_owner_mut(),
    );
    assert!(children.is_empty());
}

#[test]
fn single_to_single_keyed_match_reuses() {
    let mut owner = BuildOwner::new();
    let old_views = [KeyedView::keyed(7, 70)];
    let mut children = mount_children(&old_views, &mut owner);
    let original_gen = generation_of(children[0].as_ref()).unwrap();

    let new_views = boxed_views(&[KeyedView::keyed(7, 77)]);
    let view_refs: Vec<&dyn View> = new_views.iter().map(AsRef::as_ref).collect();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 1);
    assert_eq!(generation_of(children[0].as_ref()).unwrap(), original_gen);
    assert_eq!(payload_of(children[0].as_ref()), Some(77));
}

#[test]
fn single_to_single_key_mismatch_replaces() {
    let mut owner = BuildOwner::new();
    let old_views = [KeyedView::keyed(7, 70)];
    let mut children = mount_children(&old_views, &mut owner);
    let original_gen = generation_of(children[0].as_ref()).unwrap();

    // Different key => not updatable => element replaced.
    let new_views = boxed_views(&[KeyedView::keyed(8, 80)]);
    let view_refs: Vec<&dyn View> = new_views.iter().map(AsRef::as_ref).collect();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 1);
    assert_ne!(
        generation_of(children[0].as_ref()).unwrap(),
        original_gen,
        "key mismatch must force a fresh element"
    );
    assert_eq!(payload_of(children[0].as_ref()), Some(80));
}

// ============================================================================
// Edge cases — prepend / append / middle insert+remove
// ============================================================================

#[test]
fn keyed_prepend_preserves_existing() {
    let mut owner = BuildOwner::new();
    let old_views = [KeyedView::keyed(1, 1), KeyedView::keyed(2, 2)];
    let mut children = mount_children(&old_views, &mut owner);
    let gen_1 = generation_of(children[0].as_ref()).unwrap();
    let gen_2 = generation_of(children[1].as_ref()).unwrap();

    // Prepend key=0.
    let new_views = boxed_views(&[
        KeyedView::keyed(0, 0),
        KeyedView::keyed(1, 1),
        KeyedView::keyed(2, 2),
    ]);
    let view_refs: Vec<&dyn View> = new_views.iter().map(AsRef::as_ref).collect();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 3);
    // slot 0 is brand new; slots 1,2 are the moved originals.
    assert_ne!(generation_of(children[0].as_ref()).unwrap(), gen_1);
    assert_ne!(generation_of(children[0].as_ref()).unwrap(), gen_2);
    assert_eq!(generation_of(children[1].as_ref()).unwrap(), gen_1);
    assert_eq!(generation_of(children[2].as_ref()).unwrap(), gen_2);
}

#[test]
fn keyed_append_preserves_existing() {
    let mut owner = BuildOwner::new();
    let old_views = [KeyedView::keyed(1, 1), KeyedView::keyed(2, 2)];
    let mut children = mount_children(&old_views, &mut owner);
    let gen_1 = generation_of(children[0].as_ref()).unwrap();
    let gen_2 = generation_of(children[1].as_ref()).unwrap();

    let new_views = boxed_views(&[
        KeyedView::keyed(1, 1),
        KeyedView::keyed(2, 2),
        KeyedView::keyed(3, 3),
    ]);
    let view_refs: Vec<&dyn View> = new_views.iter().map(AsRef::as_ref).collect();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 3);
    assert_eq!(generation_of(children[0].as_ref()).unwrap(), gen_1);
    assert_eq!(generation_of(children[1].as_ref()).unwrap(), gen_2);
    // slot 2 is fresh.
    assert_ne!(generation_of(children[2].as_ref()).unwrap(), gen_1);
    assert_ne!(generation_of(children[2].as_ref()).unwrap(), gen_2);
}

#[test]
fn keyed_middle_insert_and_remove_combined() {
    let mut owner = BuildOwner::new();
    // Old: [1, 2, 3, 4]
    let old_views = [
        KeyedView::keyed(1, 1),
        KeyedView::keyed(2, 2),
        KeyedView::keyed(3, 3),
        KeyedView::keyed(4, 4),
    ];
    let mut children = mount_children(&old_views, &mut owner);
    let gen_1 = generation_of(children[0].as_ref()).unwrap();
    let gen_2 = generation_of(children[1].as_ref()).unwrap();
    let gen_4 = generation_of(children[3].as_ref()).unwrap();

    // New: [1, 9, 2, 4] — key=3 removed, key=9 inserted in the middle.
    let new_views = boxed_views(&[
        KeyedView::keyed(1, 1),
        KeyedView::keyed(9, 9),
        KeyedView::keyed(2, 2),
        KeyedView::keyed(4, 4),
    ]);
    let view_refs: Vec<&dyn View> = new_views.iter().map(AsRef::as_ref).collect();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 4);
    assert_eq!(generation_of(children[0].as_ref()).unwrap(), gen_1);
    // slot 1 (key=9) is fresh.
    assert_ne!(generation_of(children[1].as_ref()).unwrap(), gen_1);
    assert_ne!(generation_of(children[1].as_ref()).unwrap(), gen_2);
    assert_ne!(generation_of(children[1].as_ref()).unwrap(), gen_4);
    assert_eq!(generation_of(children[2].as_ref()).unwrap(), gen_2);
    assert_eq!(generation_of(children[3].as_ref()).unwrap(), gen_4);
}

// ============================================================================
// Type-mismatch handling
// ============================================================================

#[test]
fn type_change_at_position_replaces_element() {
    let mut owner = BuildOwner::new();
    // Old child is a KeyedView; new view at slot 0 is an InertView
    // (a different concrete type) — must replace.
    let mut children: Vec<Box<dyn ElementBase>> = {
        let view = KeyedView::unkeyed(1);
        let mut el = view.create_element();
        el.mount(None, 0, &mut owner.element_owner_mut());
        el.perform_build(&mut owner.element_owner_mut());
        vec![el]
    };
    assert!(
        generation_of(children[0].as_ref()).is_some(),
        "the old child should be a KeyedView element"
    );

    let inert: Box<dyn View> = Box::new(InertView);
    let view_refs: Vec<&dyn View> = vec![inert.as_ref()];
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 1);
    // The new element is an InertView element — no KeyedState.
    assert!(generation_of(children[0].as_ref()).is_none());
}

// ============================================================================
// Error path — duplicate keys (first-wins, non-panicking)
// ============================================================================

#[test]
fn duplicate_keys_in_new_list_first_wins_no_panic() {
    let mut owner = BuildOwner::new();
    let old_views = [KeyedView::keyed(1, 1), KeyedView::keyed(2, 2)];
    let mut children = mount_children(&old_views, &mut owner);
    let gen_1 = generation_of(children[0].as_ref()).unwrap();

    // New list has key=1 TWICE. Defined behavior: the first occurrence
    // claims the matching old element; the second gets a fresh element.
    // Must not panic.
    let new_views = boxed_views(&[
        KeyedView::keyed(1, 100),
        KeyedView::keyed(1, 999),
        KeyedView::keyed(2, 2),
    ]);
    let view_refs: Vec<&dyn View> = new_views.iter().map(AsRef::as_ref).collect();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 3);
    // First key=1 reused the original element.
    assert_eq!(generation_of(children[0].as_ref()).unwrap(), gen_1);
    assert_eq!(payload_of(children[0].as_ref()), Some(100));
    // Second key=1 is a fresh element (first-wins).
    assert_ne!(generation_of(children[1].as_ref()).unwrap(), gen_1);
    assert_eq!(payload_of(children[1].as_ref()), Some(999));
}

// ============================================================================
// Large lists — linear-time sanity
// ============================================================================

#[test]
fn large_keyed_list_full_reuse() {
    let mut owner = BuildOwner::new();
    let old_views: Vec<KeyedView> = (0..200).map(|i| KeyedView::keyed(i, i)).collect();
    let mut children = mount_children(&old_views, &mut owner);
    let generations: Vec<u64> = children
        .iter()
        .map(|c| generation_of(c.as_ref()).unwrap())
        .collect();

    // Same keys, new payloads — every element should be reused in place.
    let new_views = boxed_views(
        &(0..200)
            .map(|i| KeyedView::keyed(i, i + 1000))
            .collect::<Vec<_>>(),
    );
    let view_refs: Vec<&dyn View> = new_views.iter().map(AsRef::as_ref).collect();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 200);
    for (i, child) in children.iter().enumerate() {
        assert_eq!(
            generation_of(child.as_ref()).unwrap(),
            generations[i],
            "element {i} should have been reused"
        );
    }
}

#[test]
fn large_keyed_list_full_reverse_preserves_all() {
    let mut owner = BuildOwner::new();
    let old_views: Vec<KeyedView> = (0..100).map(|i| KeyedView::keyed(i, i)).collect();
    let mut children = mount_children(&old_views, &mut owner);
    // Map key -> generation.
    let gen_by_key: std::collections::HashMap<u32, u64> = (0..100)
        .map(|i| (i, generation_of(children[i as usize].as_ref()).unwrap()))
        .collect();

    // Reverse the order.
    let new_views = boxed_views(
        &(0..100)
            .rev()
            .map(|i| KeyedView::keyed(i, i))
            .collect::<Vec<_>>(),
    );
    let view_refs: Vec<&dyn View> = new_views.iter().map(AsRef::as_ref).collect();
    reconcile_children(
        flui_foundation::ElementId::new(1),
        &mut children,
        &view_refs,
        &mut owner.element_owner_mut(),
    );

    assert_eq!(children.len(), 100);
    // Every element should still be present, matched by key, none rebuilt.
    for (slot, key) in (0..100).rev().enumerate() {
        assert_eq!(
            generation_of(children[slot].as_ref()).unwrap(),
            gen_by_key[&key],
            "key {key} element should have moved, not rebuilt"
        );
    }
}
