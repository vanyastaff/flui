//! The icon face this package brings into an application (ADR-0397).

use flui_painting::parley_text::ParagraphSpec;
use flui_painting::typography::{TextAlign, TextStyle};
use flui_painting::{FontCollection, TextContext};
use flui_sdk::foundation::TextDirection;

/// The glyph index `text` shapes to in `family` on a new collection.
fn glyph_in(family: &str, text: &str) -> u16 {
    let fonts = FontCollection::new();
    let style = TextStyle::new().with_font_family(family);
    let spans = [(text.to_owned(), None)];
    let paragraph = TextContext::new(&fonts)
        .shape(&ParagraphSpec {
            font_weight_adjustment: 0,
            spans: &spans,
            default_style: Some(&style),
            font_size: 24.0,
            max_width: None,
            min_width: 0.0,
            text_align: TextAlign::Start,
            line_height: None,
            direction: TextDirection::Ltr,
            max_lines: None,
            ellipsis: None,
        })
        .expect("one codepoint shapes")
        .to_shaped(None);
    let glyphs: Vec<u16> = paragraph
        .runs()
        .flat_map(|run| run.glyphs().iter().map(|glyph| glyph.id))
        .collect();
    assert_eq!(glyphs.len(), 1, "one codepoint is one glyph: {glyphs:?}");
    glyphs[0]
}

/// A build that depends on this package holds the Cupertino Icons face: a codepoint of
/// the family shapes to its glyph and not to `.notdef`, which is what it
/// shapes to once the package's manifest stops turning on `flui-sdk`'s
/// `cupertino-icons` feature.
#[test]
fn an_icon_of_the_family_shapes_to_a_glyph_of_the_embedded_face() {
    assert_ne!(
        glyph_in("CupertinoIcons", "\u{F35D}"),
        0,
        "U+F35D has a glyph in CupertinoIcons"
    );
}
