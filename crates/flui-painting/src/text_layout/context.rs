//! The Parley path's font collection and per-UI runtime text context
//! (ADR-0092 §2–§3).
//!
//! One [`FontCollection`] serves the app: every UI runtime builds its own
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
//! The app's collection is fed from the host: the faces one scan of the host
//! found, its generic families and FLUI's fallback lists for the host
//! (ADR-0092 §7). The app builds it with [`FontCollection::with_host_feed`],
//! which holds the bundled faces at once, and runs the returned
//! [`HostFontFeed`] off the owner thread; the host's faces then arrive as a
//! registration's do, through the collection's generation.
//! [`FontCollection::with_host_fonts`] feeds synchronously instead.
//! Measurement, paint and carets all read the one layout shaped on the
//! collection, so a face registered on it reaches all three together; the
//! scan is read once, to feed it, and never again.
//! [`FontCollection::new`] holds the bundled faces alone; without
//! `bundled-fonts` it starts empty, text shapes with no face until one is
//! registered, and the first registered family that can set Latin text
//! becomes every generic family ([`FontCollection::register_font`]).

use std::fmt;
#[cfg(any(test, feature = "testing"))]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use super::host::{HostData, HostFaces, HostFonts};
use crate::error::RegisterFontError;
use crate::parley_text::SpanBrush;

/// The app's font collection: shared, add-only, passed explicitly.
///
/// A clone is the same collection ([`FontCollection::ptr_eq`]). Built once by
/// the composition root and handed to every UI runtime, which shapes through a
/// [`TextContext`] of its own.
#[derive(Clone)]
pub struct FontCollection(Arc<FontCollectionInner>);

struct FontCollectionInner {
    /// Bumped once per registration that added a face; a measurement
    /// cached against an older value is stale.
    generation: AtomicU64,
    /// fontique's collection in shared mode. It never scans the host itself:
    /// host faces come from a [`HostFonts`] scan.
    collection: parley::fontique::Collection,
    /// One source cache shared by every context built from the collection.
    source_cache: parley::fontique::SourceCache,
    /// Whether a host feed has finished on the collection; read by tests
    /// through `testing::host_fed`.
    #[cfg(any(test, feature = "testing"))]
    host_fed: AtomicBool,
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
    /// every face `host` found whose families the collection does not
    /// already hold, every generic family the collection leaves unbound
    /// bound to the family `host` picked for it (system-ui to sans-serif's),
    /// and `host`'s fallback lists past a style's family.
    ///
    /// With `bundled-fonts` the bundled Roboto keeps every generic and a
    /// host copy of a bundled family is never fed, so text naming no family
    /// measures alike on every host; the host's faces serve families only it
    /// has and the fallback past them (ADR-0092 §7). The app's composition
    /// root builds one per app, before the first frame; it parses each font
    /// file the scan found, so it costs more than the scan itself. A file
    /// that cannot be read is skipped. The collection keeps no handle on
    /// `host`: a face registered on it later reaches the collection alone.
    #[must_use]
    pub fn with_host_fonts(host: &HostFonts) -> Self {
        Self(Arc::new(FontCollectionInner::build(Some(&host.faces()))))
    }

