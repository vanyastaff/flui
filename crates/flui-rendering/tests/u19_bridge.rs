//! D-block PR-A1b U19 — `RenderObject<BoxProtocol>::perform_layout_raw`
//! blanket-impl bridge integration tests.
//!
//! These exercise the **real layout path** through the trait-erased
//! `perform_layout_raw` signature: a Direct-storage `BoxLayoutCtx` is
//! constructed by the test caller (mimicking the pipeline), coerced to
//! `&mut dyn BoxLayoutCtxErased`, and handed to the
//! `RenderObject<BoxProtocol>` blanket impl. The blanket impl
//! reconstructs a typed `BoxLayoutCtx<T::Arity, T::ParentData>` via the
//! `Proxy` storage variant and calls `T::perform_layout`. The asserted
//! result is the computed `Size` returned to the caller.
//!
//! Coverage matches plan U19 §404-407: Leaf (`RenderColoredBox`),
//! Single (`RenderPadding`), Variable (`RenderFlex`).
//!
//! Refs:
//!   * docs/plans/2026-05-23-001-feat-pipeline-wiring-d-block-plan.md §U19
//!   * docs/research/2026-05-23-d-block-architecture-decision-memo.md §D5

use std::sync::{Arc, Mutex};

use flui_foundation::RenderId;
use flui_rendering::{
    constraints::BoxConstraints,
    objects::{RenderColoredBox, RenderFlex, RenderPadding},
    parent_data::{BoxParentData, FlexParentData},
    // `BoxLayoutCtxErased` is intentionally NOT re-exported under
    // `protocol::*` (PR #141 review comment 3293746269) — `protocol::*`
    // glob would pull it in alongside `LayoutContextApi` and collide
    // on method names. Pull it from the explicit module path here.
    protocol::{
        BoxLayoutCtx, BoxProtocol, ChildState, Protocol, RenderObject,
        box_protocol::BoxLayoutCtxErased,
    },
};
use flui_tree::{Leaf, Single, Variable};
use flui_types::{Size, geometry::px};

// ============================================================================
// U19 §404 — Leaf bridge: RenderColoredBox via blanket perform_layout_raw
// ============================================================================

/// Plan U19 §405 happy path: `RenderColoredBox` (Leaf arity) layout via
/// the blanket bridge returns the correctly constrained `Size`.
///
/// Pre-U19 the blanket `perform_layout_raw` shipped as a no-op
/// returning `*self.size()` — for a fresh `RenderColoredBox` that meant
/// `Size::ZERO`. After U19 the blanket impl drives the user's
/// `RenderBox::perform_layout`, which constrains `preferred_size`
/// against the parent's constraints and completes layout.
#[test]
fn u19_leaf_bridge_returns_constrained_size() {
    let mut obj = RenderColoredBox::red(100.0, 50.0);
    let constraints = BoxConstraints::tight(Size::new(px(100.0), px(50.0)));

    let mut direct_ctx: BoxLayoutCtx<'_, Leaf, BoxParentData> = BoxLayoutCtx::new(constraints);
    let erased: &mut dyn BoxLayoutCtxErased = &mut direct_ctx;

    let size = <RenderColoredBox as RenderObject<BoxProtocol>>::perform_layout_raw(
        &mut obj,
        // GAT resolves `<BoxProtocol as Protocol>::LayoutCtxErased<'_>` to
        // `dyn BoxLayoutCtxErased + '_`; the coercion above gives us
        // exactly that.
        erased,
    );

    assert_eq!(
        size,
        Size::new(px(100.0), px(50.0)),
        "Leaf bridge must return the user's perform_layout-completed size, \
         not Size::ZERO (pre-U19 placeholder behaviour)",
    );
}

/// Edge case: looser parent constraints — the user code constrains its
/// `preferred_size` to the parent constraints, yielding the requested
/// size when constraints permit it.
#[test]
fn u19_leaf_bridge_honours_loose_constraints() {
    let mut obj = RenderColoredBox::blue(80.0, 40.0);
    let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));

    let mut direct_ctx: BoxLayoutCtx<'_, Leaf, BoxParentData> = BoxLayoutCtx::new(constraints);
    let erased: &mut dyn BoxLayoutCtxErased = &mut direct_ctx;

    let size =
        <RenderColoredBox as RenderObject<BoxProtocol>>::perform_layout_raw(&mut obj, erased);

    assert_eq!(size, Size::new(px(80.0), px(40.0)));
}

