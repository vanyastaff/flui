//! Layout cycle guard tests.
//!
//! Verifies [`PipelineOwner::layout_dirty_root`] surfaces
//! [`RenderError::LayoutCycle`] on cyclic re-entry (via the
//! `SubtreeArena::by_id` per-slot `AtomicBool` in-flight flag +
//! RAII `LayoutCycleGuard`), and that the guard's `Drop` runs on
//! panic so the in-flight flag stays consistent across frames.
//!
//! Refs:
//!   * docs/plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md
//!   * docs/research/2026-05-23-d-block-architecture-decision-memo.md

use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{constraints::BoxConstraints, error::RenderError, pipeline::PipelineOwner};
use flui_types::{Size, geometry::px};

fn fresh_layout_pipeline() -> PipelineOwner<flui_rendering::pipeline::Layout> {
    PipelineOwner::new().into_layout()
}

// ============================================================================
// Structural cycle on leaf-only path — guard does NOT trigger
// ============================================================================

/// This test's name was chosen to avoid a prior misleading name that
/// claimed the test surfaces LayoutCycle — it does NOT. The contract
/// it verifies is: a structural cycle on a leaf-traversal path
/// (RenderColoredBox child whose `children()` lists its parent) does
/// NOT trigger the guard because the leaf widget's `perform_layout`
/// never calls `ctx.layout_child` for the cyclic edge.
///
/// Cycle protection layers exercised:
/// - `collect_subtree_ids`'s visited `HashSet` dedups the cycle
///   edge → returned `Vec<RenderId>` is unique → `get_subtree_mut`
///   precondition satisfied.
/// - Padding's `perform_layout` calls `layout_child(0)` for ColoredBox.
/// - ColoredBox is a leaf — never enters the layout-child callback
///   chain for the cyclic edge → guard never fires.
///
/// Result: layout succeeds. The cycle exists structurally but is
/// invisible to the layout walk. (The `LayoutCycle`-surfacing
/// contract is tested separately by
/// `callback_reentry_marks_parent_dirty_for_retry`.)
#[test]
fn structural_cycle_on_leaf_path_does_not_trigger_guard() {
    let mut pipeline = fresh_layout_pipeline();

    let padding_id = pipeline
        .render_tree_mut()
        .insert_box(Box::new(RenderPadding::all(5.0)));
    let child_id = pipeline
        .render_tree_mut()
        .insert_box_child(padding_id, Box::new(RenderColoredBox::red(20.0, 20.0)))
        .expect("child insert");
    // Inject cycle: ColoredBox's children list now contains Padding.
    pipeline
        .render_tree_mut()
        .get_mut(child_id)
        .expect("child in tree")
        .add_child(padding_id);

    let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
    let result = pipeline.layout_dirty_root(padding_id, constraints);
    assert!(
        result.is_ok(),
        "structural-only cycle on a leaf-traversal path must not \
         trigger LayoutCycle — leaf widgets never call layout_child \
         for the cyclic edge; got {result:?}",
    );
}

// ============================================================================
// Callback re-entry — guard fires, structural cycle poisons the node
// ============================================================================

/// The LayoutCycle Err is collapsed at the inner callback, never
/// reaching the outer caller, so this test asserts `result.is_ok()` and
/// verifies the bounded-retry state afterwards.
///
/// The contract: when a user widget's `perform_layout` calls
/// `ctx.layout_child` for an ancestor id that's already in flight up
/// the recursion stack, the `LayoutCycleGuard::enter` collision
/// returns `Err(RenderError::LayoutCycle(id))`. The layout-child
/// callback in `layout_subtree_borrowed` collapses that Err to
/// `Size::ZERO` + sets `descendant_error_flag` for the current call
/// frame. A layout cycle is a structural failure, so the layout poison
/// engages on the first occurrence: the re-entered node is poisoned and
/// the flags the cycle left set are cleared — the retry is bounded to
/// one attempt per fresh external invalidation rather than running every
/// frame.
///
/// Trigger: Padding(P1) → Padding(P2) with P2.children additionally
/// containing P1 (cyclic edge). Both widgets call `layout_child(0)`
/// for their declared first child, so the cycle is reachable.
#[test]
fn callback_reentry_poisons_structural_cycle() {
    let mut pipeline = fresh_layout_pipeline();
    let p1 = pipeline
        .render_tree_mut()
        .insert_box(Box::new(RenderPadding::all(5.0)));
    let p2 = pipeline
        .render_tree_mut()
        .insert_box_child(p1, Box::new(RenderPadding::all(2.0)))
        .expect("p2 insert");
    pipeline
        .render_tree_mut()
        .get_mut(p2)
        .expect("p2 in tree")
        .add_child(p1);

    let constraints = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
    // P1.perform_layout → layout_child(0) → recurses into P2.
    // P2.perform_layout → layout_child(0) → recurses into P1 (cyclic
    // edge). P1's in-flight flag is already set → guard returns
    // Err(LayoutCycle(P1)) → callback collapses to Size::ZERO; P2's
    // descendant_error_flag set → P2 keeps NEEDS_LAYOUT for the walk.
    // After the walk, the poison bookkeeping tips P1 (structural
    // failure) and clears the flags the cycle left set. P1's callback
    // only sees Ok(Size) from P2, so P1 is marked clean by the walk
    // itself as well.
    let result = pipeline.layout_dirty_root(p1, constraints);
    assert!(
        result.is_ok(),
        "cyclic re-entry must not panic — LayoutCycle Err is collapsed \
         at the inner callback boundary, outer Ok is returned; got \
         {result:?}",
    );

    // Bounded-retry contract: a structural cycle poisons instead of
    // holding the dirty bit for an unbounded next-frame retry, so P2's
    // NEEDS_LAYOUT is cleared by the poison bookkeeping.
    let p2_node = pipeline.render_tree().get(p2).expect("p2 in tree");
    assert!(
        !p2_node.needs_layout(),
        "P2's NEEDS_LAYOUT must be cleared after the LayoutCycle engaged \
         the layout poison — the cyclic child is skipped in later walks \
         until freshly invalidated, rather than retried every frame",
    );
}