    /// [`FontCollection::new`]'s collection, and the feed that adds the
    /// host's faces to it.
    ///
    /// The collection holds the bundled faces at once, so a first frame
    /// renders before the host is scanned. [`HostFontFeed::run`], on a thread
    /// other than the owner's, scans the host and adds what
    /// [`FontCollection::with_host_fonts`] would: every face whose families
    /// the collection does not hold when the feed starts, the generics it
    /// leaves unbound, and the host's fallback order. If that changed the
    /// collection it then raises [`Self::generation`] by one, so text laid
    /// out before measures again at each pipeline's next frame, as after a
    /// registration; a feed that added nothing leaves it alone.
    ///
    /// Without `bundled-fonts` the collection would start with no face, and
    /// a first frame would draw no text at all; this then feeds the host's
    /// faces before it returns, and the returned feed has nothing left to
    /// do.
    pub fn with_host_feed() -> (Self, HostFontFeed) {
        #[cfg(feature = "bundled-fonts")]
        {
            let fonts = Self::new();
            let feed = HostFontFeed {
                fonts: Some(fonts.clone()),
                #[cfg(any(test, feature = "testing"))]
                host: None,
            };
            (fonts, feed)
        }
        #[cfg(not(feature = "bundled-fonts"))]
        {
            let fonts = Self::with_host_fonts(&HostFonts::scan());
            let feed = HostFontFeed {
                fonts: None,
                #[cfg(any(test, feature = "testing"))]
                host: None,
            };
            (fonts, feed)
        }
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

    /// Whether the collection was fed from a host scan: built by
    /// [`FontCollection::with_host_fonts`], or a [`HostFontFeed`] on it
    /// finished.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn host_fed(&self) -> bool {
        self.0.host_fed.load(Ordering::Acquire)
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
            // A private build starts at generation zero whatever it holds,
            // so whether the feed changed anything does not matter here.
            let _changed = feed_host_faces(&mut collection, host, Registration::Private);
        }
        collection.make_shared();
        Self {
            generation: AtomicU64::new(0),
            collection,
            source_cache: SourceCache::new_shared(),
            #[cfg(any(test, feature = "testing"))]
            host_fed: AtomicBool::new(host.is_some()),
        }
    }
}

/// The host's faces, still to be added to a collection built by
/// [`FontCollection::with_host_feed`].
///
/// `Send`: built on the owner thread and run on another. Dropping it unrun
/// leaves the collection with the faces it had.
#[must_use = "the host's faces reach the collection only when the feed runs"]
pub struct HostFontFeed {
    /// The collection to feed; `None` once nothing is left to do.
    fonts: Option<FontCollection>,
    /// The host the feed reads instead of scanning; set by
    /// `testing::feed_with_host`.
    #[cfg(any(test, feature = "testing"))]
    host: Option<HostFonts>,
}

impl HostFontFeed {
    /// Scans the host's fonts and adds them to the collection, then raises
    /// its generation by one if that changed the collection.
    ///
    /// Blocking, and meant for a thread other than the owner's: the scan
    /// takes a few milliseconds and the feed, which reads every font file
    /// the scan found, tens. Each file is added on its own, so a UI runtime
    /// shaping meanwhile waits at most for one file's registration, and may
    /// see some of the host's faces before the feed ends; the generation
    /// rises once, at the end, however many sources were added, and not at
    /// all when the feed added no source, bound no generic and left every
    /// fallback list as it was. A source that panics or holds no family on
    /// a trial read is skipped before the collection sees it; the trial
    /// guards a file that panics on every read, not one replaced between
    /// the trial and the registration, which reads it again. If the feed
    /// itself unwinds, the generation still rises, so the faces already
    /// added are not left unannounced.
    pub fn run(self) {
        let Some(fonts) = self.fonts else {
            return;
        };
        #[cfg(any(test, feature = "testing"))]
        let host = self.host.unwrap_or_else(HostFonts::scan);
        #[cfg(not(any(test, feature = "testing")))]
        let host = HostFonts::scan();
        let faces = host.faces();
        drop(host);
        fonts.feed_shared(&faces);
    }

    /// The feed, reading `host` instead of scanning.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn with_host(mut self, host: HostFonts) -> Self {
        self.host = Some(host);
        self
    }
}

impl fmt::Debug for HostFontFeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostFontFeed")
            .field("fonts", &self.fonts)
            .finish_non_exhaustive()
    }
}

impl FontCollection {
    /// Adds `host`'s faces to this shared collection, one source per
    /// registration, and raises the generation once if the feed changed the
    /// collection. A feed that unwinds raises it too: what it added before
    /// the panic is not left unannounced.
    fn feed_shared(&self, host: &HostFaces) {
        /// Announces the feed when it ends, unless it ended having changed
        /// nothing.
        struct Landed<'a> {
            inner: &'a FontCollectionInner,
            /// Whether to raise the generation; stays set while the feed
            /// runs, so an unwind announces.
            announce: bool,
        }

