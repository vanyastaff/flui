//! swash on Parley-shaped glyphs against what cosmic-text's scaler drew
//! (ADR-0092 §10 step 1).
//!
//! Parley shapes each sample on a test-local `FontContext` with no host
//! discovery, over one face the repository vendors, so the run is the same on
//! every host. Every distinct key is rasterized by [`SwashRasterizer`] and
//! compared with the reference: what cosmic-text 0.19's `SwashCache` drew for
//! the same face bytes, glyph, size and bin, recorded from it before it left
//! the workspace (`support/raster_recorded.rs`).

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use flui_painting::glyphs::{
    FaceKey, FontRegistry, GlyphKey, SubpixelBin, SwashRasterizer, Synthesis,
};
use flui_painting::{GlyphContent, GlyphImage, GlyphImageError, GlyphRasterizer};
use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::layout::PositionedLayoutItem;
use parley::style::{FontFamily, FontFamilyName, StyleProperty};
use parley::{FontContext, Layout, LayoutContext};

#[path = "support/raster_recorded.rs"]
mod raster_recorded;

const ROBOTO: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");
const MATERIAL_ICONS: &[u8] = include_bytes!("../assets/fonts/MaterialIcons-Regular.ttf");
const LATIN: &str = "The quick brown fox jumps over the lazy dog 0123456789";
const SIZES: [f32; 3] = [13.0, 18.0, 32.0];

/// A custom rasterizer cannot admit incomplete buffers or an overflowing byte
/// layout; every named input runs even after another row fails.
pub(crate) fn glyph_images_admit_only_complete_mask_and_color_buffers() {
    use glyph_image_admission as rows;
    super::cases::run_cases(
        "glyph_image_admission",
        &[
            ("complete_mask", rows::complete_mask),
            ("complete_color", rows::complete_color),
            ("zero_mask_width", rows::zero_mask_width),
            ("zero_mask_height", rows::zero_mask_height),
            ("zero_color_width", rows::zero_color_width),
            ("zero_color_height", rows::zero_color_height),
            ("both_color_axes_zero", rows::both_color_axes_zero),
            ("truncated_mask", rows::truncated_mask),
            ("excess_mask", rows::excess_mask),
            ("truncated_color", rows::truncated_color),
            ("excess_color", rows::excess_color),
            ("mask_bytes_as_color", rows::mask_bytes_as_color),
            ("color_bytes_as_mask", rows::color_bytes_as_mask),
            ("bytes_on_zero_color_width", rows::bytes_on_zero_color_width),
            ("bytes_on_zero_mask_height", rows::bytes_on_zero_mask_height),
            ("color_size_wraps_to_zero", rows::color_size_wraps_to_zero),
            ("maximum_color_size", rows::maximum_color_size),
            ("maximum_mask_size", rows::maximum_mask_size),
            (
                "healthy_image_after_refusals",
                rows::healthy_image_after_refusals,
            ),
        ],
    );
}

mod glyph_image_admission {
    use super::{GlyphContent, GlyphImage, GlyphImageError};

    fn admit(width: u32, height: u32, content: GlyphContent, data: Vec<u8>) {
        let image = GlyphImage::try_new(i32::MIN, i32::MAX, width, height, content, data.clone())
            .expect("complete row-major bytes are admitted");
        assert_eq!(
            (image.left(), image.top(), image.width(), image.height()),
            (i32::MIN, i32::MAX, width, height),
            "bearings and empty-axis dimensions must be preserved"
        );
        assert_eq!(image.content(), content);
        assert_eq!(image.data(), data);
    }

    fn reject_length(
        width: u32,
        height: u32,
        content: GlyphContent,
        bytes: usize,
        expected: usize,
    ) {
        assert_eq!(
            GlyphImage::try_new(0, 0, width, height, content, vec![255; bytes]),
            Err(GlyphImageError::InvalidDataLength {
                expected,
                actual: bytes
            }),
            "{width}x{height} {content:?} with {bytes} bytes"
        );
    }

    pub(super) fn complete_mask() {
        admit(2, 2, GlyphContent::Mask, vec![0, 64, 128, 255]);
    }

    pub(super) fn complete_color() {
        admit(
            2,
            1,
            GlyphContent::Color,
            vec![10, 20, 30, 40, 50, 60, 70, 80],
        );
    }

    pub(super) fn zero_mask_width() {
        admit(0, u32::MAX, GlyphContent::Mask, Vec::new());
    }

