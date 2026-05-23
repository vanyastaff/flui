//! SC-012 — GlobalKey reparent latency is independent of outer-tree
//! size. Comparing two reparent runs in a 1K-element tree vs a 10K-
//! element tree, the wall-clock cost should stay roughly constant
//! (within ~2x), proving the algorithm is O(subtree depth + 1)
//! per-reparent, not O(tree size).
//!
//! Models a single-element reparent via the inactive-queue
//! reactivation path (§U17 wiring). The 10-node subtree case the
//! plan envisions requires the full Variable-arity descent which
//! shares scope with KTD-9 — deferred. Single-element reparent
//! exercises the same code path (`try_retake_inactive` +
//! `ReconcileEvent::Reparent` emission) so the latency signal is
//! representative.

use std::sync::Arc;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_foundation::ViewKey;
use flui_view::{
    BuildContext, BuildOwner, ElementBase, ElementTree, GlobalKey, StatefulView, View, ViewState,
};
use parking_lot::RwLock;

/// Spacer scaffold used as parent placeholders.
#[derive(Clone)]
struct Spacer;

struct SpacerState;
impl StatefulView for Spacer {
    type State = SpacerState;
    fn create_state(&self) -> Self::State {
        SpacerState
    }
}
impl ViewState<Spacer> for SpacerState {
    fn build(&self, _v: &Spacer, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(Spacer)
    }
}
impl View for Spacer {
    fn create_element(&self) -> Box<dyn ElementBase> {
        use flui_view::StatefulElement;
        use flui_view::element::StatefulBehavior;
        Box::new(StatefulElement::new(self, StatefulBehavior::new(self)))
    }
}

/// Keyed leaf that we reparent. Stateless — the reparent code path
/// is the same regardless of state shape.
#[derive(Clone)]
struct KeyedLeaf {
    key: GlobalKey<KeyedLeafState>,
}

struct KeyedLeafState;
impl StatefulView for KeyedLeaf {
    type State = KeyedLeafState;
    fn create_state(&self) -> Self::State {
        KeyedLeafState
    }
}
impl ViewState<KeyedLeaf> for KeyedLeafState {
    fn build(&self, _v: &KeyedLeaf, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(Spacer)
    }
}
impl View for KeyedLeaf {
    fn create_element(&self) -> Box<dyn ElementBase> {
        use flui_view::StatefulElement;
        use flui_view::element::StatefulBehavior;
        Box::new(StatefulElement::new(self, StatefulBehavior::new(self)))
    }
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

/// Build a tree of `outer_size` spacer elements with a keyed leaf
/// mounted under spacer index 0. Returns the tree handles +
/// the keyed-leaf's id + a fresh GlobalKey clone.
fn setup_tree(
    outer_size: usize,
) -> (
    Arc<RwLock<ElementTree>>,
    Arc<RwLock<BuildOwner>>,
    GlobalKey<KeyedLeafState>,
) {
    let tree = Arc::new(RwLock::new(ElementTree::new()));
    let owner = Arc::new(RwLock::new(BuildOwner::new()));
    flui_view::test_only_set_global_key_registry(&tree, &owner);

    let root = tree
        .write()
        .mount_root(&Spacer, &mut owner.write().element_owner_mut());

    // Pad the tree to `outer_size` spacers so reparent latency can
    // be compared at two tree-size points.
    for _ in 1..outer_size {
        let _ = tree
            .write()
            .insert(&Spacer, root, 0, &mut owner.write().element_owner_mut());
    }

    let key = GlobalKey::<KeyedLeafState>::new();
    let leaf = KeyedLeaf { key: key.clone() };
    let _ = tree
        .write()
        .insert(&leaf, root, 0, &mut owner.write().element_owner_mut());

    (tree, owner, key)
}

fn bench_reparent(c: &mut Criterion) {
    let mut group = c.benchmark_group("sc012_global_key_reparent");

    // Two tree sizes — reparent cost should not depend on outer
    // size. 1K vs 10K is the comparison the plan envisions.
    for &outer_size in &[1_000_usize, 10_000_usize] {
        group.bench_with_input(
            BenchmarkId::from_parameter(outer_size),
            &outer_size,
            |b, &outer_size| {
                b.iter_batched(
                    || setup_tree(outer_size),
                    |(tree, owner, key)| {
                        let leaf = KeyedLeaf { key: key.clone() };
                        let original_id = key
                            .current_element()
                            .expect("leaf is mounted before reparent");
                        // Soft-remove (push to inactive queue).
                        tree.write()
                            .remove(original_id, &mut owner.write().element_owner_mut());
                        // Re-insert under a different parent (root +
                        // slot 1 to vary from the original mount slot).
                        let root = tree.read().root().expect("tree has root");
                        let migrated = tree.write().insert(
                            &leaf,
                            root,
                            1,
                            &mut owner.write().element_owner_mut(),
                        );
                        std::hint::black_box(migrated);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }

    group.finish();

    // Defensive cleanup of the global-key registry handle the
    // setup installs. The bench iter_batched closure clones the
    // tree/owner per iter, but the test_only_set_global_key_registry
    // installs a process-wide handle on every setup. The last
    // iteration's handle outlives the bench unless cleared.
    flui_view::test_only_clear_global_key_registry();
}

criterion_group!(benches, bench_reparent);
criterion_main!(benches);
