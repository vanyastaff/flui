//! swash on Parley-shaped glyphs against the scaler FLUI draws with today
//! (ADR-0092 §10 step 1).
//!
//! Parley shapes each sample on a test-local `FontContext` with no host
//! discovery; the faces come from the bundled Roboto and from host fonts found
//! through `cosmic_text::fontdb`. Every distinct key is rasterized by
//! [`SwashRasterizer`] and by the reference: cosmic-text's `SwashCache`, as
//! `SharedFontSystem::rasterize` drives it, on a font system local to the test
//! and fed the same face bytes, glyph, size and bin. Nothing here touches the
//! process-wide font system.

#![allow(clippy::unwrap_used, clippy::print_stdout, reason = "test")]

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cosmic_text::{CacheKey, CacheKeyFlags, FontSystem, SwashCache, SwashContent, fontdb};
use flui_painting::parley_text::{
    FaceKey, FontRegistry, ParleyGlyphKey, SubpixelBin, SwashRasterizer, Synthesis,
};
use flui_painting::{GlyphContent, GlyphImage, GlyphRasterizer};
use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::layout::PositionedLayoutItem;
use parley::style::{FontFamily, FontFamilyName, StyleProperty};
use parley::{FontContext, FontData, Layout, LayoutContext};

const ROBOTO: &[u8] = include_bytes!("../assets/fonts/Roboto-Regular.ttf");
const LATIN: &str = "The quick brown fox jumps over the lazy dog 0123456789";
const SIZES: [f32; 3] = [13.0, 18.0, 32.0];
const ORIGINS: [f32; 4] = [0.0, 0.25, 0.5, 0.75];

/// A host script: its sample, and whether its face must carry colour tables.
struct HostScript {
    name: &'static str,
    text: &'static str,
    color: bool,
}

/// The complex scripts of which the host must carry at least one.
const COMPLEX_SCRIPTS: [&str; 3] = ["arabic", "devanagari", "hebrew"];

