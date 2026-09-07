//! Criterion 4 of #538: a vertical layout must not take a `Directionality`
//! dependency it cannot use.
//!
//! `Directionality::maybe_of` REGISTERS an inherited dependency, so calling it
//! unconditionally makes every `Column` rebuild when the ambient direction
//! changes -- including a centred one whose layout cannot move. The reference
//! gates the lookup itself (`widgets/basic.dart`'s `Flex._needTextDirection`:
//! horizontal always; vertical only for `CrossAxisAlignment::Start`/`End`,
//! because those name a reading edge rather than a physical one).
//!
//! The oracle asks the dependency directly. A rebuild-counting version of this
//! test was written first and is NOT usable: changing an inherited value means
//! rebuilding the widget that provides it, which rebuilds the whole subtree
//! regardless of who depends on what, so it reports the same count for a
//! dependent and a non-dependent child. It looked convincing -- red exactly
//! like the defect -- and applying the fix did not turn it green.

use crate::common::{lay_out, tight};
use flui_rendering::constraints::BoxConstraints;
use flui_types::Axis;
use flui_types::geometry::px;
use flui_types::typography::TextDirection;
use flui_widgets::{Column, CrossAxisAlignment, Directionality, ListBody, SizedBox};

/// Dependents of the ambient `Directionality` for a `Column` with `cross`.
fn directionality_dependents(cross: CrossAxisAlignment) -> usize {
    let mut laid = lay_out(
        Directionality::new(
            TextDirection::Ltr,
            Column::new((SizedBox::square(10.0),)).cross_axis_alignment(cross),
        ),
        tight(200.0, 200.0),
    );
    laid.inherited_dependent_count::<Directionality>()
}

/// `Start` names a READING edge, so a direction change genuinely moves this
/// column's children. It must take the dependency.
///
/// This is the control that keeps its sibling honest: without it, a zero there
/// could mean the lookup is gated correctly OR that nothing ever registers.
#[test]
fn a_start_aligned_column_depends_on_directionality() {
    assert_eq!(
        directionality_dependents(CrossAxisAlignment::Start),
        1,
        "CrossAxisAlignment::Start resolves against the reading direction, so \
         the column must depend on Directionality"
    );
}

/// `Center` is a physical edge — a direction change cannot move it, so taking
/// the dependency only buys a rebuild on every direction change.
#[test]
fn a_centred_column_does_not_depend_on_directionality() {
    assert_eq!(
        directionality_dependents(CrossAxisAlignment::Center),
        0,
        "a centred column cannot move under a direction change, so it must not \
         register as a Directionality dependent"
    );
}

/// Dependents of the ambient `Directionality` for a `ListBody` on `axis`.
fn list_body_dependents(axis: Axis) -> usize {
    // `RenderListBody` requires UNBOUNDED space along its own main axis, so
    // the constraints have to follow `axis` rather than be tight both ways.
    let constraints = match axis {
        Axis::Vertical => BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(f32::INFINITY)),
        Axis::Horizontal => BoxConstraints::new(px(0.0), px(f32::INFINITY), px(0.0), px(200.0)),
    };
    let mut laid = lay_out(
        Directionality::new(
            TextDirection::Ltr,
            ListBody::new((SizedBox::square(10.0),)).main_axis(axis),
        ),
        constraints,
    );
    laid.inherited_dependent_count::<Directionality>()
}

/// A horizontal `ListBody` resolves its axis direction from the reading
/// direction, so it must depend on it. The control for its sibling.
#[test]
fn a_horizontal_list_body_depends_on_directionality() {
    assert_eq!(
        list_body_dependents(Axis::Horizontal),
        1,
        "a horizontal ListBody maps the reading direction onto its axis \
         direction, so it must register as a dependent"
    );
}

/// A vertical `ListBody` discards the direction outright -- `resolve_axis_direction`'s
/// vertical arm never reads it -- so taking the dependency buys nothing but a
/// rebuild on every direction change.
#[test]
fn a_vertical_list_body_does_not_depend_on_directionality() {
    assert_eq!(
        list_body_dependents(Axis::Vertical),
        0,
        "the vertical arm of resolve_axis_direction ignores the reading \
         direction, so the lookup must not be made at all"
    );
}
