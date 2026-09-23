//! The process-wide font system and [`TextLayout`], a shaped cosmic-text
//! `Buffer` with truncation and cursor / hit-test / line-metric queries.
//!
//! The font system is taken per shape, never on the per-command path.

use std::sync::{Arc, OnceLock};

use cosmic_text::fontdb::Family;
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, Style, SwashCache, Weight};
use flui_types::{
    geometry::{Offset, Pixels, Rect},
    styling::Color,
    typography::{
        FontStyle, LineMetrics, TextAffinity, TextBox, TextDirection, TextPosition, TextRange,
        TextStyle,
    },
};
use parking_lot::Mutex;
use unicode_segmentation::UnicodeSegmentation;

use crate::error::RegisterFontError;

use super::TextLayoutResult;
use super::font_resolve::{self, InstalledFamilies};
use super::glyphs::{GlyphContent, GlyphImage, GlyphKey, PlacedGlyph};

/// The font database and the derived index that describes it, kept together so
/// the index cannot be consulted about a database it was not built from.
///
/// They travel behind one lock rather than two: every read of the index
/// happens while the database is borrowed, so the index can never be observed
/// stale relative to it, and there is no second lock whose acquisition order
/// could invert against the raster thread's.
#[derive(Debug)]
pub(super) struct FontState {
    pub(super) system: FontSystem,
    installed_families: InstalledFamilies,
    /// The swash scaler context [`SharedFontSystem::rasterize`] draws with.
    /// Kept beside the database because a scaler reads face data the lock
    /// guards; its own memo tables are unused — the engine's atlas is the
    /// cache.
    scaler: SwashCache,
    /// Bumped by [`SharedFontSystem::register_font`], the one door through
    /// which the database changes after construction. Counting mutations
    /// rather than faces means an index keyed on it can never describe a
    /// database that no longer exists; shaping ([`SharedFontSystem::shape`])
    /// cannot reach the database and so never bumps it.
    db_generation: u64,
}

impl FontState {
    fn new(system: FontSystem) -> Self {
        Self {
            system,
            installed_families: InstalledFamilies::default(),
            scaler: SwashCache::new(),
            db_generation: 0,
        }
    }

    /// The family `style` should be shaped with — see
    /// [`Shaper::resolve_font`], which this backs.
    ///
    /// Lives here so the borrow of the database and of the index describing it
    /// are taken together: no call site can pair one with the other's
    /// generation.
    pub(super) fn resolve_family<'a>(&mut self, style: Option<&'a TextStyle>) -> Family<'a> {
        let Self {
            system,
            installed_families,
            db_generation,
            ..
        } = self;
        font_resolve::resolve_family(style, system, installed_families, *db_generation)
    }

    /// The family AND the weight to shape `style` with, resolved together.
    ///
    /// Together because the weight decision depends on the family: cosmic-text
    /// abandons a family that carries no face at the requested weight, taking a
    /// `common_fallback()` family that happens to own it — so a run in a
    /// present family renders in a platform font instead (issue #929). Asking
    /// for a weight the resolved family can serve is what keeps it.
    ///
    /// One call rather than two so the pair cannot be taken from different
    /// states, and so both are answered under the single lock this module's
    /// own doc asks callers to hold briefly.
    pub(super) fn resolve_family_and_weight<'a>(
        &mut self,
        style: Option<&'a TextStyle>,
    ) -> (Family<'a>, Option<u16>) {
        let family = self.resolve_family(style);
        // `FontWeight::value()`, never `as u16`: the enum carries no explicit
        // discriminants, so a cast yields the VARIANT INDEX — `W400 as u16` is
        // 3, not 400 — and every snap below would then be computed against a
        // number no face can carry.
        let requested = style
            .and_then(|style| style.font_weight)
            .map(|weight| weight.value());
        let snapped = requested
            .map(|requested| font_resolve::snap_weight(self.system.db(), &family, requested));
        (family, snapped)
    }
}

/// Global font system instance.
///
/// cosmic-text requires a `FontSystem` for font discovery and shaping.
/// We use a global instance with interior mutability for convenience.
///
/// # Poisoning caveat
///
/// We deliberately use `parking_lot::Mutex`, which does *not* poison
/// the lock when a panic occurs while it is held. If `cosmic-text`
/// panics mid-`set_text` or mid-`shape_until_scroll` (e.g. an internal
/// invariant trips during font fallback), the surviving `FontSystem`
/// is conceptually corrupt but no subsequent caller will observe a
/// `PoisonError`. We accept that today because (a) cosmic-text panics
/// are rare in practice, and (b) `std::sync::Mutex`'s poisoning would
/// force every call site to `match` the lock result. A `catch_unwind`
/// wrapper around `set_text` / `shape_until_scroll` would be the
/// principled fix; it is not built, and no cosmic-text panic has surfaced
/// in practice.
static FONT_SYSTEM: OnceLock<Arc<Mutex<FontState>>> = OnceLock::new();

