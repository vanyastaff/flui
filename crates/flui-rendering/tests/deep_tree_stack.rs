//! Deep-tree stack-safety: the recursive pipeline walks must survive
//! trees far deeper than the fixed OS stack would allow with plain
//! recursion.
//!
//! Before the `ensure_stack` probes, a ~1000-level single-child chain
//! crashed the process with STATUS_STACK_OVERFLOW on Windows (1 MiB
//! main-thread stack) — caught by the `layout/deep/1000` bench and
//! reproducible on a production tree of the same depth. These tests
//! pin every recursive walk at 2500 levels: layout + paint (via
//! `run_frame`), hit-testing, and the memoized intrinsic/dry-layout
//! query walks.
//!
//! Ignored under miri: the probes fall back to plain recursion there
//! (psm's stack-switching assembly cannot be interpreted), and the
//! interpreter could not finish a 2500-level walk in reasonable time
//! anyway. The 50-level miri coverage lives in `pipeline_scenarios`.

use flui_rendering::{
    constraints::BoxConstraints,
    hit_testing::HitTestResult,
    objects::{RenderColoredBox, RenderConstrainedBox, RenderPadding},
    pipeline::PipelineOwner,
    storage::IntrinsicDimension,
};
use flui_types::{Offset, Size, geometry::px};

type BoxedRenderObject =
    Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;

const DEPTH: usize = 2_500;

/// Layout + paint + hit-test through a 2500-level padding chain.
#[test]
#[cfg_attr(miri, ignore = "plain-recursion fallback; depth covered natively")]
fn deep_chain_survives_layout_paint_and_hit_walks() {
    let mut owner = PipelineOwner::new();
    let root = owner.insert(Box::new(RenderPadding::all(1.0)) as BoxedRenderObject);
    let mut parent = root;
    for _ in 1..DEPTH {
        parent = owner
            .insert_child_render_object(parent, Box::new(RenderPadding::all(1.0)))
            .expect("chain link insert");
    }
    let leaf = owner
        .insert_child_render_object(parent, Box::new(RenderColoredBox::red(10.0, 10.0)))
        .expect("leaf insert");

    owner.set_root_id(Some(root));
    // Loose constraints big enough that 2500 nested 1px paddings leave
    // room for the 10×10 leaf.
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(6000.0),
        px(0.0),
        px(6000.0),
    )));

    let (owner, result) = owner.run_frame();
    let tree = result
        .expect("deep frame must not error")
        .expect("deep frame must paint");
    assert!(!tree.is_empty(), "the painted chain produces layers");

    // Hit straight through all 2500 paddings into the leaf
    // (leaf-first path, paddings are hit-transparent).
    let mut hits = HitTestResult::new();
    let offset = px(DEPTH as f32) + px(5.0);
    owner.hit_test(Offset::new(offset, offset), &mut hits);
    assert_eq!(
        hits.path().first().map(|e| e.target),
        Some(leaf),
        "the leaf at the bottom of the chain must be hittable",
    );
}

/// Disposal of a 2500-level chain through the owner's removal path
/// (previously a plain recursive `remove_recursive`, now iterative).
#[test]
#[cfg_attr(miri, ignore = "plain-recursion fallback; depth covered natively")]
fn deep_chain_survives_subtree_disposal() {
    let mut owner = PipelineOwner::new();
    let root = owner.insert(Box::new(RenderPadding::all(1.0)) as BoxedRenderObject);
    let mut parent = root;
    for _ in 1..DEPTH {
        parent = owner
            .insert_child_render_object(parent, Box::new(RenderPadding::all(1.0)))
            .expect("chain link insert");
    }

    let removed = owner.remove_render_object(root);
    assert_eq!(removed, DEPTH, "every chain node must be disposed");
    assert!(
        owner.root_id().is_none(),
        "removing the root must clear root_id",
    );
}

/// Intrinsic + dry-layout query walks through a 2500-level
/// ConstrainedBox chain (the child-forwarding query recursion).
#[test]
#[cfg_attr(miri, ignore = "plain-recursion fallback; depth covered natively")]
fn deep_chain_survives_intrinsic_and_dry_layout_queries() {
    let mut owner = PipelineOwner::new();
    let loose = BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(100.0));
    let root = owner.insert(Box::new(RenderConstrainedBox::new(loose)) as BoxedRenderObject);
    let mut parent = root;
    for _ in 1..DEPTH {
        parent = owner
            .insert_child_render_object(parent, Box::new(RenderConstrainedBox::new(loose)))
            .expect("chain link insert");
    }
    owner
        .insert_child_render_object(parent, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("leaf insert");

    let width = owner
        .box_intrinsic_dimension(root, IntrinsicDimension::MaxWidth, 100.0)
        .expect("deep intrinsic query must not error");
    assert!(
        width.is_finite(),
        "intrinsic answer must be a real number, got {width}",
    );

    // The leaf's DEFAULT dry layout is `constraints.smallest()` (only
    // the child-forwarding containers implement dry queries so far),
    // so the answer is 0×0 — what matters here is that the
    // child-forwarding recursion reached it through 2500 levels
    // without exhausting the stack.
    let size = owner
        .box_dry_layout(
            root,
            BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(100.0)),
        )
        .expect("deep dry-layout query must not error");
    assert_eq!(
        size,
        Size::new(px(0.0), px(0.0)),
        "the chain forwards the leaf's default dry answer unchanged",
    );
}
