//! DisplayList unit tests extracted from
//! `crates/flui-painting/src/display_list/mod.rs` during the display-list module split.

use std::sync::Arc;

use flui_painting::{Canvas, DisplayList, DrawCommand, DrawOp, Paint, Shader};
use flui_types::{
    geometry::{Matrix4, Rect, px},
    styling::Color,
};

#[test]
fn test_display_list_creation() {
    let display_list = DisplayList::new();
    assert!(display_list.is_empty());
    assert_eq!(display_list.len(), 0);
    assert_eq!(display_list.bounds(), None);
}

#[test]
fn isolated_append_preserves_bounds_and_scopes_the_run() {
    let rect = Rect::from_xywh(px(100.0), px(200.0), px(30.0), px(40.0));
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

#[test]
fn test_display_list_command_iteration() {
    // Dogfoods the `testing::record` builder (no manual Canvas::new/finish).
    let dl = flui_painting::testing::record(|canvas| {
        canvas.draw_rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(50.0), px(50.0)),
            &Paint::default(),
        );
        canvas.draw_rect(
            Rect::from_ltrb(px(50.0), px(50.0), px(100.0), px(100.0)),
            &Paint::default(),
        );
    });

    let count = dl.commands().len();
    assert_eq!(count, 2);

    // Each command is a DrawRect.
    for cmd in dl.commands() {
        assert!(matches!(cmd.op, DrawOp::Rect { .. }));
    }
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

    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(10.0), px(10.0)),
        &paint,
    );
    canvas.draw_rect(
        Rect::from_ltrb(px(20.0), px(20.0), px(30.0), px(30.0)),
        &paint,
    );

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

#[test]
fn interning_keeps_distinct_paints_separate() {
    // Two distinct paints must produce two distinct `Arc<Paint>`
    // entries. The values still compare equal field-by-field as
    // before; only the identity differs.
    let mut canvas = Canvas::new();
    let red = Paint::fill(Color::RED);
    let blue = Paint::fill(Color::BLUE);

    canvas.draw_rect(Rect::from_ltrb(px(0.0), px(0.0), px(10.0), px(10.0)), &red);
    canvas.draw_rect(
        Rect::from_ltrb(px(20.0), px(20.0), px(30.0), px(30.0)),
        &blue,
    );

    let dl = canvas.finish();
    let cmds: Vec<&DrawCommand> = dl.commands().iter().collect();
    assert_eq!(cmds.len(), 2);

    let p0 = rect_paint(cmds[0]);
    let p1 = rect_paint(cmds[1]);
    assert!(
        !Arc::ptr_eq(p0, p1),
        "distinct paints must NOT share an Arc"
    );
    assert_eq!(p0.color, Color::RED);
    assert_eq!(p1.color, Color::BLUE);
}

/// Proxy benchmark: 100 `draw_rect` calls with the same paint must
/// land in a single `Arc<Paint>` whose strong-count is at least 100
/// (one per DrawRect that holds it). Without interning each call
/// would have cloned the `Paint` value and strong-count would be 1
/// per command. The exact strong-count includes the pool's own
/// retained `Arc::clone`, so we assert `>= 100`.
#[test]
fn interning_100_draws_share_single_arc() {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::GREEN);

    for i in 0..100 {
        let f = i as f32;
        canvas.draw_rect(
            Rect::from_ltrb(px(f), px(f), px(f + 1.0), px(f + 1.0)),
            &paint,
        );
    }

    let dl = canvas.finish();
    let cmds: Vec<&DrawCommand> = dl.commands().iter().collect();
    assert_eq!(cmds.len(), 100);

    let first_paint = rect_paint(cmds[0]);
    let count = Arc::strong_count(first_paint);
    assert!(
        count >= 100,
        "100 identical paint draws should share one Arc with strong_count >= 100, got {count}",
    );

    // Spot-check the last command holds the same Arc identity.
    let last_paint = rect_paint(cmds[99]);
    assert!(Arc::ptr_eq(first_paint, last_paint));
}

#[test]
fn interning_distinguishes_paints_with_different_shaders() {
    // Two paints that differ ONLY in their shader must NOT share an
    // Arc — the public `Paint::PartialEq` ignores the shader, but
    // the per-canvas pool's equality predicate layers shader
    // comparison on top so the interning never silently merges
    // visually-distinct paints.
    let mut canvas = Canvas::new();

    let solid = Paint::fill(Color::WHITE);
    let with_shader = Paint::fill(Color::WHITE).with_shader(Shader::Solid {
        color: Color::BLACK,
    });

    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(10.0), px(10.0)),
        &solid,
    );
    canvas.draw_rect(
        Rect::from_ltrb(px(20.0), px(20.0), px(30.0), px(30.0)),
        &with_shader,
    );

    let dl = canvas.finish();
    let cmds: Vec<&DrawCommand> = dl.commands().iter().collect();

    let p0 = rect_paint(cmds[0]);
    let p1 = rect_paint(cmds[1]);
    assert!(
        !Arc::ptr_eq(p0, p1),
        "paints differing only in shader must not share an Arc",
    );
    assert!(p0.shader.is_none());
    assert!(p1.shader.is_some());
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
    let far = Rect::from_ltrb(px(100.0), px(100.0), px(150.0), px(150.0));

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
    let rect = Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0));
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
        Some(Rect::from_xywh(px(100.0), px(200.0), px(15.0), px(10.0))),
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

/// A command carries its full transform, so `Save`/`Restore` carry nothing:
/// they are unit markers whose only job is to scope clips for the backend.
/// The canvas still restores its own transform on `restore`, and a command
/// recorded after the scope closes is stamped with the outer transform.
#[test]
fn save_and_restore_carry_no_state_but_the_clip_scope() {
    let rect = Rect::from_xywh(px(0.0), px(0.0), px(10.0), px(10.0));
    let list = flui_painting::testing::record(|canvas| {
        canvas.translate(1.0, 0.0);
        canvas.save();
        canvas.translate(0.0, 2.0);
        canvas.clip_rect(rect);
        canvas.draw_rect(rect, &Paint::fill(Color::RED));
        canvas.restore();
        canvas.draw_rect(rect, &Paint::fill(Color::RED));
    });
    let cmds = list.commands();
    assert!(matches!(cmds[0].op, DrawOp::Save));
    assert!(matches!(cmds[3].op, DrawOp::Restore));
    assert_eq!(
        cmds[1].transform,
        Matrix4::translation(1.0, 2.0, 0.0),
        "the clip is stamped with the transform it was recorded under"
    );
    assert_eq!(cmds[2].transform, Matrix4::translation(1.0, 2.0, 0.0));
    assert_eq!(
        cmds[4].transform,
        Matrix4::translation(1.0, 0.0, 0.0),
        "restore unwinds the canvas transform for what follows"
    );
    // The markers' own transforms are what the canvas held at the time —
    // informational only; nothing reads them.
    assert_eq!(cmds[0].transform, Matrix4::translation(1.0, 0.0, 0.0));
}