// ============================================================================
// U19 §406 — Single bridge: RenderPadding via blanket perform_layout_raw
// ============================================================================

/// Plan U19 §406 happy path: `RenderPadding` (Single arity) layout via
/// the blanket bridge forwards to
/// `T::perform_layout(BoxLayoutContext<Single, BoxParentData>)`, which
/// deflates the parent constraints, calls `ctx.layout_child(0,
/// child_constraints)`, positions the child, and completes layout with
/// `child_size + padding`.
///
/// The Direct ctx carries one synthetic child whose layout callback
/// returns whatever max-size the (deflated) child constraints allow —
/// the Padding's perform_layout then adds the padding back, and the
/// final size returned by the bridge equals `parent_constraints.max`
/// when those are loose enough.
#[test]
fn u19_single_bridge_pads_child_and_returns_total_size() {
    let mut obj = RenderPadding::all(10.0);

    // Parent gives us up to 200×100 of space. Padding deflates by
    // (left+right=20, top+bottom=20) → child gets up to 180×80. The
    // callback returns the child's max constraints as the child's size,
    // so child = 180×80. Final size = 180+20 = 200, 80+20 = 100.
    let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(100.0));

    let mut children: Vec<ChildState<BoxParentData>> = vec![ChildState::new(RenderId::new(1))];
    let child_ids = [RenderId::new(1)];

    // Synthetic child callback: respond at the largest allowed size so
    // the Padding can grow to fill the parent's constraints.
    let layout_child_callback: Arc<
        dyn Fn(flui_foundation::RenderId, BoxConstraints) -> Size + Send + Sync,
    > = Arc::new(|_id, c| Size::new(c.max_width, c.max_height));

    let mut direct_ctx: BoxLayoutCtx<'_, Single, BoxParentData> =
        BoxLayoutCtx::with_layout_callback(
            constraints,
            &mut children,
            &child_ids,
            layout_child_callback.as_ref(),
        );

    let erased: &mut dyn BoxLayoutCtxErased = &mut direct_ctx;

    let size = <RenderPadding as RenderObject<BoxProtocol>>::perform_layout_raw(&mut obj, erased);

    assert_eq!(
        size,
        Size::new(px(200.0), px(100.0)),
        "RenderPadding bridge must complete with child_size + padding"
    );

    // Child's recorded offset should be the padding's top-left
    // (10, 10) — written by `ctx.position_child(0, …)` inside
    // `RenderPadding::perform_layout`. The Proxy-mode `position_child`
    // delegates back through `erased.position_child`, which writes to
    // the underlying Direct ctx's children Vec.
    let child_offset = children[0].offset;
    assert!(
        (child_offset.dx.get() - 10.0).abs() < f32::EPSILON
            && (child_offset.dy.get() - 10.0).abs() < f32::EPSILON,
        "child offset {:?} should equal (10, 10)",
        child_offset
    );
}

// ============================================================================
// U19 §407 — Variable bridge: RenderFlex via blanket perform_layout_raw
// ============================================================================

