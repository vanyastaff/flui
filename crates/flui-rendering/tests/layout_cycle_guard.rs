//! Layout cycle guard tests.
//!
//! Verifies [`PipelineOwner::layout_dirty_root`] surfaces
//! [`RenderError::LayoutCycle`] on cyclic re-entry (via the
//! `SubtreeArena::by_id` per-slot `AtomicBool` in-flight flag +
//! RAII `LayoutCycleGuard`), and that the guard's `Drop` runs on
//! panic so the in-flight flag stays consistent across frames.

use flui_foundation::geometry::Size;
use flui_objects::{RenderColoredBox, RenderPadding};
use flui_rendering::{constraints::BoxConstraints, error::RenderError};

use crate::common::fresh_layout_pipeline;

// ============================================================================
// Structural cycle on leaf-only path — guard does NOT trigger
// ============================================================================

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
pub(crate) fn callback_reentry_poisons_structural_cycle() {
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

    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
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
pub(crate) fn drop_guard_clears_id_on_perform_layout_panic() {
    use flui_foundation::Diagnosticable;
    use flui_foundation::Single;
    use flui_rendering::{
        context::{BoxHitTestContext, BoxLayoutContext},
        hit_testing::HitTestBehavior,
        parent_data::BoxParentData,
        traits::RenderBox,
    };
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

    let constraints = BoxConstraints::tight(Size::new(50.0, 50.0));

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