        impl Drop for Landed<'_> {
            fn drop(&mut self) {
                #[cfg(any(test, feature = "testing"))]
                self.inner.host_fed.store(true, Ordering::Release);
                if self.announce {
                    self.inner.generation.fetch_add(1, Ordering::AcqRel);
                }
            }
        }

        let mut landed = Landed {
            inner: &self.0,
            announce: true,
        };
        let mut collection = self.0.collection.clone();
        landed.announce = feed_host_faces(&mut collection, host, Registration::Shared);
    }
}

/// How [`feed_host_faces`] registers each source.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Registration {
    /// On a collection no one else reads yet: each source is registered
    /// directly.
    Private,
    /// On the shared collection, under fontique's lock: each source is first
    /// read on a scratch collection, where a panic poisons nothing, and is
    /// skipped if that read panics or finds no family.
    Shared,
}

/// Adds `host`'s faces, generics and fallback order to `collection`.
///
/// A source any of whose families the collection holds when the feed starts
/// is left out, so a host copy never joins a bundled or registered family.
/// Each source is registered on its own, so on a shared collection
/// fontique's lock is held for one file at a time. A generic the collection
/// already binds keeps its binding (with `bundled-fonts`, Roboto keeps every
/// one); an unbound generic binds to the host's family for it when the
/// collection then holds that family.
///
/// Returns whether the collection changed: a source was registered, a
/// generic bound, or a fallback list rewritten with a different order.
#[tracing::instrument(skip_all, fields(sources = host.sources.len()))]
fn feed_host_faces(
    collection: &mut parley::fontique::Collection,
    host: &HostFaces,
    registration: Registration,
) -> bool {
    use std::collections::HashSet;

    use parley::fontique::GenericFamily;

    use super::fallback_chain::install_into;

    let held: HashSet<String> = collection.family_names().map(str::to_lowercase).collect();
    let mut fed = 0_usize;
    let mut bound = false;
    for source in &host.sources {
        if source
            .families
            .iter()
            .any(|family| held.contains(&family.to_lowercase()))
        {
            continue;
        }
        if registration == Registration::Shared && !reads_a_family(&source.data) {
            continue;
        }
        register_source(collection, &source.data);
        fed += 1;
    }

    for (generic, name) in [
        (GenericFamily::SansSerif, &host.sans_serif),
        (GenericFamily::SystemUi, &host.sans_serif),
        (GenericFamily::Serif, &host.serif),
        (GenericFamily::Monospace, &host.monospace),
        (GenericFamily::Cursive, &host.cursive),
        (GenericFamily::Fantasy, &host.fantasy),
    ] {
        if collection.generic_families(generic).next().is_some() {
            continue;
        }
        if let Some(id) = collection.family_id(name) {
            collection.set_generic_families(generic, std::iter::once(id));
            bound = true;
        }
    }
    let reordered = install_into(&host.chain, collection);
    // The span above records how long the feed took; no clock is read here,
    // since `std::time::Instant` panics on wasm32-unknown-unknown.
    tracing::debug!(
        sources = fed,
        "fed the host's faces into the font collection"
    );
    fed > 0 || bound || reordered
}

/// Registers one source's faces on `collection`. A file that cannot be read
/// adds nothing.
fn register_source(collection: &mut parley::fontique::Collection, data: &HostData) {
    use parley::fontique::Blob;

    match data {
        HostData::Path(path) => collection.load_fonts_from_paths([path.as_path()]),
        HostData::Blob(bytes) => {
            collection.register_fonts(Blob::new(Arc::clone(bytes)), None);
        }
    }
}

