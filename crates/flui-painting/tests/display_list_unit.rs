//! DisplayList unit tests extracted from
//! `crates/flui-painting/src/display_list/mod.rs` during the display-list module split.

use std::sync::Arc;

use flui_foundation::geometry::{Matrix4, Rect};
use flui_painting::styling::Color;
use flui_painting::{Canvas, DisplayList, DrawCommand, DrawOp, Paint, Shader};

#[test]
fn test_display_list_creation() {
    let display_list = DisplayList::new();
    assert!(display_list.is_empty());
    assert_eq!(display_list.len(), 0);
    assert_eq!(display_list.bounds(), None);
}

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

#[test]
fn test_display_list_command_iteration() {
    // Dogfoods the `testing::record` builder (no manual Canvas::new/finish).
    let dl = flui_painting::testing::record(|canvas| {
        canvas.draw_rect(Rect::from_ltrb(0.0, 0.0, 50.0, 50.0), &Paint::default());
        canvas.draw_rect(Rect::from_ltrb(50.0, 50.0, 100.0, 100.0), &Paint::default());
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

#[test]
fn interning_keeps_distinct_paints_separate() {
    // Two distinct paints must produce two distinct `Arc<Paint>`
    // entries. The values still compare equal field-by-field as
    // before; only the identity differs.
    let mut canvas = Canvas::new();
    let red = Paint::fill(Color::RED);
    let blue = Paint::fill(Color::BLUE);

    canvas.draw_rect(Rect::from_ltrb(0.0, 0.0, 10.0, 10.0), &red);
    canvas.draw_rect(Rect::from_ltrb(20.0, 20.0, 30.0, 30.0), &blue);

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
        let f = i as f64;
        canvas.draw_rect(Rect::from_ltrb(f, f, f + 1.0, f + 1.0), &paint);
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

    canvas.draw_rect(Rect::from_ltrb(0.0, 0.0, 10.0, 10.0), &solid);
    canvas.draw_rect(Rect::from_ltrb(20.0, 20.0, 30.0, 30.0), &with_shader);

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

/// A command carries its full transform, so `Save`/`Restore` carry nothing:
/// they are unit markers whose only job is to scope clips for the backend.
/// The canvas still restores its own transform on `restore`, and a command
/// recorded after the scope closes is stamped with the outer transform.
#[test]
fn save_and_restore_carry_no_state_but_the_clip_scope() {
    let rect = Rect::from_xywh(0.0, 0.0, 10.0, 10.0);
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

/// The rect covering a bounded damage extent in the list's own space.
fn bounded(list: &DisplayList) -> Rect<f64> {
    list.damage_extent()
        .and_then(flui_painting::DamageExtent::covering_rect)
        .unwrap_or_else(|| panic!("expected a bounded damage extent, got {list:?}"))
}

/// A full-canvas fill has no `bounds()` (it is not a layout box), but it
/// changes every pixel its clip allows, so a damage region built from the
/// layout answer would leave the rest of the fill stale.
#[test]
fn a_color_fill_makes_the_extent_unbounded() {
    let rect = Rect::from_xywh(10.0, 10.0, 20.0, 20.0);
    let list = flui_painting::testing::record(|canvas| {
        canvas.draw_rect(rect, &Paint::fill(Color::RED));
        canvas.draw_color(Color::BLUE, flui_painting::paint::BlendMode::SrcOver);
    });
    assert_eq!(list.bounds(), Some(rect), "the layout box ignores the fill");
    assert_eq!(
        list.damage_extent(),
        Some(flui_painting::DamageExtent::Unbounded)
    );

    let paint_fill = flui_painting::testing::record(|canvas| {
        canvas.draw_paint(&Paint::fill(Color::RED));
    });
    assert_eq!(
        paint_fill.damage_extent(),
        Some(flui_painting::DamageExtent::Unbounded)
    );
    let unbounded_layer = flui_painting::testing::record(|canvas| {
        canvas.save_layer(None, &Paint::fill(Color::RED));
        canvas.restore();
    });
    assert_eq!(
        unbounded_layer.damage_extent(),
        Some(flui_painting::DamageExtent::Unbounded)
    );
    assert_eq!(
        flui_painting::testing::record(|canvas| canvas.clip_rect(rect)).damage_extent(),
        None,
        "a clip draws nothing"
    );
}

/// Every glyph bitmap the rasterizer produces for a paragraph lies inside
/// the list's damage extent, however far the ink reaches past the laid-out
/// box: under a line height a third of the font size, the glyphs' ascenders
/// and descenders stand well outside the line box.
///
/// The oracle is the rasterizer itself: each glyph's bitmap placed as the
/// engine places it (`TextLayout::placed_glyphs` plus the bitmap's bearings),
/// not a restatement of how the extent is computed.
#[test]
fn paragraph_extent_covers_every_rasterized_glyph() {
    use flui_foundation::geometry::Offset;
    use flui_painting::TextPainter;
    use flui_painting::typography::{TextDirection, TextSpan, TextStyle};

    for (text, line_height) in [("Hello, FLUI!", None), ("Hgjpqy|", Some(0.3))] {
        let mut style = TextStyle::new().with_font_size(40.0);
        if let Some(height) = line_height {
            style = style.with_height(height);
        }
        let mut painter = TextPainter::new()
            .with_text(TextSpan::new(text).with_style(style))
            .with_text_direction(TextDirection::Ltr);
        painter.layout(0.0, f64::INFINITY);
        let origin = Offset::new(40.0, 50.0);
        let mut canvas = Canvas::new();
        painter.paint(&mut canvas, origin);
        let list = canvas.finish();
        let extent = bounded(&list);

        let (layout, offset) = list
            .iter()
            .find_map(|command| match &command.op {
                DrawOp::Paragraph { layout, offset, .. } => Some((layout.clone(), *offset)),
                _ => None,
            })
            .expect("the painter records a paragraph");
        let fonts = flui_painting::shared_font_system();
        let mut glyphs = 0;
        let mut past_box = false;
        let layout_box = list.bounds().expect("a painted span has bounds");
        for glyph in layout.placed_glyphs((offset.dx as f32, offset.dy as f32), 1.0) {
            let Some(image) = fonts.rasterize(glyph.key) else {
                continue;
            };
            if image.width == 0 || image.height == 0 {
                continue;
            }
            glyphs += 1;
            let ink = Rect::from_xywh(
                f64::from(glyph.x + image.left),
                f64::from(glyph.y - image.top),
                f64::from(image.width),
                f64::from(image.height),
            );
            past_box |= !layout_box.contains_rect(&ink);
            assert!(
                extent.contains_rect(&ink),
                "{text:?} (line height {line_height:?}): glyph ink {ink:?} escapes the                  damage extent {extent:?}"
            );
        }
        assert!(glyphs > 0, "precondition: {text:?} rasterizes glyphs");
        if line_height.is_some() {
            assert!(
                past_box,
                "precondition: under a tight line height the ink leaves the layout box                  {layout_box:?}"
            );
        }
    }
}

/// A stroke's miter reaches a full width past the geometry (the layout box
/// adds half), and a shadow reaches 3.5 elevations: three blur sigmas of one
/// elevation each past a copy offset half an elevation down, the reach of the
/// GPU's analytic shadow (the layout box adds one elevation).
#[test]
fn stroke_and_shadow_extents_cover_their_outsets() {
    let rect = Rect::from_xywh(100.0, 100.0, 50.0, 50.0);
    let stroked = flui_painting::testing::record(|canvas| {
        canvas.draw_rect(rect, &Paint::stroke(Color::RED, 8.0));
    });
    assert_eq!(stroked.bounds(), Some(rect.expand(4.0)));
    assert_eq!(bounded(&stroked), rect.expand(8.0));

    let path = flui_painting::paint::Path::rectangle(rect);
    let shadow = flui_painting::testing::record(|canvas| {
        canvas.draw_shadow(&path, Color::BLACK, 6.0);
    });
    assert_eq!(shadow.bounds(), Some(rect.expand(6.0)));
    assert_eq!(bounded(&shadow), rect.expand(21.0));

    // A transform maps the widened local rect, as `bounds()` does.
    let scaled = flui_painting::testing::record(|canvas| {
        canvas.scale(2.0, 2.0);
        canvas.draw_rect(rect, &Paint::stroke(Color::RED, 8.0));
    });
    assert_eq!(
        bounded(&scaled),
        Matrix4::scaling(2.0, 2.0, 1.0).transform_rect(&rect.expand(8.0))
    );
}

/// A shadow's blur reaches equally far on both axes of the target, because
/// the renderer blurs with one sigma taken from the transform's largest
/// scale: under `scale(4, 0.25)` the compressed vertical axis gets the full
/// 3.5 x elevation x 4 too, whether the scale is the canvas's own or a
/// layer's above the list.
#[test]
fn shadow_extent_spreads_by_the_largest_scale_on_both_axes() {
    use flui_painting::DamageExtent;

    let rect = Rect::from_xywh(10.0, 20.0, 10.0, 40.0);
    let path = flui_painting::paint::Path::rectangle(rect);
    let scale = Matrix4::scaling(4.0, 0.25, 1.0);
    // Device rect of the path: (40, 5)-(80, 15); reach 3.5 x 2 x 4 = 28.
    let expected = Rect::from_ltrb(12.0, -23.0, 108.0, 43.0);

    let scaled = flui_painting::testing::record(|canvas| {
        canvas.scale(4.0, 0.25);
        canvas.draw_shadow(&path, Color::BLACK, 2.0);
    });
    assert_eq!(bounded(&scaled), expected, "a canvas scale");

    let plain = flui_painting::testing::record(|canvas| {
        canvas.draw_shadow(&path, Color::BLACK, 2.0);
    });
    assert_eq!(
        plain
            .damage_extent()
            .map(|extent| extent.transformed(&scale))
            .and_then(DamageExtent::covering_rect),
        Some(expected),
        "a layer's scale above the list"
    );
}

/// A line and a point are stroked whatever the paint's style: the renderer
/// draws a fill-style `draw_line` at its raw `stroke_width`, and a point as a
/// circle of half of it, so their extents reach that far too rather than
/// collapsing to the bare geometry.
#[test]
fn fill_style_lines_and_points_reach_their_stroke_width() {
    use flui_foundation::geometry::Point;
    use flui_painting::paint::PointMode;

    let mut paint = Paint::fill(Color::RED);
    paint.stroke_width = 6.0;
    let line = flui_painting::testing::record(|canvas| {
        canvas.draw_line(Point::new(10.0, 20.0), Point::new(90.0, 20.0), &paint);
    });
    let extent = bounded(&line);
    assert!(
        extent.contains_rect(&Rect::from_ltrb(7.0, 17.0, 93.0, 23.0)),
        "the line's 6 px body is covered: {extent:?}"
    );

    let points = flui_painting::testing::record(|canvas| {
        canvas.draw_points_with_mode(PointMode::Points, vec![Point::new(50.0, 50.0)], &paint);
    });
    let extent = bounded(&points);
    assert!(
        extent.contains_rect(&Rect::from_ltrb(47.0, 47.0, 53.0, 53.0)),
        "the point's 3 px radius is covered: {extent:?}"
    );
}

/// An atlas sprite's extent is where the renderer lands it, the sprite's size
/// at its transform's translation, not the source rect mapped by the
/// transform (which is where the sprite sits in the image).
#[test]
fn atlas_extent_covers_the_sprite_destination() {
    let image = flui_painting::paint::Image::solid_color(64, 64, Color::RED);
    let list = flui_painting::testing::record(|canvas| {
        canvas.draw_atlas(
            image,
            vec![Rect::from_ltrb(50.0, 50.0, 60.0, 60.0)],
            vec![Matrix4::translation(100.0, 100.0, 0.0)],
            None,
            flui_painting::paint::BlendMode::SrcOver,
            None,
        );
    });
    let extent = bounded(&list);
    assert!(
        extent.contains_rect(&Rect::from_ltrb(100.0, 100.0, 110.0, 110.0)),
        "the sprite lands at the translation: {extent:?}"
    );
}