/// Gets or initializes the process-wide font system as a shared handle.
///
/// Held in an `Arc` (per ADR-0016) so the render engine's glyph pipeline
/// can shape against the *same* `FontSystem` this module measures with:
/// a font registered through
/// [`SharedFontSystem::register_font`]
/// becomes visible to both measurement and rendering, closing the historic
/// two-`FontSystem` gap where a registered face could measure but not paint.
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
        // The embedded faces go in before the generics are bound, so an
        // empty host binds sans-serif to Roboto rather than to nothing.
        #[cfg(feature = "bundled-fonts")]
        crate::fonts::load_missing_into(discovered.db_mut());
        font_resolve::bind_generic_families(discovered.db_mut());
        // Then rebuild once around the host's own emoji faces. Binding the
        // generics closes the fall-through for styles that name *no* family;
        // this closes the remaining one, for a style that names a family the
        // host does not have. cosmic-text snapshots `forbidden_fallback()`
        // into its `Fallbacks` inside the constructor and exposes no setter,
        // so installing it means constructing a second time — cheap, because
        // `into_locale_and_db` moves the populated database across and the
        // second pass never rescans the host.
        let (locale, db) = discovered.into_locale_and_db();
        let forbidden = font_resolve::EmojiForbiddenFallback::new(&db);
        let system = FontSystem::new_with_locale_and_db_and_fallback(locale, db, forbidden);
        Arc::new(Mutex::new(FontState::new(system)))
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
    let forbidden = font_resolve::EmojiForbiddenFallback::new(&db);
    let font_system =
        FontSystem::new_with_locale_and_db_and_fallback(locale.to_owned(), db, forbidden);
    FONT_SYSTEM
        .set(Arc::new(Mutex::new(FontState::new(font_system))))
        .is_ok()
}

/// The process-wide font system, as a shared handle.
///
/// The render engine's glyph pipeline shapes against the exact same faces
/// this module measures with (ADR-0016): a font registered through
/// [`SharedFontSystem::register_font`] is visible to both.
pub fn shared_font_system() -> SharedFontSystem {
    SharedFontSystem(Arc::clone(font_system_arc()))
}

/// Folds a **shaped** buffer's layout runs into [`TextLayoutResult`] metrics.
///
/// Supplies [`TextLayout::metrics`] with dimensions and baselines from the shaped runs.
/// Baselines come from the shaper: cosmic-text's `LayoutRun::line_y` IS the
/// alphabetic baseline of the line; the ideographic baseline is bounded by
/// the first line's descent edge (cosmic exposes no per-font ideographic
/// metric). The `height * 0.8` approximation survives ONLY in the
/// empty-text branch, where no shaped run exists to ask.
pub(super) fn metrics_from_shaped_buffer(
    buffer: &Buffer,
    line_height: f32,
    truncated: bool,
) -> TextLayoutResult {
    let mut total_height = 0.0f32;
    let mut max_line_width = 0.0f32;
    let mut line_count = 0usize;
    let mut first_baseline = 0.0f32;
    let mut first_descent_edge = 0.0f32;

    for run in buffer.layout_runs() {
        line_count += 1;
        max_line_width = max_line_width.max(run.line_w);
        total_height = total_height.max(run.line_top + run.line_height);

        if line_count == 1 {
            first_baseline = run.line_y;
            first_descent_edge = run.line_top + run.line_height;
        }
    }

    if line_count == 0 {
        // Empty text: nothing was shaped, synthesize from the line box.
        line_count = 1;
        total_height = line_height;
        first_baseline = line_height * 0.8;
        first_descent_edge = line_height;
    }

    TextLayoutResult {
        width: max_line_width,
        height: total_height,
        line_count,
        max_line_width,
        alphabetic_baseline: first_baseline,
        ideographic_baseline: first_descent_edge,
        truncated,
    }
}

/// A cheaply-cloneable handle to the process-wide [`FontSystem`] the
/// framework shapes and measures text with.
///
/// cosmic-text's `FontSystem` needs `&mut` access to shape and owns a large
/// font database plus shaping caches, so it cannot be snapshotted or handed
/// out by value. This handle shares one instance behind a lock (per
/// ADR-0016) and mediates access through a scoped callback, so the lock type
/// never appears in a public signature (SP-6). `Clone` is an `Arc` bump —
/// clone it to give another subsystem access to the *same* faces, so a font
/// registered through
/// [`SharedFontSystem::register_font`]
/// is visible to both measurement and rendering.
#[derive(Clone)]
pub struct SharedFontSystem(Arc<Mutex<FontState>>);

