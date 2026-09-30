//! The process-wide font system: host font discovery, the generic-family
//! bindings and the fallback lists a collection fed from the host
//! (`FontCollection::with_host_faces`) is built from. It lays nothing out;
//! it leaves with cosmic-text at ADR-0092 §10 step 6.

#![expect(
    clippy::disallowed_types,
    reason = "the cosmic-text path's shared FontSystem; leaves at ADR-0092 §10 step 6"
)]

use std::sync::{Arc, OnceLock};

use cosmic_text::FontSystem;
use cosmic_text::fontdb::Family;
use parking_lot::Mutex;

use crate::error::RegisterFontError;
use crate::typography::TextStyle;

use super::fallback_chain::{ChainFallback, FallbackChain};
use super::font_resolve;

/// The process font system and what is kept beside it, behind one lock.
#[derive(Debug)]
pub(super) struct FontState {
    pub(super) system: FontSystem,
    /// Bumped by [`SharedFontSystem::add_face`], the one door through
    /// which the database changes after construction.
    db_generation: u64,
    /// The fallback lists `system` was built with, shared with every
    /// collection fed from this font system (`SharedFontSystem::host_faces`).
    chain: Arc<FallbackChain>,
    /// How many collections were built from the host
    /// ([`SharedFontSystem::count_host_feed`]): one per app.
    host_feeds: u64,
}

impl FontState {
    fn new(system: FontSystem, chain: Arc<FallbackChain>) -> Self {
        Self {
            system,
            db_generation: 0,
            chain,
            host_feeds: 0,
        }
    }
}

/// Global font system instance: the host's discovered faces, the generic
/// bindings and the fallback lists, read once per app to feed its
/// collection. Nothing shapes on it.
///
/// `parking_lot::Mutex`, which does not poison: every critical section only
/// reads the database or appends a face to it.
static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>> = OnceLock::new();

/// Gets or initializes the process-wide font system as a shared handle.
///
/// Held in an `Arc` (per ADR-0016) so every handle reaches the *same*
/// `FontSystem`. `TextPainter` measures, paints and places carets on Parley
/// through the realm's `TextContext` (ADR-0092 §10 steps 4 and 5).
fn font_system_arc() -> &'static Arc<Mutex<FontState>> {
    FONT_SYSTEM.get_or_init(|| {
        tracing::debug!("Initializing global FontSystem");
        // Host discovery first, then point the generic families at what it
        // actually found. `FontSystem::new` hard-codes sans-serif to
        // "Open Sans", which a stock Debian/Ubuntu desktop does not install,
        // and a generic resolving to nothing takes every style naming no
        // family into cosmic-text's unfiltered, emoji-first fallback tail.
        //
        // Bound after construction rather than before: the constructor derives
        // its monospace and per-script tables from each face's `monospaced`
        // flag and its GPOS/GSUB scripts, never from the generic names, so
        // binding afterwards changes nothing it froze.
        let mut discovered = FontSystem::new();
        // The embedded faces go in, and the generics point at Roboto, before
        // the host's generics are bound: a collection fed from this font
        // system binds its generics to the families these name, and Roboto
        // is the face text measures in on every host (mapping decision 16).
        // The binding below keeps a generic that already names a carried
        // family.
        #[cfg(feature = "bundled-fonts")]
        {
            crate::fonts::install_bundled(discovered.db_mut());
            crate::fonts::bind_generics_to_bundled(discovered.db_mut());
        }
        font_resolve::bind_generic_families(discovered.db_mut());
        // Then rebuild once around the host's own emoji faces. Binding the
        // generics closes the fall-through for styles that name *no* family;
        // this closes the remaining one, for a style that names a family the
        // host does not have. cosmic-text snapshots `forbidden_fallback()`
        // into its `Fallbacks` inside the constructor and exposes no setter,
        // so installing it means constructing a second time — cheap, because
        // `into_locale_and_db` moves the populated database across and the
        // second pass never rescans the host.
        //
        // The fallback lists are kept beside the font system as a
        // `FallbackChain`, which a collection fed from this database reads
        // too, so both shapers fall back in the same order.
        let (locale, db) = discovered.into_locale_and_db();
        let chain = Arc::new(FallbackChain::new(
            locale.clone(),
            font_resolve::EmojiForbiddenFallback::new(&db),
        ));
        let system = FontSystem::new_with_locale_and_db_and_fallback(
            locale,
            db,
            ChainFallback(Arc::clone(&chain)),
        );
        Arc::new(Mutex::new(FontState::new(system, chain)))
    })
}

