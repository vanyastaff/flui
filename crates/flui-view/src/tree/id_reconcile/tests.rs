use super::*;
use crate::view::{IntoView, View, ViewExt};
use crate::{BuildContext, BuildOwner, StatelessView};
use flui_foundation::{ValueKey, ViewKey};

/// A keyless leaf-ish stateless test view. `tag` distinguishes
/// instances; the self-returning `build` is never driven here (the
/// id-reconciler does not call `perform_build`), so it cannot
/// recurse.
#[derive(Clone)]
struct TestView {
    #[expect(
        dead_code,
        reason = "carried only so distinct instances differ under Clone"
    )]
    tag: u32,
}

impl TestView {
    fn new(tag: u32) -> Self {
        Self { tag }
    }
}

impl StatelessView for TestView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for TestView {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateless(self)
    }
}

/// A keyed stateless test view carrying a `ValueKey<u32>`. Reuses the
/// same `StatelessElement` machinery; `key()` is overridden so the
/// reconciler can match by key.
#[derive(Clone)]
struct KeyedView {
    key: ValueKey<u32>,
}

impl KeyedView {
    fn new(key: u32) -> Self {
        Self {
            key: ValueKey::new(key),
        }
    }
}

impl StatelessView for KeyedView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
    }
}

impl View for KeyedView {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

/// Build a `Vec<Box<dyn View>>` of keyless `TestView`s with the given
/// tags.
fn plain_views(tags: &[u32]) -> Vec<Box<dyn View>> {
    tags.iter()
        .map(|&t| Box::new(TestView::new(t)) as Box<dyn View>)
        .collect()
}

/// Build a `Vec<Box<dyn View>>` of keyed `KeyedView`s with the given
/// keys.
fn keyed_views(keys: &[u32]) -> Vec<Box<dyn View>> {
    keys.iter()
        .map(|&k| Box::new(KeyedView::new(k)) as Box<dyn View>)
        .collect()
}

/// Mount a fresh keyless root and return `(tree, owner, root_id)`.
fn fixture() -> (ElementTree, BuildOwner, ElementId) {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root_id = tree.mount_root(&TestView::new(0), &mut owner.element_owner_mut());
    (tree, owner, root_id)
}

/// Keyed reorder: permuting keyed children makes the stored ids
/// follow their keys (the element — and thus its state — moves with
/// its key, it is not absorbed by the sibling in the old position).
fn keyed_reorder_ids_follow_keys() {
    let (mut tree, mut owner, root) = fixture();

    reconcile_children_by_id(
        &mut tree,
        root,
        &keyed_views(&[1, 2, 3]),
        &mut owner.element_owner_mut(),
    );
    let before = tree.get(root).unwrap().child_ids().to_vec();
    assert_eq!(before.len(), 3);
    let (id1, id2, id3) = (before[0], before[1], before[2]);

    // Reorder keys [1,2,3] -> [3,1,2].
    reconcile_children_by_id(
        &mut tree,
        root,
        &keyed_views(&[3, 1, 2]),
        &mut owner.element_owner_mut(),
    );
    let after = tree.get(root).unwrap().child_ids().to_vec();

    assert_eq!(
        after,
        vec![id3, id1, id2],
        "each keyed child id must move to the slot its key now occupies",
    );
    // No element was created or destroyed: same three ids, same slab size.
    assert_eq!(tree.len(), 4, "reorder must not insert or remove any node");
    for id in [id1, id2, id3] {
        assert!(
            tree.get(id).is_some(),
            "every reordered id must still resolve"
        );
    }
}

/// Type-mismatch replacement: a keyless slot whose view type changes
/// is removed and a fresh element of the new type is inserted (not
/// reused).
fn type_mismatch_replaces_child() {
    let (mut tree, mut owner, root) = fixture();

    // Start with one keyless TestView child.
    reconcile_children_by_id(
        &mut tree,
        root,
        &plain_views(&[1]),
        &mut owner.element_owner_mut(),
    );
    let old_id = tree.get(root).unwrap().child_ids()[0];

    // Replace with a single KeyedView (different concrete type).
    reconcile_children_by_id(
        &mut tree,
        root,
        &keyed_views(&[9]),
        &mut owner.element_owner_mut(),
    );
    let new_id = tree.get(root).unwrap().child_ids()[0];

    assert_ne!(new_id, old_id, "type change must mint a fresh element");
    assert!(
        tree.get(old_id).is_none(),
        "replaced element must be removed"
    );
    assert!(
        tree.get(new_id).is_some(),
        "replacement element must resolve"
    );
}

#[test]
fn id_reconcile_matrix() {
    crate::table_test::run_table(
        "id_reconcile_matrix",
        &[
            (
                "keyed_reorder_ids_follow_keys",
                keyed_reorder_ids_follow_keys as fn(),
            ),
            (
                "type_mismatch_replaces_child",
                type_mismatch_replaces_child as fn(),
            ),
        ],
    );
}

/// `flui::reconcile` emission coverage on the LIVE slab path
/// (catalog #3). The production reconciler
/// emits one typed [`ReconcileEvent`](super::ReconcileEvent) per
/// child disposition so devtools / selection-persistence subscribers
/// reconstruct each frame's outcome WITHOUT a tree diff. Before this
/// wiring `reconcile_children_by_id` emitted ZERO events, so every
/// test here fails its multiset assertion (a real red→green guard,
/// not a tautology).
///
/// Per the collector's process-global tracing-callsite caveat, these
/// install a per-thread dispatcher and are `#[serial]`-gated so a
/// concurrent dispatcher swap cannot make a freshly installed
/// collector miss events.
mod emission {
    use flui_foundation::ElementId;
    use serial_test::serial;
    use tracing::dispatcher::Dispatch;
    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt;

