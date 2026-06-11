//! `BoxDecoration` painter: Flutter's draw order (shadows → background
//! → image → border), gradient resolution against the paint rect, and
//! rounded-rect hit testing.
//!
//! All assertions are sans-IO over the recorded display list — the
//! same contract the fragment paint model relies on.

use flui_painting::{
    Canvas, DisplayListCore, DrawCommand, box_decoration_hit_test, paint_box_decoration,
    resolve_gradient,
};
use flui_types::{
    Offset, Pixels,
    geometry::{Rect, px},
    painting::Shader,
    styling::{
        Border, BorderRadius, BorderRadiusExt, BorderSide, BorderStyle, BoxDecoration, BoxShadow,
        Color, Gradient, LinearGradient,
    },
};

fn rect100() -> Rect<Pixels> {
    Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(50.0))
}

fn commands(decoration: &BoxDecoration<Pixels>) -> Vec<DrawCommand> {
    let mut canvas = Canvas::new();
    paint_box_decoration(&mut canvas, rect100(), decoration);
    canvas.finish().commands().cloned().collect()
}

#[test]
fn color_only_paints_a_rect() {
    let cmds = commands(&BoxDecoration::with_color(Color::RED));
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], DrawCommand::DrawRect { .. }));
}

#[test]
fn radius_switches_to_rounded_primitives() {
    let decoration = BoxDecoration::with_color(Color::RED)
        .set_border_radius(Some(BorderRadius::circular(px(8.0))));
    let cmds = commands(&decoration);
    assert_eq!(cmds.len(), 1);
    assert!(matches!(cmds[0], DrawCommand::DrawRRect { .. }));
}

#[test]
fn flutter_paint_order_shadow_background_border() {
    let decoration = BoxDecoration::with_color(Color::WHITE)
        .set_border(Some(Border::all(BorderSide::new(
            Color::BLACK,
            px(2.0),
            BorderStyle::Solid,
        ))))
        .set_box_shadow(Some(vec![BoxShadow {
            color: Color::BLACK,
            offset: Offset::new(px(0.0), px(2.0)),
            blur_radius: px(4.0),
            spread_radius: px(1.0),
            inset: false,
        }]));
    let cmds = commands(&decoration);
    assert_eq!(cmds.len(), 3, "shadow + background + border");
    assert!(
        matches!(cmds[0], DrawCommand::DrawShadow { .. }),
        "shadows paint FIRST (behind everything)"
    );
    assert!(matches!(cmds[1], DrawCommand::DrawRect { .. }));
    assert!(
        matches!(cmds[2], DrawCommand::DrawDRRect { .. }),
        "a uniform border strokes inside via an outer/inner pair, LAST"
    );
}

#[test]
fn gradient_wins_over_color_and_resolves_alignment() {
    let gradient = Gradient::Linear(LinearGradient::new(
        flui_types::Alignment::CENTER_LEFT,
        flui_types::Alignment::CENTER_RIGHT,
        vec![Color::RED, Color::BLUE],
        None,
        flui_types::painting::TileMode::Clamp,
    ));
    let decoration = BoxDecoration::with_color(Color::WHITE).set_gradient(Some(gradient.clone()));
    let cmds = commands(&decoration);
    assert_eq!(
        cmds.len(),
        1,
        "a gradient replaces the flat color entirely (Flutter contract)"
    );
    assert!(matches!(cmds[0], DrawCommand::DrawGradient { .. }));

    // Default LinearGradient runs centerLeft → centerRight: alignment
    // resolves against the CONCRETE rect.
    let Shader::LinearGradient { from, to, .. } = resolve_gradient(&gradient, rect100()) else {
        panic!("linear gradient must resolve to a linear shader");
    };
    assert_eq!(from, Offset::new(px(0.0), px(25.0)));
    assert_eq!(to, Offset::new(px(100.0), px(25.0)));
}

#[test]
fn non_uniform_border_paints_per_side_rects() {
    let decoration = BoxDecoration::<Pixels>::new().set_border(Some(Border {
        top: Some(BorderSide::new(Color::RED, px(2.0), BorderStyle::Solid)),
        right: None,
        bottom: Some(BorderSide::new(Color::BLUE, px(4.0), BorderStyle::Solid)),
        left: None,
    }));
    let cmds = commands(&decoration);
    assert_eq!(cmds.len(), 2, "one rect per non-empty side");
    assert!(
        cmds.iter()
            .all(|c| matches!(c, DrawCommand::DrawRect { .. }))
    );
}

#[test]
fn hit_test_respects_rounded_corners() {
    let decoration = BoxDecoration::with_color(Color::RED)
        .set_border_radius(Some(BorderRadius::circular(px(20.0))));
    let rect = rect100();

    // Center: inside.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(50.0), px(25.0))
    ));
    // The exact corner of the BOUNDING rect lies outside the rounded
    // shape (radius 20 cuts it off).
    assert!(!box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(1.0), px(1.0))
    ));
    // Just inside the corner arc.
    assert!(box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(20.0), px(20.0))
    ));
    // Outside the rect entirely.
    assert!(!box_decoration_hit_test(
        rect,
        &decoration,
        Offset::new(px(150.0), px(25.0))
    ));
}
