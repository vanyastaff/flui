//! Headless render-object inspector.
//!
//! Builds a few representative trees with the `flui-rendering` test harness,
//! drives them through the real pipeline at both run depths, and prints what
//! came out — no window, no GPU. This is the "run them and see they work"
//! surface for the harness.
//!
//! Run with:
//!
//! ```text
//! cargo run -p flui-rendering --example render_inspector --features testing
//! ```
//!
//! `println!` is used deliberately: this is a dev-only example binary, not
//! shipped library code (the harness itself never writes to stdout — it only
//! returns reports and values).

use flui_objects::{
    RenderColoredBox, RenderFlex, RenderPadding, RenderSliverFixedExtentList, RenderViewport,
};
use flui_rendering::testing::{Probe, RenderTester, box_node, sliver_node};
use flui_types::{Size, geometry::px, layout::AxisDirection};

fn header(title: &str) {
    println!("\n========== {title} ==========");
}

fn main() {
    box_full_frame();
    box_layout_only();
    sliver_layout_only();
}

/// Box tree driven through a full frame: layer structure + picture bounds.
fn box_full_frame() {
    header("Box / run_frame: flex row of three colored boxes");

    let run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("red"))
            .child(box_node(RenderColoredBox::green(60.0, 40.0)).label("green"))
            .child(box_node(RenderColoredBox::blue(20.0, 40.0)).label("blue")),
    )
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_frame();

    println!("{}", run.report());
    for label in ["red", "green", "blue"] {
        let id = run.id(label);
        println!(
            "  {label:<6} offset={:?} size={:?}",
            run.offset(id),
            run.box_geometry(id),
        );
    }
    println!("\n-- diagnostics tree --\n{}", run.dump());
}

/// Box tree inspected at the layout phase only — geometry without a frame.
fn box_layout_only() {
    header("Box / run_layout: nested padding, geometry without a frame");

    let run = RenderTester::mount(
        box_node(RenderPadding::all(8.0))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("inner")),
    )
    .with_size(Size::new(px(200.0), px(200.0)))
    .run_layout();

    let inner = run.id("inner");
    println!("  root   size={:?}", run.box_geometry(run.root()));
    println!(
        "  inner  offset={:?} size={:?}",
        run.offset(inner),
        run.box_geometry(inner),
    );
}

/// Sliver tree inspected at the layout phase: committed sliver geometry.
fn sliver_layout_only() {
    header("Sliver / run_layout: fixed-extent list under a viewport");

    let run = RenderTester::mount(
        box_node(RenderViewport::new(AxisDirection::TopToBottom)).child(
            sliver_node(RenderSliverFixedExtentList::new(30.0))
                .label("list")
                .child(box_node(RenderColoredBox::red(300.0, 1000.0)))
                .child(box_node(RenderColoredBox::green(300.0, 1000.0)))
                .child(box_node(RenderColoredBox::blue(300.0, 1000.0))),
        ),
    )
    .with_size(Size::new(px(300.0), px(100.0)))
    .run_layout();

    let list = run.id("list");
    let geometry = run.sliver_geometry(list);
    println!("  list scroll_extent = {}", geometry.scroll_extent);
    println!("  list paint_extent  = {}", geometry.paint_extent);
    println!("  list offset        = {:?}", run.offset(list));
}
