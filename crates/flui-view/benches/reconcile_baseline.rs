//! Smoke bench for the `reconcile_children` path. Proves the criterion harness
//! loads against `flui-view`; no real perf measurement.
//!
//! This bench exists per **U1** of the View / Element / Core Contracts plan
//! ([`docs/plans/2026-05-22-005-feat-view-element-core-contracts-plan.md`]).
//! U2 (S1 KeyId interning prototype) and U3 (S2 static-path algorithm sketch)
//! add the substantive benches that produce the Phase 0 gate-report verdicts.
//!
//! The smoke bench produces a measurable baseline so U2/U3 deltas are
//! interpretable. It does NOT measure production behavior — `reconcile_children`
//! is still the scaffold with the keyed middle stub (per `reconciliation.rs:91-98`)
//! at the time this bench lands.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use flui_view::{
    BuildContext, BuildOwner, ElementBase, ElementTree, StatelessBehavior, StatelessElement,
    StatelessView, View, reconcile_children,
};

#[derive(Clone)]
struct SmokeView {
    #[allow(dead_code)]
    id: u32,
}

impl StatelessView for SmokeView {
    fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
        Box::new(self.clone())
    }
}

impl View for SmokeView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(StatelessElement::new(self, StatelessBehavior))
    }
}

/// Empty-to-empty reconcile — fast-path, no allocation, harness sanity check.
fn bench_empty_to_empty(c: &mut Criterion) {
    c.bench_function("reconcile_baseline/empty_to_empty", |b| {
        b.iter_batched(
            || {
                let mut tree = ElementTree::new();
                let mut owner = BuildOwner::new();
                let root = SmokeView { id: 0 };
                let parent = tree.mount_root(&root, &mut owner.element_owner_mut());
                (tree, owner, parent)
            },
            |(mut tree, mut owner, parent)| {
                let result =
                    reconcile_children(&mut tree, parent, &[], &[], &mut owner.element_owner_mut());
                std::hint::black_box(result);
            },
            BatchSize::SmallInput,
        );
    });
}

/// 10-child same-type same-length reconcile — prefix-scan fast path.
fn bench_10_same_type(c: &mut Criterion) {
    c.bench_function("reconcile_baseline/10_same_type", |b| {
        b.iter_batched(
            || {
                let mut tree = ElementTree::new();
                let mut owner = BuildOwner::new();
                let root = SmokeView { id: 0 };
                let parent = tree.mount_root(&root, &mut owner.element_owner_mut());
                let mut child_ids = Vec::with_capacity(10);
                for i in 1..=10 {
                    let v = SmokeView { id: i };
                    let id =
                        tree.insert(&v, parent, (i - 1) as usize, &mut owner.element_owner_mut());
                    child_ids.push(id);
                }
                let new_views: Vec<SmokeView> =
                    (1..=10).map(|i| SmokeView { id: i + 100 }).collect();
                (tree, owner, parent, child_ids, new_views)
            },
            |(mut tree, mut owner, parent, child_ids, new_views)| {
                let view_refs: Vec<&dyn View> = new_views.iter().map(|v| v as &dyn View).collect();
                let result = reconcile_children(
                    &mut tree,
                    parent,
                    &child_ids,
                    &view_refs,
                    &mut owner.element_owner_mut(),
                );
                std::hint::black_box(result);
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_empty_to_empty, bench_10_same_type);
criterion_main!(benches);
