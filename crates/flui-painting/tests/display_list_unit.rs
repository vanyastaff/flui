//! DisplayList unit tests extracted from
//! `crates/flui-painting/src/display_list/mod.rs` during Mythos chain U8.

use std::sync::Arc;

use flui_painting::display_list::Shader;
use flui_painting::{BlendMode, Canvas, DisplayList, DisplayListCore, DrawCommand, Paint};
use flui_types::{
    geometry::{Matrix4, Rect, px},
    styling::Color,
};

#[test]
fn test_display_list_creation() {
    let display_list = DisplayList::new();
    assert!(display_list.is_empty());
    assert_eq!(display_list.len(), 0);
    assert_eq!(display_list.bounds(), Rect::ZERO);
}

#[test]
fn test_display_list_clear() {
    // Build via Canvas because `DisplayList::push` is pub(crate).
    use flui_painting::Canvas;

    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)),
        &Paint::default(),
    );

    let mut display_list = canvas.finish();
    assert!(!display_list.is_empty());

    display_list.clear();
    assert!(display_list.is_empty());
    assert_eq!(display_list.bounds(), Rect::ZERO);
}

#[test]
fn test_display_list_apply_transform_via_canvas() {
    use flui_painting::Canvas;

    let mut canvas = Canvas::new();
    let rect = Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0));
    canvas.draw_rect(rect, &Paint::fill(Color::RED));

    let mut dl = canvas.finish();
    assert_eq!(dl.len(), 1);

    let translation = Matrix4::translation(50.0, 0.0, 0.0);
    dl.apply_transform(translation);

    // Bounds should have shifted (left + 50.0)
    assert!(dl.bounds().left() > px(0.0));
}

#[test]
fn test_display_list_command_iteration() {
    use flui_painting::Canvas;

    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(50.0), px(50.0)),
        &Paint::default(),
    );
    canvas.draw_rect(
        Rect::from_ltrb(px(50.0), px(50.0), px(100.0), px(100.0)),
        &Paint::default(),
    );

    let dl = canvas.finish();
    let count = dl.commands().count();
    assert_eq!(count, 2);

    // Each command is a DrawRect.
    for cmd in dl.commands() {
        assert!(matches!(cmd, DrawCommand::DrawRect { .. }));
    }
}

/// Regression test for the `MAX_EFFECT_DEPTH` saturation guard added
/// during the Mythos code-review fixup pass.
///
/// Builds a 256-deep `ShaderMask` chain (4× the 64-level cap) and
/// calls both `DisplayList::to_opacity` and
/// `DisplayList::apply_transform`. Without the depth cap, each
/// recursion frame holds a full `DrawCommand` value plus the
/// `Box<DisplayList>` drop ladder and blows the default thread stack
/// (~8 MB) on the 800th–1500th frame. With the cap the call returns
/// after visiting the first 64 levels and emits a `tracing::warn!`
/// saturation event for the rest.
///
/// We do *not* assert on log output (no test subscriber wired here);
/// the load-bearing assertion is that the call returns at all
/// instead of overflowing.
///
/// `DisplayList::commands` / `push` are `pub(crate)`, so we
/// construct the chain by seeding a single-command `DisplayList`
/// through `Canvas` and re-mapping it via `DisplayList::map` 256
/// times — each pass replaces the single command with a `ShaderMask`
/// whose child is the previous step's `DisplayList`.
#[test]
fn nested_shader_mask_opacity_depth_saturates_without_overflow() {
    use flui_painting::Canvas;

    // Seed: a single-command DisplayList. The exact command is
    // irrelevant — only the length-1 shape matters so `map` produces
    // a length-1 wrapper list at each step.
    let mut seed_canvas = Canvas::new();
    seed_canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(1.0), px(1.0)),
        &Paint::fill(Color::BLACK),
    );
    let mut current = seed_canvas.finish();

    for _ in 0..256 {
        let prev = current.clone();
        current = current.map(|_cmd| DrawCommand::ShaderMask {
            child: Box::new(prev.clone()),
            shader: Shader::Solid {
                color: Color::WHITE,
            },
            bounds: Rect::from_ltrb(px(0.0), px(0.0), px(10.0), px(10.0)),
            blend_mode: BlendMode::SrcOver,
            transform: Matrix4::identity(),
        });
    }

    // Both calls must return without stack overflow.
    let _ = current.to_opacity(0.5);

    let mut transformed = current.clone();
    transformed.apply_transform(Matrix4::translation(1.0, 2.0, 0.0));
}

// ============================================================================
// Cycle 5 U10 — Paint interning proof
//
// The three tests below pin the per-Canvas Paint interning behaviour
// added in U10 (origin R15, audit P-7). They depend only on the
// public DrawCommand enum surface and on Arc identity, so they are
// stable against future representational tweaks.
// ============================================================================

/// Helper: pull the `Arc<Paint>` out of a `DrawRect` command for
/// identity comparisons in the tests below. Panics on the wrong
/// variant so a mis-recorded command fails the test loudly instead
/// of silently passing.
fn rect_paint(cmd: &DrawCommand) -> &Arc<Paint> {
    match cmd {
        DrawCommand::DrawRect { paint, .. } => paint,
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
    let cmds: Vec<&DrawCommand> = dl.commands().collect();
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
    let cmds: Vec<&DrawCommand> = dl.commands().collect();
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
    let cmds: Vec<&DrawCommand> = dl.commands().collect();
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
    let cmds: Vec<&DrawCommand> = dl.commands().collect();

    let p0 = rect_paint(cmds[0]);
    let p1 = rect_paint(cmds[1]);
    assert!(
        !Arc::ptr_eq(p0, p1),
        "paints differing only in shader must not share an Arc",
    );
    assert!(p0.shader.is_none());
    assert!(p1.shader.is_some());
}