    pub(super) fn zero_mask_height() {
        admit(u32::MAX, 0, GlyphContent::Mask, Vec::new());
    }

    pub(super) fn zero_color_width() {
        admit(0, u32::MAX, GlyphContent::Color, Vec::new());
    }

    pub(super) fn zero_color_height() {
        admit(u32::MAX, 0, GlyphContent::Color, Vec::new());
    }

    pub(super) fn both_color_axes_zero() {
        admit(0, 0, GlyphContent::Color, Vec::new());
    }

    pub(super) fn truncated_mask() {
        reject_length(2, 2, GlyphContent::Mask, 3, 4);
    }

    pub(super) fn excess_mask() {
        reject_length(2, 2, GlyphContent::Mask, 5, 4);
    }

    pub(super) fn truncated_color() {
        reject_length(2, 1, GlyphContent::Color, 7, 8);
    }

    pub(super) fn excess_color() {
        reject_length(2, 1, GlyphContent::Color, 9, 8);
    }

    pub(super) fn mask_bytes_as_color() {
        reject_length(2, 1, GlyphContent::Color, 2, 8);
    }

    pub(super) fn color_bytes_as_mask() {
        reject_length(2, 1, GlyphContent::Mask, 8, 2);
    }

    pub(super) fn bytes_on_zero_color_width() {
        reject_length(0, u32::MAX, GlyphContent::Color, 1, 0);
    }

    pub(super) fn bytes_on_zero_mask_height() {
        reject_length(u32::MAX, 0, GlyphContent::Mask, 1, 0);
    }

    pub(super) fn color_size_wraps_to_zero() {
        assert_eq!(
            GlyphImage::try_new(0, 0, 1 << 31, 1 << 31, GlyphContent::Color, Vec::new()),
            Err(GlyphImageError::SizeOverflow),
            "overflow must not wrap the expected byte count to an empty buffer"
        );
    }

    pub(super) fn maximum_color_size() {
        assert_eq!(
            GlyphImage::try_new(0, 0, u32::MAX, u32::MAX, GlyphContent::Color, Vec::new()),
            Err(GlyphImageError::SizeOverflow),
            "RGBA byte count overflows on both 32-bit and 64-bit targets"
        );
    }

    pub(super) fn maximum_mask_size() {
        #[cfg(target_pointer_width = "32")]
        assert_eq!(
            GlyphImage::try_new(0, 0, u32::MAX, u32::MAX, GlyphContent::Mask, Vec::new()),
            Err(GlyphImageError::SizeOverflow),
            "mask texel count itself overflows a 32-bit target"
        );
        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            GlyphImage::try_new(0, 0, u32::MAX, u32::MAX, GlyphContent::Mask, Vec::new()),
            Err(GlyphImageError::InvalidDataLength {
                expected: 18_446_744_065_119_617_025,
                actual: 0,
            }),
            "the mask byte count is representable on a 64-bit target"
        );
    }

    pub(super) fn healthy_image_after_refusals() {
        admit(1, 1, GlyphContent::Mask, vec![255]);
    }
}
const ORIGINS: [f32; 4] = [0.0, 0.25, 0.5, 0.75];

/// A script the oracle compares, on the vendored face that covers it.
struct Sample {
    name: &'static str,
    face: &'static [u8],
    text: &'static str,
}

const SAMPLES: [Sample; 4] = [
    Sample {
        name: "latin",
        face: ROBOTO,
        text: LATIN,
    },
    Sample {
        name: "cyrillic",
        face: ROBOTO,
        text: "Съешь же ещё этих мягких французских булок",
    },
    Sample {
        name: "greek",
        face: ROBOTO,
        text: "Ξεσκεπάζω την ψυχοφθόρα βδελυγμία",
    },
    Sample {
        name: "icons",
        // Private-use codepoints: home, search, close, add, settings.
        face: MATERIAL_ICONS,
        text: "\u{e88a} \u{e8b6} \u{e5cd} \u{e145} \u{e8b8}",
    },
];

/// A test-local Parley context: fontique's shared collection, no host scan.
struct Shaper {
    font_cx: FontContext,
    layout_cx: LayoutContext<()>,
}

impl Shaper {
    fn new() -> Self {
        Self {
            font_cx: FontContext {
                collection: Collection::new(CollectionOptions {
                    shared: true,
                    system_fonts: false,
                }),
                source_cache: SourceCache::new_shared(),
            },
            layout_cx: LayoutContext::new(),
        }
    }