/// Initializes the process-wide font system from an explicit face set,
/// bypassing host-font discovery.
///
/// Returns `false` — changing nothing — if the font system has already been
/// initialized, because a `FontSystem` freezes state at construction that no
/// later mutation reaches.
///
/// # Why this exists rather than "clear the database and reload it"
///
/// `FontSystem::new()` loads the *host machine's* fonts, so text measurement —
/// and the layout of everything sized to its text — differs between machines.
/// Emptying the database afterwards and loading known faces into it does not
/// undo that: `FontSystem` computes its fallback chain and its monospace face
/// list once, at construction, from the environment and the database it was
/// built with, and `db_mut` invalidates only the family-match cache. Measured
/// on this repository's Cupertino demo, a database mutated down to three
/// known faces still measured a button 61.18 px wide on a host with fonts
/// installed and 129.55 px on a host without — same faces, same code. Building
/// the font system *from* the pinned database is what makes the host stop
/// mattering.
///
/// `faces` are raw font-file bytes; `default_family` must name a family one of
/// them provides and becomes the target of every generic family, so text whose
/// style names no family cannot fall through to a host font. `locale` fixes
/// the language-dependent parts of shaping (`"en-US"` unless a caller needs
/// otherwise).
///
/// # Panics
///
/// Panics if `faces` is empty or none of them load: an empty database makes
/// every measurement zero-width and panics inside cosmic-text's shaper, which
/// is a far more confusing failure than this one.
///
/// # Availability
///
/// Behind the `testing` feature, and deliberately so. Claiming `FONT_SYSTEM` is
/// irreversible for the process, so on the shipped surface this would let any
/// downstream caller win the race against `SharedEngineServices`' own
/// construction and pin every later measurement to faces of its choosing. The
/// only caller that needs it is test support, which reaches it through
/// `flui_testing::fonts::pin_font_faces`.
#[cfg(any(test, feature = "testing"))]
pub fn init_font_system_with_faces(faces: &[&[u8]], default_family: &str, locale: &str) -> bool {
    FONT_SYSTEM
        .set(Arc::new(Mutex::new(pinned_font_state(
            faces,
            default_family,
            locale,
        ))))
        .is_ok()
}

/// A font state built from `faces` alone, as [`init_font_system_with_faces`]
/// describes, without claiming the process-wide slot.
///
/// # Panics
///
/// As [`init_font_system_with_faces`].
#[cfg(any(test, feature = "testing"))]
fn pinned_font_state(faces: &[&[u8]], default_family: &str, locale: &str) -> FontState {
    assert!(
        !faces.is_empty(),
        "init_font_system_with_faces: at least one face is required -- an empty \
         font database makes every measurement zero-width and panics inside the \
         shaper",
    );

    let mut db = cosmic_text::fontdb::Database::new();
    for face in faces {
        db.load_font_data((*face).to_vec());
    }
    assert!(
        db.faces().next().is_some(),
        "init_font_system_with_faces: none of the supplied faces loaded -- every \
         measurement would be zero-width",
    );

    // Every generic family, not just sans-serif: a style that names no family
    // resolves through one of these, and leaving any pointing at a family this
    // database does not carry reopens the hole this function closes.
    db.set_sans_serif_family(default_family);
    db.set_serif_family(default_family);
    db.set_monospace_family(default_family);
    db.set_cursive_family(default_family);
    db.set_fantasy_family(default_family);

    // Same emoji suppression the host-discovery path installs, so a pinned
    // database and a discovered one do not disagree about where an unmatched
    // family lands.
    let chain = Arc::new(FallbackChain::new(
        locale.to_owned(),
        font_resolve::EmojiForbiddenFallback::new(&db),
    ));
    let font_system = FontSystem::new_with_locale_and_db_and_fallback(
        locale.to_owned(),
        db,
        ChainFallback(Arc::clone(&chain)),
    );
    FontState::new(font_system, chain)
}

/// Whether the process-wide font system has been built. A test of a path that
/// must not touch process-global font state asserts `false` after running it.
#[cfg(any(test, feature = "testing"))]
#[must_use]
pub fn font_system_initialized() -> bool {
    FONT_SYSTEM.get().is_some()
}

/// The process-wide font system, as a shared handle.
///
/// The caret layout shapes on it, and the app's collection is fed from its
/// host faces: a font registered through that collection
/// ([`FontCollection::register_font`](super::FontCollection::register_font))
/// is loaded here too, so carets sit on the glyphs measurement and paint
/// shaped.
pub fn shared_font_system() -> SharedFontSystem {
    SharedFontSystem(Arc::clone(font_system_arc()))
}

