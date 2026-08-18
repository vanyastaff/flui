//! What a semantics assembly pass costs, as a function of render-tree size.
//!
//! The companion to flui-semantics' `publish_cost` bench, one layer up: that
//! one measures the owner's flush (translate + diff + publish); this one
//! measures `run_semantics` end to end — mark resolution, fragment assembly,
//! arena update, and the flush it drives. The number to beat is the classic
//! full rebuild, which re-assembled the entire render tree for any mark; the
//! mark-scoped pass should hold the one-mark cost near the marked subtree's
//! size while the full-rebuild scenario tracks the tree.
//!
//! # Scenarios
//!
//! - **one_mark** — a single leaf marked, the shape of a checkbox toggling.
//!   Under mark-scoped assembly this re-assembles one boundary's subtree
//!   (constant size here), so it should stay flat as the tree grows.
//! - **root_mark** — the root marked, which no graft can scope (a marked
//!   root's own forming is in question): the full-rebuild ceiling, and the
//!   cost every pass used to pay.
//!
//! Sizes span 64..1024 nodes, structured as 8-leaf boundary branches — a
//! semantics tree holds boundaries, not widgets, so 1024 is a large app.

// Bench binary: criterion's generated main has no docs (house precedent:
// the flui-view and flui-testing benches carry the same allow).
#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_rendering::pipeline::phase::Idle;
use flui_rendering::prelude::{
    BoxLayoutContext, BoxParentData, Leaf, PipelineOwner, RenderBox, SemanticsConfiguration, Size,
};
use flui_types::geometry::px;

/// A minimal semantics-carrying leaf, built on the public `RenderBox`
/// surface (the in-crate test fixtures are `cfg(test)` and invisible here).
#[derive(Debug)]
struct Labeled {
    label: &'static str,
    boundary: bool,
}

impl flui_foundation::Diagnosticable for Labeled {}

impl RenderBox for Labeled {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        ctx.constraints().constrain(Size::new(px(10.0), px(10.0)))
    }

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        config.set_semantics_boundary(self.boundary);
        config.set_label(self.label);
    }
}

/// A pipeline whose render tree holds `branches` boundary branches of 8
/// labeled leaves each, semantics enabled and settled (first pass done).
/// Returns the owner at Idle plus one leaf id to mark.
fn settled_owner(branches: usize) -> (PipelineOwner<Idle>, flui_foundation::RenderId) {
    let mut owner = PipelineOwner::new();
    let root = owner.set_root_render_object(Box::new(Labeled {
        label: "root",
        boundary: false,
    }));
    let mut a_leaf = None;
    for _ in 0..branches {
        let branch = owner
            .insert_child_render_object(
                root,
                Box::new(Labeled {
                    label: "branch",
                    boundary: true,
                }),
            )
            .expect("branch inserted");
        for _ in 0..8 {
            let leaf = owner
                .insert_child_render_object(
                    branch,
                    Box::new(Labeled {
                        label: "leaf",
                        boundary: false,
                    }),
                )
                .expect("leaf inserted");
            a_leaf = Some(leaf);
        }
    }
    owner.set_semantics_enabled(true);

    let owner = owner.into_layout().into_compositing().into_paint();
    let mut owner = owner.into_semantics();
    owner.run_semantics().expect("initial semantics pass");
    (owner.finish(), a_leaf.expect("at least one branch"))
}

/// Drives one full semantics pass and hands the owner back at Idle.
fn pass(owner: PipelineOwner<Idle>) -> PipelineOwner<Idle> {
    let owner = owner.into_layout().into_compositing().into_paint();
    let mut owner = owner.into_semantics();
    owner.run_semantics().expect("semantics pass");
    owner.finish()
}

fn assembly_cost(c: &mut Criterion) {
    let mut group = c.benchmark_group("semantics_assembly");

    // branches × (1 branch node + 8 leaves) + root ≈ node count.
    for branches in [7usize, 28, 113] {
        let nodes = branches * 9 + 1;

        group.bench_with_input(
            BenchmarkId::new("one_mark", nodes),
            &branches,
            |b, &branches| {
                let (owner, leaf) = settled_owner(branches);
                let mut slot = Some(owner);
                b.iter(|| {
                    let mut owner = slot.take().expect("owner is always returned");
                    owner.mark_needs_semantics(leaf);
                    slot = Some(pass(owner));
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("root_mark", nodes),
            &branches,
            |b, &branches| {
                let (owner, _) = settled_owner(branches);
                let root = owner.root_id().expect("tree is rooted");
                let mut slot = Some(owner);
                b.iter(|| {
                    let mut owner = slot.take().expect("owner is always returned");
                    owner.mark_needs_semantics(root);
                    slot = Some(pass(owner));
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, assembly_cost);
criterion_main!(benches);