    /// Registers every face in `bytes`; returns the first family's name.
    fn register(&mut self, bytes: Vec<u8>) -> String {
        let families = self
            .font_cx
            .collection
            .register_fonts(Blob::new(Arc::new(bytes)), None);
        let (family, _) = families.first().expect("the bytes hold a face");
        self.font_cx
            .collection
            .family_name(*family)
            .expect("a registered family has a name")
            .to_owned()
    }

    fn layout(&mut self, text: &str, size: f32, family: &str) -> Layout<()> {
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(size));
        builder.push_default(StyleProperty::FontFamily(FontFamily::Single(
            FontFamilyName::Named(Cow::Owned(family.to_owned())),
        )));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(None);
        layout
    }
}

/// Shapes `text` and keys every glyph at `origin_x`, registering the faces
/// and interning the variation instances the keys name in `fonts`.
fn place(
    shaper: &mut Shaper,
    fonts: &mut FontRegistry,
    text: &str,
    size: f32,
    family: &str,
    origin_x: f32,
) -> Vec<GlyphKey> {
    let layout = shaper.layout(text, size, family);
    let mut keys = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let font = run.font();
            let face = FaceKey {
                blob_id: font.data.id(),
                index: font.index,
            };
            fonts
                .register_face(face, Arc::new(font.data.clone()))
                .expect("a face Parley shaped with is a face");
            let variation = fonts.intern_variation(run.normalized_coords());
            let synthesis = run.synthesis();
            #[expect(
                clippy::cast_possible_truncation,
                reason = "fontique's skew is whole degrees"
            )]
            let skew_degrees = synthesis.skew().map_or(0, |degrees| degrees as i8);
            for glyph in glyph_run.positioned_glyphs() {
                let (_, x_bin) = SubpixelBin::split(origin_x + glyph.x);
                keys.push(
                    GlyphKey::new(
                        face,
                        u16::try_from(glyph.id).expect("an OpenType glyph id"),
                        run.font_size(),
                        x_bin,
                    )
                    .with_variation(variation)
                    .with_synthesis(Synthesis {
                        embolden: synthesis.embolden(),
                        skew_degrees,
                    }),
                );
            }
        }
    }
    keys
}

/// The characters of `sample` (spaces aside) its face does not map.
fn unmapped(sample: &Sample) -> Vec<char> {
    let charmap = swash::FontRef::from_index(sample.face, 0)
        .expect("a vendored face")
        .charmap();
    sample
        .text
        .chars()
        .filter(|c| !c.is_whitespace() && charmap.map(*c) == 0)
        .collect()
}

fn latin_keys(shaper: &mut Shaper, fonts: &mut FontRegistry, family: &str) -> Vec<GlyphKey> {
    place(shaper, fonts, LATIN, 16.0, family, 0.3)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The recorded row for `key` in `sample`, if the recording has one.
fn recorded(sample: &str, key: GlyphKey) -> Option<&'static raster_recorded::Recorded> {
    let bin = (key.x_bin().offset() * 4.0).round();
    raster_recorded::RECORDED.iter().find(|row| {
        row.0 == sample
            && row.1 == key.glyph_id()
            && (row.2 - key.size()).abs() < 1e-3
            && f32::from(row.3) == bin
    })
}

