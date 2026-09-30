//! The Parley path's font collection and per-realm text context
//! (ADR-0092 §2–§3).
//!
//! One [`FontCollection`] serves the app: every realm builds its own
//! [`TextContext`] from it, and a face registered on the collection reaches
//! every context built from it, including ones built earlier. The collection
//! is add-only: there is no way to remove a face, so a glyph key that names a
//! face never outlives it (ADR-0092 §2).
//!
//! No FLUI lock guards either type. The collection is fontique's shared mode:
//! a registration writes under fontique's own mutex and bumps a version, and
//! each context re-reads the data on its next query after a bump, one atomic
//! load otherwise. A context is owner-thread state used through `&mut`.
//!
//! The app's collection is fed from the host
//! ([`FontCollection::with_host_faces`]): the faces the process font system
//! discovered, its generic families and its fallback order (ADR-0092 §7).
//! Measurement, paint and carets all read the one layout shaped on the
//! collection, so a face registered on it reaches all three together; the
//! process font system is read once, to feed it, and never again.
//! [`FontCollection::new`] holds the bundled faces alone; without
//! `bundled-fonts` it starts empty, text shapes with no face until one is
//! registered, and the first registered family that can set Latin text
//! becomes every generic family ([`FontCollection::register_font`]).

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use super::layout::{HostFaces, SharedFontSystem};
use crate::error::RegisterFontError;
use crate::parley_text::SpanBrush;

/// The app's font collection: shared, add-only, passed explicitly.
///
/// A clone is the same collection ([`FontCollection::ptr_eq`]). Built once by
/// the composition root and handed to every realm, which shapes through a
/// [`TextContext`] of its own.
#[derive(Clone)]
pub struct FontCollection(Arc<FontCollectionInner>);

struct FontCollectionInner {
    /// Bumped once per registration that added a face; a measurement
    /// cached against an older value is stale.
    generation: AtomicU64,
    /// fontique's collection in shared mode. It never scans the host itself:
    /// host faces come from the process font system's discovery.
    collection: parley::fontique::Collection,
    /// One source cache shared by every context built from the collection.
    source_cache: parley::fontique::SourceCache,
}

