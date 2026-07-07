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

// Bench harness, not public API; `criterion_group!` generates the
// undocumentable entry fn.
#![allow(missing_docs)]

use std::sync::Arc;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_foundation::ViewKey;
use flui_view::{
    BuildContext, BuildOwner, ElementTree, GlobalKey, IntoView, StatefulView, View, ViewExt,
    ViewState,
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
    fn build(&self, _v: &Spacer, _ctx: &dyn BuildContext) -> impl IntoView {
        Spacer.boxed()
    }
}
impl View for Spacer {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
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
    fn build(&self, _v: &KeyedLeaf, _ctx: &dyn BuildContext) -> impl IntoView {
        Spacer.boxed()
    }
}
impl View for KeyedLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

/// Bench fixture tuple: tree handle, build-owner handle, GlobalKey,
/// and the keyed leaf's mounted ElementId. Extracted to silence
/// `clippy::type_complexity` on the setup-fn signature.
type SetupOutputs = (
    Arc<RwLock<ElementTree>>,
    Arc<RwLock<BuildOwner>>,
    GlobalKey<KeyedLeafState>,
    flui_foundation::ElementId,
);

/// Build a tree of `outer_size` spacer elements with a keyed leaf
/// mounted under spacer index 0. Returns the tree handles, the
/// keyed-leaf's id, and a fresh GlobalKey clone.
///
/// Reviewer fix (adversarial finding #2): the GlobalKey REGISTRY is
/// a process-wide singleton, and `test_only_set_global_key_registry`
/// REPLACES the prior handle on every call. With criterion's
/// `BatchSize::LargeInput`, setups are run in a batch BEFORE the
/// measurement loop — only the LAST setup's handle survives in
/// REGISTRY, so an earlier iter's `key.current_element()` resolves
/// against a later iter's tree (returns None → `.expect()` panics).
/// Two fixes layered: (1) return the leaf's `ElementId` directly
/// so the measurement doesn't depend on the REGISTRY at all; (2)
/// switch to `BatchSize::PerIteration` so setup + measurement
/// interleave, AND install the registry inside the measurement
/// closure (each iter installs against its own tree). Either fix
/// alone would prevent the panic; both layers belt-and-braces
/// because the registry singleton is fundamentally fragile.
fn setup_tree(outer_size: usize) -> SetupOutputs {
    let tree = Arc::new(RwLock::new(ElementTree::new()));
    let owner = Arc::new(RwLock::new(BuildOwner::new()));

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
    let leaf_id = tree
        .write()
        .insert(&leaf, root, 0, &mut owner.write().element_owner_mut());

    (tree, owner, key, leaf_id)
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
                // `PerIteration` interleaves setup with measurement.
                // The batched alternative (`LargeInput`) batched all
                // setups first, then measured — and the GlobalKey
                // REGISTRY singleton meant only the last setup's
                // handle survived, panicking earlier-iter measurements.
                b.iter_batched(
                    || setup_tree(outer_size),
                    |(tree, owner, key, leaf_id)| {
                        // Install the GlobalKey registry handle
                        // INSIDE the measurement closure so each iter
                        // gets the handle pointing at its own tree.
                        flui_view::test_only_set_global_key_registry(&tree, &owner);
                        let leaf = KeyedLeaf { key: key.clone() };
                        // Soft-remove (push to inactive queue). Use
                        // the captured leaf_id directly — no REGISTRY
                        // lookup that could race.
                        tree.write()
                            .remove(leaf_id, &mut owner.write().element_owner_mut());
                        // Re-insert under a different slot to vary
                        // from the original mount slot (1 instead of 0).
                        let root = tree.read().root().expect("tree has root");
                        let migrated = tree.write().insert(
                            &leaf,
                            root,
                            1,
                            &mut owner.write().element_owner_mut(),
                        );
                        std::hint::black_box(migrated);
                        // Clear the registry before the next iter's
                        // setup runs — keeps the singleton in a known
                        // state and avoids the cross-iter handle leak.
                        flui_view::test_only_clear_global_key_registry();
                    },
                    BatchSize::PerIteration,
                );
            },
        );
    }

    group.finish();

    // Final defensive cleanup. The measurement loop clears per-iter,
    // but if the bench is interrupted mid-iter the last setup's handle
    // could otherwise leak across to a follow-up bench.
    flui_view::test_only_clear_global_key_registry();
}

criterion_group!(benches, bench_reparent);
criterion_main!(benches);