/// swash on Parley's keys draws what cosmic-text's scaler drew, as recorded
/// (`support/raster_recorded.rs`): every distinct key of every sample, at
/// every size and bin, matches its row in placement, content kind and bytes,
/// and every row is drawn. Every sample runs on its vendored face alone, in a
/// collection of its own. Fails on a bitmap that moves by a pixel or one
/// coverage value, and on a sample that shapes other glyphs than it did.
pub(crate) fn swash_matches_the_recorded_reference() {
    let mut failures = Vec::new();
    let mut matched = 0;
    for sample in &SAMPLES {
        assert_eq!(
            unmapped(sample),
            Vec::<char>::new(),
            "{}: the vendored face maps the sample",
            sample.name
        );
        let mut shaper = Shaper::new();
        let family = shaper.register(sample.face.to_vec());
        let mut rasterizer = SwashRasterizer::new();
        let mut seen = HashSet::new();
        for size in SIZES {
            for origin in ORIGINS {
                let keys: Vec<GlyphKey> = place(
                    &mut shaper,
                    rasterizer.fonts_mut(),
                    sample.text,
                    size,
                    &family,
                    origin,
                )
                .into_iter()
                .filter(|key| seen.insert(*key))
                .collect();
                for key in keys {
                    let Some(row) = recorded(sample.name, key) else {
                        failures.push(format!("{}: no recorded row for {key:?}", sample.name));
                        continue;
                    };
                    let image = rasterizer.rasterize(key).expect("swash rasterizes");
                    let got = (
                        image.width(),
                        image.height(),
                        image.left(),
                        image.top(),
                        image.content() == GlyphContent::Color,
                        fnv1a(image.data()),
                    );
                    if got != (row.4, row.5, row.6, row.7, row.8, row.9) {
                        failures.push(format!(
                            "{}: {key:?} drew {got:?}, recorded {row:?}",
                            sample.name
                        ));
                    }
                    matched += 1;
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        matched,
        raster_recorded::RECORDED.len(),
        "every recorded key is shaped again"
    );
}

/// Rasterizing the same keys twice on one rasterizer, the second pass in
/// reverse order, draws the same bitmaps: the scaler carries nothing from
/// one glyph to the next that changes what it draws.
pub(crate) fn rasterizing_a_key_twice_draws_the_same_bitmap() {
    let mut shaper = Shaper::new();
    let family = shaper.register(ROBOTO.to_vec());
    let mut rasterizer = SwashRasterizer::new();
    let keys = latin_keys(&mut shaper, rasterizer.fonts_mut(), &family);
    let first: Vec<GlyphImage> = keys
        .iter()
        .map(|key| rasterizer.rasterize(*key).expect("swash rasterizes"))
        .collect();
    let again: Vec<GlyphImage> = keys
        .iter()
        .rev()
        .map(|key| rasterizer.rasterize(*key).expect("swash rasterizes"))
        .collect();
    assert!(first.len() > 1, "the sample shapes several keys");
    assert!(
        first.iter().eq(again.iter().rev()),
        "a key drew a different bitmap the second time"
    );
}

/// Cached glyphs remain drawable after the registered font source retires.
pub(crate) fn registered_fonts_release_the_source_and_keep_rasterizing() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Source {
        bytes: Vec<u8>,
        reads: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
    }

    impl AsRef<[u8]> for Source {
        fn as_ref(&self) -> &[u8] {
            self.reads.fetch_add(1, Ordering::Relaxed);
            &self.bytes
        }
    }

    impl Drop for Source {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    let face = FaceKey {
        blob_id: 1,
        index: 0,
    };
    let glyph = swash::FontRef::from_index(ROBOTO, 0)
        .expect("the vendored Roboto is a face")
        .charmap()
        .map('A');
    assert_ne!(glyph, 0, "Roboto covers the sample");
    let key = GlyphKey::new(face, glyph, 18.0, SubpixelBin::Zero);
    let mut reference = SwashRasterizer::new();
    reference
        .fonts_mut()
        .register_face(face, Arc::new(ROBOTO))
        .expect("the reference face registers");
    let expected = reference.rasterize(key).expect("the reference glyph draws");
    assert!(
        expected.width() > 0 && expected.height() > 0,
        "the sample has ink"
    );

    let reads = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let source = Arc::new(Source {
        bytes: ROBOTO.to_vec(),
        reads: Arc::clone(&reads),
        drops: Arc::clone(&drops),
    });
    let mut rasterizer = SwashRasterizer::with_owned_fonts();
    rasterizer
        .fonts_mut()
        .register_face(face, source.clone())
        .expect("the disposable source registers");
    drop(source);
    reads.store(0, Ordering::Relaxed);

    // This first rasterization must read the owned font, not a cached bitmap.
    let actual = rasterizer
        .rasterize(key)
        .expect("the glyph survives source retirement");
    assert_eq!(actual, expected, "retirement must not change the glyph");
    assert_eq!(
        reads.load(Ordering::Relaxed),
        0,
        "cached glyph drawing must not call the source"
    );
    assert_eq!(
        drops.load(Ordering::Relaxed),
        1,
        "the registry must release its source"
    );
}

fn split_maximum_float_saturates() {
    assert_eq!(SubpixelBin::split(f32::MAX), (i32::MAX, SubpixelBin::Zero));
}
fn split_minimum_float_saturates() {
    assert_eq!(SubpixelBin::split(f32::MIN), (i32::MIN, SubpixelBin::Zero));
}
fn split_positive_infinity_saturates() {
    assert_eq!(
        SubpixelBin::split(f32::INFINITY),
        (i32::MAX, SubpixelBin::Zero)
    );
}
fn split_negative_infinity_saturates() {
    assert_eq!(
        SubpixelBin::split(f32::NEG_INFINITY),
        (i32::MIN, SubpixelBin::Zero)
    );
}
fn split_nan_keeps_the_documented_zero() {
    assert_eq!(SubpixelBin::split(f32::NAN), (0, SubpixelBin::Zero));
}
fn split_quarter_pixel_edges_keep_their_bins() {
    assert_eq!(SubpixelBin::split(0.125), (0, SubpixelBin::One));
    assert_eq!(SubpixelBin::split(0.375), (0, SubpixelBin::Two));
    assert_eq!(SubpixelBin::split(0.625), (0, SubpixelBin::Three));
    assert_eq!(SubpixelBin::split(0.875), (1, SubpixelBin::Zero));
    assert_eq!(SubpixelBin::split(-0.125), (-1, SubpixelBin::Three));
    assert_eq!(SubpixelBin::split(-0.375), (-1, SubpixelBin::Two));
    assert_eq!(SubpixelBin::split(-0.625), (-1, SubpixelBin::One));
    assert_eq!(SubpixelBin::split(-0.875), (-1, SubpixelBin::Zero));
}

pub(crate) fn subpixel_split_is_total_across_the_float_domain() {
    crate::cases::run_cases(
        "subpixel_split",
        &[
            ("maximum finite", split_maximum_float_saturates),
            ("minimum finite", split_minimum_float_saturates),
            ("positive infinity", split_positive_infinity_saturates),
            ("negative infinity", split_negative_infinity_saturates),
            ("NaN", split_nan_keeps_the_documented_zero),
            (
                "quarter pixel edges",
                split_quarter_pixel_edges_keep_their_bins,
            ),
        ],
    );
}

fn placement_paragraph(
    style: flui_painting::typography::TextStyle,
) -> (Arc<flui_painting::ShapedParagraph>, f64) {
    use flui_painting::typography::{TextDirection, TextSpan};
    use flui_painting::{Canvas, DrawOp, FontCollection, TextBaseline, TextContext, TextPainter};
    let mut context = TextContext::new(&FontCollection::new());
    let mut painter = TextPainter::new()
        .with_text(TextSpan::styled("AA", style))
        .with_text_direction(TextDirection::Ltr);
    painter
        .layout(&mut context, 0.0, f64::INFINITY)
        .expect("valid fixture lays out");
    let baseline = painter.compute_distance_to_actual_baseline(TextBaseline::Alphabetic);
    let mut canvas = Canvas::new();
    painter.paint(&mut canvas, flui_foundation::geometry::Offset::ZERO);
    let list = canvas.finish();
    let paragraph = list
        .iter()
        .find_map(|command| match &command.op {
            DrawOp::Paragraph { paragraph, .. } => Some(Arc::clone(paragraph)),
            _ => None,
        })
        .expect("public paint records the shaped paragraph");
    assert_eq!(
        paragraph.runs().count(),
        1,
        "fixture is one bundled-font run"
    );
    (paragraph, baseline)
}

fn place_paragraph(
    paragraph: &flui_painting::ShapedParagraph,
    origin: (f32, f32),
    scale: f32,
) -> Vec<flui_painting::glyphs::PlacedGlyph> {
    let run = paragraph
        .runs()
        .next()
        .expect("the public paragraph has a run");
    let mut fonts = FontRegistry::new();
    let key = fonts.prepare_run(&run).expect("the shaped face registers");
    run.placed_glyphs(key, origin, scale).collect()
}

fn assert_origin_is_omitted(origin: (f32, f32)) {
    let (paragraph, _) = placement_paragraph(flui_painting::typography::TextStyle::default());
    let healthy = place_paragraph(&paragraph, (0.0, 0.0), 1.0);
    assert_eq!(healthy.len(), 2);
    assert!(
        place_paragraph(&paragraph, origin, 1.0).is_empty(),
        "invalid origin {origin:?} reached placement"
    );
    assert_eq!(
        place_paragraph(&paragraph, (0.0, 0.0), 1.0),
        healthy,
        "next ordinary placement changed"
    );
}
fn placed_maximum_horizontal_origin_is_omitted() {
    assert_origin_is_omitted((f32::MAX, 0.0));
}
fn placed_minimum_horizontal_origin_is_omitted() {
    assert_origin_is_omitted((f32::MIN, 0.0));
}
fn placed_positive_infinite_origin_is_omitted() {
    assert_origin_is_omitted((f32::INFINITY, 0.0));
}
fn placed_negative_infinite_origin_is_omitted() {
    assert_origin_is_omitted((0.0, f32::NEG_INFINITY));
}
fn placed_nan_horizontal_origin_is_omitted() {
    assert_origin_is_omitted((f32::NAN, 0.0));
}
fn placed_nan_vertical_origin_is_omitted() {
    assert_origin_is_omitted((0.0, f32::NAN));
}
fn placed_maximum_vertical_origin_is_omitted() {
    assert_origin_is_omitted((0.0, f32::MAX));
}
fn placed_minimum_vertical_origin_is_omitted() {
    assert_origin_is_omitted((0.0, f32::MIN));
}
fn placed_exact_upper_integer_boundary_is_omitted() {
    // This IEEE f32 value is exactly 2^31, one above i32::MAX.
    assert_origin_is_omitted((f32::from_bits(0x4f00_0000), 0.0));
}
fn placed_final_vertical_row_overflow_is_omitted() {
    let (paragraph, _) = placement_paragraph(flui_painting::typography::TextStyle::default());
    let healthy = place_paragraph(&paragraph, (0.0, 0.0), 16.0);
    assert_eq!(healthy.len(), 2);
    assert!(
        healthy[0].y > 127,
        "fixture baseline must overflow after the largest in-range f32 offset"
    );
    assert_eq!(
        place_paragraph(&paragraph, (0.0, f32::from_bits(0x4eff_ffff)), 16.0),
        Vec::new()
    );
    assert_eq!(place_paragraph(&paragraph, (0.0, 0.0), 16.0), healthy);
}

pub(crate) fn placed_glyphs_omit_unrepresentable_coordinates() {
    crate::cases::run_cases(
        "glyph_coordinate_admission",
        &[
            (
                "maximum horizontal",
                placed_maximum_horizontal_origin_is_omitted,
            ),
            (
                "minimum horizontal",
                placed_minimum_horizontal_origin_is_omitted,
            ),
            (
                "positive infinity",
                placed_positive_infinite_origin_is_omitted,
            ),
            (
                "negative infinity",
                placed_negative_infinite_origin_is_omitted,
            ),
            ("horizontal NaN", placed_nan_horizontal_origin_is_omitted),
            ("vertical NaN", placed_nan_vertical_origin_is_omitted),
            (
                "maximum vertical",
                placed_maximum_vertical_origin_is_omitted,
            ),
            (
                "minimum vertical",
                placed_minimum_vertical_origin_is_omitted,
            ),
            (
                "exact upper boundary",
                placed_exact_upper_integer_boundary_is_omitted,
            ),
            (
                "final vertical row overflow",
                placed_final_vertical_row_overflow_is_omitted,
            ),
        ],
    );
}

pub(crate) fn placed_glyphs_keep_representable_extremes_and_hinting() {
    let (paragraph, _) = placement_paragraph(flui_painting::typography::TextStyle::default());
    let healthy = place_paragraph(&paragraph, (0.0, 0.0), 1.0);
    assert_eq!(healthy.len(), 2);
    let low = place_paragraph(&paragraph, (-f32::from_bits(0x4f00_0000), 0.0), 1.0);
    assert_eq!(low.len(), 2);
    assert_eq!(low[0].x, i32::MIN);
    let high = place_paragraph(&paragraph, (f32::from_bits(0x4eff_ffff), 0.0), 1.0);
    assert_eq!(high.len(), 2);
    assert_eq!(high[0].x, 2_147_483_520);
    let fractional = place_paragraph(&paragraph, (0.125, 1.75), 1.0);
    assert_eq!(fractional[0].x, healthy[0].x);
    assert_eq!(fractional[0].key.x_bin(), SubpixelBin::One);
    assert_eq!(fractional[0].y, healthy[0].y + 1);
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the public baseline is a widened f32 layout value, narrowed back to its native precision"
)]
pub(crate) fn placed_glyphs_keep_cancelling_vertical_coordinates() {
    let (paragraph, baseline) = placement_paragraph(
        flui_painting::typography::TextStyle::default().with_height(4_294_967_296.0),
    );
    assert!(
        baseline > f64::from(i32::MAX),
        "fixture baseline must exceed the device integer range"
    );
    let origin_y = -(baseline as f32).round();
    let placed = place_paragraph(&paragraph, (0.0, origin_y), 1.0);
    assert_eq!(
        placed.len(),
        2,
        "valid cancellation must not discard the run"
    );
    assert_eq!(placed[0].y, 0);
    assert_eq!(placed[1].y, 0);
}
