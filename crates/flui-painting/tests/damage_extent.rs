//! Damage-extent contracts of `DisplayList`: how far each recorded op can
//! change pixels, which the frame damage producer unions into the region it
//! repaints. Every extent must cover what the renderer actually writes, so a
//! partial repaint never leaves part of an op stale.

use flui_foundation::geometry::{Matrix4, Rect};
use flui_painting::styling::Color;
use flui_painting::{Canvas, DisplayList, DrawOp, Paint};

/// The rect covering a bounded damage extent in the list's own space.
fn bounded(list: &DisplayList) -> Rect<f64> {
    list.damage_extent()
        .and_then(flui_painting::DamageExtent::covering_rect)
        .unwrap_or_else(|| panic!("expected a bounded damage extent, got {list:?}"))
}

/// A full-canvas fill has no `bounds()` (it is not a layout box), but it
/// changes every pixel its clip allows, so a damage region built from the
/// layout answer would leave the rest of the fill stale.
pub(crate) fn a_color_fill_makes_the_extent_unbounded() {
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
pub(crate) fn paragraph_extent_covers_every_rasterized_glyph() {
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
        painter.layout(
            &mut flui_painting::TextContext::new(&flui_painting::FontCollection::new()),
            0.0,
            f64::INFINITY,
        );
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
                "{text:?} (line height {line_height:?}): glyph ink {ink:?} escapes the \
                 damage extent {extent:?}"
            );
        }
        assert!(glyphs > 0, "precondition: {text:?} rasterizes glyphs");
        if line_height.is_some() {
            assert!(
                past_box,
                "precondition: under a tight line height the ink leaves the layout box \
                 {layout_box:?}"
            );
        }
    }
}

/// A stroke's miter reaches a full width past the geometry (the layout box
/// adds half), and a shadow reaches 3.5 elevations: three blur sigmas of one
/// elevation each past a copy offset half an elevation down, the reach of the
/// GPU's analytic shadow (the layout box adds one elevation).
pub(crate) fn stroke_and_shadow_extents_cover_their_outsets() {
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
pub(crate) fn shadow_extent_spreads_by_the_largest_scale_on_both_axes() {
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
pub(crate) fn fill_style_lines_and_points_reach_their_stroke_width() {
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
pub(crate) fn atlas_extent_covers_the_sprite_destination() {
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

/// Which blends change a destination their source leaves transparent, and
/// which colour filters paint a transparent pixel: the two answers a damage
/// producer widens a layer's composite by.
pub(crate) fn transparent_source_and_transparent_black_predicates() {
    use flui_painting::paint::{BlendMode, ColorFilter};

    let changing = [
        BlendMode::Clear,
        BlendMode::Src,
        BlendMode::SrcIn,
        BlendMode::DstIn,
        BlendMode::SrcOut,
        BlendMode::DstATop,
        BlendMode::Modulate,
    ];
    for mode in [
        BlendMode::SrcOver,
        BlendMode::Dst,
        BlendMode::DstOver,
        BlendMode::DstOut,
        BlendMode::SrcATop,
        BlendMode::Xor,
        BlendMode::Plus,
        BlendMode::Screen,
        BlendMode::Multiply,
    ] {
        assert!(
            mode.keeps_destination_under_transparent_source(),
            "{mode:?}"
        );
    }
    for mode in changing {
        assert!(
            !mode.keeps_destination_under_transparent_source(),
            "{mode:?}"
        );
    }

    let mode = |blend_mode| ColorFilter::Mode {
        color: Color::BLUE,
        blend_mode,
    };
    assert!(mode(BlendMode::Src).modifies_transparent_black());
    assert!(mode(BlendMode::SrcOver).modifies_transparent_black());
    assert!(!mode(BlendMode::SrcIn).modifies_transparent_black());
    assert!(!mode(BlendMode::Modulate).modifies_transparent_black());
    assert!(
        !ColorFilter::Mode {
            color: Color::TRANSPARENT,
            blend_mode: BlendMode::Src,
        }
        .modifies_transparent_black()
    );
    assert!(!ColorFilter::grayscale().modifies_transparent_black());
    let mut offset = [0.0_f32; 20];
    offset[19] = 1.0;
    assert!(ColorFilter::matrix(offset).modifies_transparent_black());
    assert!(!ColorFilter::linear_to_srgb_gamma().modifies_transparent_black());

    // An image filter answers as the colour matrix it applies, through a
    // composition too; a blur or a morphology keeps transparent transparent.
    use flui_painting::paint::ImageFilter;
    use flui_painting::paint::effects::{ColorAdjustment, ColorMatrix};
    let visible = ColorMatrix::new(offset);
    let mut colour_only = ColorMatrix::identity();
    colour_only.values[4] = 1.0;
    assert!(ImageFilter::Matrix(visible).modifies_transparent_black());
    assert!(!ImageFilter::Matrix(colour_only).modifies_transparent_black());
    assert!(
        ImageFilter::ColorAdjust(ColorAdjustment::Matrix(visible)).modifies_transparent_black()
    );
    assert!(
        !ImageFilter::ColorAdjust(ColorAdjustment::Brightness(0.5)).modifies_transparent_black()
    );
    assert!(
        ImageFilter::Compose(vec![ImageFilter::blur(2.0), ImageFilter::Matrix(visible)])
            .modifies_transparent_black()
    );
    for filter in [
        ImageFilter::blur(2.0),
        ImageFilter::dilate(2.0),
        ImageFilter::erode(2.0),
        ImageFilter::Compose(vec![ImageFilter::blur(2.0), ImageFilter::dilate(1.0)]),
    ] {
        assert!(!filter.modifies_transparent_black(), "{filter:?}");
    }
}