impl FontCollection {
    /// A new collection.
    ///
    /// With `bundled-fonts` it holds the embedded Roboto, Material Icons and
    /// Cupertino Icons faces, and binds the generic families (sans-serif,
    /// serif, monospace, cursive, fantasy, system-ui) to Roboto, so text shapes the same on
    /// every host. Without `bundled-fonts` it starts empty: text measures
    /// with no face until one is registered, and the first registered family
    /// that can set Latin text then serves as every generic family
    /// ([`FontCollection::register_font`]).
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(FontCollectionInner::build(None)))
    }

    /// A collection fed from the host: [`FontCollection::new`]'s faces, then
    /// every face `fonts` holds whose families the collection does not
    /// already hold, with the generic families bound to the families `fonts`
    /// binds them to (system-ui to sans-serif's) and the fallback order
    /// `fonts` was built with.
    ///
    /// Text measured on it resolves families by the rule the process font
    /// system resolves them by, and falls back in its order past them
    /// (ADR-0092 §7). The app's composition root builds one per app, before
    /// the first frame; it reads the font files again, outside `fonts`'
    /// lock, so it costs a second scan of the host's fonts. A file that
    /// cannot be read is skipped. The collection keeps no handle on `fonts`:
    /// a face registered on it later reaches the collection alone.
    #[must_use]
    pub fn with_host_faces(fonts: &SharedFontSystem) -> Self {
        fonts.count_host_feed();
        Self(Arc::new(FontCollectionInner::build(Some(
            &fonts.host_faces(),
        ))))
    }

    /// Whether `a` and `b` are the same collection.
    #[must_use]
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }

    /// How many registrations have added a face to this collection.
    ///
    /// Starts at zero and only grows. A measurement cached against an older
    /// value may shape differently now, so it is re-measured; a registration
    /// that found no face leaves the value alone. A pipeline compares it with
    /// the value it last saw to learn that text it laid out may measure
    /// differently now.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.0.generation.load(Ordering::Acquire)
    }

    /// What a measurement on this collection is taken against: the
    /// collection and its current generation.
    pub(crate) fn key(&self) -> FontsKey {
        FontsKey {
            collection: Arc::downgrade(&self.0),
            generation: self.generation(),
        }
    }

    /// How many handles hold this collection: every clone, including the one
    /// inside each [`TextContext`] built from it.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn holders(&self) -> usize {
        Arc::strong_count(&self.0)
    }

    /// Whether the collection holds a family named `family`.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn holds(&self, family: &str) -> bool {
        self.0.collection.clone().family_id(family).is_some()
    }

    /// Adds every face in `font_bytes` to the collection.
    ///
    /// Visible to every [`TextContext`] built from this collection, including
    /// ones built before the call: each measures, paints and places carets
    /// with it from its next shape. This is the one registration door.
    /// [`Self::generation`] rises by one, which is how a
    /// pipeline learns that text it laid out may measure differently now.
    ///
    /// Registration goes through a local clone of fontique's collection,
    /// whose shared mode propagates the change; FLUI adds no lock. Faces are
    /// never removed.
    ///
    /// A generic family bound to nothing is bound to the first family the
    /// bytes hold that can set Latin text (a face of it maps the space and a
    /// basic Latin letter), so text that names no family (or a generic)
    /// measures in it rather than in no face. An icon font maps no space, so
    /// it binds nothing and a text face registered after it still takes the
    /// generics. Only a collection built without `bundled-fonts` and without
    /// a host feed has an unbound generic; a bound one is never moved. Two
    /// registrations racing on an unbound generic each bind a family they
    /// registered, and the later write stays.
    ///
    /// # Errors
    ///
    /// [`RegisterFontError`] if the collection finds no face in the bytes:
    /// nothing is added and the generation does not move.
    #[tracing::instrument(skip_all, fields(bytes = font_bytes.len()))]
    pub fn register_font(&self, font_bytes: &[u8]) -> Result<(), RegisterFontError> {
        // Checked before fontique sees the bytes: its registration bumps the
        // shared version even when it finds no face, which would make every
        // context deep-copy the collection for nothing. The scratch
        // collection takes no shared lock and bumps no version; the shared
        // registration below reads the same bytes the same way.
        let blob = parley::fontique::Blob::new(Arc::new(font_bytes.to_vec()));
        if ::swash::FontRef::from_index(font_bytes, 0).is_none() || !measures_a_family(&blob) {
            return Err(RegisterFontError);
        }
        let mut collection = self.0.collection.clone();
        let families = collection.register_fonts(blob, None);
        if families.is_empty() {
            return Err(RegisterFontError);
        }
        let text_family = families.iter().find_map(|(family, faces)| {
            faces
                .iter()
                .any(|face| sets_latin_text(font_bytes, face.index()))
                .then_some(*family)
        });
        if let Some(family) = text_family {
            bind_unbound_generics(&mut collection, family);
        }
        self.0.generation.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Whether [`Self::register_font`] would accept `font_bytes`: the
    /// collection finds a face in them. Changes nothing anywhere.
    ///
    /// For a caller that must answer for bytes before it has a collection
    /// to register them on, such as the app holding a registration made
    /// before its first window.
    ///
    /// # Errors
    ///
    /// [`RegisterFontError`] if the collection finds no face in the bytes.
    pub fn check_font(font_bytes: &[u8]) -> Result<(), RegisterFontError> {
        if ::swash::FontRef::from_index(font_bytes, 0).is_none()
            || !measures_a_family(&parley::fontique::Blob::new(Arc::new(font_bytes.to_vec())))
        {
            return Err(RegisterFontError);
        }
        Ok(())
    }
}

/// Whether the face at `index` in `font_bytes` can set Latin text: it maps
/// the space and at least one basic Latin letter. An icon font fails the
/// first (Material Icons maps lowercase letters for its ligatures, but no
/// space), so it never becomes a generic family.
fn sets_latin_text(font_bytes: &[u8], index: u32) -> bool {
    let Ok(index) = usize::try_from(index) else {
        return false;
    };
    ::swash::FontRef::from_index(font_bytes, index).is_some_and(|font| {
        let charmap = font.charmap();
        charmap.map(' ') != 0
            && ('A'..='Z')
                .chain('a'..='z')
                .any(|letter| charmap.map(letter) != 0)
    })
}