/// Plan U19 §407 happy path: `RenderFlex` (Variable arity, with typed
/// `FlexParentData`) via the blanket bridge correctly walks the child
/// slice. Validates two things:
///
/// 1. `ctx.child_count()` reports the count from the underlying Direct
///    ctx's children Vec (Proxy delegates via `erased.child_count()`).
/// 2. `ctx.child_parent_data(i)` returns typed `&FlexParentData` —
///    the Proxy variant downcasts through `&dyn ParentData`. This is
///    the test for the parent-data downcast soundness path documented
///    on `BoxLayoutCtxErased::child_parent_data_dyn`.
///
/// # Deterministic flex math
///
/// `FlexParentData::flexible(n)` uses `FlexFit::Tight` (see
/// [`FlexParentData::flexible`]). With two children of flex factors 1
/// and 2, no inflexible children, and parent constraints
/// `(0..300, 0..100)`:
///
/// - `total_flex = 3`, `inflexible_main = 0`, `remaining = 300`
/// - Child A (flex=1): `allocated = 300 * 1/3 = 100`, tight constraints
///   `(100, 100, 0, 100)`; the callback returns `(max_w, max_h) =
///   (100, 100)`.
/// - Child B (flex=2): `allocated = 300 * 2/3 = 200`, tight constraints
///   `(200, 200, 0, 100)`; the callback returns `(200, 100)`.
/// - `total_main = 100 + 200 = 300`, `cross = 100`.
/// - Final `size = (300, 100)`.
///
/// We assert exact dimensions AND each child's received constraints to
/// prove the Proxy bridge actually forwarded the correct `flex` factor
/// through `child_parent_data` (a failed downcast would treat children
/// as inflexible — `total_flex = 0` and the layout would collapse to
/// `(0, 0)` per the `if total_flex > 0` guard in `RenderFlex`).
#[test]
fn u19_variable_bridge_walks_child_slice_with_typed_parent_data() {
    let mut obj = RenderFlex::row();

    // Two children with distinct flex factors so we can verify typed
    // parent-data access through the Proxy → erased downcast path.
    let constraints = BoxConstraints::new(px(0.0), px(300.0), px(0.0), px(100.0));
    let mut children: Vec<ChildState<FlexParentData>> = vec![
        ChildState::with_parent_data(RenderId::new(1), FlexParentData::flexible(1)),
        ChildState::with_parent_data(RenderId::new(2), FlexParentData::flexible(2)),
    ];
    let child_ids = [RenderId::new(1), RenderId::new(2)];

    // Capture per-child (id, constraints) so we can assert the exact
    // tight constraints each child was offered — proves the Proxy
    // bridge forwarded the typed `flex` factor correctly.
    let observed: Arc<Mutex<Vec<(RenderId, BoxConstraints)>>> = Arc::new(Mutex::new(Vec::new()));
    let observed_for_cb = Arc::clone(&observed);
    let layout_child_callback: Arc<
        dyn Fn(flui_foundation::RenderId, BoxConstraints) -> Size + Send + Sync,
    > = Arc::new(move |id, c| {
        observed_for_cb.lock().unwrap().push((id, c));
        // Respond at the largest allowed size — tight constraints give
        // (max, max) which is exactly the allocated flex slice.
        Size::new(c.max_width, c.max_height)
    });

    let mut direct_ctx: BoxLayoutCtx<'_, Variable, FlexParentData> =
        BoxLayoutCtx::with_layout_callback(
            constraints,
            &mut children,
            &child_ids,
            layout_child_callback.as_ref(),
        );

    let erased: &mut dyn BoxLayoutCtxErased = &mut direct_ctx;

    let size = <RenderFlex as RenderObject<BoxProtocol>>::perform_layout_raw(&mut obj, erased);

    // Deterministic flex math (see test doc): both children flex with
    // factors 1:2, parent max_width=300, no inflexible/spacing.
    assert_eq!(
        size,
        Size::new(px(300.0), px(100.0)),
        "Variable bridge with flex 1:2 over 300px main axis must produce \
         exact (300, 100) — actual {:?}",
        size,
    );

    // Each child's constraints prove the typed parent-data round-tripped
    // through Proxy → erased. If the FlexParentData downcast had failed,
    // RenderFlex would have treated both children as inflexible (flex =
    // None) — total_flex = 0 — and not invoked any layout_child calls
    // (because the inflexible pre-pass also skips when flex is None per
    // the `if flex_factors[i].is_none() || flex_factors[i] == Some(0)`
    // guard — wait, actually inflexible children DO get laid out with
    // unbounded constraints in pass 1). Either way the captured
    // constraints would not be the tight allocated slices below.
    let obs = observed.lock().unwrap();
    assert_eq!(
        obs.len(),
        2,
        "Both flex children must have triggered a single layout_child call each",
    );
    // Child A (flex=1): allocated = 300 * 1/3 = 100, tight.
    assert_eq!(obs[0].0, RenderId::new(1));
    assert_eq!(
        obs[0].1,
        BoxConstraints::new(px(100.0), px(100.0), px(0.0), px(100.0)),
        "Child A (flex=1) must receive tight 100×{{0..100}} constraints",
    );
    // Child B (flex=2): allocated = 300 * 2/3 = 200, tight.
    assert_eq!(obs[1].0, RenderId::new(2));
    assert_eq!(
        obs[1].1,
        BoxConstraints::new(px(200.0), px(200.0), px(0.0), px(100.0)),
        "Child B (flex=2) must receive tight 200×{{0..100}} constraints",
    );
}