impl SharedFontSystem {
    /// Shapes under the font lock.
    ///
    /// `f` receives a [`Shaper`], which resolves fonts and shapes with one
    /// lock acquisition; it cannot reach the database, so shaping never
    /// invalidates the family index. The lock is not reentrant: `f` must not
    /// call any other text API of this crate (there is no need to — every
    /// shaping input is available on the `Shaper`).
    pub fn shape<R>(&self, f: impl FnOnce(&mut Shaper<'_>) -> R) -> R {
        let mut state = self.0.lock();
        f(&mut Shaper { state: &mut state })
    }

    /// Rasterises one glyph under the font lock.
    ///
    /// The bitmap for `key` as the shaper placed it — hinted, at the key's
    /// size and subpixel bin, synthesised italic/bold where the key says so.
    /// `None` when the key's face is not in the database, which cannot
    /// happen for a key this crate produced (faces are never removed); an
    /// atlas treats it as an empty glyph. Colour bitmaps (emoji) come back as
    /// [`GlyphContent::Color`]; everything else as a coverage mask.
    ///
    /// Not cached here: the caller's atlas is the cache, and a second copy
    /// under the lock would pin every bitmap ever drawn for the life of the
    /// process.
    #[must_use]
    pub fn rasterize(&self, key: GlyphKey) -> Option<GlyphImage> {
        let mut state = self.0.lock();
        let FontState { system, scaler, .. } = &mut *state;
        let image = scaler.get_image_uncached(system, key.0)?;
        let content = match image.content {
            cosmic_text::SwashContent::Color => GlyphContent::Color,
            // Subpixel (LCD) masks are not produced — the scaler is asked for
            // `Format::Alpha` — so this arm is a mask by construction.
            cosmic_text::SwashContent::Mask | cosmic_text::SwashContent::SubpixelMask => {
                GlyphContent::Mask
            }
        };
        Some(GlyphImage {
            left: image.placement.left,
            top: image.placement.top,
            width: image.placement.width,
            height: image.placement.height,
            content,
            data: image.data,
        })
    }

    /// The number of times the font database has changed since the font
    /// system was built. A cache of shaped text keys on it: a face registered
    /// after the cache was filled changes what the same text shapes to.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.0.lock().db_generation
    }

    /// Loads every face in `font_bytes` into the shared font database.
    ///
    /// The only public mutation of the database, and append-only: a face is
    /// never removed, so a font id recorded anywhere stays valid for the life
    /// of the process. The face is visible to measurement and to the engine's
    /// glyph pipeline from the next shape onward, and [`Self::generation`]
    /// advances so shaped-text caches refill — text already laid out is not
    /// re-laid-out by this call.
    ///
    /// # Errors
    ///
    /// [`RegisterFontError`] when `font_bytes` parses to zero loadable faces
    /// (empty, truncated, or not a font at all).
    #[tracing::instrument(skip(self, font_bytes), fields(bytes = font_bytes.len()))]
    pub fn register_font(&self, font_bytes: &[u8]) -> Result<(), RegisterFontError> {
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
}

/// One lock acquisition of the shared font system: resolves fonts and
/// shapes, and nothing else. Handed out only by [`SharedFontSystem::shape`].
pub struct Shaper<'a> {
    state: &'a mut FontState,
}

impl Shaper<'_> {
    /// The font family and weight `style` should be shaped with on this
    /// machine — see [`ResolvedFont`].
    pub fn resolve_font<'s>(&mut self, style: Option<&'s TextStyle>) -> ResolvedFont<'s> {
        let (family, weight) = self.state.resolve_family_and_weight(style);
        ResolvedFont { family, weight }
    }

    /// The cosmic-text font system, for shaping a `Buffer`.
    pub(crate) fn font_system(&mut self) -> &mut FontSystem {
        &mut self.state.system
    }
}

impl std::fmt::Debug for Shaper<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shaper").finish_non_exhaustive()
    }
}

/// The family **and** the weight to shape a style with.
///
/// One value rather than two calls, because taking them apart is a defect with
/// a name: the weight decision depends on the family (cosmic-text abandons a
/// family carrying no face at the requested weight — issue #929), so a caller
/// holding a resolved family and the style's *original* weight shapes against
/// a family the resolution already ruled out. That is exactly what happened:
/// measurement asked for the pair while the raster path asked only for the
/// family and read the weight off the style, so one string measured in one
/// font and painted in another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedFont<'a> {
    /// The family cosmic-text should be handed.
    pub family: Family<'a>,
    /// The weight to request, snapped to one the family can serve. `None` when
    /// the style named no weight, in which case cosmic-text's default stands.
    pub weight: Option<u16>,
}

impl std::fmt::Debug for SharedFontSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The `FontSystem` itself is a large, non-Debug font database; the
        // handle's identity is all that is meaningful to print.
        f.debug_struct("SharedFontSystem").finish_non_exhaustive()
    }
}

/// A laid out text buffer with cursor and hit testing support.
///
/// Wraps a cosmic-text `Buffer` and provides methods for:
/// - Cursor positioning (offset → screen position).
/// - Hit testing (screen position → text offset).
/// - Line metrics.
/// - Selection boxes.
pub struct TextLayout {
    /// The underlying cosmic-text buffer.
    buffer: Buffer,
    /// Source text (concatenation of all runs), kept for byte-based
    /// queries (e.g. `get_word_boundary`) that need to inspect
    /// characters around a caret without re-walking every glyph in
    /// every layout run.
    text: String,
    /// The styled runs the buffer was shaped from. Owned so the
    /// max-lines truncation can re-shape a SLICED prefix with the same
    /// per-run attributes — a rich layout truncates without losing the
    /// styling of the kept spans.
    runs: Vec<OwnedRun>,
    /// Font size used for layout.
    font_size: f32,
    /// Line height used for layout.
    line_height: f32,
    /// Text direction.
    direction: TextDirection,
    /// Whether `with_overflow` truncated the text to a max line count.
    truncated: bool,
}

