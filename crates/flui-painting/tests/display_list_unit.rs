//! DisplayList unit tests extracted from
//! `crates/flui-painting/src/display_list/mod.rs` during the display-list module split.

use std::sync::Arc;

use flui_foundation::geometry::{Matrix4, Rect};
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

/// A bounds-less command recorded first must not drag the origin into the
/// list's bounds.
///
/// The recording path used to seed its union on `commands.is_empty()`, which
/// asks whether anything was *recorded* — not whether anything *contributed
/// bounds*. A clip, a `Save`, or (until recently) a `DrawTextSpan` answers
/// those two questions differently, so the next real command unioned against
/// a still-unset `Rect::ZERO` and every such list claimed to reach back to
/// (0, 0).
#[test]
fn bounds_less_leading_command_does_not_seed_the_origin() {
    let far = Rect::from_ltrb(100.0, 100.0, 150.0, 150.0);

    let mut canvas = Canvas::new();
    canvas.clip_rect(far);
    canvas.draw_rect(far, &Paint::fill(Color::RED));

    assert_eq!(canvas.finish().bounds(), Some(far));
}

/// `draw_picture` replays a recorded list under the caller's transform:
/// each command's absolute transform becomes `ctm * recorded`, so a picture
/// recorded at the origin lands where the canvas is currently translated,
/// and a nested translation inside the picture composes with it.
#[test]
fn draw_picture_restamps_by_the_current_transform() {
    let rect = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
    let picture = flui_painting::testing::record(|canvas| {
        canvas.draw_rect(rect, &Paint::fill(Color::RED));
        canvas.save();
        canvas.translate(5.0, 0.0);
        canvas.draw_rect(rect, &Paint::fill(Color::BLUE));
        canvas.restore();
    });
    assert_eq!(picture.len(), 4);

    let replayed = flui_painting::testing::record(|canvas| {
        canvas.translate(100.0, 200.0);
        canvas.draw_picture(&picture);
    });
    assert_eq!(
        replayed.len(),
        picture.len(),
        "every command replays, scopes included"
    );

    let ctm = Matrix4::translation(100.0, 200.0, 0.0);
    for (original, copy) in picture.iter().zip(replayed.iter()) {
        assert_eq!(copy.transform, ctm * original.transform);
    }
    assert_eq!(
        replayed.bounds(),
        Some(Rect::from_xywh(100.0, 200.0, 15.0, 10.0)),
        "the replayed bounds are the picture's bounds under the ctm"
    );
    let ops: Vec<&DrawOp> = replayed.iter().map(|c| &c.op).collect();
    assert!(matches!(
        ops.as_slice(),
        [
            DrawOp::Rect { .. },
            DrawOp::Save,
            DrawOp::Rect { .. },
            DrawOp::Restore
        ]
    ));
}
