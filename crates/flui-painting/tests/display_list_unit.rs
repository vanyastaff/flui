//! DisplayList unit tests extracted from
//! `crates/flui-painting/src/display_list/mod.rs` during Mythos chain U8.

use flui_painting::display_list::Shader;
use flui_painting::{BlendMode, DisplayList, DisplayListCore, DrawCommand, Paint};
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
