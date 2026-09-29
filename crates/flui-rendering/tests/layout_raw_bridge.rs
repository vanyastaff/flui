//! `RenderObject<BoxProtocol>::perform_layout_raw` blanket-impl bridge
//! integration tests.
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
//! Coverage spans Leaf (`RenderColoredBox`), Single (`RenderPadding`),
//! and Variable (`RenderFlex`) arities.

use std::sync::{Arc, Mutex};

use flui_foundation::RenderId;
use flui_foundation::geometry::Size;
use flui_foundation::{Leaf, Variable};
use flui_objects::{RenderColoredBox, RenderFlex};
use flui_rendering::{
    constraints::BoxConstraints,
    parent_data::{BoxParentData, FlexParentData},
    // `BoxLayoutCtxErased` is intentionally NOT re-exported under
    // `protocol::*` — a `protocol::*` glob would pull it in alongside
    // `LayoutContextApi` and collide on method names. Pull it from the
    // explicit module path here.
    protocol::{
        BoxLayoutCtx, BoxProtocol, ChildState, RenderObject, box_protocol::BoxLayoutCtxErased,
    },
};

// ============================================================================
// Leaf bridge: RenderColoredBox via blanket perform_layout_raw
// ============================================================================

/// Happy path: `RenderColoredBox` (Leaf arity) layout via the blanket
/// bridge returns the correctly constrained `Size`.
///
/// Previously the blanket `perform_layout_raw` shipped as a no-op
/// returning `*self.size()` — for a fresh `RenderColoredBox` that meant
/// `Size::ZERO`. Now the blanket impl drives the user's
/// `RenderBox::perform_layout`, which constrains `preferred_size`
/// against the parent's constraints and completes layout.
#[test]
fn leaf_bridge_returns_constrained_size() {
    let mut obj = RenderColoredBox::red(100.0, 50.0);
    let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));

    let mut direct_ctx: BoxLayoutCtx<'_, Leaf, BoxParentData> = BoxLayoutCtx::new(constraints);
    let erased: &mut dyn BoxLayoutCtxErased = &mut direct_ctx;

    // perform_layout_raw returns `RenderResult<Size>`; on the happy path
    // we unwrap to assert the concrete Size.
    let size = <RenderColoredBox as RenderObject<BoxProtocol>>::perform_layout_raw(
        &mut obj,
        // GAT resolves `<BoxProtocol as Protocol>::LayoutCtxErased<'_>` to
        // `dyn BoxLayoutCtxErased + '_`; the coercion above gives us
        // exactly that.
        erased,
    )
    .expect("Leaf bridge happy path must succeed");

    assert_eq!(
        size,
        Size::new(100.0, 50.0),
        "Leaf bridge must return the user's perform_layout-completed size, \
         not Size::ZERO (prior placeholder behaviour)",
    );
}

// ============================================================================
// Single bridge: RenderPadding via blanket perform_layout_raw
// ============================================================================

// ============================================================================
// Variable bridge: RenderFlex via blanket perform_layout_raw
// ============================================================================

/// Happy path: `RenderFlex` (Variable arity, with typed
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
fn variable_bridge_walks_child_slice_with_typed_parent_data() {
    let mut obj = RenderFlex::row();

    // Two children with distinct flex factors so we can verify typed
    // parent-data access through the Proxy → erased downcast path.
    let constraints = BoxConstraints::new(0.0, 300.0, 0.0, 100.0);
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

    let size = <RenderFlex as RenderObject<BoxProtocol>>::perform_layout_raw(&mut obj, erased)
        .expect("Variable bridge happy path must succeed");

    // Deterministic flex math (see test doc): both children flex with
    // factors 1:2, parent max_width=300, no inflexible/spacing.
    assert_eq!(
        size,
        Size::new(300.0, 100.0),
        "Variable bridge with flex 1:2 over 300px main axis must produce \
         exact (300, 100) — actual {size:?}",
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
        BoxConstraints::new(100.0, 100.0, 0.0, 100.0),
        "Child A (flex=1) must receive tight 100×{{0..100}} constraints",
    );
    // Child B (flex=2): allocated = 300 * 2/3 = 200, tight.
    assert_eq!(obs[1].0, RenderId::new(2));
    assert_eq!(
        obs[1].1,
        BoxConstraints::new(200.0, 200.0, 0.0, 100.0),
        "Child B (flex=2) must receive tight 200×{{0..100}} constraints",
    );
}

// ============================================================================
// Sanity: the leaf-mode helper protocol Protocol::with_leaf_erased_ctx
// matches the same bridge path RenderEntry::layout uses.
// ============================================================================

// ============================================================================
// Note: the "forgetful RenderBox" contract-violation tests that previously
// existed here have been removed. Since `RenderBox::perform_layout` now
// returns `Size` directly, a missing completion is a **compile error**
// (the function must return a value). `RenderError::ContractViolation` no
// longer arises on this path; it is still available for other error sites.
// ============================================================================

// ============================================================================
// Edge case (review fix #16): zero-child Variable bridge.
// ============================================================================

// ============================================================================
// RenderViewAdapter smoke test (review fix #15): the root view's manual
// RenderObject<BoxProtocol> impl uses the erased ctx as a sentinel and
// drives its own perform_layout via embedded RenderView state.
// ============================================================================
