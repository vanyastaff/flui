//! Integration tests for `ShrinkWrappingViewport`.

mod common;

use common::lay_out;
use flui_rendering::constraints::BoxConstraints;
use flui_types::{Size, geometry::px};
use flui_widgets::prelude::*;

#[test]
fn shrink_wrapping_viewport_sizes_to_sliver_content() {
    let laid = lay_out(
        ShrinkWrappingViewport::new(vec![
            SliverFixedExtentList::new(25.0, vec![SizedBox::square(10.0), SizedBox::square(10.0)])
                .boxed(),
        ]),
        BoxConstraints::new(px(300.0), px(300.0), px(0.0), px(1_000.0)),
    );

    let viewport = laid.find_by_render_type("RenderShrinkWrappingViewport");
    assert_eq!(
        laid.size(viewport),
        Size::new(px(300.0), px(50.0)),
        "ShrinkWrappingViewport must take its height from the fixed-extent sliver"
    );
}

#[test]
fn shrink_wrapping_viewport_clamps_to_parent_max_height() {
    let laid = lay_out(
        ShrinkWrappingViewport::new(vec![
            SliverFixedExtentList::new(
                50.0,
                vec![
                    SizedBox::square(10.0),
                    SizedBox::square(10.0),
                    SizedBox::square(10.0),
                    SizedBox::square(10.0),
                ],
            )
            .boxed(),
        ]),
        BoxConstraints::new(px(300.0), px(300.0), px(0.0), px(120.0)),
    );

    let viewport = laid.find_by_render_type("RenderShrinkWrappingViewport");
    assert_eq!(
        laid.size(viewport),
        Size::new(px(300.0), px(120.0)),
        "parent max height must clamp the shrink-wrapped content height"
    );
}
