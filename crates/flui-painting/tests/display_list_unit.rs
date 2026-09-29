//! DisplayList unit tests extracted from
//! `crates/flui-painting/src/display_list/mod.rs` during the display-list module split.

use std::sync::Arc;

use flui_foundation::geometry::Rect;
use flui_painting::styling::Color;
use flui_painting::{Canvas, DisplayList, DrawCommand, DrawOp, Paint};

#[test]
fn isolated_append_preserves_bounds_and_scopes_the_run() {
    let rect = Rect::from_xywh(100.0, 200.0, 30.0, 40.0);
    let run = flui_painting::testing::record(|canvas| {
        canvas.clip_rect(rect);
        canvas.draw_rect(rect, &Paint::fill(Color::RED));
    });
    let mut combined = DisplayList::new();
    combined.append_isolated(DisplayList::new());
    assert!(combined.is_empty());
    combined.append_isolated(run);
    assert_eq!(combined.bounds(), Some(rect));
    let ops: Vec<&DrawOp> = combined.iter().map(|c| &c.op).collect();
    assert!(matches!(
        ops.as_slice(),
        [
            DrawOp::Save,
            DrawOp::ClipRect { .. },
            DrawOp::Rect { .. },
            DrawOp::Restore,
        ]
    ));
    combined.append_isolated(DisplayList::new());
    assert_eq!(combined.len(), 4);
    assert_eq!(combined.bounds(), Some(rect));
}

// ============================================================================
// Paint interning proof
//
// The three tests below pin the per-Canvas Paint interning behaviour.
// They depend only on the public DrawCommand enum surface and on Arc
// identity, so they are stable against future representational tweaks.
// ============================================================================

/// Helper: pull the `Arc<Paint>` out of a `DrawRect` command for
/// identity comparisons in the tests below. Panics on the wrong
/// variant so an incorrectly-recorded command fails the test loudly
/// instead of silently passing.
fn rect_paint(cmd: &DrawCommand) -> &Arc<Paint> {
    match &cmd.op {
        DrawOp::Rect { paint, .. } => paint,
        other => panic!("expected DrawRect, got {other:?}"),
    }
}

#[test]
fn interning_shares_arc_for_identical_paints() {
    // Two `draw_rect` calls with the same `Paint` value must end up
    // sharing one `Arc<Paint>` in the recorded `DrawCommand`s.
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);

    canvas.draw_rect(Rect::from_ltrb(0.0, 0.0, 10.0, 10.0), &paint);
    canvas.draw_rect(Rect::from_ltrb(20.0, 20.0, 30.0, 30.0), &paint);

    let dl = canvas.finish();
    let cmds: Vec<&DrawCommand> = dl.commands().iter().collect();
    assert_eq!(cmds.len(), 2);

    let p0 = rect_paint(cmds[0]);
    let p1 = rect_paint(cmds[1]);
    assert!(
        Arc::ptr_eq(p0, p1),
        "identical paints must share one Arc allocation"
    );
}
