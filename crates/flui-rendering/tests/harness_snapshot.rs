//! Structural paint-snapshot dogfood for the render harness (sub-project A).

#[test]
fn insta_tooling_smoke() {
    insta::assert_snapshot!("smoke", "line one\nline two");
}

use flui_rendering::objects::RenderColoredBox;
use flui_rendering::testing::{DrawKind, RenderTester, box_node};
use flui_types::{Size, geometry::px};

#[test]
fn frame_snapshot_and_predicate() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(40.0), px(40.0)))
        .run_frame();
    insta::assert_snapshot!("colored_box", run.snapshot());
    run.assert_paints_any(|c| c.kind == DrawKind::Rect);
}

#[test]
#[should_panic(expected = "no painted command matched")]
fn assert_paints_any_fails_on_absent_op() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(40.0, 40.0)))
        .with_size(Size::new(px(40.0), px(40.0)))
        .run_frame();
    run.assert_paints_any(|c| c.kind == DrawKind::Shadow);
}