/// Converts FLUI `TextStyle` to cosmic-text `Attrs`.
///
/// `family` comes from the crate's font resolution rather than from
/// `style.font_family` directly: a family the host does not
/// carry must not reach the shaper, or the run falls into cosmic-text's
/// unfiltered, emoji-first fallback tail. The caller resolves it because
/// resolution needs the font database, and the caller is what holds the lock.
///
/// The returned `Attrs` borrows the style's family string for
/// `Family::Name`, hence the shared lifetime.
fn style_to_attrs<'a>(
    style: Option<&'a TextStyle>,
    family: Family<'a>,
    weight: Option<u16>,
) -> Attrs<'a> {
    let mut attrs = Attrs::new().family(family);

    if let Some(style) = style {
        // `weight` is the style's own request AFTER the resolved family has
        // been consulted (`FontState::resolve_family_and_weight`): unchanged
        // when the family can serve it, and the nearest weight the family does
        // carry when it cannot. Asking for a weight the family has no face for
        // makes cosmic-text discard the family entirely in favour of a
        // `common_fallback()` one that happens to own it (issue #929), so the
        // request that survives here is the one that keeps the family.
        if let Some(weight) = weight {
            attrs = attrs.weight(Weight(weight));
        }

        if let Some(font_style) = style.font_style {
            let cosmic_style = match font_style {
                FontStyle::Normal => Style::Normal,
                FontStyle::Italic => Style::Italic,
            };
            attrs = attrs.style(cosmic_style);
        }
    }

    attrs
}

/// The colour a style paints its glyphs with: `foreground` wins over
/// `color`, as in Flutter's `TextStyle`.
pub(crate) fn paint_color(style: &TextStyle) -> Option<flui_types::Color> {
    style.foreground.or(style.color)
}

/// One styled run feeding rich shaping (`Buffer::set_rich_text`).
#[derive(Clone)]
struct OwnedRun {
    text: String,
    attrs: cosmic_text::AttrsOwned,
}

impl TextLayout {
    /// Creates a new text layout with no line-count limit.
    pub fn new(
        text: &str,
        style: Option<&TextStyle>,
        font_size: f32,
        max_width: Option<f32>,
        line_height: Option<f32>,
        direction: TextDirection,
    ) -> Self {
        Self::with_overflow(
            text,
            style,
            font_size,
            max_width,
            line_height,
            direction,
            None,
            None,
        )
    }

    /// Creates a text layout enforcing an optional maximum visual line
    /// count.
    ///
    /// When the shaped text exceeds `max_lines`, the buffer is RE-SHAPED
    /// on the truncated prefix so the layout's size, line metrics, and
    /// paint output all agree — lines beyond the limit do not exist,
    /// they are not merely skipped at paint. With an `ellipsis`, glyphs
    /// are dropped from the last kept line until the ellipsis fits the
    /// width constraint, then it is appended (Flutter
    /// `ParagraphStyle.maxLines` + `ellipsis` semantics); without one,
    /// the text is cut at the last kept line's end (clip semantics).
    ///
    /// Worst case the fit loop re-shapes once per dropped glyph on the
    /// last line — bounded by that line's glyph count; typical case is
    /// one extra shape.
    #[expect(clippy::too_many_arguments)] // mirrors the shaping input surface
    pub fn with_overflow(
        text: &str,
        style: Option<&TextStyle>,
        font_size: f32,
        max_width: Option<f32>,
        line_height: Option<f32>,
        direction: TextDirection,
        max_lines: Option<usize>,
        ellipsis: Option<&str>,
    ) -> Self {
        Self::from_spans(
            vec![(text.to_string(), style.cloned())],
            style,
            font_size,
            max_width,
            line_height,
            direction,
            max_lines,
            ellipsis,
        )
    }