/// A cheaply-cloneable handle to the process-wide [`FontSystem`] the caret
/// layout shapes with, and the host faces the app's collection is fed from.
///
/// cosmic-text's `FontSystem` needs `&mut` access to shape and owns a large
/// font database plus shaping caches, so it cannot be snapshotted or handed
/// out by value. This handle shares one instance behind a lock (per
/// ADR-0016) and mediates access through a scoped callback, so the lock type
/// never appears in a public signature. `Clone` is an `Arc` bump —
/// clone it to give another subsystem access to the *same* faces, so a font
/// registered through the collection fed from it
/// ([`FontCollection::register_font`](super::FontCollection::register_font))
/// is visible to measurement, paint and carets alike.
#[derive(Clone)]
pub struct SharedFontSystem(Arc<Mutex<FontState>>);

impl SharedFontSystem {
    /// A font system of its own, built from `faces` alone as
    /// [`init_font_system_with_faces`] builds the process one, for a test
    /// that must not touch the process-wide font system.
    #[cfg(test)]
    pub(crate) fn pinned(faces: &[&[u8]], default_family: &str) -> Self {
        Self(Arc::new(Mutex::new(pinned_font_state(
            faces,
            default_family,
            "en-US",
        ))))
    }
}

impl SharedFontSystem {
    /// The number of times the font database has changed since the font
    /// system was built. A cache of shaped text keys on it: a face registered
    /// after the cache was filled changes what the same text shapes to.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.0.lock().db_generation
    }

    /// Loads every face in `font_bytes` into the shared font database.
    ///
    /// The one mutation of the database, and append-only: a face is never
    /// removed, so a font id recorded anywhere stays valid for the life of
    /// the process. The face is visible to the caret layout from the next
    /// shape onward, and [`Self::generation`] advances so a `TextPainter`
    /// drops its caret layout at its next `layout()`. Reached through
    /// [`FontCollection::register_font`](super::FontCollection::register_font),
    /// which loads the face for measurement and paint as well and tells the
    /// pipelines what to lay out again.
    ///
    /// # Errors
    ///
    /// [`RegisterFontError`] when `font_bytes` parses to zero loadable faces
    /// (empty, truncated, or not a font at all).
    #[tracing::instrument(skip(self, font_bytes), fields(bytes = font_bytes.len()))]
    pub(crate) fn add_face(&self, font_bytes: &[u8]) -> Result<(), RegisterFontError> {
        let mut state = self.0.lock();
        let faces_before = state.system.db().len();
        state.system.db_mut().load_font_data(font_bytes.to_vec());
        let faces_added = state.system.db().len() - faces_before;
        if faces_added == 0 {
            return Err(RegisterFontError);
        }
        state.db_generation = state.db_generation.wrapping_add(1);
        tracing::debug!(faces_added, "registered font");
        Ok(())
    }

    /// What this font system holds, for a collection to be fed from
    /// ([`FontCollection::with_host_faces`](super::FontCollection::with_host_faces)).
    ///
    /// Takes the lock once and only copies: each font file's path, each
    /// in-memory font's data (shared, not copied), every source's family
    /// names, the five generic families' names and the fallback chain.
    /// Parsing the files again is the caller's work, outside the lock.
    pub(crate) fn host_faces(&self) -> HostFaces {
        use cosmic_text::fontdb::Source;
        use std::collections::HashMap;

        #[derive(PartialEq, Eq, Hash)]
        enum SourceKey {
            Path(std::path::PathBuf),
            Blob(usize),
        }

        let state = self.0.lock();
        let db = state.system.db();
        let mut sources: Vec<HostSource> = Vec::new();
        let mut index: HashMap<SourceKey, usize> = HashMap::new();
        for face in db.faces() {
            let (key, data) = match &face.source {
                Source::Binary(data) => (
                    SourceKey::Blob(Arc::as_ptr(data).cast::<()>() as usize),
                    HostData::Blob(Arc::clone(data)),
                ),
                Source::File(path) | Source::SharedFile(path, _) => {
                    (SourceKey::Path(path.clone()), HostData::Path(path.clone()))
                }
            };
            let slot = *index.entry(key).or_insert_with(|| {
                sources.push(HostSource {
                    data,
                    families: Vec::new(),
                });
                sources.len() - 1
            });
            let families = &mut sources[slot].families;
            for (name, _) in &face.families {
                if !families.contains(name) {
                    families.push(name.clone());
                }
            }
        }
        let generic = |family: Family<'_>| db.family_name(&family).to_owned();
        HostFaces {
            sources,
            sans_serif: generic(Family::SansSerif),
            serif: generic(Family::Serif),
            monospace: generic(Family::Monospace),
            cursive: generic(Family::Cursive),
            fantasy: generic(Family::Fantasy),
            chain: Arc::clone(&state.chain),
        }
    }

    /// Records that a collection was built from this font system's host
    /// faces ([`FontCollection::with_host_faces`](super::FontCollection::with_host_faces)),
    /// whether or not the build has the Parley path to feed them into.
    pub(crate) fn count_host_feed(&self) {
        self.0.lock().host_feeds += 1;
    }

    /// How many collections were built from the host.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn host_feeds(&self) -> u64 {
        self.0.lock().host_feeds
    }

    /// The family the sans-serif generic names.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn sans_serif_family(&self) -> String {
        self.0
            .lock()
            .system
            .db()
            .family_name(&Family::SansSerif)
            .to_owned()
    }

    /// The first family name of every face, each once, in database order.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn family_names(&self) -> Vec<String> {
        let state = self.0.lock();
        let mut names: Vec<String> = Vec::new();
        for face in state.system.db().faces() {
            if let Some((name, _)) = face.families.first()
                && !names.contains(name)
            {
                names.push(name.clone());
            }
        }
        names
    }

    /// Whether some face in this font system maps every character of
    /// `text` that is not whitespace.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn covers(&self, text: &str) -> bool {
        let state = self.0.lock();
        let db = state.system.db();
        text.chars()
            .filter(|c| !c.is_whitespace())
            .all(|c| db.faces().any(|face| face_maps(db, face, c)))
    }

    /// Whether every character of `text` that is not whitespace is mapped by
    /// a face of a family both shapers reach past the style's own: the
    /// sans-serif generic's family, the chain's list for the character's
    /// script, or the chain's common list. cosmic-text's last resort, the
    /// walk over every other face, is left out, since the Parley path has
    /// none (flui-painting `ARCHITECTURE.md`, mapping decision 17).
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn chain_covers(&self, text: &str) -> bool {
        use unicode_script::UnicodeScript as _;

        let state = self.0.lock();
        let db = state.system.db();
        let sans_serif = db.family_name(&Family::SansSerif);
        text.chars().filter(|c| !c.is_whitespace()).all(|c| {
            let script = state.chain.script(c.script());
            let reachable = |name: &str| {
                name == sans_serif || script.contains(&name) || state.chain.common().contains(&name)
            };
            db.faces().any(|face| {
                face.families.iter().any(|(name, _)| reachable(name)) && face_maps(db, face, c)
            })
        })
    }
}