// ============================================================================
// Sanity: the leaf-mode helper protocol Protocol::with_leaf_erased_ctx
// matches the same bridge path RenderEntry::layout uses.
// ============================================================================

/// Verifies the entry-layout path: protocol's `with_leaf_erased_ctx`
/// helper hands `RenderColoredBox::perform_layout_raw` an erased ctx
/// and the result matches the user's `perform_layout` output.
#[test]
fn u19_with_leaf_erased_ctx_matches_direct_bridge_call() {
    let mut obj = RenderColoredBox::green(60.0, 30.0);
    let constraints = BoxConstraints::tight(Size::new(px(60.0), px(30.0)));

    // Mirror what RenderEntry::layout does.
    let size = <BoxProtocol as Protocol>::with_leaf_erased_ctx(constraints, |erased| {
        <RenderColoredBox as RenderObject<BoxProtocol>>::perform_layout_raw(&mut obj, erased)
    });

    assert_eq!(size, Size::new(px(60.0), px(30.0)));
}

// ============================================================================
// Bridge contract violation (review fix #4): a RenderBox::perform_layout
// that forgets to call ctx.complete_with_size() must trip the bridge's
// expect/panic, naming the offending render object.
// ============================================================================

/// Plan U19 §407 contract: if `RenderBox::perform_layout` returns without
/// calling `ctx.complete_with_size()`, the blanket bridge raises
/// `RenderError::ContractViolation` via `std::panic::panic_any(...)`.
/// `RenderEntry::layout`'s `catch_unwind` handler downcasts the panic
/// payload to recover the typed value and returns it through
/// `RenderResult` — distinct from `RenderError::Poisoned`, which is
/// reserved for unstructured runtime panics.
///
/// This test:
/// 1. Drives a deliberately-broken `ForgetfulBox` directly through the
///    blanket `perform_layout_raw` and asserts the panic payload is a
///    typed `RenderError::ContractViolation` carrying the render
///    object's debug name and the contract description.
/// 2. (See [`u19_contract_violation_surfaces_through_render_entry_layout`])
///    Wraps the same fixture in a `RenderEntry<BoxProtocol>` and
///    asserts the typed error round-trips out as `Err(RenderError::ContractViolation)`
///    — verifying the entry-layout payload-downcast handler.
#[test]
fn u19_bridge_panics_on_missing_complete_with_size() {
    use flui_foundation::Diagnosticable;
    use flui_rendering::{
        context::{BoxHitTestContext, BoxLayoutContext},
        error::RenderError,
        hit_testing::HitTestBehavior,
        traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
    };
    use flui_types::{Point, Rect};

    #[derive(Debug, Default)]
    struct ForgetfulBox {
        size: Size,
    }

    impl Diagnosticable for ForgetfulBox {}
    impl PaintEffectsCapability for ForgetfulBox {}
    impl SemanticsCapability for ForgetfulBox {}
    impl HotReloadCapability for ForgetfulBox {}

    impl RenderBox for ForgetfulBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
            // INTENTIONAL: forgets to call ctx.complete_with_size(...)
        }

        fn size(&self) -> &Size {
            &self.size
        }
        fn size_mut(&mut self) -> &mut Size {
            &mut self.size
        }
        fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
            false
        }
        fn hit_test_behavior(&self) -> HitTestBehavior {
            HitTestBehavior::Opaque
        }
        fn box_paint_bounds(&self) -> Rect {
            Rect::from_origin_size(Point::ZERO, self.size)
        }
    }

    let mut obj = ForgetfulBox::default();
    let constraints = BoxConstraints::tight(Size::new(px(10.0), px(10.0)));
    let mut direct_ctx: BoxLayoutCtx<'_, Leaf, BoxParentData> = BoxLayoutCtx::new(constraints);
    let erased: &mut dyn BoxLayoutCtxErased = &mut direct_ctx;

    // Panic propagates from the blanket impl's `panic_any`; assert via
    // catch_unwind. AssertUnwindSafe is fine — we own all the borrowed
    // state on this test stack frame.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        <ForgetfulBox as RenderObject<BoxProtocol>>::perform_layout_raw(&mut obj, erased)
    }));

    let panic_payload =
        result.expect_err("perform_layout that forgets complete_with_size must panic");

    // **Review fix #5 (Option B).** Payload is a typed `RenderError`
    // (via `std::panic::panic_any(RenderError::ContractViolation { ... })`
    // in the blanket impl), not an opaque string. Downcast to the typed
    // value and assert variant + fields.
    let typed_err = panic_payload
        .downcast::<RenderError>()
        .map(|boxed| *boxed)
        .unwrap_or_else(|_| {
            panic!("panic payload should be RenderError, got non-RenderError payload")
        });
    match typed_err {
        RenderError::ContractViolation {
            render_object,
            what,
        } => {
            assert!(
                render_object.contains("ForgetfulBox"),
                "render_object should name the offending type, got: {render_object}",
            );
            assert!(
                what.contains("complete_with_size"),
                "what should describe the missing-completion contract, got: {what}",
            );
        }
        other => panic!("expected ContractViolation, got {other:?}"),
    }
}