    /// Creates a RICH text layout from styled spans.
    ///
    /// Each span carries its own (already inheritance-merged) style:
    /// per-span font selection, weight, style, font size, and letter
    /// spacing all reach the shaper — a bold or larger child span
    /// measures as bold or larger instead of being flattened to the
    /// root style.
    /// `default_style` and `font_size` describe the buffer-level
    /// defaults applied where a span has no style of its own.
    ///
    /// Truncation (`max_lines`/`ellipsis`) slices the SPANS, so a
    /// truncated rich layout keeps the styling of everything it kept;
    /// the ellipsis inherits the last kept span's attributes.
    #[expect(clippy::too_many_arguments)] // mirrors the shaping input surface
    pub fn from_spans(
        spans: Vec<(String, Option<TextStyle>)>,
        default_style: Option<&TextStyle>,
        font_size: f32,
        max_width: Option<f32>,
        line_height: Option<f32>,
        direction: TextDirection,
        max_lines: Option<usize>,
        ellipsis: Option<&str>,
    ) -> Self {
        debug_assert!(
            font_size > 0.0 && font_size.is_finite(),
            "TextLayout font_size must be positive and finite, got {font_size}"
        );

        let line_height = line_height.unwrap_or(font_size * 1.2);

        // One acquisition of the font lock covers resolution and shaping:
        // both need the database, and nothing between them does.
        shared_font_system().shape(|shaper| {
            let default = shaper.resolve_font(default_style);
            let default_attrs = cosmic_text::AttrsOwned::new(&style_to_attrs(
                default_style,
                default.family,
                default.weight,
            ));
            let root_color = default_style.and_then(paint_color);
            let runs: Vec<OwnedRun> = spans
                .into_iter()
                .map(|(text, style)| {
                    let attrs = match &style {
                        Some(style) => {
                            let resolved = shaper.resolve_font(Some(style));
                            let mut attrs =
                                style_to_attrs(Some(style), resolved.family, resolved.weight);
                            // A span colour rides in the attrs only when it
                            // differs from the root's: the root colour rides
                            // on the paragraph command instead, so a root-only
                            // recolour paints without reshaping.
                            if let Some(color) = paint_color(style)
                                && Some(color) != root_color
                            {
                                attrs = attrs.color(cosmic_text::Color::rgba(
                                    color.r, color.g, color.b, color.a,
                                ));
                            }
                            // Per-span font size/line height ride on the attrs
                            // (cosmic's per-span Metrics); spans without one
                            // inherit the buffer-level default.
                            // f64 style sizes → f32 shaping space
                            if let Some(size) = style.font_size.map(|s| s as f32) {
                                let span_line_height =
                                    style.height.map_or(size * 1.2, |h| h as f32 * size);
                                attrs = attrs.metrics(Metrics::new(size, span_line_height));
                                // cosmic letter spacing is in EM; ours is in
                                // logical px.
                                if let Some(spacing) = style.letter_spacing.map(|s| s as f32)
                                    && size > 0.0
                                {
                                    attrs = attrs.letter_spacing(spacing / size);
                                }
                            }
                            cosmic_text::AttrsOwned::new(&attrs)
                        }
                        None => default_attrs.clone(),
                    };
                    OwnedRun { text, attrs }
                })
                .collect();
            // cosmic-text 0.19: `new_empty` skips the empty-string shape pass
            // `Buffer::new` performs, and `set_size` is lazy.
            let mut buffer = Buffer::new_empty(Metrics::new(font_size, line_height));
            buffer.set_size(max_width, None);
            let text: String = runs.iter().map(|run| run.text.as_str()).collect();
            let mut this = Self {
                buffer,
                text,
                runs,
                font_size,
                line_height,
                direction,
                truncated: false,
            };
            let font_system = shaper.font_system();
            this.shape_runs(font_system);
            if let Some(max_lines) = max_lines
                && max_lines > 0
            {
                this.enforce_max_lines(font_system, max_lines, ellipsis, max_width);
            }
            this
        })
    }

    /// (Re-)shapes the buffer from the current runs.
    fn shape_runs(&mut self, font_system: &mut FontSystem) {
        self.buffer.set_rich_text(
            self.runs
                .iter()
                .map(|run| (run.text.as_str(), run.attrs.as_attrs())),
            &cosmic_text::Attrs::new(),
            Shaping::Advanced,
            None,
        );
        self.buffer.shape_until_scroll(font_system, false);
    }