const HOST_SCRIPTS: [HostScript; 4] = [
    HostScript {
        name: "arabic",
        text: "مرحبا بالعالم هذا نص عربي",
        color: false,
    },
    HostScript {
        name: "devanagari",
        text: "नमस्ते दुनिया यह हिंदी पाठ है",
        color: false,
    },
    HostScript {
        name: "hebrew",
        text: "שלום עולם זה טקסט בעברית",
        color: false,
    },
    HostScript {
        name: "emoji",
        text: "👍 🎉 😀 🚀",
        color: true,
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

    /// A second context over the same collection and source cache.
    fn fork(&self) -> Self {
        Self {
            font_cx: FontContext {
                collection: self.font_cx.collection.clone(),
                source_cache: self.font_cx.source_cache.clone(),
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
    key: ParleyGlyphKey,
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
                let key = ParleyGlyphKey::new(
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
            .expect("the reference rasterizes");
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

/// A host face whose charmap covers `script`'s sample (spaces aside), picked
/// by family name so the choice is stable on a host; `None` if the host has
/// none.
///
/// A static face is preferred over a variable one: Parley may place a
/// variable face at coordinates cosmic-text would not, and the reference
/// skips those keys.
fn host_face(db: &fontdb::Database, script: &HostScript) -> Option<(String, Vec<u8>)> {
    let mut candidates: Vec<(&str, fontdb::ID)> = db
        .faces()
        .filter_map(|face| Some((face.families.first()?.0.as_str(), face.id)))
        .collect();
    candidates.sort_unstable();
    let pick = |allow_variable: bool| {
        candidates.iter().find_map(|(family, id)| {
            db.with_face_data(*id, |data, index| {
                let face = swash::FontRef::from_index(data, usize::try_from(index).ok()?)?;
                let charmap = face.charmap();
                let covers = script
                    .text
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .all(|c| charmap.map(c) != 0);
                let has = |tag: &[u8; 4]| {
                    swash::TableProvider::table_by_tag(&face, u32::from_be_bytes(*tag)).is_some()
                };
                let colored = [b"COLR", b"CBDT", b"sbix"].into_iter().any(has);
                let variable = has(b"fvar");
                (covers
                    && (colored || !script.color)
                    && (allow_variable || !variable)
                    && index == 0)
                    .then(|| ((*family).to_owned(), data.to_vec()))
            })
            .flatten()
        })
    };
    pick(false).or_else(|| pick(true))
}

#[derive(Default)]
struct Report {
    unique: usize,
    swash_exact: usize,
    color: usize,
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
                report.color += usize::from(expected.content == GlyphContent::Color);
            }
        }
    }
    report
}

/// swash on Parley's keys draws exactly what cosmic-text's scaler draws.
///
/// Latin is the bundled Roboto and always runs. Of Arabic, Devanagari and
/// Hebrew at least one must be on the host; emoji runs when the host has a
/// colour face.
#[test]
fn swash_matches_cosmic_text_bit_for_bit() {
    let mut shaper = Shaper::new();
    let roboto = shaper.register(ROBOTO.to_vec());
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let mut scripts = vec![("latin".to_owned(), roboto, LATIN, false)];
    for script in &HOST_SCRIPTS {
        match host_face(&db, script) {
            Some((family, bytes)) => {
                let registered = shaper.register(bytes);
                println!(
                    "{}: host face {family} (registered as {registered})",
                    script.name
                );
                scripts.push((
                    script.name.to_owned(),
                    registered,
                    script.text,
                    script.color,
                ));
            }
            None => println!("{}: no host face covers the sample", script.name),
        }
    }
    assert!(
        scripts
            .iter()
            .any(|(name, ..)| COMPLEX_SCRIPTS.contains(&name.as_str())),
        "no host font covers any of {COMPLEX_SCRIPTS:?}; install one (DejaVu Sans, \
         which Linux images ship with fontconfig, covers Arabic and Hebrew)"
    );

    println!(
        "{:<11} {:>7} {:>12} {:>6}  skipped",
        "script", "unique", "swash_exact", "color"
    );
    for (name, family, text, color) in &scripts {
        let report = compare(&mut shaper, family, text);
        println!(
            "{:<11} {:>7} {:>12} {:>6}  {:?}",
            name,
            report.unique,
            format!("{}/{}", report.swash_exact, report.unique),
            report.color,
            report.skipped
        );
        assert!(report.unique > 0, "{name}: something was compared");
        assert_eq!(
            report.swash_exact, report.unique,
            "{name}: swash is bit-identical"
        );
        if *color {
            assert!(
                report.color > 0,
                "{name}: colour glyphs rasterize as colour"
            );
        }
    }
}

fn latin_keys(shaper: &mut Shaper, fonts: &mut FontRegistry, family: &str) -> Vec<ParleyGlyphKey> {
    place(shaper, fonts, LATIN, 16.0, family, 0.3)
        .into_iter()
        .map(|placed| placed.key)
        .collect()
}

#[test]
fn shaping_twice_yields_equal_keys() {
    let mut shaper = Shaper::new();
    let family = shaper.register(ROBOTO.to_vec());
    let mut fonts = FontRegistry::new();
    let first = latin_keys(&mut shaper, &mut fonts, &family);
    let second = latin_keys(&mut shaper, &mut fonts, &family);
    assert!(!first.is_empty());
    assert_eq!(first, second);
}

/// Two contexts over one shared collection shape a face registered once as
/// one blob, so their keys agree and would share atlas slots.
#[test]
fn two_contexts_over_one_collection_produce_equal_keys() {
    let mut a = Shaper::new();
    let family = a.register(ROBOTO.to_vec());
    let mut b = a.fork();
    let mut fonts = FontRegistry::new();
    let from_a = latin_keys(&mut a, &mut fonts, &family);
    let from_b = latin_keys(&mut b, &mut fonts, &family);
    assert!(!from_a.is_empty());
    assert_eq!(from_a, from_b);
}

/// Shaping and rasterizing on this path never builds `FONT_SYSTEM`. Nothing
/// else in this binary touches it, so the check holds under nextest and
/// `cargo test` alike.
#[test]
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

/// The rasterizer draws on another thread while this one keeps shaping: it
/// owns its faces and shares nothing with the shaping context.
#[test]
fn rasterization_runs_off_the_shaping_thread() {
    let mut shaper = Shaper::new();
    let family = shaper.register(ROBOTO.to_vec());
    let mut rasterizer = SwashRasterizer::new();
    let keys = latin_keys(&mut shaper, rasterizer.fonts_mut(), &family);
    let drawn = std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            keys.iter()
                .filter(|key| rasterizer.rasterize(**key).is_some())
                .count()
        });
        let mut fonts = FontRegistry::new();
        let mut shaped = 0;
        for _ in 0..20 {
            shaped += latin_keys(&mut shaper, &mut fonts, &family).len();
        }
        assert!(shaped > 0);
        worker.join().unwrap()
    });
    assert!(drawn > 0);
}