/// Review fix #5 (Option B) round-trip: a `RenderBox` that forgets to
/// call `ctx.complete_with_size(...)` produces
/// `Err(RenderError::ContractViolation { ... })` when driven through
/// `RenderEntry::<BoxProtocol>::layout`, not `Err(RenderError::Poisoned)`.
/// `Poisoned` is reserved for unstructured runtime panics.
#[test]
fn u19_contract_violation_surfaces_through_render_entry_layout() {
    use flui_foundation::Diagnosticable;
    use flui_rendering::{
        context::{BoxHitTestContext, BoxLayoutContext},
        error::RenderError,
        hit_testing::HitTestBehavior,
        storage::RenderEntry,
        traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
    };
    use flui_types::{Point, Rect};

    #[derive(Debug, Default)]
    struct ForgetfulBox {
        size: Size,
    }

    impl Diagnosticable for ForgetfulBox {}
    impl PaintEffectsCapability for ForgetfulBox {}
    impl SemanticsCapability for ForgetfulBox {}
    impl HotReloadCapability for ForgetfulBox {}

    impl RenderBox for ForgetfulBox {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
            // INTENTIONAL: forgets ctx.complete_with_size(...)
        }

        fn size(&self) -> &Size {
            &self.size
        }
        fn size_mut(&mut self) -> &mut Size {
            &mut self.size
        }
        fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
            false
        }
        fn hit_test_behavior(&self) -> HitTestBehavior {
            HitTestBehavior::Opaque
        }
        fn box_paint_bounds(&self) -> Rect {
            Rect::from_origin_size(Point::ZERO, self.size)
        }
    }

    let obj: Box<dyn RenderObject<BoxProtocol>> = Box::new(ForgetfulBox::default());
    let mut entry: RenderEntry<BoxProtocol> = RenderEntry::new(obj);
    let constraints = BoxConstraints::tight(Size::new(px(10.0), px(10.0)));

    let result = entry.layout_leaf_only(constraints);

    let err = result.expect_err("ForgetfulBox::perform_layout must surface as Err");
    match err {
        RenderError::ContractViolation {
            render_object,
            what,
        } => {
            assert!(
                render_object.contains("ForgetfulBox"),
                "render_object should name the offending type, got: {render_object}",
            );
            assert!(
                what.contains("complete_with_size"),
                "what should describe the missing-completion contract, got: {what}",
            );
        }
        other => panic!(
            "expected RenderError::ContractViolation, got {other:?} — \
                 Poisoned should be reserved for unstructured runtime panics",
        ),
    }

    // State invariants: geometry not set, NEEDS_LAYOUT still true so
    // the pipeline can retry next frame after the offending node is
    // removed or fixed.
    let box_entry = entry; // shadow to drop the explicit type annotation
    assert!(
        box_entry.state().geometry().is_none(),
        "geometry must not be recorded on the contract-violation path",
    );
    assert!(
        box_entry.state().needs_layout(),
        "NEEDS_LAYOUT must remain set so a retry is possible next frame",
    );
}