/// Whether fontique finds a family in `blob`, asked of a scratch collection
/// that shares nothing.
fn measures_a_family(blob: &parley::fontique::Blob<u8>) -> bool {
    let mut scratch = parley::fontique::Collection::new(parley::fontique::CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    !scratch.register_fonts(blob.clone(), None).is_empty()
}

/// A collection and its generation, as a cached measurement records them.
///
/// Holds the collection weakly: the allocation outlives the key, so another
/// collection can never take its address and match a stale key, and the key
/// keeps no faces alive.
#[derive(Clone)]
pub(crate) struct FontsKey {
    collection: Weak<FontCollectionInner>,
    generation: u64,
}

impl FontsKey {
    /// Whether both keys name the same collection at the same generation.
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.generation == other.generation && Weak::ptr_eq(&self.collection, &other.collection)
    }
}

impl fmt::Debug for FontsKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontsKey")
            .field("collection", &self.collection.as_ptr())
            .field("generation", &self.generation)
            .finish()
    }
}

impl FontCollectionInner {
    /// Builds the collection unshared, so no step takes fontique's shared
    /// mutex, and shares it last: it starts at generation zero whatever it
    /// holds.
    fn build(host: Option<&HostFaces>) -> Self {
        use parley::fontique::{Collection, CollectionOptions, SourceCache};

        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        #[cfg(feature = "bundled-fonts")]
        bind_bundled_faces(&mut collection);
        if let Some(host) = host {
            feed_host_faces(&mut collection, host);
        }
        collection.make_shared();
        Self {
            generation: AtomicU64::new(0),
            collection,
            source_cache: SourceCache::new_shared(),
        }
    }
}

/// Adds `host`'s faces, generics and fallback order to `collection`.
///
/// A source any of whose families the collection already holds is left out,
/// so a host copy never joins a bundled family. Generics bind only to a
/// family the collection then holds; one that is absent keeps its binding.
#[tracing::instrument(skip_all, fields(sources = host.sources.len()))]
fn feed_host_faces(collection: &mut parley::fontique::Collection, host: &HostFaces) {
    use std::collections::HashSet;

    use parley::fontique::{Blob, GenericFamily};

    use super::fallback_chain::install_into;
    use super::layout::HostData;

    let held: HashSet<String> = collection.family_names().map(str::to_lowercase).collect();
    let mut paths = Vec::new();
    for source in &host.sources {
        if source
            .families
            .iter()
            .any(|family| held.contains(&family.to_lowercase()))
        {
            continue;
        }
        match &source.data {
            HostData::Path(path) => paths.push(path.as_path()),
            HostData::Blob(data) => {
                collection.register_fonts(Blob::new(Arc::clone(data)), None);
            }
        }
    }
    let files = paths.len();
    collection.load_fonts_from_paths(paths);

    for (generic, name) in [
        (GenericFamily::SansSerif, &host.sans_serif),
        (GenericFamily::SystemUi, &host.sans_serif),
        (GenericFamily::Serif, &host.serif),
        (GenericFamily::Monospace, &host.monospace),
        (GenericFamily::Cursive, &host.cursive),
        (GenericFamily::Fantasy, &host.fantasy),
    ] {
        if let Some(id) = collection.family_id(name) {
            collection.set_generic_families(generic, std::iter::once(id));
        }
    }
    install_into(&host.chain, collection);
    // The span above records how long the feed took; no clock is read here,
    // since `std::time::Instant` panics on wasm32-unknown-unknown.
    tracing::debug!(files, "fed the host's faces into the font collection");
}

/// Every generic family a style can name, `system-ui` included.
const GENERICS: [parley::fontique::GenericFamily; 6] = {
    use parley::fontique::GenericFamily;
    [
        GenericFamily::SansSerif,
        GenericFamily::Serif,
        GenericFamily::Monospace,
        GenericFamily::Cursive,
        GenericFamily::Fantasy,
        GenericFamily::SystemUi,
    ]
};

/// Binds `family` to every generic family of `collection` that is bound to
/// nothing.
fn bind_unbound_generics(
    collection: &mut parley::fontique::Collection,
    family: parley::fontique::FamilyId,
) {
    for generic in GENERICS {
        if collection.generic_families(generic).next().is_none() {
            collection.set_generic_families(generic, std::iter::once(family));
        }
    }
}