    use super::super::reconcile_children_by_id;
    use super::{fixture, keyed_views};
    use std::sync::OnceLock;

    use crate::BuildOwner;
    use crate::tree::ElementTree;
    use crate::tree::ReconcileEventKind;
    use crate::tree::test_utils::{CollectedEvent, ReconcileEventCollector};
    use crate::view::View;

    /// Process-global guard so the keep-alive subscriber installs once.
    static GLOBAL_SUBSCRIBER: OnceLock<()> = OnceLock::new();

    /// Install an *interested* process-global default subscriber ONCE
    /// for this test binary.
    ///
    /// The `flui::reconcile` callsite is shared by every reconcile,
    /// including the many `ElementTree::insert` calls (an emit site) in
    /// non-collector tests that run in PARALLEL. If one hits the callsite
    /// while the only default is the no-op global, tracing caches
    /// `Interest::never` and the callsite goes dead — later per-thread
    /// collectors are then bypassed (tests pass in isolation but fail in
    /// the full suite). `tracing::callsite::rebuild_interest_cache()` is
    /// NOT sufficient: it re-evaluates against the GLOBAL default (still
    /// the no-op), so under the suite's parallel emit pressure the
    /// callsite re-poisons. Installing an interested global default (a
    /// bare `Registry`, no layer → records nothing) keeps the callsite
    /// permanently armed; per-event dispatch still routes to the CURRENT
    /// thread's `with_default` collector, so each test's events stay
    /// isolated. Confined to this test binary (a separate process from
    /// every `tests/*.rs`), and nothing else here installs a global
    /// default, so it cannot interfere.
    fn ensure_global_subscriber() {
        GLOBAL_SUBSCRIBER.get_or_init(|| {
            // Ignore Err: only need *an* interested global default present.
            let _ = tracing::subscriber::set_global_default(Registry::default());
        });
    }

    /// Capture the `flui::reconcile` events `body` emits on this thread.
    fn capture<F: FnOnce()>(body: F) -> Vec<CollectedEvent> {
        ensure_global_subscriber();
        let collector = ReconcileEventCollector::new();
        let subscriber = Registry::default().with(collector.layer());
        // Disarm `tracing`'s process-global callsite-interest cache first: it is
        // computed on whichever thread reaches a callsite FIRST, so without this a
        // sibling test can have it cached as `never` and silently empty this capture.
        // See `flui_testing::log_capture`.
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
        collector.events()
    }

