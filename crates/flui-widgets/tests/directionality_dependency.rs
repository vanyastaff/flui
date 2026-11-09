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
use flui_painting::typography::TextDirection;
use flui_painting::{Alignment, AlignmentDirectional};
use flui_rendering::constraints::AxisDirection;
use flui_widgets::{Align, Column, CrossAxisAlignment, Directionality, SizedBox, Stack, Viewport};

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
pub(crate) fn a_start_aligned_column_depends_on_directionality() {
    assert_eq!(
        directionality_dependents(CrossAxisAlignment::Start),
        1,
        "CrossAxisAlignment::Start resolves against the reading direction, so \
         the column must depend on Directionality"
    );
}

/// Dependents of the ambient `Directionality` for a render-object widget mounted
/// under it.
fn render_dependents(view: impl flui_view::View) -> usize {
    let mut laid = lay_out(
        Directionality::new(TextDirection::Ltr, view),
        tight(200.0, 200.0),
    );
    laid.inherited_dependent_count::<Directionality>()
}

/// An absolute alignment names physical edges, so `Align` must not register a
/// `Directionality` dependency; a directional one names a reading edge and must.
/// `Stack` always registers one — it resolves the reading direction for its
/// `Positioned` children's `start`/`end` edges, which it cannot see statically
/// (T09).
pub(crate) fn directional_geometry_registers_a_direction_dependency_only_when_directional() {
    let absolute_align =
        render_dependents(Align::new(Alignment::CENTER).child(SizedBox::square(10.0)));
    assert_eq!(
        absolute_align, 0,
        "an absolute alignment names physical edges: no Directionality dependency"
    );
    let directional_align = render_dependents(
        Align::new(AlignmentDirectional::TOP_START).child(SizedBox::square(10.0)),
    );
    assert_eq!(
        directional_align, 1,
        "a directional alignment names a reading edge"
    );

    let absolute_stack = render_dependents(Stack::new((SizedBox::square(10.0),)));
    assert_eq!(
        absolute_stack, 1,
        "Stack always resolves the reading direction for Positioned start/end"
    );
    let directional_stack = render_dependents(
        Stack::new((SizedBox::square(10.0),)).alignment(AlignmentDirectional::TOP_START),
    );
    assert_eq!(
        directional_stack, 1,
        "a directional Stack alignment names a reading edge"
    );
}

/// A vertical viewport lays its horizontal cross axis against the reading
/// direction, so it must depend on it; a horizontal one lays a fixed
/// top-to-bottom cross axis and must not (T09).
pub(crate) fn a_vertical_viewport_depends_on_directionality_but_a_horizontal_one_does_not() {
    let vertical = render_dependents(Viewport::new((SizedBox::square(10.0),)));
    assert_eq!(
        vertical, 1,
        "a vertical viewport's cross axis is horizontal (reading)"
    );
    let horizontal = render_dependents(
        Viewport::new((SizedBox::square(10.0),)).axis_direction(AxisDirection::LeftToRight),
    );
    assert_eq!(
        horizontal, 0,
        "a horizontal viewport's cross axis is top-to-bottom"
    );
}
