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
//! discovered, its generic families and its fallback order, so the Parley
//! path measures text in the face cosmic-text paints it with (ADR-0092 §7).
//! [`FontCollection::new`] holds the bundled faces alone; without
//! `bundled-fonts` it starts empty, and text shapes with no face until one is
//! registered.

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
    /// with no face until one is registered.
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
    /// Text measured on it resolves the family the process font system
    /// paints with, and falls back in the same order past it (ADR-0092 §7).
    /// The app's composition root builds one per app, before the first
    /// frame; it reads the font files again, outside `fonts`' lock, so it
    /// costs a second scan of the host's fonts. A file that cannot be read is
    /// skipped.
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
    /// that found no face leaves the value alone.
    #[must_use]
    pub(crate) fn generation(&self) -> u64 {
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
    /// ones built before the call: each sees it at its next shape.
    /// Registration goes through a local clone of fontique's collection, whose
    /// shared mode propagates the change; FLUI adds no lock.
    ///
    /// # Errors
    ///
    /// [`RegisterFontError`] if the bytes hold no face; nothing is added and
    /// no context re-reads the collection.
    pub fn register_font(&self, font_bytes: &[u8]) -> Result<(), RegisterFontError> {
        // Checked before fontique sees the bytes: its registration bumps the
        // shared version even when it finds no face, which would make every
        // context deep-copy the collection for nothing.
        if ::swash::FontRef::from_index(font_bytes, 0).is_none() {
            return Err(RegisterFontError);
        }
        let mut collection = self.0.collection.clone();
        let families = collection.register_fonts(
            parley::fontique::Blob::new(Arc::new(font_bytes.to_vec())),
            None,
        );
        if families.is_empty() {
            return Err(RegisterFontError);
        }
        self.0.generation.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
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

/// Registers the embedded faces, binds every generic family to Roboto and
/// makes Roboto every script's fallback.
///
/// fontique has no last resort past a style's families and its script's
/// fallbacks, so without the fallback a glyph the style's family lacks (a
/// Cyrillic letter in an icon font) would measure as `.notdef`, where
/// cosmic-text paints it in Roboto. A host feed replaces the fallbacks with
/// the host's order (`install_into`).
#[cfg(feature = "bundled-fonts")]
fn bind_bundled_faces(collection: &mut parley::fontique::Collection) {
    use parley::fontique::{Blob, FallbackKey, GenericFamily};

    use crate::fonts::{CUPERTINO_ICONS, MATERIAL_ICONS_REGULAR, ROBOTO_REGULAR};

    let mut register =
        |bytes: &'static [u8]| collection.register_fonts(Blob::new(Arc::new(bytes)), None);
    let roboto: Vec<_> = register(ROBOTO_REGULAR)
        .into_iter()
        .map(|(family, _)| family)
        .collect();
    register(MATERIAL_ICONS_REGULAR);
    register(CUPERTINO_ICONS);
    for generic in [
        GenericFamily::SansSerif,
        GenericFamily::Serif,
        GenericFamily::Monospace,
        GenericFamily::Cursive,
        GenericFamily::Fantasy,
        GenericFamily::SystemUi,
    ] {
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
    use super::FontCollectionInner;

    const ROBOTO: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");

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
}