// ============================================================================
// Edge case (review fix #16): zero-child Variable bridge.
// ============================================================================

/// `RenderFlex::row()` with zero children completes layout with
/// `constraints.smallest()` per the early-return path in
/// `RenderFlex::perform_layout` (`if child_count == 0`). The bridge must
/// propagate this small size — it must not assume children are present
/// just because the typed wrapper has `Variable` arity.
#[test]
fn u19_variable_bridge_handles_zero_children() {
    let mut obj = RenderFlex::row();
    // min=0 / max=300 etc — `smallest()` is (0, 0).
    let constraints = BoxConstraints::new(px(0.0), px(300.0), px(0.0), px(100.0));
    let mut children: Vec<ChildState<FlexParentData>> = vec![];
    let child_ids: [RenderId; 0] = [];

    let cb: Arc<dyn Fn(RenderId, BoxConstraints) -> Size + Send + Sync> =
        Arc::new(|_, _| Size::ZERO);

    let mut direct_ctx: BoxLayoutCtx<'_, Variable, FlexParentData> =
        BoxLayoutCtx::with_layout_callback(constraints, &mut children, &child_ids, cb.as_ref());

    let erased: &mut dyn BoxLayoutCtxErased = &mut direct_ctx;
    let size = <RenderFlex as RenderObject<BoxProtocol>>::perform_layout_raw(&mut obj, erased);

    assert_eq!(
        size,
        Size::new(px(0.0), px(0.0)),
        "Zero-child RenderFlex must complete with constraints.smallest() = (0, 0)",
    );
}

// ============================================================================
// RenderViewAdapter smoke test (review fix #15): the root view's manual
// RenderObject<BoxProtocol> impl uses the erased ctx as a sentinel and
// drives its own perform_layout via embedded RenderView state.
// ============================================================================

/// `RenderViewAdapter::perform_layout_raw` ignores the erased ctx and
/// calls `RenderView::perform_layout()`, which reads logical
/// constraints from the embedded `ViewConfiguration` and writes
/// `RenderView::size` to `logical_constraints.smallest()`. Returns
/// that size unchanged.
///
/// This smoke test covers the manual (non-blanket) `RenderObject<P>`
/// implementation path. The blanket bridge handles user
/// `RenderBox`/`RenderSliver` impls; the root view sidesteps it
/// because its layout shape (no parent constraints, own-configuration
/// driven) doesn't fit the `RenderBox::perform_layout(ctx)` signature.
#[test]
fn u19_render_view_adapter_bridge_smoke() {
    use flui_rendering::view::{RenderView, RenderViewAdapter, ViewConfiguration};

    // Tight configuration: logical_size = (200, 150) at 1x DPR.
    let config = ViewConfiguration::from_size(Size::new(px(200.0), px(150.0)), 1.0);
    let mut view = RenderView::with_configuration(config);
    // Without prepare_initial_frame*, root_transform stays None and
    // perform_layout asserts. The without-owner variant is the
    // sanctioned test-side path (see RenderView::prepare_initial_frame).
    view.prepare_initial_frame_without_owner();

    let mut adapter = RenderViewAdapter::new(view);

    // Hand the adapter a sentinel erased ctx — its body ignores the
    // ctx and drives layout from its own configuration.
    let sentinel_constraints = BoxConstraints::tight(Size::new(px(999.0), px(999.0)));
    let size = <BoxProtocol as Protocol>::with_leaf_erased_ctx(sentinel_constraints, |erased| {
        <RenderViewAdapter as RenderObject<BoxProtocol>>::perform_layout_raw(&mut adapter, erased)
    });

    // Logical constraints from from_size(200×150, 1.0) are tight at
    // (200, 150). RenderView::perform_layout writes size =
    // constraints.smallest() = (200, 150).
    assert_eq!(
        size,
        Size::new(px(200.0), px(150.0)),
        "RenderViewAdapter must layout from its embedded configuration, \
         not from the sentinel erased-ctx constraints",
    );
}