    /// Truncates the shaped buffer to `max_lines` visual lines,
    /// optionally appending `ellipsis` to the last kept line.
    ///
    /// Rich-aware: the cut slices the styled RUNS (a run boundary is a
    /// char boundary of the concatenated text by construction), so the
    /// kept prefix re-shapes with its original per-span attributes and
    /// the ellipsis inherits the last kept run's.
    fn enforce_max_lines(
        &mut self,
        font_system: &mut FontSystem,
        max_lines: usize,
        ellipsis: Option<&str>,
        max_width: Option<f32>,
    ) {
        // Visual-line end positions as byte offsets into the ORIGINAL
        // text. Glyph offsets are relative to their source line, so the
        // per-line base offsets are reconstructed from the same '\n'
        // split cosmic uses for buffer lines.
        let line_bases: Vec<usize> = {
            let mut bases = vec![0usize];
            for (i, b) in self.text.bytes().enumerate() {
                if b == b'\n' {
                    bases.push(i + 1);
                }
            }
            bases
        };
        let run_ends: Vec<usize> = self
            .buffer
            .layout_runs()
            .map(|run| {
                let base = line_bases.get(run.line_i).copied().unwrap_or(0);
                base + run.glyphs.last().map_or(0, |g| g.end)
            })
            .collect();
        if run_ends.len() <= max_lines {
            return;
        }
        self.truncated = true;

        // The original (untruncated) inputs survive the fit loop; the
        // layout's own fields are only committed on success.
        let full_runs = std::mem::take(&mut self.runs);
        let full_text = std::mem::take(&mut self.text);

        let mut cut = run_ends[max_lines - 1];
        loop {
            let mut candidate = Self::sliced_runs(&full_runs, cut);
            if let Some(ellipsis) = ellipsis
                && !ellipsis.is_empty()
            {
                let attrs = candidate.last().or(full_runs.first()).map_or_else(
                    || cosmic_text::AttrsOwned::new(&cosmic_text::Attrs::new()),
                    |run| run.attrs.clone(),
                );
                candidate.push(OwnedRun {
                    text: ellipsis.to_string(),
                    attrs,
                });
            }
            self.runs = candidate;
            self.shape_runs(font_system);

            let lines = self.buffer.layout_runs().count();
            let last_width = self
                .buffer
                .layout_runs()
                .last()
                .map_or(0.0, |run| run.line_w);
            let fits_width = max_width.is_none_or(|w| last_width <= w);
            let exhausted = cut == 0;
            if (lines <= max_lines && fits_width) || exhausted {
                // Commit: concatenated text mirrors the shaped runs.
                self.text = self.runs.iter().map(|run| run.text.as_str()).collect();
                return;
            }

            // Drop one more character (never splitting a codepoint) and
            // retry; an empty prefix terminates the loop with just the
            // ellipsis (or the empty string) shaped.
            let Some((prev, _)) = full_text[..cut].char_indices().next_back() else {
                self.text = self.runs.iter().map(|run| run.text.as_str()).collect();
                return;
            };
            cut = prev;
        }
    }

    /// The styled runs covering `text[..cut]`: full runs before the
    /// cut, the run containing it sliced at the (char-boundary) cut.
    fn sliced_runs(runs: &[OwnedRun], cut: usize) -> Vec<OwnedRun> {
        let mut out = Vec::new();
        let mut base = 0usize;
        for run in runs {
            let end = base + run.text.len();
            if cut <= base {
                break;
            }
            if cut >= end {
                out.push(run.clone());
            } else {
                out.push(OwnedRun {
                    text: run.text[..cut - base].to_string(),
                    attrs: run.attrs.clone(),
                });
                break;
            }
            base = end;
        }
        out
    }

    /// The text the layout was shaped from (every run concatenated).
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// One line per shaped run naming what reached the shaper — family,
    /// weight, style, metrics, and a span colour where one was baked in — so
    /// a snapshot that prints it moves when a span is restyled.
    #[must_use]
    pub fn describe_runs(&self) -> Vec<String> {
        self.runs
            .iter()
            .map(|run| {
                let attrs = run.attrs.as_attrs();
                let mut parts = vec![
                    format!("len={}", run.text.len()),
                    format!("family={:?}", attrs.family),
                    format!("weight={}", attrs.weight.0),
                    format!("style={:?}", attrs.style),
                ];
                if let Some(metrics) = attrs.metrics_opt {
                    parts.push(format!("size={:.2}", Metrics::from(metrics).font_size));
                }
                if let Some(color) = attrs.color_opt {
                    parts.push(format!(
                        "color=#{:02x}{:02x}{:02x}{:02x}",
                        color.r(),
                        color.g(),
                        color.b(),
                        color.a()
                    ));
                }
                parts.join(" ")
            })
            .collect()
    }

