//! Directional alignment resolves in render-view configuration and follows
//! changes to the ambient reading direction.

use crate::common::{lay_out, offset, tight};
use flui_painting::typography::TextDirection;
use flui_painting::{Alignment, AlignmentDirectional, AlignmentGeometry};
use flui_widgets::{Align, Directionality, IndexedStack, SizedBox, Stack, row};

pub(crate) fn align_top_start_follows_directionality() {
    let view = |direction| {
        Directionality::new(
            direction,
            Align::new(AlignmentDirectional::TOP_START).child(SizedBox::square(20.0)),
        )
    };
    let mut laid = lay_out(view(TextDirection::Ltr), tight(200.0, 200.0));
    let child = laid.only_child(laid.root());
    assert_eq!(laid.offset(child), offset(0.0, 0.0));

    laid.pump_widget(view(TextDirection::Rtl));
    let child = laid.only_child(laid.root());
    assert_eq!(laid.offset(child), offset(180.0, 0.0));
}

pub(crate) fn stack_directional_start_differs_from_absolute_top_left() {
    let view = |direction, alignment: AlignmentGeometry| {
        Directionality::new(
            direction,
            Stack::new(row![SizedBox::square(20.0)]).alignment(alignment),
        )
    };
    let directional = AlignmentGeometry::from(AlignmentDirectional::TOP_START);
    let mut laid = lay_out(view(TextDirection::Ltr, directional), tight(200.0, 200.0));
    let child = laid.only_child(laid.root());
    assert_eq!(laid.offset(child), offset(0.0, 0.0));
    assert_eq!(laid.inherited_dependent_count::<Directionality>(), 1);

    laid.pump_widget(view(TextDirection::Rtl, directional));
    let child = laid.only_child(laid.root());
    assert_eq!(laid.offset(child), offset(180.0, 0.0));
    assert_eq!(laid.inherited_dependent_count::<Directionality>(), 1);

    laid.pump_widget(view(
        TextDirection::Rtl,
        AlignmentGeometry::from(Alignment::TOP_LEFT),
    ));
    let child = laid.only_child(laid.root());
    assert_eq!(laid.offset(child), offset(0.0, 0.0));
    assert_eq!(
        laid.inherited_dependent_count::<Directionality>(),
        0,
        "switching to an absolute alignment drops the directional dependency"
    );

    laid.pump_widget(view(
        TextDirection::Ltr,
        AlignmentGeometry::from(Alignment::TOP_LEFT),
    ));
    let child = laid.only_child(laid.root());
    assert_eq!(laid.offset(child), offset(0.0, 0.0));
    assert_eq!(laid.inherited_dependent_count::<Directionality>(), 0);
}

pub(crate) fn indexed_stack_top_start_follows_directionality() {
    let view = |direction| {
        Directionality::new(
            direction,
            IndexedStack::new(row![SizedBox::square(20.0)])
                .alignment(AlignmentDirectional::TOP_START),
        )
    };
    let mut laid = lay_out(view(TextDirection::Ltr), tight(200.0, 200.0));
    let child = laid.only_child(laid.root());
    assert_eq!(laid.offset(child), offset(0.0, 0.0));

    laid.pump_widget(view(TextDirection::Rtl));
    let child = laid.only_child(laid.root());
    assert_eq!(laid.offset(child), offset(180.0, 0.0));
}