// ============================================================================
// Drop-guard panic safety — guard removes id on perform_layout panic
// ============================================================================

/// RAII safety: a non-leaf user widget whose
/// `perform_layout` panics must leave the in-flight flag clean
/// for the next frame (the panic is caught by `catch_unwind` in the
/// non-leaf path AND the `LayoutCycleGuard`'s Drop runs on unwind).
///
/// Frame 1: panicking widget → catch_unwind catches the panic →
/// `layout_dirty_root` returns `Err(RenderError::Poisoned)`. Guard's
/// Drop runs as the stack unwinds out of `layout_subtree_borrowed`.
/// Frame 2: same widget retried (after, e.g., a fixed render-object
/// swap). Set is empty, no spurious LayoutCycle, layout succeeds.
///
/// The frame-2 retry shape verifies the guard's panic-safety property
/// without needing a separate mock for the set state.
#[test]
fn drop_guard_clears_id_on_perform_layout_panic() {
    use flui_foundation::Diagnosticable;
    use flui_rendering::{
        context::{BoxHitTestContext, BoxLayoutContext},
        hit_testing::HitTestBehavior,
        parent_data::BoxParentData,
        traits::RenderBox,
    };
    use flui_tree::Single;
    /// Single-arity user widget that panics on the FIRST perform_layout
    /// call and succeeds on subsequent calls (state-tracked panic).
    #[derive(Debug, Default)]
    struct PanicOnceWidget {
        already_panicked: bool,
    }

    impl Diagnosticable for PanicOnceWidget {}

    impl RenderBox for PanicOnceWidget {
        type Arity = Single;
        type ParentData = BoxParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
        ) -> Size {
            if !self.already_panicked {
                self.already_panicked = true;
                panic!("PanicOnceWidget intentional first-call panic");
            }
            let constraints = *ctx.constraints();
            ctx.layout_child(0, constraints)
        }

        fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
            false
        }
        fn hit_test_behavior(&self) -> HitTestBehavior {
            HitTestBehavior::Opaque
        }
    }

    let mut pipeline = fresh_layout_pipeline();
    let parent_id = pipeline
        .render_tree_mut()
        .insert_box(Box::new(PanicOnceWidget::default()));
    let _child_id = pipeline
        .render_tree_mut()
        .insert_box_child(parent_id, Box::new(RenderColoredBox::red(20.0, 20.0)))
        .expect("child insert");

    let constraints = BoxConstraints::tight(Size::new(px(50.0), px(50.0)));

    // Frame 1: panic surfaces as Poisoned (the non-leaf path wraps
    // perform_layout_raw in catch_unwind).
    let frame_1 = pipeline.layout_dirty_root(parent_id, constraints);
    assert!(
        matches!(frame_1, Err(RenderError::Poisoned { .. })),
        "frame 1 PanicOnceWidget panic must surface as Poisoned; got {frame_1:?}",
    );

    // The aborted pass committed nothing and left NEEDS_LAYOUT set, so the
    // node is eligible for the frame-2 retry below. (Completion is now the
    // return value of `perform_layout`, so a panicked pass cannot have
    // half-committed a size.)
    assert!(
        pipeline
            .render_tree()
            .get(parent_id)
            .expect("panicked node stays in the tree")
            .needs_layout(),
        "a Poisoned layout must leave NEEDS_LAYOUT set for next-frame retry",
    );

    // Frame 2: retry must succeed. Guard's Drop on the unwind path
    // cleared parent_id's in-flight flag — no flag set, no
    // spurious LayoutCycle. Widget's `already_panicked = true` so the
    // perform_layout body completes normally.
    let frame_2 = pipeline.layout_dirty_root(parent_id, constraints);
    assert!(
        frame_2.is_ok(),
        "frame 2 retry must succeed (drop-guard cleared parent_id's \
         in-flight flag on the frame-1 unwind); got {frame_2:?}",
    );
}

// ============================================================================
// Sequential calls — guard insert+remove between calls (no spurious cycle)
// ============================================================================

/// Sequential `layout_dirty_root` calls on the same root must not
/// trigger LayoutCycle — each call's guard inserts and removes
/// cleanly; the next call sees an empty set.
#[test]
fn sequential_calls_on_same_root_do_not_trigger_cycle() {
    let mut pipeline = fresh_layout_pipeline();
    let padding_id = pipeline
        .render_tree_mut()
        .insert_box(Box::new(RenderPadding::all(5.0)));
    let _child_id = pipeline
        .render_tree_mut()
        .insert_box_child(padding_id, Box::new(RenderColoredBox::red(20.0, 20.0)))
        .expect("child insert");

    let constraints = BoxConstraints::tight(Size::new(px(50.0), px(50.0)));

    // 3 sequential calls — each must succeed.
    for frame in 1..=3 {
        let result = pipeline.layout_dirty_root(padding_id, constraints);
        assert!(
            result.is_ok(),
            "frame {frame}: sequential layout_dirty_root call must succeed \
             — guard insert+remove must not leak state across calls; got \
             {result:?}",
        );
    }
}
