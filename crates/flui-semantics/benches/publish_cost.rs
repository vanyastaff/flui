//! What a semantics-enabled frame costs, as a function of tree size.
//!
//! Filed as the prerequisite for the pipeline's scalability work: three costs
//! are documented as "correct but O(tree)" — full assembly per pass, a
//! whole-tree `TreeUpdate` per change, and an idle dirty check that scans. Any
//! optimisation of those needs a number to beat, or the improvement claim is an
//! assertion.
//!
//! This benchmark covers the two costs that live in this crate. Assembly is
//! upstream (`PipelineOwner::run_semantics`) and is not measured here.
//!
//! # The three scenarios
//!
//! - **idle** — a clean tree, the common case while a screen reader is
//!   attached but nothing is changing. The dirty check is counter-backed
//!   O(1); before that counter it was an `any()` scan whose worst case was
//!   exactly this scenario (a clean tree was scanned to the end), so idle
//!   was the most expensive check of the three.
//! - **one_dirty** — a single genuinely changed node, the shape of a
//!   checkbox toggling. The incremental flush translates the one dirty node
//!   and publishes the one changed node; before the diff this scenario
//!   translated and published the whole tree, tracking `all_dirty`.
//! - **all_dirty** — every node changed, the shape of a route transition,
//!   and the full-republish path (`send_full_tree`) by construction: the
//!   ceiling `one_dirty` must stay far below.
//!
//! Sizes span 16..1024 nodes. A real application's semantics tree holds
//! *boundaries*, not widgets, so 1024 is a large app rather than an absurd one.

// Bench binary: criterion's generated main has no docs (house precedent:
// the flui-view and flui-testing benches carry the same allow).

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_foundation::RenderId;
use flui_semantics::{SemanticsNode, SemanticsOwner};

/// A render-backed node — production assembly always attaches one, and it is
/// what makes a node publishable at all.
fn node(index: u32) -> SemanticsNode {
    let mut node = SemanticsNode::new().with_source_render_id(RenderId::new_gen(
        index,
        core::num::NonZeroU32::new(1).expect("bench generation is non-zero"),
    ));
    node.config_mut().set_label(format!("node {index}"));
    node.config_mut().set_button(index.is_multiple_of(3));
    node
}

/// A rooted tree of `count` nodes: one root with `count - 1` children.
///
/// Flat rather than deep on purpose — the costs under measurement are per-node
/// (a scan and a translation), so depth would only add noise.
fn owner_with(count: u32) -> SemanticsOwner {
    let mut owner = SemanticsOwner::new(Arc::new(|_| {}));
    let root = owner.insert(node(0));
    for index in 1..count {
        let child = owner.insert(node(index));
        owner.add_child(root, child);
    }
    owner.set_root(Some(root));
    // Settle: everything starts dirty, so publish once and let the scenarios
    // decide what is dirty from here.
    owner.flush();
    owner
}

fn publish_cost(c: &mut Criterion) {
    let mut group = c.benchmark_group("semantics_publish");

    for count in [16u32, 64, 256, 1024] {
        // A clean tree publishes nothing. What it pays is the dirty check
        // alone — counter-backed O(1) now; it used to be a scan that walked
        // furthest precisely when there was nothing to do.
        group.bench_with_input(BenchmarkId::new("idle", count), &count, |b, &count| {
            let mut owner = owner_with(count);
            b.iter(|| {
                owner.flush();
                black_box(owner.needs_flush())
            });
        });

        // One genuinely changed node — the shape of a checkbox toggling.
        // The label alternates so every iteration publishes a real
        // single-node diff; a no-op re-mark would measure only the
        // empty-diff path. This is the scenario the incremental flush
        // exists for: translate the one dirty node, publish the one
        // changed node, never the tree.
        group.bench_with_input(BenchmarkId::new("one_dirty", count), &count, |b, &count| {
            let mut owner = owner_with(count);
            let target = owner.root().expect("the tree is rooted");
            let mut flip = false;
            b.iter(|| {
                flip = !flip;
                if let Some(node) = owner.get_mut(target) {
                    node.config_mut()
                        .set_label(if flip { "toggled on" } else { "toggled off" });
                }
                owner.flush();
            });
        });

        // Every node changed — a route transition. The ceiling.
        group.bench_with_input(BenchmarkId::new("all_dirty", count), &count, |b, &count| {
            let mut owner = owner_with(count);
            b.iter(|| {
                owner.send_full_tree();
            });
        });
    }

    group.finish();
}

criterion_group!(benches, publish_cost);
criterion_main!(benches);
