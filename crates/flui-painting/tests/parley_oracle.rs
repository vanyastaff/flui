//! swash on Parley-shaped glyphs against the scaler FLUI draws with today
//! (ADR-0092 §10 step 1).
//!
//! Parley shapes each sample on a test-local `FontContext` with no host
//! discovery, over one face the repository vendors, so the run is the same on
//! every host. Every distinct key is rasterized by [`SwashRasterizer`] and by
//! the reference: cosmic-text's `SwashCache`, as `SharedFontSystem::rasterize`
//! drives it, on a font system local to the test and fed the same face bytes,
//! glyph, size and bin. Nothing here touches the process-wide font system.

#![allow(clippy::unwrap_used, clippy::print_stdout, reason = "test")]

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cosmic_text::{CacheKey, CacheKeyFlags, FontSystem, SwashCache, SwashContent, fontdb};
use flui_painting::glyphs::{
    FaceKey, FontRegistry, GlyphKey, SubpixelBin, SwashRasterizer, Synthesis,
};
use flui_painting::{GlyphContent, GlyphImage, GlyphRasterizer};
use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::layout::PositionedLayoutItem;
use parley::style::{FontFamily, FontFamilyName, StyleProperty};
use parley::{FontContext, FontData, Layout, LayoutContext};

#[path = "support/cases.rs"]
mod cases;
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

/// Scripts no vendored face covers, and so the oracle does not compare.
/// Host fonts would cover some of them, but make the result depend on what
/// the host ships.
const UNCOVERED: [(&str, &str); 4] = [
    ("arabic", "no vendored face carries Arabic"),
    ("devanagari", "no vendored face carries Devanagari"),
    ("hebrew", "no vendored face carries Hebrew"),
    ("emoji", "no vendored face carries colour glyphs"),
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

/// One placed glyph: its key, and what the reference needs to draw it.
#[derive(Clone)]
struct Placed {
    key: GlyphKey,
    font: FontData,
    coords: Vec<i16>,
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
) -> Vec<Placed> {
    let layout = shaper.layout(text, size, family);
    let mut placed = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let font = run.font().clone();
            let face = FaceKey {
                blob_id: font.data.id(),
                index: font.index,
            };
            fonts
                .register_face(face, Arc::new(font.data.clone()))
                .expect("a face Parley shaped with is a face");
            let coords = run.normalized_coords().to_vec();
            let variation = fonts.intern_variation(&coords);
            let synthesis = run.synthesis();
            #[expect(
                clippy::cast_possible_truncation,
                reason = "fontique's skew is whole degrees"
            )]
            let skew_degrees = synthesis.skew().map_or(0, |degrees| degrees as i8);
            for glyph in glyph_run.positioned_glyphs() {
                let (_, x_bin) = SubpixelBin::split(origin_x + glyph.x);
                let key = GlyphKey::new(
                    face,
                    u16::try_from(glyph.id).expect("an OpenType glyph id"),
                    run.font_size(),
                    x_bin,
                )
                .with_variation(variation)
                .with_synthesis(Synthesis {
                    embolden: synthesis.embolden(),
                    skew_degrees,
                });
                placed.push(Placed {
                    key,
                    font: font.clone(),
                    coords: coords.clone(),
                });
            }
        }
    }
    placed
}

/// The cosmic-text path's scaler on a font system local to the test.
struct Reference {
    system: FontSystem,
    cache: SwashCache,
    ids: HashMap<FaceKey, fontdb::ID>,
}

/// Why the reference cannot draw a glyph the way the key asks.
#[derive(Debug, PartialEq, Eq, Hash)]
enum Skip {
    /// The face is variable and Parley chose coordinates cosmic-text would not.
    Coordinates,
    /// Synthetic bold: cosmic-text has none.
    Bold,
}

impl Reference {
    fn new() -> Self {
        Self {
            system: FontSystem::new_with_locale_and_db("en-US".into(), fontdb::Database::new()),
            cache: SwashCache::new(),
            ids: HashMap::new(),
        }
    }

    fn image(&mut self, placed: &Placed) -> Result<GlyphImage, Skip> {
        let key = placed.key;
        if key.synthesis().embolden {
            return Err(Skip::Bold);
        }
        if cosmic_coords(&placed.font) != padded(&placed.coords, &placed.font) {
            return Err(Skip::Coordinates);
        }
        let face = key.face();
        let id = *self.ids.entry(face).or_insert_with(|| {
            let ids = self
                .system
                .db_mut()
                .load_font_source(fontdb::Source::Binary(Arc::new(
                    placed.font.data.data().to_vec(),
                )));
            ids[usize::try_from(face.index).unwrap()]
        });
        let flags = match key.synthesis().skew_degrees {
            0 => CacheKeyFlags::empty(),
            14 => CacheKeyFlags::FAKE_ITALIC,
            other => panic!("cosmic-text skews by 14 degrees only, not {other}"),
        };
        let flags = if key.hinted() {
            flags
        } else {
            flags | CacheKeyFlags::DISABLE_HINTING
        };
        let (cache_key, _, _) = CacheKey::new(
            id,
            key.glyph_id(),
            key.size(),
            (key.x_bin().offset(), 0.0),
            fontdb::Weight(400),
            flags,
        );
        let image = self
            .cache
            .get_image_uncached(&mut self.system, cache_key)
            .unwrap_or_else(|| panic!("the reference rasterizes {key:?}"));
        Ok(GlyphImage {
            left: image.placement.left,
            top: image.placement.top,
            width: image.placement.width,
            height: image.placement.height,
            content: match image.content {
                SwashContent::Color => GlyphContent::Color,
                SwashContent::Mask | SwashContent::SubpixelMask => GlyphContent::Mask,
            },
            data: image.data,
        })
    }
}

