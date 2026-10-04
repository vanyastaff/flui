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
use flui_painting::{GlyphContent, GlyphImage, GlyphRasterizer};
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
                        image.width,
                        image.height,
                        image.left,
                        image.top,
                        image.content == GlyphContent::Color,
                        fnv1a(&image.data),
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
        expected.width > 0 && expected.height > 0,
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
