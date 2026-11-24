//! The icon face this package brings into an application (ADR-0397), and the
//! glyphs of it the package's own widgets draw.

use flui_material::{BackButton, Chip, FilterChip, Theme, ThemeData};
use flui_painting::parley_text::ParagraphSpec;
use flui_painting::typography::{TextAlign, TextStyle};
use flui_painting::{DrawOp, FontCollection, TextContext};
use flui_sdk::foundation::TextDirection;
use flui_sdk::view::{BoxedView, ViewExt};
use flui_sdk::widgets::Text;

use crate::common::{lay_out, loose};

/// The glyph indexes `text` shapes to in Material Icons on a new collection.
fn glyphs_of(text: &str) -> Vec<u16> {
    let fonts = FontCollection::new();
    let style = TextStyle::new().with_font_family("Material Icons");
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
        .expect("the text shapes")
        .to_shaped(None);
    paragraph
        .runs()
        .flat_map(|run| run.glyphs().iter().map(|glyph| glyph.id))
        .collect()
}

/// A build that depends on this package holds the Material Icons face: a
/// codepoint of the family shapes to its glyph and not to `.notdef`. A
/// workspace build unifies the feature on from other members, so whether the
/// package's own manifest turns it on is held by the facade's
/// `an_icon_face_reaches_a_consumer_only_with_its_catalog_or_its_feature`.
#[test]
fn an_icon_of_the_family_shapes_to_a_glyph_of_the_embedded_face() {
    let home = glyphs_of("\u{E88A}");
    assert_eq!(home.len(), 1, "one codepoint is one glyph: {home:?}");
    assert_ne!(home[0], 0, "U+E88A has a glyph in Material Icons");
}

/// Each icon a widget of this package draws by itself is the glyph the face
/// names so: the face's own ligature of the icon's name gives the expected
/// glyph, so a codepoint taken from another numbering of the icon set (it
/// then draws `.notdef` or another icon) fails its row.
#[test]
fn a_widget_draws_the_glyph_the_face_names_for_its_icon() {
    let back_button = || -> BoxedView { BackButton::new().on_pressed(|_ctx| {}).boxed() };
    let chip = || -> BoxedView { Chip::new(Text::new("Tag")).on_deleted(|_ctx| {}).boxed() };
    let filter_chip = || -> BoxedView {
        FilterChip::new(Text::new("Tag"))
            .on_deleted(|_ctx| {})
            .boxed()
    };
    // The label's own glyphs come from the text face; an icon is the one
    // paragraph of a single glyph.
    for (widget, icon_name, build) in [
        (
            "BackButton",
            "arrow_back",
            &back_button as &dyn Fn() -> BoxedView,
        ),
        ("Chip", "cancel", &chip),
        ("FilterChip", "clear", &filter_chip),
    ] {
        let named = glyphs_of(icon_name);
        assert_eq!(
            named.len(),
            1,
            "the face has a ligature for `{icon_name}`: {named:?}"
        );
        assert_ne!(named[0], 0, "`{icon_name}` is a glyph of the face");

        let laid = lay_out(Theme::new(ThemeData::light(), build()), loose(300.0));
        let single_glyphs: Vec<u16> = laid
            .draw_ops()
            .into_iter()
            .filter_map(|command| match command.op {
                DrawOp::Paragraph { paragraph, .. } => {
                    let glyphs: Vec<u16> = paragraph
                        .runs()
                        .flat_map(|run| run.glyphs().iter().map(|glyph| glyph.id))
                        .collect();
                    (glyphs.len() == 1).then(|| glyphs[0])
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            single_glyphs,
            [named[0]],
            "{widget} draws the face's `{icon_name}`"
        );
    }
}
