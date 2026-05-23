//! SC-006 — keyed-reorder reconciliation is O(N) linear, not O(shift-distance).
//!
//! Three permutation patterns over N=10K keyed leaves:
//!   - full-reverse: every child moves to the opposite slot.
//!   - single-rotate: rotate the first child to the end (N-1 shifts).
//!   - swap-first-last: only the first and last swap (single move).
//!
//! All three must complete within a constant factor of N (the
//! plan's "~2x of each other" target). Growth from N=1K → N=10K
//! must be linear (~10x), not super-linear (~100x).
//!
//! Criterion will produce HTML reports in `target/criterion/` and
//! comparison printouts in stderr. The bench itself does not
//! assert (criterion's `--save-baseline` workflow is the regression
//! oracle); instead it sets up the workload for inspection.

use std::any::TypeId;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_foundation::{ElementId, ValueKey, ViewKey};
use flui_view::{BuildOwner, ElementBase, View, element::Lifecycle, reconcile_children};

// Reuse the keyed-leaf fixture shape from the reconciliation tests:
// a leaf element with stable identity + a ValueKey<u32>. Defined
// inline here because cargo benches cannot import test-only items.

#[derive(Clone)]
struct KeyedLeafView {
    key: ValueKey<u32>,
}

impl KeyedLeafView {
    fn new(tag: u32) -> Self {
        Self {
            key: ValueKey::new(tag),
        }
    }
}

impl View for KeyedLeafView {
    fn create_element(&self) -> Box<dyn ElementBase> {
        Box::new(KeyedLeafElement {
            view_type: TypeId::of::<KeyedLeafView>(),
            depth: 0,
            lifecycle: Lifecycle::Initial,
            key: Box::new(self.key.clone()),
        })
    }
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

struct KeyedLeafElement {
    view_type: TypeId,
    depth: usize,
    lifecycle: Lifecycle,
    key: Box<dyn ViewKey>,
}

impl ElementBase for KeyedLeafElement {
    fn view_type_id(&self) -> TypeId {
        self.view_type
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
        if let Some(k) = new_view.key() {
            self.key = k.clone_key();
        }
    }
    fn mark_needs_build(&mut self) {}
    fn perform_build(&mut self, _owner: &mut flui_view::ElementOwner<'_>) {}
    fn visit_children(&self, _visitor: &mut dyn FnMut(ElementId)) {}
}

fn parent_id() -> ElementId {
    ElementId::new(1)
}

/// Build initial mounted children for a tree of size `n`.
fn mount_n_children(owner: &mut BuildOwner, n: u32) -> Vec<Box<dyn ElementBase>> {
    let mut children: Vec<Box<dyn ElementBase>> = Vec::with_capacity(n as usize);
    for i in 0..n {
        let v = KeyedLeafView::new(i);
        let mut el = v.create_element();
        el.mount(None, i as usize, &mut owner.element_owner_mut());
        children.push(el);
    }
    children
}

fn full_reverse(views: &[KeyedLeafView]) -> Vec<&dyn View> {
    views.iter().rev().map(|v| v as &dyn View).collect()
}

fn single_rotate(views: &[KeyedLeafView]) -> Vec<&dyn View> {
    let mut out: Vec<&dyn View> = views[1..].iter().map(|v| v as &dyn View).collect();
    out.push(&views[0]);
    out
}

fn swap_first_last(views: &[KeyedLeafView]) -> Vec<&dyn View> {
    let n = views.len();
    let mut out: Vec<&dyn View> = views.iter().map(|v| v as &dyn View).collect();
    if n >= 2 {
        out.swap(0, n - 1);
    }
    out
}

fn bench_pattern(
    c: &mut Criterion,
    name: &str,
    build_views: fn(&[KeyedLeafView]) -> Vec<&dyn View>,
) {
    let mut group = c.benchmark_group(format!("sc006_{name}"));
    // Two sizes to confirm linear growth visually in the report.
    for &n in &[1_000_u32, 10_000_u32] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let owner = BuildOwner::new();
                    let old_views: Vec<KeyedLeafView> = (0..n).map(KeyedLeafView::new).collect();
                    let children = {
                        let mut owner = BuildOwner::new();
                        mount_n_children(&mut owner, n)
                    };
                    let _ = owner; // construction-only; bench owns its own
                    (BuildOwner::new(), children, old_views)
                },
                |(mut owner, mut children, old_views)| {
                    let view_refs = build_views(&old_views);
                    reconcile_children(
                        parent_id(),
                        &mut children,
                        &view_refs,
                        &mut owner.element_owner_mut(),
                    );
                    std::hint::black_box(children);
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

fn bench_sc006(c: &mut Criterion) {
    bench_pattern(c, "full_reverse", full_reverse);
    bench_pattern(c, "single_rotate", single_rotate);
    bench_pattern(c, "swap_first_last", swap_first_last);
}

criterion_group!(benches, bench_sc006);
criterion_main!(benches);
