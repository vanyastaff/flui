//! Integration tests for `ShrinkWrappingViewport`.

use crate::common::lay_out;
use flui_foundation::geometry::Size;
use flui_rendering::constraints::BoxConstraints;
use flui_widgets::prelude::*;

#[test]
fn shrink_wrapping_viewport_sizes_to_sliver_content() {
    let laid = lay_out(
        ShrinkWrappingViewport::new(vec![
            SliverFixedExtentList::new(25.0, vec![SizedBox::square(10.0), SizedBox::square(10.0)])
                .boxed(),
        ]),
        BoxConstraints::new(300.0, 300.0, 0.0, 1_000.0),
    );

    let viewport = laid.find_by_render_type("RenderShrinkWrappingViewport");
    assert_eq!(
        laid.size(viewport),
        Size::new(300.0, 50.0),
        "ShrinkWrappingViewport must take its height from the fixed-extent sliver"
    );
}

#[test]
fn shrink_wrapping_viewport_adopts_the_new_axis_on_rebuild() {
    // A shrink-wrap sizes to content on its MAIN axis and fills the cross axis.
    // Loose on both axes (0..300) so the content (2×25 = 50px) shrinks whichever
    // axis is main. Reconciliation reuses the render object across a rebuild, so
    // a vertical→horizontal axis change must flip the layout — this is a
    // regression guard for the axis staying stale from construction instead
    // of updating on rebuild.
    let constraints = BoxConstraints::new(0.0, 300.0, 0.0, 300.0);
    let content = || {
        vec![
            SliverFixedExtentList::new(25.0, vec![SizedBox::square(10.0), SizedBox::square(10.0)])
                .boxed(),
        ]
    };

    let mut laid = lay_out(
        ShrinkWrappingViewport::new(content()).axis_direction(AxisDirection::TopToBottom),
        constraints,
    );
    let viewport = laid.find_by_render_type("RenderShrinkWrappingViewport");
    assert_eq!(
        laid.size(viewport),
        Size::new(300.0, 50.0),
        "vertical: height shrinks to the 50px content, width fills the 300 cross axis",
    );

    // Root-swap to a horizontal axis — the same render object is reused.
    laid.pump_widget(
        ShrinkWrappingViewport::new(content()).axis_direction(AxisDirection::LeftToRight),
    );
    let viewport = laid.find_by_render_type("RenderShrinkWrappingViewport");
    assert_eq!(
        laid.size(viewport),
        Size::new(50.0, 300.0),
        "after rebuild to horizontal the reused render object must adopt the new \
         axis: width shrinks to the 50px content, height fills the 300 cross axis \
         (stays (300, 50) if the axis was left stale)",
    );
}
