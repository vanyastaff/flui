//! Slot-addressed side tables: membership by full key, clears that cost what
//! they hold.

use std::num::NonZeroU32;

use flui_foundation::generation::Generation;
use flui_foundation::{ElementId, RenderId, SlotIndex, SlotSet};

fn gen_of(raw: u32) -> Generation {
    Generation::try_from(NonZeroU32::new(raw).expect("non-zero")).expect("below the reserved bit")
}

#[test]
fn a_reused_slot_is_not_a_member_under_its_old_generation() {
    let mut set = SlotSet::<ElementId>::new();
    let old = ElementId::new_gen(7, gen_of(1));
    let reused = ElementId::new_gen(7, gen_of(2));
    assert!(set.insert(old));
    assert!(set.contains(old));
    assert!(!set.contains(reused), "the slot alone is not the identity");
    assert!(!set.remove(reused));
    assert!(set.contains(old));
}

#[test]
fn insert_is_idempotent_and_clear_forgets_every_member() {
    let mut set = SlotSet::<RenderId>::new();
    let ids: Vec<RenderId> = (1..=5).map(RenderId::new).collect();
    for &id in &ids {
        assert!(set.insert(id));
        assert!(!set.insert(id), "{id:?} is already a member");
    }
    assert_eq!(set.as_slice(), ids.as_slice());
    set.clear();
    assert!(set.is_empty());
    for &id in &ids {
        assert!(!set.contains(id), "{id:?} survived the clear");
        assert!(set.insert(id), "{id:?} is insertable again");
    }
}

#[test]
fn remove_moves_the_last_member_and_keeps_it_findable() {
    let mut set = SlotSet::<RenderId>::new();
    let [a, b, c] = [1, 2, 3].map(RenderId::new);
    for id in [a, b, c] {
        set.insert(id);
    }
    assert!(set.remove(a));
    assert_eq!(set.as_slice(), &[c, b]);
    assert!(set.contains(b) && set.contains(c) && !set.contains(a));
    assert!(set.remove(c));
    assert_eq!(set.as_slice(), &[b]);
}

#[test]
fn an_index_confirms_the_entry_after_a_reorder() {
    let mut entries: Vec<RenderId> = [3, 1, 2].map(RenderId::new).to_vec();
    let mut index = SlotIndex::<RenderId>::new();
    index.reindex(entries.iter().copied());
    entries.sort();
    // Not yet reindexed: a stale position never answers with a neighbour.
    for &id in &entries {
        if let Some(position) = index.find(&entries, id, |e| *e) {
            assert_eq!(entries[position], id);
        }
    }
    index.reindex(entries.iter().copied());
    for (position, &id) in entries.iter().enumerate() {
        assert_eq!(index.find(&entries, id, |e| *e), Some(position));
    }
}
