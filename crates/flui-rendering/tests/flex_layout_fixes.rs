//! RenderFlex fixes pinned against the Flutter reference (`flex.dart`).
//!
//! Four confirmed audit findings plus the loose-cross rule, each
//! verified against the reference before fixing (the spacing
//! "double-count" finding from the same audit was REFUTED there and is
//! deliberately absent):
//!
//! 1. `free_space` is clamped at zero (`flex.dart:1339`) — an
//!    overflowing row must not shift children by negative space;
//! 2. `CrossAxisAlignment::Stretch` TIGHTENS the cross constraints
//!    (`:889-898`) — pre-fix it only changed the cross offset;
//! 3. unbounded main + flex factors → children are demoted to
//!    inflexible (`:1232`) — pre-fix a Tight fit collapsed them to 0×0;
//! 4. `MainAxisSize::Max` (default) claims the bounded main extent
//!    (`:1298`) — pre-fix the container always shrink-wrapped, so
//!    alignment had no free space under loose constraints;
//! 5. non-stretch children receive a LOOSE cross even when the
//!    incoming cross is tight.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use flui_foundation::RenderId;
use flui_objects::{MainAxisAlignment, MainAxisSize, RenderFlex};
use flui_rendering::{
    constraints::BoxConstraints,
    parent_data::FlexParentData,
    protocol::{
        BoxLayoutCtx, BoxProtocol, ChildState, RenderObject, box_protocol::BoxLayoutCtxErased,
    },
};
use flui_tree::Variable;
use flui_types::{Offset, Size, geometry::px};

type Observed = Arc<Mutex<Vec<(RenderId, BoxConstraints)>>>;

/// Drives `perform_layout_raw` over a Direct ctx with N children whose
/// layout callback answers `min(preferred, max)` per axis — a stand-in
/// for fixed-size leaves. Returns (flex size, child states, observed
/// per-child constraints).
fn lay_out(
    flex: &mut RenderFlex,
    constraints: BoxConstraints,
    children_pd: Vec<FlexParentData>,
    preferred: Size,
) -> (Size, Vec<ChildState<FlexParentData>>, Observed) {
    let ids: Vec<RenderId> = (1..=children_pd.len() as u32)
        .map(|n| RenderId::new(n as usize))
        .collect();
    let mut children: Vec<ChildState<FlexParentData>> = ids
        .iter()
        .zip(children_pd)
        .map(|(&id, pd)| ChildState::with_parent_data(id, pd))
        .collect();

    let observed: Observed = Arc::new(Mutex::new(Vec::new()));
    let observed_cb = Arc::clone(&observed);
    let callback: Arc<dyn Fn(RenderId, BoxConstraints) -> Size + Send + Sync> =
        Arc::new(move |id, c| {
            observed_cb.lock().unwrap().push((id, c));
            Size::new(
                preferred.width.min(c.max_width).max(c.min_width),
                preferred.height.min(c.max_height).max(c.min_height),
            )
        });

    let size = {
        let mut ctx: BoxLayoutCtx<'_, Variable, FlexParentData> =
            BoxLayoutCtx::with_layout_callback(constraints, &mut children, &ids, callback.as_ref());
        let erased: &mut dyn BoxLayoutCtxErased = &mut ctx;
        <RenderFlex as RenderObject<BoxProtocol>>::perform_layout_raw(flex, erased)
            .expect("flex layout must succeed")
    };
    (size, children, observed)
}

fn inflexible() -> FlexParentData {
    FlexParentData::default()
}

// ============================================================================
// 1. Overflow clamps free_space at zero
// ============================================================================

#[test]
fn overflow_does_not_shift_children_by_negative_space() {
    let mut flex = RenderFlex::row().with_main_axis_alignment(MainAxisAlignment::End);
    // Two 100-wide children into a 150-wide row: 50px overflow.
    let (size, children, _) = lay_out(
        &mut flex,
        BoxConstraints::new(px(0.0), px(150.0), px(0.0), px(50.0)),
        vec![inflexible(), inflexible()],
        Size::new(px(100.0), px(40.0)),
    );

    assert_eq!(size.width, px(150.0), "row clamps to its max width");
    assert_eq!(
        children[0].offset,
        Offset::new(px(0.0), px(0.0)),
        "End alignment under overflow must clamp free space to zero — \
         a negative shift would drag the first child off-screen left",
    );
    assert_eq!(children[1].offset, Offset::new(px(100.0), px(0.0)));
}