/// Whether `data` holds a family, read on a scratch collection that shares
/// nothing. A read that panics answers no: under the shared collection's
/// lock the same panic would poison it, and every UI runtime would panic at its
/// next query.
fn reads_a_family(data: &HostData) -> bool {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let read = catch_unwind(AssertUnwindSafe(|| {
        let mut scratch = parley::fontique::Collection::new(parley::fontique::CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        register_source(&mut scratch, data);
        scratch.family_names().next().is_some()
    }));
    if let Ok(holds) = read {
        if !holds {
            tracing::debug!(source = ?data, "a host font source holds no family; skipped");
        }
        holds
    } else {
        tracing::warn!(source = ?data, "a host font source panicked when read; skipped");
        false
    }
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
/// Cyrillic letter in an icon font) would measure and paint as `.notdef`.
/// A host feed replaces the fallbacks with the host's order, which ends in
/// Roboto too (`install_into`).
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

/// One UI runtime's text service: Parley's font and layout contexts over a clone
/// of the app's [`FontCollection`].
///
/// Owner-thread state, used through `&mut`. It is `Send`, so a UI runtime can be
/// built on one thread and run on another, and it holds no lock: two UI runtimes
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
    use super::super::host::{HostData, HostFaces, HostSource};
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
            host_fed: std::sync::atomic::AtomicBool::new(false),
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
                    min_width: 0.0,
                    text_align: crate::typography::TextAlign::Start,
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

    const PROBE_SANS: &[u8] = include_bytes!("../../assets/fonts/probe-sans-400.ttf");

    /// A font's bytes with the family names (name IDs 1 and 16) rewritten to
    /// `family` and, when given, the `OS/2` weight class set to `weight`.
    /// Checksums are left stale, which neither fontique nor swash checks.
    fn renamed(bytes: &[u8], family: &str, weight: Option<u16>) -> Vec<u8> {
        let mut out = bytes.to_vec();
        let be16 =
            |data: &[u8], at: usize| usize::from(u16::from_be_bytes([data[at], data[at + 1]]));
        let table = |data: &[u8], tag: &[u8; 4]| {
            (0..be16(data, 4))
                .map(|index| 12 + index * 16)
                .find(|&record| &data[record..record + 4] == tag)
                .map(|record| {
                    u32::from_be_bytes([
                        data[record + 8],
                        data[record + 9],
                        data[record + 10],
                        data[record + 11],
                    ]) as usize
                })
                .expect("the probe face has the table")
        };
        let name = table(&out, b"name");
        let storage = name + be16(&out, name + 4);
        for index in 0..be16(&out, name + 2) {
            let record = name + 6 + index * 12;
            if !matches!(be16(&out, record + 6), 1 | 16) {
                continue;
            }
            let encoded: Vec<u8> = if be16(&out, record) == 1 {
                family.bytes().collect()
            } else {
                family.encode_utf16().flat_map(u16::to_be_bytes).collect()
            };
            assert!(encoded.len() <= be16(&out, record + 8), "the new name fits");
            let at = storage + be16(&out, record + 10);
            out[at..at + encoded.len()].copy_from_slice(&encoded);
            let length = u16::try_from(encoded.len()).expect("a short name");
            out[record + 8..record + 10].copy_from_slice(&length.to_be_bytes());
        }
        if let Some(weight) = weight {
            let os2 = table(&out, b"OS/2");
            out[os2 + 4..os2 + 6].copy_from_slice(&weight.to_be_bytes());
        }
        out
    }

    /// A font file that is gone by the time the collection reads it is
    /// skipped, and the feed still completes: the other sources are added,
    /// the generics the collection already binds (the bundled Roboto) keep
    /// their binding though the host names another family for them, and the
    /// host's fallback order is installed, ending in the sans-serif family.
    /// Fails if the feed rebinds a bound generic, stops at the missing file,
    /// or skips the fallback order.
    #[test]
    fn a_missing_path_is_skipped_and_the_feed_completes() {
        const PROBE: &str = "FLUI Probe Mono";
        let host = HostFaces {
            sources: vec![
                HostSource {
                    data: HostData::Path("no/such/dir/ghost-face.ttf".into()),
                    families: vec!["Ghost Family".to_owned()],
                },
                HostSource {
                    data: HostData::Blob(Arc::new(PROBE_MONO)),
                    families: vec![PROBE.to_owned()],
                },
            ],
            sans_serif: PROBE.to_owned(),
            serif: "Ghost Family".to_owned(),
            monospace: PROBE.to_owned(),
            cursive: PROBE.to_owned(),
            fantasy: PROBE.to_owned(),
            chain: FallbackChain::from_lists(&[PROBE], &[]),
        };
        let mut collection = FontCollectionInner::build(Some(&host)).collection;

        let probe = collection
            .family_id(PROBE)
            .expect("the readable source is fed");
        let roboto = collection.family_id("Roboto").expect("Roboto is bundled");
        assert!(collection.family_id("Ghost Family").is_none());
        for generic in [
            GenericFamily::SansSerif,
            GenericFamily::SystemUi,
            GenericFamily::Monospace,
            GenericFamily::Serif,
        ] {
            assert_eq!(
                collection.generic_families(generic).collect::<Vec<_>>(),
                [roboto],
                "{generic:?} keeps the bundled Roboto"
            );
        }
        assert_eq!(
            collection
                .fallback_families(FallbackKey::new(Script::from_str_unchecked("Latn"), None))
                .collect::<Vec<_>>(),
            [probe, roboto],
            "the host's order is installed, ending in the sans-serif family"
        );
        assert_eq!(
            collection
                .generic_families(GenericFamily::Emoji)
                .collect::<Vec<_>>(),
            [probe]
        );
    }

    /// A host source that panics when read is skipped by the off-thread
    /// feed, which goes on: the source after it is fed, the generation rises
    /// once, a context still shapes and a later registration succeeds.
    /// Fails without the trial read: the panic then happens under fontique's
    /// shared lock, which it poisons, so the feed stops there and every
    /// later query or registration on the collection panics.
    #[test]
    fn a_source_that_panics_is_skipped_and_the_collection_stays_usable() {
        struct Unreadable;

        impl AsRef<[u8]> for Unreadable {
            #[expect(clippy::panic, reason = "a read that panics is the failure under test")]
            fn as_ref(&self) -> &[u8] {
                panic!("a font source that panics when read")
            }
        }

        const PROBE: &str = "FLUI Probe Mono";
        let host = HostFaces {
            sources: vec![
                HostSource {
                    data: HostData::Blob(Arc::new(Unreadable)),
                    families: vec!["Unreadable Family".to_owned()],
                },
                HostSource {
                    data: HostData::Blob(Arc::new(PROBE_MONO)),
                    families: vec![PROBE.to_owned()],
                },
            ],
            sans_serif: PROBE.to_owned(),
            serif: PROBE.to_owned(),
            monospace: PROBE.to_owned(),
            cursive: PROBE.to_owned(),
            fantasy: PROBE.to_owned(),
            chain: FallbackChain::from_lists(&[PROBE], &[]),
        };
        let (fonts, _unused) = FontCollection::with_host_feed();
        let before = fonts.generation();

        let fed =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fonts.feed_shared(&host)));

        assert!(fed.is_ok(), "the feed contains the source's panic");
        assert!(fonts.holds(PROBE), "the source after it is fed");
        assert_eq!(fonts.generation(), before + 1);
        let spans: Vec<(String, Option<TextStyle>)> = vec![("Shaped".to_owned(), None)];
        let width = TextContext::new(&fonts)
            .shape(&ParagraphSpec {
                spans: &spans,
                default_style: None,
                font_size: 20.0,
                max_width: None,
                min_width: 0.0,
                text_align: crate::typography::TextAlign::Start,
                line_height: None,
                direction: TextDirection::Ltr,
                max_lines: None,
                ellipsis: None,
            })
            .metrics()
            .width;
        assert!(width > 1.0, "a context still shapes, got {width}");
        assert_eq!(fonts.register_font(ROBOTO), Ok(()));
    }

    /// A host copy of a bundled family never joins it: a host whose "Roboto"
    /// is another face, at 400 and at 700, is fed, and "Roboto", sans-serif
    /// and a style naming no family still measure in the bundled bytes at
    /// 400, 500 and 700, as on a bundled-only collection. Fails if the feed
    /// stops skipping a family the collection holds: the host's 700 face
    /// then serves bold Roboto.
    #[test]
    fn a_host_copy_of_a_bundled_family_is_not_fed() {
        use crate::typography::FontWeight;

        let host = HostFaces {
            sources: [None, Some(700)]
                .into_iter()
                .map(|weight| HostSource {
                    data: HostData::Blob(Arc::new(renamed(PROBE_SANS, "Roboto", weight))),
                    families: vec!["Roboto".to_owned()],
                })
                .collect(),
            sans_serif: "Roboto".to_owned(),
            serif: "Roboto".to_owned(),
            monospace: "Roboto".to_owned(),
            cursive: "Roboto".to_owned(),
            fantasy: "Roboto".to_owned(),
            chain: FallbackChain::from_lists(&["Roboto"], &[]),
        };
        let fed = FontCollection(Arc::new(FontCollectionInner::build(Some(&host))));
        let bundled = FontCollection::new();
        let width = |fonts: &FontCollection, family: Option<&str>, weight: FontWeight| {
            let style = TextStyle {
                font_family: family.map(str::to_owned),
                font_weight: Some(weight),
                ..TextStyle::default()
            };
            let spans: Vec<(String, Option<TextStyle>)> =
                vec![("Hamburgefonstiv".to_owned(), Some(style))];
            TextContext::new(fonts)
                .shape(&ParagraphSpec {
                    spans: &spans,
                    default_style: None,
                    font_size: 20.0,
                    max_width: None,
                    min_width: 0.0,
                    text_align: crate::typography::TextAlign::Start,
                    line_height: None,
                    direction: TextDirection::Ltr,
                    max_lines: None,
                    ellipsis: None,
                })
                .metrics()
                .width
        };
        let mut failures = Vec::new();
        for family in [Some("Roboto"), Some("sans-serif"), None] {
            for weight in [FontWeight::W400, FontWeight::W500, FontWeight::W700] {
                let (got, want) = (width(&fed, family, weight), width(&bundled, family, weight));
                if (got - want).abs() > 1e-6 {
                    failures.push(format!("{family:?} at {weight:?}: {got} vs {want}"));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "measured in a host face, not the bundled Roboto: {failures:?}"
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
                host_fed: std::sync::atomic::AtomicBool::new(false),
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
                    min_width: 0.0,
                    text_align: crate::typography::TextAlign::Start,
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
    /// and carets all read the layout shaped on it. Driven on a host scan of
    /// the test's own, over a database holding Roboto alone.
    mod registration_contract {
        use super::super::super::host::HostFonts;
        use super::super::FontCollection;
        use super::{PROBE_MONO, ROBOTO};

        const PROBE: &str = "FLUI Probe Mono";

        fn host() -> HostFonts {
            let mut db = fontdb::Database::new();
            db.load_font_data(ROBOTO.to_vec());
            HostFonts::from_database(db, "en-US")
        }

        /// A face registered on a collection fed from a host scan reaches
        /// the collection, whose generation rises by one. Fails if
        /// registration skips the collection.
        fn a_registration_reaches_the_collection() {
            let fonts = FontCollection::with_host_fonts(&host());
            let before = fonts.generation();
            assert!(!fonts.holds(PROBE));

            assert_eq!(
                fonts.register_font(PROBE_MONO),
                Ok(()),
                "the probe face loads"
            );

            assert!(fonts.holds(PROBE), "the collection holds the face");
            assert_eq!(fonts.generation(), before + 1);
        }

        /// Bytes with no face are refused and move nothing. Fails if a
        /// refused registration bumps the generation, which would lay out
        /// every UI runtime's text again for nothing.
        fn bytes_with_no_face_are_refused() {
            let fonts = FontCollection::with_host_fonts(&host());
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
            let fonts = FontCollection::with_host_fonts(&host());
            let before = fonts.generation();
            let bytes = probe_without_cmap();

            assert!(FontCollection::check_font(&bytes).is_err());
            assert!(fonts.register_font(&bytes).is_err());

            assert!(!fonts.holds(PROBE));
            assert_eq!(fonts.generation(), before);
        }

        /// A collection fed from no host scan registers too.
        fn a_bundled_only_collection_registers() {
            let fonts = FontCollection::new();

            assert_eq!(
                fonts.register_font(PROBE_MONO),
                Ok(()),
                "the probe face loads"
            );

            assert!(fonts.holds(PROBE));
            assert_eq!(fonts.generation(), 1);
        }

        #[test]
        fn registration_contract() {
            let cases: &[(&str, fn())] = &[
                (
                    "a_registration_reaches_the_collection",
                    a_registration_reaches_the_collection,
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
                    "a_bundled_only_collection_registers",
                    a_bundled_only_collection_registers,
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

    /// The family rule (`resolve_family_name`) over what a collection holds,
    /// and what it keeps out of a shaped run (ADR-0059; ADR-0092 §7, gate 5).
    mod family_resolution {
        use std::sync::Arc;

        use parley::fontique::{Blob, Collection, CollectionOptions};

        use super::super::super::fallback_chain::FallbackChain;
        use super::super::super::font_resolve::{Family, resolve_family_name};
        use super::super::super::host::{HostData, HostFaces, HostSource};
        use super::super::{FontCollection, FontCollectionInner, TextContext};
        use super::ROBOTO;
        use crate::parley_text::{ParagraphSpec, holds_exactly};
        use crate::typography::{TextDirection, TextStyle};

        const MATERIAL_ICONS: &[u8] =
            include_bytes!("../../assets/fonts/MaterialIcons-Regular.ttf");
        /// Maps only `U+0020`, at 1.3 em, as family `FLUI Decoy Emoji` with
        /// "Emoji" in its PostScript name (`tools/decoy-face/generate.py`).
        const DECOY_WIDE_SPACE: &[u8] = include_bytes!("../../assets/fonts/decoy-wide-space.ttf");

        /// A collection holding exactly `faces`.
        fn holding(faces: &[&'static [u8]]) -> Collection {
            let mut collection = Collection::new(CollectionOptions {
                shared: false,
                system_fonts: false,
            });
            for bytes in faces {
                collection.register_fonts(Blob::new(Arc::new(*bytes)), None);
            }
            collection
        }

        fn chained(family: &str, chain: &[&str]) -> TextStyle {
            TextStyle {
                font_family: Some(family.to_owned()),
                font_family_fallback: chain.iter().map(|name| (*name).to_owned()).collect(),
                ..TextStyle::default()
            }
        }

        /// Each style resolves to one family: a held primary; the first held
        /// or generic entry of its chain past absent ones; the sans-serif
        /// degrade; and a held family spelled in another case, which the
        /// rule does not match and so degrades too. A held family that sets
        /// no Latin (Material Icons) still stops the chain, since fallback is
        /// per style and not per glyph; an absent primary with the same chain
        /// reaches Roboto.
        fn a_style_resolves_by_the_family_rule() {
            let mut collection = holding(&[ROBOTO, MATERIAL_ICONS]);
            let rows = [
                (TextStyle::default(), Family::SansSerif),
                (chained("Roboto", &[]), Family::Name("Roboto")),
                (chained("roboto", &[]), Family::SansSerif),
                (
                    chained("material icons", &["Material Icons"]),
                    Family::Name("Material Icons"),
                ),
                (
                    chained("CupertinoSystemText", &["-apple-system", "Material Icons"]),
                    Family::Name("Material Icons"),
                ),
                (
                    chained(
                        "CupertinoSystemText",
                        &["-apple-system", "monospace", "Roboto"],
                    ),
                    Family::Monospace,
                ),
                (
                    chained("Nothing Carries This", &["system-ui"]),
                    Family::SansSerif,
                ),
                (
                    chained("Material Icons", &["Roboto"]),
                    Family::Name("Material Icons"),
                ),
                (
                    chained("Nothing Carries This", &["Roboto"]),
                    Family::Name("Roboto"),
                ),
            ];
            for (style, expected) in &rows {
                let got =
                    resolve_family_name(Some(style), |name| holds_exactly(&mut collection, name));
                assert_eq!(got, *expected, "{style:?}");
            }
        }

        /// A style naming a family the collection lacks never takes its space
        /// from an emoji face (issue #927). On a collection fed from a host
        /// whose only listed fallback family is its emoji face (the host
        /// shape of issue #927), ahead of Roboto, `"Ao Bo"`
        /// styled `CupertinoSystemText` shapes letters and space in one face,
        /// the space under half an em. Fails if the family reaches Parley
        /// unresolved: Parley then walks the fallback order per cluster, and
        /// the space, which the decoy maps, lands in it at 1.3 em while the
        /// letters fall through to Roboto.
        fn a_missing_family_never_takes_its_space_from_an_emoji_face() {
            const SIZE: f32 = 32.0;
            let host = HostFaces {
                sources: vec![
                    HostSource {
                        data: HostData::Blob(Arc::new(ROBOTO)),
                        families: vec!["Roboto".to_owned()],
                    },
                    HostSource {
                        data: HostData::Blob(Arc::new(DECOY_WIDE_SPACE)),
                        families: vec!["FLUI Decoy Emoji".to_owned()],
                    },
                ],
                sans_serif: "Roboto".to_owned(),
                serif: "Roboto".to_owned(),
                monospace: "Roboto".to_owned(),
                cursive: "Roboto".to_owned(),
                fantasy: "Roboto".to_owned(),
                chain: FallbackChain::from_lists(&["FLUI Decoy Emoji"], &[]),
            };
            let fonts = FontCollection(Arc::new(FontCollectionInner::build(Some(&host))));
            let style = TextStyle {
                font_family: Some("CupertinoSystemText".to_owned()),
                ..TextStyle::default()
            };
            let spans: Vec<(String, Option<TextStyle>)> = vec![("Ao Bo".to_owned(), Some(style))];
            let paragraph = TextContext::new(&fonts)
                .shape(&ParagraphSpec {
                    spans: &spans,
                    default_style: None,
                    font_size: SIZE,
                    max_width: None,
                    min_width: 0.0,
                    text_align: crate::typography::TextAlign::Start,
                    line_height: None,
                    direction: TextDirection::Ltr,
                    max_lines: None,
                    ellipsis: None,
                })
                .to_shaped(None);
            let glyphs: Vec<_> = paragraph
                .runs()
                .flat_map(|run| {
                    let face = run.face().key();
                    run.glyphs().iter().map(move |glyph| (face, *glyph))
                })
                .collect();
            assert_eq!(glyphs.len(), 5, "one glyph per character: {glyphs:?}");
            assert!(
                glyphs
                    .iter()
                    .all(|(face, glyph)| glyph.id != 0 && *face == glyphs[0].0),
                "letters and space shape in one face, none as .notdef: {glyphs:?}"
            );
            let space_em = (glyphs[3].1.x - glyphs[2].1.x) / SIZE;
            assert!(
                space_em < 0.5,
                "a space of {space_em} em is a foreign face's advance, not a text face's"
            );
        }

        #[test]
        fn family_resolution() {
            let cases: &[(&str, fn())] = &[
                (
                    "a_style_resolves_by_the_family_rule",
                    a_style_resolves_by_the_family_rule,
                ),
                (
                    "a_missing_family_never_takes_its_space_from_an_emoji_face",
                    a_missing_family_never_takes_its_space_from_an_emoji_face,
                ),
            ];
            let failed: Vec<&str> = cases
                .iter()
                .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
                .map(|(name, _)| *name)
                .collect();
            assert!(
                failed.is_empty(),
                "family_resolution: failing cases: {failed:?}"
            );
        }
    }
}
