//! Recording contracts of `Canvas` and `DisplayList`: save/restore balance,
//! isolated appends, paint interning, and hand-off to another thread.

use std::sync::Arc;
use std::thread;

use flui_foundation::geometry::Rect;
use flui_painting::styling::Color;
use flui_painting::{Canvas, DisplayList, DrawCommand, DrawOp, Paint};

pub(crate) fn save_restore_tracks_the_save_count() {
    let mut canvas = Canvas::new();
    assert_eq!(canvas.save_count(), 1);
    canvas.save();
    assert_eq!(canvas.save_count(), 2);
    canvas.translate(50.0, 50.0);
    canvas.save();
    assert_eq!(canvas.save_count(), 3);
    canvas.restore();
    assert_eq!(canvas.save_count(), 2);
    canvas.restore();
    assert_eq!(canvas.save_count(), 1);
}

pub(crate) fn shear_factors_map_the_named_axes_in_recorded_commands() {
    let list = flui_painting::testing::record(|canvas| {
        canvas.translate(10.0, 20.0);
        canvas.skew(2.0, 3.0);
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 1.0, 1.0),
            &Paint::fill(Color::RED),
        );
    });
    let command = list.iter().next().expect("the rectangle was recorded");
    assert_eq!(command.transform.transform_point(4.0, 5.0), (24.0, 37.0));
    assert_eq!(list.bounds(), Some(Rect::from_ltrb(10.0, 20.0, 13.0, 24.0)));
}

/// `Canvas::finish` wires a `debug_assert!` to catch unrestored `save()`
/// calls; release builds finalise silently, so the row is debug-only.
#[cfg(debug_assertions)]
pub(crate) fn finish_panics_in_debug_on_unrestored_save() {
    let payload = std::panic::catch_unwind(|| {
        let mut canvas = Canvas::new();
        canvas.save();
        canvas.translate(50.0, 50.0);
        let _ = canvas.finish();
    })
    .expect_err("finish() must reject an unrestored save() in debug builds");
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or_default();
    assert!(
        message.contains("unrestored save() calls"),
        "unexpected panic message: {message:?}"
    );
}

pub(crate) fn isolated_append_preserves_bounds_and_scopes_the_run() {
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

fn rect_paint(cmd: &DrawCommand) -> &Arc<Paint> {
    match &cmd.op {
        DrawOp::Rect { paint, .. } => paint,
        other => panic!("expected DrawRect, got {other:?}"),
    }
}

/// Two `draw_rect` calls with the same `Paint` value share one `Arc<Paint>`.
pub(crate) fn interning_shares_arc_for_identical_paints() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(Rect::from_ltrb(0.0, 0.0, 10.0, 10.0), &paint);
    canvas.draw_rect(Rect::from_ltrb(20.0, 20.0, 30.0, 30.0), &paint);

    let dl = canvas.finish();
    let cmds: Vec<&DrawCommand> = dl.commands().iter().collect();
    assert_eq!(cmds.len(), 2);
    assert!(
        Arc::ptr_eq(rect_paint(cmds[0]), rect_paint(cmds[1])),
        "identical paints must share one Arc allocation"
    );
}

/// A finished `DisplayList` moves to another thread (the GPU hand-off) intact.
pub(crate) fn a_finished_display_list_is_sendable_to_another_thread() {
    let mut canvas = Canvas::new();
    for i in 0..100 {
        let rect = Rect::from_ltrb(f64::from(i), 0.0, f64::from(i + 1), 50.0);
        canvas.draw_rect(rect, &Paint::fill(Color::RED));
    }
    let display_list = canvas.finish();
    let executed = thread::spawn(move || display_list.commands().iter().count())
        .join()
        .expect("the consumer thread must not panic");
    assert_eq!(executed, 100);
}
