//! The engine rasterises the paragraph it is handed and never shapes one
//! itself (ADR-0065).

use flui_foundation::geometry::Offset;
use flui_layer::SceneBuilder;
use flui_painting::{Canvas, TextPainter};
use flui_painting::{
    styling::Color,
    typography::{TextDirection, TextSpan, TextStyle},
};

const SIDE: u32 = 96;

fn sample(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let index = ((y * SIDE + x) * 4) as usize;
    [
        pixels[index],
        pixels[index + 1],
        pixels[index + 2],
        pixels[index + 3],
    ]
}

/// The engine neither shapes nor names the shaper: none of cosmic-text's
/// entry points — nor the crate itself — appears in the text path's source.
/// Every glyph it draws came from a `TextLayout` the recorder shaped and
/// measured, placed through `placed_glyphs` and rasterised through
/// `SharedFontSystem::rasterize` (ADR-0065, ADR-0067). The manifest is the
/// other half of the guard: `cosmic-text` is not a dependency of this crate.
fn the_engine_does_not_shape() {
    let sources = [
        ("glyph_atlas.rs", include_str!("glyph_atlas.rs")),
        ("batches/text.rs", include_str!("batches/text.rs")),
        ("layer_dispatcher.rs", include_str!("layer_dispatcher.rs")),
        ("dispatch.rs", include_str!("dispatch.rs")),
        ("painter/draw.rs", include_str!("painter/draw.rs")),
        ("painter/mod.rs", include_str!("painter/mod.rs")),
        ("renderer.rs", include_str!("renderer.rs")),
        ("headless.rs", include_str!("headless.rs")),
    ];
    for (name, source) in sources {
        for needle in [
            "shape_until_scroll",
            "set_rich_text",
            ".set_text(",
            "cosmic_text",
        ] {
            assert!(
                !source.contains(needle),
                "{name} reaches the shaper (`{needle}`); shaping belongs to flui-painting"
            );
        }
    }
    assert!(
        !include_str!("../Cargo.toml").contains("cosmic-text"),
        "flui-engine must not depend on cosmic-text"
    );
}

/// Shapes `text` at `size` and returns a scene of it painted in `color` at
/// the origin, wrapped by `decorate` (a clip, a layer) around the paint.
fn paragraph_scene(
    text: &str,
    size: f64,
    color: Color,
    decorate: impl FnOnce(&mut Canvas, &mut dyn FnMut(&mut Canvas)),
) -> flui_layer::LayerTree {
    let style = TextStyle::new().with_font_size(size).with_color(color);
    let mut painter = TextPainter::new()
        .with_text(TextSpan::new(text).with_style(style))
        .with_text_direction(TextDirection::Ltr);
    painter.layout(
        &mut flui_painting::TextContext::new(&flui_painting::FontCollection::new()),
        0.0,
        f64::from(SIDE as f32 * 4.0),
    );
    let mut canvas = Canvas::new();
    decorate(&mut canvas, &mut |canvas| {
        painter.paint(canvas, Offset::ZERO);
    });
    let mut builder = SceneBuilder::new();
    builder.add_picture(canvas.finish());
    builder.build()
}

/// A glyph's colour lands as recorded: a mid-grey paragraph on white
/// reaches the target as that grey where the glyph fully covers a pixel.
///
/// The engine draws in gamma space onto a `Unorm` target (see
/// `Renderer::select_surface_format`); a rasteriser that converted the
/// text colour to linear before writing — as the previous one did — turned
/// `#808080` into `#373737` on every mid-tone label while black and white
/// text, being fixed points of the transfer, looked right.
fn glyph_colour_lands_as_recorded() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let grey = Color::rgb(128, 128, 128);
    let tree = paragraph_scene("I", 80.0, grey, |canvas, paint| paint(canvas));
    let pixels = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect("the headless capture path must rasterize a paragraph");

    let inked: Vec<[u8; 4]> = (0..SIDE)
        .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
        .map(|(x, y)| sample(&pixels, x, y))
        .filter(|p| *p != [255, 255, 255, 255])
        .collect();
    assert!(!inked.is_empty(), "the glyph must paint");
    let darkest = inked.iter().map(|p| p[0]).min().unwrap();
    assert!(
        (127..=129).contains(&darkest),
        "a fully covered pixel of a #808080 glyph must read back as #808080, \
         darkest channel found {darkest} (a linear conversion would give ~55)"
    );
}

/// Paragraph contract, read back from the GPU: glyph colour lands as recorded,
/// and the engine does not shape.
#[test]
fn paragraphs_read_back_as_recorded() {
    glyph_colour_lands_as_recorded();
    the_engine_does_not_shape();
}