// ============================================================================
// 2. Stretch tightens the cross constraints
// ============================================================================

#[test]
fn stretch_tightens_child_cross_constraints() {
    let mut flex =
        RenderFlex::row().with_cross_axis_alignment(flui_objects::CrossAxisAlignment::Stretch);
    let (_, children, observed) = lay_out(
        &mut flex,
        BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(80.0)),
        vec![inflexible()],
        Size::new(px(40.0), px(40.0)),
    );

    let (_, child_constraints) = observed.lock().unwrap()[0];
    assert_eq!(
        (child_constraints.min_height, child_constraints.max_height),
        (px(80.0), px(80.0)),
        "Stretch must TIGHTEN the cross constraints, not merely move \
         the child's cross offset",
    );
    assert_eq!(
        children[0].size.height,
        px(80.0),
        "the child actually stretches to the row's cross extent",
    );
}

// ============================================================================
// 3. Unbounded main demotes flex children to inflexible
// ============================================================================

#[test]
fn unbounded_main_demotes_flex_children() {
    let mut flex = RenderFlex::row();
    // Unbounded width with a flex child: factors are meaningless — the
    // child lays out as inflexible at its preferred size.
    let (size, children, _) = lay_out(
        &mut flex,
        BoxConstraints::new(px(0.0), px(f32::INFINITY), px(0.0), px(50.0)),
        vec![FlexParentData::flexible(1)],
        Size::new(px(40.0), px(40.0)),
    );

    assert_eq!(
        children[0].size,
        Size::new(px(40.0), px(40.0)),
        "a flex child under an unbounded main axis must be demoted to \
         inflexible — pre-fix the Tight fit collapsed it to 0×0",
    );
    assert_eq!(
        size.width,
        px(40.0),
        "the row shrink-wraps the demoted child (no bounded extent to fill)",
    );
}

// ============================================================================
// 4. MainAxisSize::Max fills the bounded extent; Min shrink-wraps
// ============================================================================

#[test]
fn main_axis_size_max_gives_alignment_its_free_space() {
    let mut flex = RenderFlex::row().with_main_axis_alignment(MainAxisAlignment::Center);
    // Default MainAxisSize::Max: the row claims all 200px, so Center
    // has 120px of free space around two 40px children.
    let (size, children, _) = lay_out(
        &mut flex,
        BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(50.0)),
        vec![inflexible(), inflexible()],
        Size::new(px(40.0), px(40.0)),
    );
    assert_eq!(size.width, px(200.0), "Max claims the bounded extent");
    assert_eq!(
        children[0].offset.dx,
        px(60.0),
        "Center finally has free space to distribute — pre-fix the row \
         shrink-wrapped and alignment was a no-op under loose constraints",
    );

    let mut flex_min = RenderFlex::row()
        .with_main_axis_alignment(MainAxisAlignment::Center)
        .with_main_axis_size(MainAxisSize::Min);
    let (size_min, children_min, _) = lay_out(
        &mut flex_min,
        BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(50.0)),
        vec![inflexible(), inflexible()],
        Size::new(px(40.0), px(40.0)),
    );
    assert_eq!(size_min.width, px(80.0), "Min shrink-wraps");
    assert_eq!(children_min[0].offset.dx, px(0.0));
}

// ============================================================================
// 5. Non-stretch children get a LOOSE cross
// ============================================================================

#[test]
fn non_stretch_children_get_loose_cross_under_tight_parent() {
    let mut flex = RenderFlex::row();
    // Tight 80 cross from the parent: children must still be offered a
    // LOOSE 0..80 cross, not forced to 80.
    let (_, children, observed) = lay_out(
        &mut flex,
        BoxConstraints::new(px(0.0), px(200.0), px(80.0), px(80.0)),
        vec![inflexible()],
        Size::new(px(40.0), px(40.0)),
    );

    let (_, child_constraints) = observed.lock().unwrap()[0];
    assert_eq!(
        child_constraints.min_height,
        px(0.0),
        "an incoming tight cross must be LOOSENED for non-stretch \
         children (Flutter parity)",
    );
    assert_eq!(children[0].size.height, px(40.0));
}
