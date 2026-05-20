//! DisplayList unit tests extracted from
//! `crates/flui-painting/src/display_list/mod.rs` during Mythos chain U8.

use flui_painting::{DisplayList, DisplayListCore, DrawCommand, Paint};
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
