//! `ElementSlot` is `flui_foundation::IndexedSlot<ElementId>` (an alias, not a
//! wrapper) and round-trips `Option<ElementId>` previous-sibling payloads.
//! Co-importing `flui_view::prelude::*` and `flui_foundation::prelude::*`
//! compiles without an `ElementId` ambiguity.

use flui_foundation::{ElementId, IndexedSlot};
use flui_view::ElementSlot;

#[test]
fn element_slot_aliases_to_the_foundation_indexed_slot() {
    // ElementSlot is exactly IndexedSlot<ElementId> -- not a wrapper.
    let from_alias: ElementSlot = ElementSlot::first();
    let from_canonical: IndexedSlot<ElementId> = IndexedSlot::<ElementId>::first();
    assert_eq!(from_alias, from_canonical);
}

#[test]
fn element_slot_round_trips_previous_sibling() {
    // Mirror the Flutter `IndexedSlot<Element?>` payload semantics: the
    // first slot has no previous, subsequent slots carry the prior sibling's
    // ElementId.
    let first = ElementSlot::first();
    assert_eq!(first.index(), 0);
    assert!(first.is_first());
    assert!(first.previous().is_none());

    let id_one = ElementId::new(1);
    let second = first.next(id_one);
    assert_eq!(second.index(), 1);
    assert!(!second.is_first());
    assert_eq!(second.previous(), Some(id_one));

    // `new(idx, Some(prev))` constructs an arbitrary-index slot directly
    // -- equivalent to the deleted `ElementSlot::after(idx, prev)`.
    let manual = ElementSlot::new(5, Some(ElementId::new(42)));
    assert_eq!(manual.index(), 5);
    assert_eq!(manual.previous(), Some(ElementId::new(42)));
}

#[test]
fn element_slot_preserves_value_semantics() {
    // IndexedSlot is Copy + Eq + Hash via its derives.
    let slot_a = ElementSlot::new(2, Some(ElementId::new(7)));
    let slot_b = slot_a; // Copy
    assert_eq!(slot_a, slot_b);

    use std::collections::HashSet;
    let mut set: HashSet<ElementSlot> = HashSet::new();
    set.insert(slot_a);
    set.insert(slot_b); // duplicate
    set.insert(ElementSlot::new(2, Some(ElementId::new(8)))); // distinct payload
    assert_eq!(set.len(), 2);
}

#[test]
fn view_and_foundation_preludes_co_import_without_ambiguity() {
    // Importing both preludes must not collide. `ElementId` appears in both:
    // flui-view's must be the re-export of flui-foundation's, so the glob
    // imports name one type. `IndexedSlot` comes from flui-view's prelude
    // alone; `element_slot_aliases_to_the_foundation_indexed_slot` pins that
    // it is foundation's type.
    use flui_foundation::prelude::*;
    use flui_view::prelude::*;

    let slot: IndexedSlot<ElementId> = IndexedSlot::first();
    assert_eq!(slot.index(), 0);

    // ElementSlot resolves via flui-view's prelude.
    let alias: ElementSlot = ElementSlot::first();
    assert_eq!(alias, slot);
}