/// Registers the embedded faces, binds every generic family to Roboto and
/// makes Roboto every script's fallback.
///
/// fontique has no last resort past a style's families and its script's
/// fallbacks, so without the fallback a glyph the style's family lacks (a
/// Cyrillic letter in an icon font) would measure and paint as `.notdef`,
/// where cosmic-text shapes it in Roboto. A host feed replaces the fallbacks with
/// the host's order (`install_into`).
#[cfg(feature = "bundled-fonts")]
fn bind_bundled_faces(collection: &mut parley::fontique::Collection) {
    use parley::fontique::{Blob, FallbackKey};

    use crate::fonts::{CUPERTINO_ICONS, MATERIAL_ICONS_REGULAR, ROBOTO_REGULAR};

    let mut register =
        |bytes: &'static [u8]| collection.register_fonts(Blob::new(Arc::new(bytes)), None);
    let roboto: Vec<_> = register(ROBOTO_REGULAR)
        .into_iter()
        .map(|(family, _)| family)
        .collect();
    register(MATERIAL_ICONS_REGULAR);
    register(CUPERTINO_ICONS);
    for generic in GENERICS {
        collection.set_generic_families(generic, roboto.iter().copied());
    }
    for script in super::fallback_chain::fontique_scripts() {
        collection.set_fallbacks(FallbackKey::new(script, None), roboto.iter().copied());
    }
}

impl Default for FontCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for FontCollection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontCollection")
            .field("id", &Arc::as_ptr(&self.0))
            .finish_non_exhaustive()
    }
}

/// One realm's text service: Parley's font and layout contexts over a clone
/// of the app's [`FontCollection`].
///
/// Owner-thread state, used through `&mut`. It is `Send`, so a realm can be
/// built on one thread and run on another, and it holds no lock: two realms
/// shape at the same time without waiting on each other.
pub struct TextContext {
    fonts: FontCollection,
    /// How many measurements this context was lent for; read by tests
    /// through `testing::text_context_lends`.
    #[cfg(any(test, feature = "testing"))]
    lent: u64,
    pub(crate) font_cx: parley::FontContext,
    pub(crate) layout_cx: parley::LayoutContext<SpanBrush>,
}

impl TextContext {
    /// A context over `fonts`: it sees every face registered on the
    /// collection, before or after this call.
    #[must_use]
    pub fn new(fonts: &FontCollection) -> Self {
        Self {
            fonts: fonts.clone(),
            #[cfg(any(test, feature = "testing"))]
            lent: 0,
            font_cx: parley::FontContext {
                collection: fonts.0.collection.clone(),
                source_cache: fonts.0.source_cache.clone(),
            },
            layout_cx: parley::LayoutContext::new(),
        }
    }

    /// The collection this context was built from.
    #[must_use]
    pub fn fonts(&self) -> &FontCollection {
        &self.fonts
    }

    /// Records one measurement made through this context. Counted only in
    /// test builds, where it shows which context a layout measured on.
    #[inline]
    #[cfg_attr(
        not(any(test, feature = "testing")),
        expect(clippy::unused_self, reason = "only test builds count the loans")
    )]
    pub(crate) fn note_lent(&mut self) {
        #[cfg(any(test, feature = "testing"))]
        {
            self.lent += 1;
        }
    }

    /// How many measurements this context was lent for.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn lends(&self) -> u64 {
        self.lent
    }
}