    /// Every glyph of the paragraph placed in device pixels, for a
    /// rasteriser.
    ///
    /// `origin` is the paragraph's top-left in device pixels and `scale` the
    /// device-pixel ratio the paragraph is drawn under: glyphs are placed at
    /// `origin + glyph_position * scale`, snapped to a pixel with the
    /// fractional remainder folded into the key's subpixel bin, and the key
    /// names a bitmap rasterised at `font_size * scale` — so a 2× display
    /// gets a 2× raster rather than an upscaled 1× one.
    ///
    /// `y` is the baseline row; [`PlacedGlyph`] says how the bitmap's
    /// bearings apply. Runs come in layout order, top to bottom.
    pub fn placed_glyphs(
        &self,
        origin: (f32, f32),
        scale: f32,
    ) -> impl Iterator<Item = PlacedGlyph> + '_ {
        self.buffer.layout_runs().flat_map(move |run| {
            let baseline = (run.line_y * scale).round() as i32;
            run.glyphs.iter().map(move |glyph| {
                let physical = glyph.physical(origin, scale);
                PlacedGlyph {
                    key: GlyphKey(physical.cache_key),
                    x: physical.x,
                    y: baseline + physical.y,
                    color: glyph
                        .color_opt
                        .map(|c| Color::rgba(c.r(), c.g(), c.b(), c.a())),
                }
            })
        })
    }

    /// Returns the computed metrics for this layout.
    ///
    /// Baselines come from the shaper: cosmic-text's `LayoutRun::line_y`
    /// IS the alphabetic baseline of the line; the ideographic baseline
    /// is bounded by the first line's descent edge (cosmic exposes no
    /// per-font ideographic metric). The old `height * 0.8` / `× 1.125`
    /// approximations survive ONLY in the empty-text branch, where no
    /// shaped run exists to ask.
    pub fn metrics(&self) -> TextLayoutResult {
        metrics_from_shaped_buffer(&self.buffer, self.line_height, self.truncated)
    }

    /// Returns the screen offset for a caret at the given text
    /// position.
    pub fn get_offset_for_caret(&self, position: TextPosition) -> Offset<Pixels> {
        // Walk the laid-out runs for the glyph whose cluster contains the
        // offset; the first match wins, so a caret on a wrap boundary sits at
        // the end of the earlier line. Past every glyph, the caret trails the
        // last run.
        let mut trailing = Offset::ZERO;
        for run in self.buffer.layout_runs() {
            let mut x = 0.0f32;
            for glyph in run.glyphs {
                if glyph.start <= position.offset && position.offset <= glyph.end {
                    let progress = if glyph.end > glyph.start {
                        (position.offset - glyph.start) as f32 / (glyph.end - glyph.start) as f32
                    } else {
                        0.0
                    };
                    return Offset::new(Pixels(glyph.x + glyph.w * progress), Pixels(run.line_top));
                }
                x = glyph.x + glyph.w;
            }
            trailing = Offset::new(Pixels(x), Pixels(run.line_top));
        }
        trailing
    }

    /// Returns the text position for a screen offset.
    pub fn get_position_for_offset(&self, offset: Offset<Pixels>) -> TextPosition {
        let x = offset.dx.0;
        let y = offset.dy.0;

        let mut target_line: Option<usize> = None;
        let mut line_top = 0.0f32;

        for run in self.buffer.layout_runs() {
            if y >= run.line_top && y < run.line_top + run.line_height {
                target_line = Some(run.line_i);
                line_top = run.line_top;
                break;
            }
        }

        let target_line = if let Some(l) = target_line {
            l
        } else {
            let mut last_line = 0;
            for run in self.buffer.layout_runs() {
                last_line = run.line_i;
            }
            if y >= line_top {
                last_line
            } else {
                return TextPosition::upstream(0);
            }
        };

        for run in self.buffer.layout_runs() {
            if run.line_i == target_line {
                let mut last_offset = run.glyphs.first().map_or(0, |g| g.start);

                for glyph in run.glyphs {
                    let glyph_center = glyph.x + glyph.w / 2.0;

                    if x < glyph_center {
                        return TextPosition::new(glyph.start, TextAffinity::Downstream);
                    }

                    last_offset = glyph.end;
                }

                return TextPosition::new(last_offset, TextAffinity::Upstream);
            }
        }

        TextPosition::upstream(0)
    }

    /// Returns line metrics for all lines in the layout.
    pub fn get_line_metrics(&self) -> Vec<LineMetrics> {
        let mut metrics = Vec::new();

        for (line_number, run) in self.buffer.layout_runs().enumerate() {
            // Shaper-derived: `line_y` is the baseline, so ascent/descent
            // are exact line-box distances, not font-size guesses.
            let ascent = run.line_y - run.line_top;
            let descent = (run.line_top + run.line_height) - run.line_y;

            let start_index = run.glyphs.first().map_or(0, |g| g.start);
            let end_index = run.glyphs.last().map_or(start_index, |g| g.end);

            metrics.push(LineMetrics::new(
                true,
                ascent as f64,
                descent as f64,
                ascent as f64,
                run.line_height as f64,
                run.line_w as f64,
                0.0,
                run.line_y as f64,
                line_number,
                start_index,
                end_index,
                end_index,
                end_index,
            ));
        }

        if metrics.is_empty() {
            metrics.push(LineMetrics::new(
                true,
                (self.font_size * 0.8) as f64,
                (self.font_size * 0.2) as f64,
                (self.font_size * 0.8) as f64,
                self.line_height as f64,
                0.0,
                0.0,
                (self.font_size * 0.8) as f64,
                0,
                0,
                0,
                0,
                0,
            ));
        }

        metrics
    }

    /// Returns bounding boxes for the given text range.
    pub fn get_boxes_for_range(&self, range: TextRange) -> Vec<TextBox> {
        let mut boxes = Vec::new();

        for run in self.buffer.layout_runs() {
            let mut line_start_x: Option<f32> = None;
            let mut line_end_x = 0.0f32;

            for glyph in run.glyphs {
                if glyph.end > range.start && glyph.start < range.end {
                    if line_start_x.is_none() {
                        let start_offset = if glyph.start < range.start {
                            let progress = (range.start - glyph.start) as f32
                                / (glyph.end - glyph.start) as f32;
                            glyph.x + glyph.w * progress
                        } else {
                            glyph.x
                        };
                        line_start_x = Some(start_offset);
                    }

                    line_end_x = if glyph.end > range.end {
                        let progress =
                            (range.end - glyph.start) as f32 / (glyph.end - glyph.start) as f32;
                        glyph.x + glyph.w * progress
                    } else {
                        glyph.x + glyph.w
                    };
                }
            }

            if let Some(start_x) = line_start_x {
                let rect = Rect::from_ltrb(
                    Pixels(start_x),
                    Pixels(run.line_top),
                    Pixels(line_end_x),
                    Pixels(run.line_top + run.line_height),
                );
                boxes.push(TextBox::new(rect, self.direction));
            }
        }

        boxes
    }

    /// Returns the word boundary at the given text position.
    ///
    /// `position.offset` is a byte offset into `self.text` (matching
    /// cosmic-text's `glyph.start` convention used elsewhere in this
    /// module), snapped to the nearest preceding char boundary so an
    /// off-boundary offset never panics a slice.
    ///
    /// Segmentation is full UAX #29 word segmentation
    /// (`unicode-segmentation`'s `split_word_bound_indices`), not an
    /// ASCII-whitespace-run scan: a straight/curly apostrophe inside a
    /// word (`"don't"`) stays one segment, and a letter-digit run
    /// (`"foo123"`) stays one segment too, rather than every non-space
    /// byte being treated as one undifferentiated run. This closes the
    /// gap the previous ASCII implementation's own doc comment named as
    /// an "Outstanding refactor". Clusters-only, not dictionary-based:
    /// Thai/Lao/Khmer/Myanmar (no lexicon, no spaces — UAX #29's
    /// rule-based default cannot find a real word boundary there at all)
    /// AND Chinese/Japanese (ICU's oracle behavior uses a `cjdict`
    /// word-frequency dictionary this crate does not have; the rule-based
    /// default instead segments per character/script-run — `"東京"`
    /// splits into `"東"` and `"京"` rather than staying the one word ICU
    /// would find) are known limitations — see
    /// `flui-widgets/ARCHITECTURE.md`'s Mapping decision for this
    /// feature.
    ///
    /// # Boundary tie-break
    ///
    /// `0` always resolves to the first segment. Elsewhere, when `offset`
    /// sits exactly between two segments:
    ///
    /// - One side WHITESPACE, the other a real segment: the non-whitespace
    ///   side wins regardless of which side it is on — a caret right
    ///   after a word (`"café "`, offset at the space) answers with the
    ///   word just typed, and a caret right before one (`"foo bar"`,
    ///   offset at `b`) answers with the word about to be typed into,
    ///   never the whitespace either straddles.
    /// - Both sides non-whitespace (`"(foo"` at the `(`/`f` boundary,
    ///   `"日本語"` at the `日`/`本` boundary — UAX #29 gives every CJK
    ///   character its own segment without a dictionary, so this case is
    ///   common there, not an edge case): the FOLLOWING segment wins
    ///   (downstream affinity) — a double-tap landing exactly on the
    ///   second character of a CJK run must select THAT character, not
    ///   the one before it.
    pub fn get_word_boundary(&self, position: TextPosition) -> TextRange {
        let text = self.text.as_str();
        let total = text.len();

        if text.is_empty() {
            return TextRange::new(0, 0);
        }

        let mut offset = position.offset.min(total);
        // Snap to the nearest preceding char boundary so we never
        // dereference inside a multi-byte codepoint.
        while offset > 0 && !text.is_char_boundary(offset) {
            offset -= 1;
        }

        let segments: Vec<(usize, usize, bool)> = text
            .split_word_bound_indices()
            .map(|(idx, word)| (idx, idx + word.len(), word.chars().all(char::is_whitespace)))
            .collect();

        let Some(&(first_start, first_end, _)) = segments.first() else {
            return TextRange::new(total, total);
        };
        if offset == 0 {
            return TextRange::new(first_start, first_end);
        }

        let preceding = segments.iter().find(|&&(_, end, _)| end == offset).copied();
        let following = segments
            .iter()
            .find(|&&(start, _, _)| start == offset)
            .copied();

        match (preceding, following) {
            // Whitespace vs. word (either side): the word wins.
            (Some((_, _, true)), Some((f_start, f_end, false))) => TextRange::new(f_start, f_end),
            // Word vs. word: the following one wins (downstream affinity).
            (Some((_, _, false)), Some((f_start, f_end, false))) => TextRange::new(f_start, f_end),
            (Some((p_start, p_end, _)), _) => TextRange::new(p_start, p_end),
            (None, Some((f_start, f_end, _))) => TextRange::new(f_start, f_end),
            (None, None) => segments
                .iter()
                .find(|&&(start, end, _)| start < offset && offset < end)
                .map_or(TextRange::new(total, total), |&(start, end, _)| {
                    TextRange::new(start, end)
                }),
        }
    }
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextLayout")
            .field("font_size", &self.font_size)
            .field("line_height", &self.line_height)
            .field("direction", &self.direction)
            .finish_non_exhaustive()
    }
}