    /// Assert the captured events carry exactly the expected
    /// `(kind, slot)` dispositions as a MULTISET. Both sides are
    /// sorted before comparison so a test does not depend on the
    /// HashMap-iteration order of the keyed-middle phase's multiset
    /// contract — `expected` is written in natural emission order
    /// at the call site.
    fn assert_dispositions(events: &[CollectedEvent], expected: &[(ReconcileEventKind, u64)]) {
        let sort_key = |(kind, slot): &(ReconcileEventKind, u64)| (*kind as u8, *slot);
        let mut actual: Vec<(ReconcileEventKind, u64)> =
            events.iter().map(|e| (e.kind, e.slot)).collect();
        actual.sort_by_key(sort_key);
        let mut want = expected.to_vec();
        want.sort_by_key(sort_key);
        assert_eq!(
            actual, want,
            "reconcile disposition multiset mismatch\n  expected: {want:?}\n  actual:   {actual:?}\n  full events: {events:?}",
        );
    }

    /// Seed `parent` with `views` via direct slab inserts, bypassing
    /// the reconciler so NO `flui::reconcile` event fires during
    /// setup. This matters: tracing's callsite-interest cache is
    /// process-global, so if the production emit callsite is first
    /// exercised OUTSIDE a collector scope it can latch "no interest"
    /// and the first captured reconcile then observes zero events.
    /// Building the prior state with raw inserts keeps every emit
    /// inside a `capture` — the same discipline the reconciler test corpus uses
    /// (it mounts its initial tree directly, never via a warmup
    /// reconcile).
    fn seed(
        tree: &mut ElementTree,
        owner: &mut BuildOwner,
        parent: ElementId,
        views: &[Box<dyn View>],
    ) {
        let mut ids = Vec::with_capacity(views.len());
        for (slot, view) in views.iter().enumerate() {
            ids.push(tree.insert(view.as_ref(), parent, slot, &mut owner.element_owner_mut()));
        }
        tree.get_mut(parent)
            .expect("seeded parent resolves")
            .set_child_ids(ids);
    }

    /// The S_3 permutation corpus (FR-024(b)) on the slab:
    /// for each of the 6 permutations of keyed `[1, 2, 3]`, the
    /// disposition multiset matches the keyed-reconcile contract AND
    /// every key's element moves to its permuted slot (never rebuilt).
    /// Ports the retired box-reconciler's exhaustive permutation
    /// corpus onto the production reconciler — multiset equality
    /// because Phase-4 HashMap iteration order is not stable.
    #[test]
    #[serial]
    fn all_six_permutations_preserve_identity_and_emit_expected() {
        use ReconcileEventKind::{Reorder, Reuse};

        // Element type inferred from the first tuple's suffixes
        // (`u32` keys, `u64` slots) — an explicit annotation would
        // trip clippy::type_complexity for no readability gain.
        let cases = [
            ([1u32, 2, 3], [(Reuse, 0u64), (Reuse, 1), (Reuse, 2)]),
            ([1, 3, 2], [(Reuse, 0), (Reorder, 1), (Reorder, 2)]),
            ([2, 1, 3], [(Reorder, 0), (Reorder, 1), (Reuse, 2)]),
            ([2, 3, 1], [(Reorder, 0), (Reorder, 1), (Reorder, 2)]),
            ([3, 1, 2], [(Reorder, 0), (Reorder, 1), (Reorder, 2)]),
            ([3, 2, 1], [(Reorder, 0), (Reuse, 1), (Reorder, 2)]),
        ];

        for (perm, expected) in cases {
            let (mut tree, mut owner, root) = fixture();
            seed(&mut tree, &mut owner, root, &keyed_views(&[1, 2, 3]));
            let before = tree.get(root).unwrap().child_ids().to_vec();
            // Seed order is key-ascending, so key `k` lives at index `k - 1`.
            let id_of = |k: u32| before[(k - 1) as usize];

            let new_views = keyed_views(&perm);
            let events = capture(|| {
                reconcile_children_by_id(
                    &mut tree,
                    root,
                    &new_views,
                    &mut owner.element_owner_mut(),
                );
            });

            assert_dispositions(&events, &expected);

            let after = tree.get(root).unwrap().child_ids().to_vec();
            for (slot, &key) in perm.iter().enumerate() {
                assert_eq!(
                    after[slot],
                    id_of(key),
                    "perm {perm:?}: slot {slot} must hold the original key={key} element, not a rebuild",
                );
            }
        }
    }
}