impl fmt::Debug for TextContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextContext")
            .field("fonts", &self.fonts)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parley::fontique::{FallbackKey, GenericFamily, Script};

    use super::super::fallback_chain::FallbackChain;
    use super::super::layout::{HostData, HostFaces, HostSource};
    use super::{FontCollection, FontCollectionInner, TextContext};
    use crate::parley_text::ParagraphSpec;
    use crate::typography::{TextDirection, TextStyle};

    const ROBOTO: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");
    const PROBE_MONO: &[u8] = include_bytes!("../../assets/fonts/probe-mono-100.ttf");

    /// A collection holding no face and binding no generic: what
    /// `FontCollection::new` builds without `bundled-fonts`, which this
    /// crate's own tests always enable.
    fn unbundled() -> FontCollection {
        use parley::fontique::{Collection, CollectionOptions, SourceCache};

        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        collection.make_shared();
        FontCollection(Arc::new(FontCollectionInner {
            generation: std::sync::atomic::AtomicU64::new(0),
            collection,
            source_cache: SourceCache::new_shared(),
        }))
    }

    /// On a collection that starts with no face, text naming no family
    /// measures in the first registered family that can set Latin text: it
    /// measures nothing before the registration and four of the probe's
    /// one-em `A`s after it. An icon font registered first binds nothing, so
    /// Roboto registered after it still takes the generics and unstyled
    /// `home` measures in Roboto, not as the icon font's ligature. Fails if
    /// registration leaves the generics unbound, or binds them to whichever
    /// family came first.
    mod unbound_generics {
        use super::{
            FontCollection, PROBE_MONO, ParagraphSpec, ROBOTO, TextContext, TextDirection,
            TextStyle, unbundled,
        };

        const MATERIAL_ICONS: &[u8] =
            include_bytes!("../../assets/fonts/MaterialIcons-Regular.ttf");

        fn width(fonts: &FontCollection, text: &str) -> f64 {
            let spans: Vec<(String, Option<TextStyle>)> = vec![(text.to_owned(), None)];
            TextContext::new(fonts)
                .shape(&ParagraphSpec {
                    spans: &spans,
                    default_style: None,
                    font_size: 20.0,
                    max_width: None,
                    line_height: None,
                    direction: TextDirection::Ltr,
                    max_lines: None,
                    ellipsis: None,
                })
                .metrics()
                .width
        }

        fn a_registered_text_family_serves_the_generics() {
            let fonts = unbundled();
            assert!(
                width(&fonts, "AAAA").abs() < 1e-3,
                "no face measures nothing, got {}",
                width(&fonts, "AAAA")
            );

            fonts
                .register_font(PROBE_MONO)
                .expect("the probe face loads");
            assert!(
                (width(&fonts, "AAAA") - 80.0).abs() < 1e-3,
                "unstyled text measures in the probe, got {}",
                width(&fonts, "AAAA")
            );
        }

        fn an_icon_font_registered_first_leaves_the_generics_to_a_text_face() {
            let fonts = unbundled();
            fonts
                .register_font(MATERIAL_ICONS)
                .expect("the icon face loads");
            fonts.register_font(ROBOTO).expect("Roboto loads");
            let roboto_only = unbundled();
            roboto_only.register_font(ROBOTO).expect("Roboto loads");
            let (got, roboto) = (width(&fonts, "home"), width(&roboto_only, "home"));
            assert!(
                roboto > 1.0 && (got - roboto).abs() < 1e-3,
                "unstyled text measures in Roboto ({roboto}), not the icon font, got {got}"
            );
        }

        #[test]
        fn unbound_generics() {
            let cases: &[(&str, fn())] = &[
                (
                    "a_registered_text_family_serves_the_generics",
                    a_registered_text_family_serves_the_generics,
                ),
                (
                    "an_icon_font_registered_first_leaves_the_generics_to_a_text_face",
                    an_icon_font_registered_first_leaves_the_generics_to_a_text_face,
                ),
            ];
            let failed: Vec<&str> = cases
                .iter()
                .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
                .map(|(name, _)| *name)
                .collect();
            assert!(
                failed.is_empty(),
                "unbound_generics: failing cases: {failed:?}"
            );
        }
    }

    struct CommonIsRoboto;

    impl cosmic_text::Fallback for CommonIsRoboto {
        fn common_fallback(&self) -> &[&'static str] {
            &["Roboto"]
        }

        fn forbidden_fallback(&self) -> &[&'static str] {
            &[]
        }

        fn script_fallback(&self, _: unicode_script::Script, _: &str) -> &[&'static str] {
            &[]
        }
    }

    /// A font file that is gone by the time the collection reads it is
    /// skipped, and the feed still completes: the other sources are added,
    /// the generics bound and the fallback order installed.
    #[test]
    fn a_missing_path_is_skipped_and_the_feed_completes() {
        let host = HostFaces {
            sources: vec![
                HostSource {
                    data: HostData::Path("no/such/dir/ghost-face.ttf".into()),
                    families: vec!["Ghost Family".to_owned()],
                },
                HostSource {
                    data: HostData::Blob(Arc::new(ROBOTO)),
                    families: vec!["Roboto".to_owned()],
                },
            ],
            sans_serif: "Roboto".to_owned(),
            serif: "Ghost Family".to_owned(),
            monospace: "Roboto".to_owned(),
            cursive: "Roboto".to_owned(),
            fantasy: "Roboto".to_owned(),
            chain: Arc::new(FallbackChain::new("en-US".to_owned(), CommonIsRoboto)),
        };
        let mut collection = FontCollectionInner::build(Some(&host)).collection;

        let roboto = collection
            .family_id("Roboto")
            .expect("the readable source is fed");
        assert!(collection.family_id("Ghost Family").is_none());
        for generic in [
            GenericFamily::SansSerif,
            GenericFamily::SystemUi,
            GenericFamily::Monospace,
        ] {
            assert_eq!(
                collection.generic_families(generic).collect::<Vec<_>>(),
                [roboto],
                "{generic:?} binds to the host's family"
            );
        }
        assert_eq!(
            collection
                .fallback_families(FallbackKey::new(Script::from_str_unchecked("Latn"), None))
                .collect::<Vec<_>>(),
            [roboto],
            "the fallback order is installed"
        );
        assert_eq!(
            collection
                .generic_families(GenericFamily::Emoji)
                .collect::<Vec<_>>(),
            [roboto]
        );
    }

    /// Keys stay equal across a source-cache prune while a registry holds
    /// the face's blob (ADR-0092 §5): fontique keeps a file-backed blob only
    /// weakly, so without a holder a prune drops it, the next shape loads
    /// the file again under a new blob id, and every key naming the old id
    /// changes. The arm without a registry shows the prune does that here.
    #[test]
    fn a_held_blob_keeps_its_keys_across_a_prune() {
        use parley::fontique::{Collection, CollectionOptions, SourceCache};

        use crate::glyphs::{FaceKey, FontRegistry};

        // One file and one collection per arm, so neither arm's blob is
        // reachable from the other's source cache.
        let collection_over = |arm: &str| {
            let path = std::env::temp_dir().join(format!(
                "flui-prune-roboto-{arm}-{}-{:?}.ttf",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::write(&path, ROBOTO).expect("the temp dir is writable");
            let mut collection = Collection::new(CollectionOptions {
                shared: false,
                system_fonts: false,
            });
            collection.load_fonts_from_paths([path.as_path()]);
            collection.make_shared();
            let fonts = FontCollection(Arc::new(FontCollectionInner {
                generation: std::sync::atomic::AtomicU64::new(0),
                collection,
                source_cache: SourceCache::new_shared(),
            }));
            (fonts, path)
        };
        let style = TextStyle {
            font_family: Some("Roboto".to_owned()),
            ..TextStyle::default()
        };
        let spans: Vec<(String, Option<TextStyle>)> = vec![("Hamburg".to_owned(), Some(style))];
        let face = |cx: &mut TextContext, registry: Option<&mut FontRegistry>| -> FaceKey {
            let paragraph = cx
                .shape(&ParagraphSpec {
                    spans: &spans,
                    default_style: None,
                    font_size: 16.0,
                    max_width: None,
                    line_height: None,
                    direction: TextDirection::Ltr,
                    max_lines: None,
                    ellipsis: None,
                })
                .to_shaped(None);
            let run = paragraph.runs().next().expect("Roboto shapes a run");
            if let Some(registry) = registry {
                registry.prepare_run(&run).expect("Roboto registers");
            }
            run.face().key()
        };
        let pruned = |cx: &mut TextContext| cx.font_cx.source_cache.prune(0, true);

        let (fonts, path) = collection_over("held");
        let mut cx = TextContext::new(&fonts);
        let mut registry = FontRegistry::new();
        let held = face(&mut cx, Some(&mut registry));
        pruned(&mut cx);
        let again = face(&mut cx, None);
        let _ = std::fs::remove_file(&path);

        let (fonts, path) = collection_over("control");
        let mut control = TextContext::new(&fonts);
        let first = face(&mut control, None);
        pruned(&mut control);
        let reloaded = face(&mut control, None);
        let _ = std::fs::remove_file(&path);

        assert_eq!(
            held, again,
            "a blob the registry holds keeps its id, so its keys stay equal"
        );
        assert!(registry.contains_face(held));
        assert_ne!(
            first, reloaded,
            "precondition: without a holder the prune reloads the file under a new id"
        );
    }

    /// Registration adds a face to the collection alone: measurement, paint
    /// and carets all read the layout shaped on it. Driven on a font system
    /// of the test's own (`SharedFontSystem::pinned`), so no row touches the
    /// process-wide one.
    mod registration_contract {
        use super::super::super::layout::{SharedFontSystem, font_system_initialized};
        use super::super::FontCollection;
        use super::{PROBE_MONO, ROBOTO};

        const PROBE: &str = "FLUI Probe Mono";

        fn host() -> SharedFontSystem {
            SharedFontSystem::pinned(&[ROBOTO], "Roboto")
        }

        /// A face registered on a collection fed from a font system reaches
        /// the collection, whose generation rises by one, and not the font
        /// system, which nothing reads after the feed. Fails if registration
        /// still loads faces into the font system, or skips the collection.
        fn a_registration_reaches_the_collection_alone() {
            let host = host();
            let fonts = FontCollection::with_host_faces(&host);
            let before = fonts.generation();
            assert!(!fonts.holds(PROBE));

            assert_eq!(
                fonts.register_font(PROBE_MONO),
                Ok(()),
                "the probe face loads"
            );

            assert!(fonts.holds(PROBE), "the collection holds the face");
            assert_eq!(fonts.generation(), before + 1);
            assert!(
                !host.family_names().iter().any(|name| name == PROBE),
                "the font system the collection was fed from gains nothing"
            );
        }

        /// Bytes with no face are refused and move nothing. Fails if a
        /// refused registration bumps the generation, which would lay out
        /// every realm's text again for nothing.
        fn bytes_with_no_face_are_refused() {
            let fonts = FontCollection::with_host_faces(&host());
            let before = fonts.generation();

            for bytes in [&b"not a font"[..], &[]] {
                assert!(fonts.register_font(bytes).is_err());
                assert!(FontCollection::check_font(bytes).is_err());
            }

            assert_eq!(fonts.generation(), before);
        }

        /// The probe face with its `cmap` table hidden (its tag renamed in
        /// the table directory): it parses as a font, but fontique finds no
        /// family in it.
        fn probe_without_cmap() -> Vec<u8> {
            let mut bytes = PROBE_MONO.to_vec();
            let tables = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
            let record = (0..tables)
                .map(|table| 12 + table * 16)
                .find(|&record| &bytes[record..record + 4] == b"cmap");
            assert!(record.is_some(), "the probe face has a cmap table");
            if let Some(record) = record {
                bytes[record] = b'z';
            }
            bytes
        }

        /// Bytes that parse as a font but hold no family the collection can
        /// measure are refused, by `check_font` too, and move nothing.
        fn bytes_with_no_family_are_refused() {
            let fonts = FontCollection::with_host_faces(&host());
            let before = fonts.generation();
            let bytes = probe_without_cmap();

            assert!(FontCollection::check_font(&bytes).is_err());
            assert!(fonts.register_font(&bytes).is_err());

            assert!(!fonts.holds(PROBE));
            assert_eq!(fonts.generation(), before);
        }

        /// A collection built without a font system registers without ever
        /// building the process-wide one.
        fn a_bundled_only_collection_registers_without_the_process_font_system() {
            let fonts = FontCollection::new();

            assert_eq!(
                fonts.register_font(PROBE_MONO),
                Ok(()),
                "the probe face loads"
            );

            assert!(fonts.holds(PROBE));
            assert_eq!(fonts.generation(), 1);
            assert!(
                !font_system_initialized(),
                "registration does not build the process font system"
            );
        }

        #[test]
        fn registration_contract() {
            let cases: &[(&str, fn())] = &[
                (
                    "a_registration_reaches_the_collection_alone",
                    a_registration_reaches_the_collection_alone,
                ),
                (
                    "bytes_with_no_face_are_refused",
                    bytes_with_no_face_are_refused,
                ),
                (
                    "bytes_with_no_family_are_refused",
                    bytes_with_no_family_are_refused,
                ),
                (
                    "a_bundled_only_collection_registers_without_the_process_font_system",
                    a_bundled_only_collection_registers_without_the_process_font_system,
                ),
            ];
            let failed: Vec<&str> = cases
                .iter()
                .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
                .map(|(name, _)| *name)
                .collect();
            assert!(
                failed.is_empty(),
                "registration_contract: failing cases: {failed:?}"
            );
        }
    }
}