fn swash_face(font: &FontData) -> swash::FontRef<'_> {
    swash::FontRef::from_index(font.data.data(), usize::try_from(font.index).unwrap())
        .expect("a face")
}

/// The coordinates cosmic-text 0.19 draws a face at for weight 400: the
/// `wght` axis set, every other axis at its default.
fn cosmic_coords(font: &FontData) -> Vec<i16> {
    let face = swash_face(font);
    let variations = face.variations();
    let wght = u32::from_be_bytes(*b"wght");
    match variations.find_by_tag(wght) {
        Some(axis) => padded(
            &variations
                .normalized_coords([(wght, 400.0_f32.clamp(axis.min_value(), axis.max_value()))])
                .collect::<Vec<_>>(),
            font,
        ),
        None => padded(&[], font),
    }
}

/// `coords` extended with default (zero) coordinates to one per axis.
fn padded(coords: &[i16], font: &FontData) -> Vec<i16> {
    let axes = swash_face(font).variations().count();
    let mut out = coords.to_vec();
    out.resize(axes.max(coords.len()), 0);
    out
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

#[derive(Default)]
struct Report {
    unique: usize,
    swash_exact: usize,
    skipped: HashMap<String, usize>,
}

/// Every distinct key of `text` at every size and bin, through both scalers.
fn compare(shaper: &mut Shaper, family: &str, text: &str) -> Report {
    let mut rasterizer = SwashRasterizer::new();
    let mut reference = Reference::new();
    let mut report = Report::default();
    let mut seen = HashSet::new();
    for size in SIZES {
        for origin in ORIGINS {
            for placed in place(shaper, rasterizer.fonts_mut(), text, size, family, origin) {
                if !seen.insert(placed.key) {
                    continue;
                }
                let expected = match reference.image(&placed) {
                    Ok(image) => image,
                    Err(skip) => {
                        *report.skipped.entry(format!("{skip:?}")).or_default() += 1;
                        continue;
                    }
                };
                report.unique += 1;
                let ours = rasterizer.rasterize(placed.key).expect("swash rasterizes");
                if ours == expected {
                    report.swash_exact += 1;
                } else {
                    println!("differs: {:?}", placed.key);
                }
            }
        }
    }
    report
}

/// swash on Parley's keys draws exactly what cosmic-text's scaler draws.
///
/// Every sample runs on its vendored face alone, in a collection of its own,
/// so no host font and no other sample's face can take part in shaping.
/// Scripts without a vendored face are named as skipped, not compared.
fn swash_matches_cosmic_text_bit_for_bit() {
    for (name, reason) in UNCOVERED {
        println!("{name}: skipped, {reason}");
    }
    println!(
        "{:<11} {:>7} {:>12}  skipped",
        "script", "unique", "swash_exact"
    );
    for sample in &SAMPLES {
        let name = sample.name;
        assert_eq!(
            unmapped(sample),
            Vec::<char>::new(),
            "{name}: the vendored face maps the sample"
        );
        let mut shaper = Shaper::new();
        let family = shaper.register(sample.face.to_vec());
        let report = compare(&mut shaper, &family, sample.text);
        println!(
            "{:<11} {:>7} {:>12}  {:?}",
            name,
            report.unique,
            format!("{}/{}", report.swash_exact, report.unique),
            report.skipped
        );
        assert!(report.unique > 0, "{name}: something was compared");
        assert_eq!(
            report.swash_exact, report.unique,
            "{name}: swash is bit-identical"
        );
    }
}

fn latin_keys(shaper: &mut Shaper, fonts: &mut FontRegistry, family: &str) -> Vec<GlyphKey> {
    place(shaper, fonts, LATIN, 16.0, family, 0.3)
        .into_iter()
        .map(|placed| placed.key)
        .collect()
}

/// Shaping and rasterizing on this path never builds `FONT_SYSTEM`. Nothing
/// else in this binary touches it, so the check holds under nextest and
/// `cargo test` alike.
fn the_raster_path_never_builds_the_process_font_system() {
    let mut shaper = Shaper::new();
    let family = shaper.register(ROBOTO.to_vec());
    let mut rasterizer = SwashRasterizer::new();
    let keys = latin_keys(&mut shaper, rasterizer.fonts_mut(), &family);
    for key in keys {
        assert!(rasterizer.rasterize(key).is_some());
    }
    assert!(!flui_painting::text_layout::font_system_initialized());
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
/// and every row is drawn. Fails on a bitmap that moves by a pixel or one
/// coverage value, and on a sample that shapes other glyphs than it did.
fn swash_matches_the_recorded_reference() {
    let mut failures = Vec::new();
    let mut matched = 0;
    for sample in &SAMPLES {
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
                .map(|placed| placed.key)
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
fn rasterizing_a_key_twice_draws_the_same_bitmap() {
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

#[test]
fn parley_oracle_contract() {
    cases::run_cases(
        "parley_oracle",
        &[
            (
                "swash_matches_cosmic_text_bit_for_bit",
                swash_matches_cosmic_text_bit_for_bit,
            ),
            (
                "swash_matches_the_recorded_reference",
                swash_matches_the_recorded_reference,
            ),
            (
                "rasterizing_a_key_twice_draws_the_same_bitmap",
                rasterizing_a_key_twice_draws_the_same_bitmap,
            ),
            (
                "the_raster_path_never_builds_the_process_font_system",
                the_raster_path_never_builds_the_process_font_system,
            ),
        ],
    );
}
