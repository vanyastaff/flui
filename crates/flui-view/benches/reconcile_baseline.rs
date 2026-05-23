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
    ViewExt,
    IntoView,
    BuildContext, BuildOwner, ElementBase, StatelessBehavior, StatelessElement, StatelessView,
    View, reconcile_children,
};

#[derive(Clone)]
struct SmokeView {
    #[allow(dead_code)]
    id: u32,
}

impl StatelessView for SmokeView {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.clone().boxed()
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
            BuildOwner::new,
            |mut owner| {
                let mut old_children: Vec<Box<dyn ElementBase>> = Vec::new();
                let new_views: &[&dyn View] = &[];
                reconcile_children(
                    flui_foundation::ElementId::new(1),
                    &mut old_children,
                    new_views,
                    &mut owner.element_owner_mut(),
                );
                std::hint::black_box(old_children);
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
                let owner = BuildOwner::new();
                let old_views: Vec<SmokeView> = (1..=10).map(|i| SmokeView { id: i }).collect();
                let old_children: Vec<Box<dyn ElementBase>> =
                    old_views.iter().map(|v| v.create_element()).collect();
                let new_views: Vec<SmokeView> =
                    (1..=10).map(|i| SmokeView { id: i + 100 }).collect();
                (owner, old_children, new_views)
            },
            |(mut owner, mut old_children, new_views)| {
                let view_refs: Vec<&dyn View> = new_views.iter().map(|v| v as &dyn View).collect();
                reconcile_children(
                    flui_foundation::ElementId::new(1),
                    &mut old_children,
                    &view_refs,
                    &mut owner.element_owner_mut(),
                );
                std::hint::black_box(old_children);
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_empty_to_empty, bench_10_same_type);
criterion_main!(benches);
