//! The mount frame and the frame after it, through the UI runtime's own
//! `UiRuntime::pump`.
//!
//! A tree mounted under a headless host is laid out and painted by the same
//! frame transaction every later frame runs (ADR-0083 §4): the runtime's
//! `RenderView` roots the render tree at the window's size, the caller's root
//! receives its tight size, and a frame with no work submits nothing.
//!
//! The root render element is the legal bare mount: a render element with no
//! element-tree parent, which `render_custody` adopts without a render parent
//! and without reporting an orphan.

use std::time::Duration;

use flui_foundation::geometry::Size;
use flui_rendering::PipelineOwner;
use flui_rendering::testing::inspect;
use flui_testing::log_capture::capture;
use flui_testing::widgets::{lay_out, tight};
use flui_widgets::SizedBox;

pub(crate) fn the_mount_frame_installs_the_render_root_at_the_window_size_and_paints() {
    let (laid, log) = capture(|| lay_out(SizedBox::new(40.0, 25.0), tight(200.0, 200.0)));

    assert_eq!(
        log.count_containing("adoption would orphan the render object"),
        0,
        "the root render element is a legal bare mount: {log}",
    );
    let pipeline = laid.pipeline_owner();
    let render_root = pipeline
        .with(PipelineOwner::root_id)
        .expect("the mount frame installs a render root");
    assert_ne!(
        render_root,
        laid.root(),
        "the runtime's RenderView roots the render tree, above the caller's root",
    );
    assert_eq!(
        pipeline.with(|owner| inspect::box_geometry(owner, render_root)),
        Some(Size::new(200.0, 200.0)),
        "the mount frame lays the RenderView out at the window's size",
    );
    assert_eq!(
        laid.size(laid.root()),
        Size::new(200.0, 200.0),
        "the caller's root receives the view's tight size on the mount frame",
    );
    assert!(
        laid.did_paint_last_frame(),
        "a tree with real geometry submits a frame on its mount",
    );
}

pub(crate) fn a_frame_with_no_work_submits_nothing_and_keeps_the_committed_geometry() {
    let mut laid = lay_out(SizedBox::new(10.0, 10.0), tight(50.0, 50.0));
    let pipeline = laid.pipeline_owner();
    let render_root = pipeline
        .with(PipelineOwner::root_id)
        .expect("the mount frame installs a render root");
    let submits_after_mount = laid.painted_frame_count();

    laid.pump_for(Duration::from_millis(16));

    assert_eq!(
        laid.painted_frame_count(),
        submits_after_mount,
        "the runtime submits no frame when nothing needs layout or paint",
    );
    assert_eq!(
        pipeline.with(PipelineOwner::root_id),
        Some(render_root),
        "the next frame is an ordinary pump: no second mount, no re-rooting",
    );
    assert_eq!(
        pipeline.with(|owner| inspect::box_geometry(owner, render_root)),
        Some(Size::new(50.0, 50.0)),
        "the committed geometry survives a frame with no work",
    );

    laid.reassemble_render_tree();
    laid.pump_for(Duration::from_millis(16));
    assert_eq!(
        laid.painted_frame_count(),
        submits_after_mount + 1,
        "a frame with paint work submits again over the same root",
    );
}