/// Whether `face` maps `c` to a glyph.
#[cfg(any(test, feature = "testing"))]
fn face_maps(
    db: &cosmic_text::fontdb::Database,
    face: &cosmic_text::fontdb::FaceInfo,
    c: char,
) -> bool {
    use cosmic_text::skrifa::{self, MetadataProvider as _};

    db.with_face_data(face.id, |data, index| {
        skrifa::FontRef::from_index(data, index)
            .ok()
            .and_then(|font| font.charmap().map(c))
            .is_some()
    }) == Some(true)
}

/// The faces a process font system holds, taken by
/// `SharedFontSystem::host_faces` for a collection to be fed from.
pub(crate) struct HostFaces {
    /// Every font source, in the order the database first names it.
    pub(crate) sources: Vec<HostSource>,
    /// The family the sans-serif generic names.
    pub(crate) sans_serif: String,
    /// The family the serif generic names.
    pub(crate) serif: String,
    /// The family the monospace generic names.
    pub(crate) monospace: String,
    /// The family the cursive generic names.
    pub(crate) cursive: String,
    /// The family the fantasy generic names.
    pub(crate) fantasy: String,
    /// The fallback lists the font system was built with.
    pub(crate) chain: Arc<FallbackChain>,
}

/// One font file or in-memory font, and every family name its faces carry.
pub(crate) struct HostSource {
    pub(crate) data: HostData,
    pub(crate) families: Vec<String>,
}

/// Where a [`HostSource`]'s bytes are.
pub(crate) enum HostData {
    /// A file, which the collection maps itself.
    Path(std::path::PathBuf),
    /// Bytes the font system holds: a registered or embedded font.
    Blob(Arc<dyn AsRef<[u8]> + Send + Sync>),
}

impl std::fmt::Debug for SharedFontSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The `FontSystem` itself is a large, non-Debug font database; the
        // handle's identity is all that is meaningful to print.
        f.debug_struct("SharedFontSystem").finish_non_exhaustive()
    }
}

/// The colour a style paints its glyphs with: `foreground` wins over
/// `color`.
pub(crate) fn paint_color(style: &TextStyle) -> Option<crate::styling::Color> {
    style.foreground.or(style.color)
}
