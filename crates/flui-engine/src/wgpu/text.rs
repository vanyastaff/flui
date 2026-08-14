//! Text rendering using glyphon
//!
//! This module provides a clean wrapper around glyphon for GPU-accelerated text
//! rendering. Follows KISS principle: simple API that handles batching and
//! rendering internally.
//!
//! # Caching Strategy
//!
//! Text layout is expensive (shaping, line breaking, metrics calculation).
//! We cache `Buffer` objects keyed by (text, font_size) to avoid re-layout
//! when the same text is rendered in subsequent frames.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{Hash, Hasher};

use flui_painting::SharedFontSystem;
use flui_types::{
    geometry::{Pixels, Point},
    styling::Color,
    typography::{FontStyle, FontWeight, InlineSpan, TextSpan, TextStyle},
};
use glyphon::{
    Attrs, AttrsOwned, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics,
    Resolution, Shaping, Style, SwashCache, TextArea, TextAtlas, TextBounds,
    TextRenderer as GlyphonRenderer, Viewport, Weight,
};

// ---------------------------------------------------------------------------
// Span flattening (pure logic, no GPU types — testable without wgpu)
// ---------------------------------------------------------------------------

/// Flattens an [`InlineSpan`] tree into per-run `(text, merged style)` pairs
/// in document order, applying FLUI style inheritance: each child's style
/// merges over its ancestors' (`TextStyle::merge`), so a bold child of a
/// sized parent shapes bold at the parent's size.
///
/// `scale` is baked into every effective font size here — the shaper sees
/// final pixel sizes. Placeholder spans are emitted as `\u{FFFC}` (Unicode
/// Object Replacement Character) with the inherited style.
///
/// Average and worst case O(total spans + text bytes): one pre-order walk.
pub(super) fn collect_styled_spans(
    span: &InlineSpan,
    scale: f32,
) -> Vec<(String, Option<TextStyle>)> {
    fn walk(
        span: &TextSpan,
        inherited: Option<&TextStyle>,
        scale: f32,
        out: &mut Vec<(String, Option<TextStyle>)>,
    ) {
        let merged: Option<TextStyle> = match (inherited, span.style.as_ref()) {
            (Some(parent), Some(own)) => Some(parent.merge(own)),
            (Some(parent), None) => Some(parent.clone()),
            (None, Some(own)) => Some(own.clone()),
            (None, None) => None,
        };
        if let Some(text) = &span.text
            && !text.is_empty()
        {
            let mut effective = merged.clone();
            if let Some(style) = &mut effective {
                // Scale font_size to device pixels.
                if let Some(size) = style.font_size {
                    style.font_size = Some(size * f64::from(scale));
                }
                // Scale letter_spacing by the same DPR factor so that
                // `style_to_attrs_owned` can compute the EM ratio as
                // `spacing / font_size` using consistent (device-px) units.
                // Without this scaling, at DPR=2 a 2px spacing on a 16px
                // font yields 2/32=0.0625 EM instead of the correct 0.125 EM.
                if let Some(spacing) = style.letter_spacing {
                    style.letter_spacing = Some(spacing * f64::from(scale));
                }
            }
            out.push((text.clone(), effective));
        }
        for child in &span.children {
            walk(child, merged.as_ref(), scale, out);
        }
    }

    let mut out = Vec::new();
    match span {
        InlineSpan::Text(root) => walk(root, None, scale, &mut out),
        InlineSpan::Placeholder(_placeholder) => {
            // Emit a Unicode Object Replacement Character (\u{FFFC})
            // as a placeholder. The shaper gives it a glyph; the
            // caller tracks placeholder positions separately.
            out.push(("\u{FFFC}".to_string(), None));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Style → cosmic-text attrs conversion (mirrors flui-painting's style_to_attrs)
// ---------------------------------------------------------------------------

/// Maps a FLUI [`TextStyle`] to an owned cosmic-text [`AttrsOwned`].
///
/// Per-span font size overrides the buffer-level default via
/// `Attrs::metrics(Metrics::new(size, size * 1.2))` (cosmic-text 0.18.2
/// `attrs.rs:304`).  Color is mapped from `style.foreground` (takes
/// precedence) or `style.color`; when neither is set `base_color` is used.
///
/// API verified against:
/// `C:/.cargo/registry/src/.../cosmic-text-0.18.2/src/attrs.rs:263`
/// (`Attrs::color`), `:304` (`Attrs::metrics`), `:269` (`Attrs::family`),
/// `:285` (`Attrs::style`), `:289` (`Attrs::weight`).
pub(super) fn style_to_attrs_owned(style: Option<&TextStyle>, base_color: Color) -> AttrsOwned {
    let mut attrs = Attrs::new();

    // Resolve color: foreground > color > base_color
    let color = style
        .and_then(|s| s.foreground.or(s.color))
        .unwrap_or(base_color);
    attrs = attrs.color(GlyphonColor::rgba(color.r, color.g, color.b, color.a));

    if let Some(style) = style {
        // Font family
        if let Some(ref family) = style.font_family {
            attrs = attrs.family(match family.as_str() {
                "serif" | "Serif" => Family::Serif,
                "sans-serif" | "SansSerif" | "sans" => Family::SansSerif,
                "monospace" | "Monospace" | "mono" => Family::Monospace,
                "cursive" | "Cursive" => Family::Cursive,
                "fantasy" | "Fantasy" => Family::Fantasy,
                name => Family::Name(name),
            });
        }

        // Font weight
        if let Some(weight) = style.font_weight {
            attrs = attrs.weight(match weight {
                FontWeight::W100 => Weight::THIN,
                FontWeight::W200 => Weight::EXTRA_LIGHT,
                FontWeight::W300 => Weight::LIGHT,
                FontWeight::W400 => Weight::NORMAL,
                FontWeight::W500 => Weight::MEDIUM,
                FontWeight::W600 => Weight::SEMIBOLD,
                FontWeight::W700 => Weight::BOLD,
                FontWeight::W800 => Weight::EXTRA_BOLD,
                FontWeight::W900 => Weight::BLACK,
            });
        }

        // Font style (normal / italic)
        if let Some(font_style) = style.font_style {
            attrs = attrs.style(match font_style {
                FontStyle::Normal => Style::Normal,
                FontStyle::Italic => Style::Italic,
            });
        }

        // Per-span font size → per-span Metrics override.
        // cosmic-text 0.18.2: Attrs::metrics(Metrics) sets metrics_opt on the
        // run, overriding the buffer-level default for this run only.
        // line_height = size × height-multiplier (or ×1.2 when absent).
        #[allow(clippy::cast_possible_truncation)]
        if let Some(size) = style.font_size.map(|s| s as f32) {
            #[allow(clippy::cast_possible_truncation)]
            let line_height = style.height.map_or(size * 1.2, |h| h as f32 * size);
            attrs = attrs.metrics(Metrics::new(size, line_height));
        }

        // Letter spacing in EM (cosmic: spacing_em = px / font_size).
        #[allow(clippy::cast_possible_truncation)]
        if let Some(spacing) = style.letter_spacing.map(|s| s as f32)
            && let Some(size) = style.font_size.map(|s| s as f32)
            && size > 0.0
        {
            attrs = attrs.letter_spacing(spacing / size);
        }
    }

    AttrsOwned::new(&attrs)
}

// ---------------------------------------------------------------------------
// Cache key for rich text buffers
// ---------------------------------------------------------------------------

/// Cache key for a plain-text buffer (single font size, no per-span styling).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TextCacheKey {
    text: String,
    font_size_bits: u32, // f32 as bits for hashing
}

impl TextCacheKey {
    fn new(text: &str, font_size: f32) -> Self {
        Self {
            text: text.to_string(),
            font_size_bits: font_size.to_bits(),
        }
    }
}

/// Cache key for a rich-text buffer (per-span styled runs).
///
/// Hashed from the concatenated text + per-run serialised style fields so that
/// two spans with identical plain text but different styling map to distinct
/// buffers.  Only layout-affecting style fields enter the hash (colors alone do
/// not change glyph geometry, but they DO affect how the run is rendered, so we
/// include them to avoid the wrong color bleeding through a stale cache entry).
#[derive(Debug, Clone, PartialEq, Eq)]
struct RichTextCacheKey {
    /// All runs serialised to `"text\x00family\x00weight\x00style\x00size_bits\x00color_bits"`.
    runs_fingerprint: String,
}

impl Hash for RichTextCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.runs_fingerprint.hash(state);
    }
}

impl RichTextCacheKey {
    /// The key must fingerprint EVERY field `style_to_attrs_owned` feeds the
    /// shaper (the `TextStyle::layout_affecting_eq` set), or two
    /// identically-worded but differently-styled spans collide and reuse the
    /// wrong shaped buffer: family, weight, style, font_size, color,
    /// letter_spacing, height (per-run `Metrics` line height), plus the
    /// buffer-level `base_font_size` default and the `wrap_width` (which
    /// controls line-breaking, so two identical runs at different wrap widths
    /// must produce distinct shaped buffers).
    fn new(
        runs: &[(String, Option<TextStyle>)],
        base_font_size: f32,
        base_color: Color,
        wrap_width: Option<f32>,
    ) -> Self {
        // Variable-length strings (run text, font family) are length-prefixed
        // `<byte-len>:<bytes>` (netstring style) so the concatenation is
        // unambiguous even when the text itself contains the field separators —
        // otherwise two distinct run sequences could fingerprint identically
        // and collide on a stale buffer.
        fn push_len_prefixed(out: &mut String, s: &str) {
            out.push_str(&s.len().to_string());
            out.push(':');
            out.push_str(s);
        }

        let base_color_bits =
            u32::from_le_bytes([base_color.r, base_color.g, base_color.b, base_color.a]);
        let mut fingerprint = String::new();
        fingerprint.push_str(&base_font_size.to_bits().to_string());
        fingerprint.push('\x03');
        // Encode wrap_width: None → sentinel 0xFFFF_FFFF (never a valid f32
        // bit pattern for a positive finite width); Some(w) → w.to_bits().
        // Wrap width changes line-breaking, so two identical runs at different
        // wrap widths must not collide.
        let wrap_bits: u32 = wrap_width.map_or(0xFFFF_FFFF, f32::to_bits);
        fingerprint.push_str(&wrap_bits.to_string());
        fingerprint.push('\x03');
        for (text, style) in runs {
            push_len_prefixed(&mut fingerprint, text);
            fingerprint.push('\x00');
            if let Some(s) = style {
                if let Some(ref fam) = s.font_family {
                    push_len_prefixed(&mut fingerprint, fam);
                }
                fingerprint.push('\x01');
                if let Some(w) = s.font_weight {
                    fingerprint.push_str(&(w.value().to_string()));
                }
                fingerprint.push('\x01');
                if let Some(fs) = s.font_style {
                    fingerprint.push(match fs {
                        FontStyle::Normal => 'N',
                        FontStyle::Italic => 'I',
                    });
                }
                fingerprint.push('\x01');
                if let Some(sz) = s.font_size {
                    // f64 font-size values come from TextStyle which is a UI
                    // coordinate: practical range 1–300 pt, well within f32.
                    #[allow(clippy::cast_possible_truncation)]
                    let bits = (sz as f32).to_bits();
                    fingerprint.push_str(&bits.to_string());
                }
                fingerprint.push('\x01');
                if let Some(ls) = s.letter_spacing {
                    #[allow(clippy::cast_possible_truncation)] // UI coordinate, fits f32
                    let bits = (ls as f32).to_bits();
                    fingerprint.push_str(&bits.to_string());
                }
                fingerprint.push('\x01');
                if let Some(h) = s.height {
                    #[allow(clippy::cast_possible_truncation)] // UI coordinate, fits f32
                    let bits = (h as f32).to_bits();
                    fingerprint.push_str(&bits.to_string());
                }
                fingerprint.push('\x01');
                // Color: foreground > color > base_color
                let color = s.foreground.or(s.color).unwrap_or(base_color);
                let cbits = u32::from_le_bytes([color.r, color.g, color.b, color.a]);
                fingerprint.push_str(&cbits.to_string());
            } else {
                // No style — only base color applies
                fingerprint.push_str(&base_color_bits.to_string());
            }
            fingerprint.push('\x02');
        }
        Self {
            runs_fingerprint: fingerprint,
        }
    }
}

/// Cached text buffer with LRU tracking
struct CachedBuffer {
    buffer: Buffer,
    last_used_frame: u64,
}

/// Discriminated batch entry: either a plain-text buffer or a rich-text buffer.
///
/// Both variants carry the screen position and the glyphon default color (used
/// as `TextArea::default_color`; per-run colors come from `Attrs::color` on the
/// rich path).
enum BatchEntry {
    Plain {
        key: TextCacheKey,
        position: Point<Pixels>,
        color: GlyphonColor,
    },
    Rich {
        key: RichTextCacheKey,
        position: Point<Pixels>,
        default_color: GlyphonColor,
    },
}

/// Text rendering system using glyphon
///
/// Manages font loading, text layout, and GPU-accelerated glyph rasterization.
/// Batches text across the frame for efficient rendering.
///
/// # Caching
///
/// Text buffers are cached by content + style fingerprint to avoid expensive
/// re-layout.  Plain text uses `(text, font_size)`; rich text uses a
/// per-run style fingerprint that covers family/weight/style/size/color.
/// Cache is automatically pruned to remove stale entries.
///
/// # Example
/// ```ignore
/// let mut text_renderer = TextRenderer::new(&device, &queue, surface_format, font_system);
///
/// // Add plain text during frame
/// text_renderer.add_text("Hello, World!", Point::new(10.0, 10.0), 16.0, Color::BLACK);
///
/// // Render a batch range at its draw-order position, then finish the pass
/// text_renderer.render_range(&device, &queue, &view, &mut encoder, (800, 600), 0..usize::MAX)?;
/// text_renderer.finish_pass();
/// ```
pub struct TextRenderer {
    /// The framework's single shared font system (ADR-0016), injected by the
    /// caller rather than resolved ambiently.
    ///
    /// This is a clone of the handle owned by `flui-painting`, so glyphs are
    /// shaped here against the exact same faces that text *measurement* uses:
    /// a font registered via `PaintingBinding::register_font` is visible to
    /// both paths, with no second database to keep in sync. Taking this as a
    /// constructor parameter (rather than reaching for
    /// `PaintingBinding::instance()` internally) means constructing a
    /// `TextRenderer` on a non-owner thread (the raster thread) never has to
    /// stand up a second, disconnected binding of its own — the caller
    /// decides which font system this renderer shapes against.
    font_system: SharedFontSystem,

    /// Swash cache (rasterizes glyphs)
    swash_cache: SwashCache,

    /// Text atlas (texture atlas for glyphs)
    text_atlas: TextAtlas,

    /// Viewport (manages resolution and transforms)
    viewport: Viewport,

    /// Ordered batch of text buffers for the current frame.
    batch: Vec<BatchEntry>,

    /// Pooled per-segment glyphon renderers, in draw-slot order.
    ///
    /// Each glyphon renderer owns one vertex buffer that `prepare`
    /// OVERWRITES, and wgpu render passes read buffer state at submit time
    /// — so one renderer cannot prepare-and-draw more than once per
    /// submitted encoder. Ordered text therefore needs one renderer per
    /// text-bearing segment; the pool is grown on demand and reused across
    /// frames (the shared atlas is untouched by pooling).
    segment_renderers: Vec<GlyphonRenderer>,
    /// Next pool slot to hand out this render cycle; reset by
    /// [`Self::finish_pass`].
    next_segment_slot: usize,

    /// Cache of plain-text buffers (text + font_size → Buffer)
    plain_cache: HashMap<TextCacheKey, CachedBuffer>,

    /// Cache of rich-text buffers (style fingerprint → Buffer)
    rich_cache: HashMap<RichTextCacheKey, CachedBuffer>,

    /// Current frame number for LRU tracking
    current_frame: u64,

    /// Max entries per cache (plain and rich each limited independently)
    max_cache_size: usize,

    /// Cache statistics
    cache_hits: u64,
    cache_misses: u64,
}

impl TextRenderer {
    /// Guarantees the shared font system has at least one usable face.
    ///
    /// `FontSystem::new()` (in `flui-painting`) already loads system fonts on
    /// desktop platforms. When none are present (CI, headless, minimal
    /// containers) the shared database would be empty and all text would
    /// render blank, so we fall back to the embedded Roboto-Regular. The
    /// face-count guard makes this idempotent: if the shared system already
    /// has faces — because the OS provided them or a prior `TextRenderer`
    /// (or an explicit `register_font`) already populated it — this is a
    /// no-op, so multiple renderers never double-load the fallback.
    fn ensure_fonts_available(font_system: &mut FontSystem) {
        // Text fallback: on an empty shared DB (headless / CI / minimal
        // containers with no OS fonts) text would render blank, so load the
        // embedded Roboto. Idempotent via the face count — a system-provided or
        // previously-loaded DB is left untouched.
        if font_system.db().faces().count() == 0 {
            const ROBOTO_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");
            tracing::warn!("shared FontSystem has no faces; loading embedded Roboto-Regular");
            font_system.db_mut().load_font_data(ROBOTO_REGULAR.to_vec());
            if font_system.db().faces().count() == 0 {
                tracing::error!("failed to load any fonts; text rendering may be blank");
            }
        }

        // Icon fonts: always present, independent of the text-font count above.
        // Neither the OS text fonts nor Roboto carry the private-use glyphs that
        // Material / Cupertino icons resolve to, so `Icon` renders tofu without
        // these.
        //
        // Idempotency is checked per FontSystem INSTANCE (by family-name
        // presence in `font_system`'s own db), not via a process-global flag.
        // A process-global `AtomicBool` (the previous shape) is wrong here:
        // it flips true the first time any `FontSystem` loads the icon
        // fonts, and every OTHER `FontSystem` afterwards -- including one
        // constructed independently on a non-owner thread -- reads that
        // flag as already-satisfied and silently skips loading into its OWN
        // (empty) db, so `Icon` renders tofu on that instance forever.
        // Checking this instance's own faces makes the guard correct per
        // instance: idempotent on a repeat call against the SAME
        // `FontSystem` (the family is already there), and always loads for a
        // genuinely different one.
        //
        // The two embedded fonts are probed and loaded INDEPENDENTLY, each
        // keyed by its own family name, rather than gating both on a single
        // "Material Icons present?" check. A single shared gate is wrong the
        // moment the two faces can appear separately: a system-installed or
        // previously `register_font`-ed "Material Icons" face (this is a
        // real family name, not exclusive to the embedded TTF) would make
        // the combined check see the family as present and skip loading
        // BOTH fonts -- silently dropping the embedded Material Icons face
        // update *and* Cupertino Icons entirely, even though nothing ever
        // provided Cupertino Icons. Each font's presence is real evidence
        // only for that font.
        Self::ensure_embedded_font_loaded(
            font_system,
            "Material Icons",
            include_bytes!("../../assets/fonts/MaterialIcons-Regular.ttf"),
        );
        Self::ensure_embedded_font_loaded(
            font_system,
            "CupertinoIcons",
            include_bytes!("../../assets/fonts/CupertinoIcons.ttf"),
        );
    }

    /// Loads `font_data` into `font_system`'s database unless a face already
    /// carries `family` — the per-font idempotency check
    /// [`Self::ensure_fonts_available`]'s doc comment explains.
    fn ensure_embedded_font_loaded(font_system: &mut FontSystem, family: &str, font_data: &[u8]) {
        let already_present = font_system
            .db()
            .faces()
            .any(|face| face.families.iter().any(|(name, _)| name == family));
        if already_present {
            return;
        }
        font_system.db_mut().load_font_data(font_data.to_vec());
        tracing::info!(
            family,
            count = font_system.db().faces().count(),
            "loaded embedded icon font"
        );
    }

    /// Creates a new `TextRenderer` bound to the given wgpu `device`/`queue`,
    /// shaping against the injected `font_system`.
    ///
    /// `font_system` is a caller-supplied [`SharedFontSystem`] handle rather
    /// than an ambient lookup: the caller decides which font database this
    /// renderer shapes against, so constructing a `TextRenderer` on a
    /// non-owner thread (the raster thread) never has to stand up a second,
    /// disconnected binding of its own just to reach the font DB.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        font_system: SharedFontSystem,
    ) -> Self {
        tracing::trace!(format = ?format, "TextRenderer::new");

        font_system.with_mut(Self::ensure_fonts_available);
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let text_atlas = TextAtlas::new(device, queue, &cache, format);
        let viewport = Viewport::new(device, &cache);

        Self {
            font_system,
            swash_cache,
            text_atlas,
            viewport,
            batch: Vec::new(),
            segment_renderers: Vec::new(),
            next_segment_slot: 0,
            plain_cache: HashMap::new(),
            rich_cache: HashMap::new(),
            current_frame: 0,
            max_cache_size: 256,
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    // ------------------------------------------------------------------
    // Plain-text path (single font size + color; no per-span styling)
    // ------------------------------------------------------------------

    /// Ensures a plain-text buffer for `key` is present in the cache,
    /// shaping and inserting it on a miss. The buffer is read back from the
    /// cache by `key` at batch-build time, so this returns nothing.
    fn ensure_plain_buffer(&mut self, key: &TextCacheKey) {
        // entry() avoids the double-lookup of get() + insert().
        match self.plain_cache.entry(key.clone()) {
            Entry::Occupied(mut e) => {
                e.get_mut().last_used_frame = self.current_frame;
                self.cache_hits += 1;
            }
            Entry::Vacant(e) => {
                let font_size = f32::from_bits(key.font_size_bits);
                let line_height = font_size * 1.2;
                // Shape against the shared FontSystem; the closure holds the
                // lock only for the shaping calls and captures no `self`
                // field, so the vacant `plain_cache` entry `e` stays valid.
                let buffer = self.font_system.with_mut(|font_system| {
                    let mut buffer = Buffer::new(font_system, Metrics::new(font_size, line_height));
                    // Unbounded width — wrap-width matching is a follow-up (paint seam).
                    buffer.set_size(font_system, Some(f32::MAX), None);
                    let attrs = Attrs::new().family(Family::SansSerif);
                    buffer.set_text(font_system, &key.text, &attrs, Shaping::Advanced, None);
                    buffer.shape_until_scroll(font_system, false);
                    buffer
                });
                e.insert(CachedBuffer {
                    buffer,
                    last_used_frame: self.current_frame,
                });
                self.cache_misses += 1;
            }
        }
    }

    /// Batches a plain-text string for rendering this frame.
    ///
    /// Buffers are cached by `(text, font_size)` to avoid re-layout when the
    /// same string appears in subsequent frames.
    pub fn add_text(&mut self, text: &str, position: Point<Pixels>, font_size: f32, color: Color) {
        // `text_len`, never `text`. This string is every label, message body,
        // and text-field value the UI draws, and a tracing field is world-
        // readable in Apple's unified log and in logcat. The length and the
        // geometry are what a rendering trace is actually for.
        tracing::trace!(
            text_len = text.len(),
            ?position,
            font_size,
            ?color,
            "TextRenderer::add_text"
        );

        let key = TextCacheKey::new(text, font_size);
        self.ensure_plain_buffer(&key);
        let glyphon_color = GlyphonColor::rgba(color.r, color.g, color.b, color.a);
        self.batch.push(BatchEntry::Plain {
            key,
            position,
            color: glyphon_color,
        });
    }

    // ------------------------------------------------------------------
    // Rich-text path (per-span family / weight / style / size / color)
    // ------------------------------------------------------------------

    /// Batches a set of styled runs for rendering this frame.
    ///
    /// `runs` is the output of [`collect_styled_spans`]: each entry is
    /// `(text_fragment, merged_style)` with the `text_scale_factor` already
    /// baked into `style.font_size`.  `base_font_size` is the buffer-level
    /// default applied to runs that carry no explicit size; `base_color` is
    /// the fallback color for runs with no color override.
    ///
    /// Buffers are cached by a style fingerprint that covers all layout- and
    /// paint-affecting fields so that differently-styled identical strings
    /// never collide.
    pub fn add_rich_text(
        &mut self,
        runs: &[(String, Option<TextStyle>)],
        position: Point<Pixels>,
        base_font_size: f32,
        base_color: Color,
        wrap_width: Option<f32>,
    ) {
        if runs.is_empty() {
            return;
        }

        tracing::trace!(
            run_count = runs.len(),
            ?position,
            base_font_size,
            ?base_color,
            ?wrap_width,
            "TextRenderer::add_rich_text"
        );

        let key = RichTextCacheKey::new(runs, base_font_size, base_color, wrap_width);

        // Borrow `key` on hit to avoid an allocation; move it into the map on miss.
        if let Some(entry) = self.rich_cache.get_mut(&key) {
            entry.last_used_frame = self.current_frame;
            self.cache_hits += 1;
        } else {
            let line_height = base_font_size * 1.2;
            // Use wrap_width from the layout constraint so glyphon
            // respects the same line-breaking as cosmic-text.
            // None = unbounded (no wrapping); Some(w) = wrap at w pixels.
            let buffer_width = wrap_width.unwrap_or(f32::MAX);

            // Build per-run AttrsOwned; the iterator borrows from the vec
            // of owned values, satisfying set_rich_text's lifetime.
            let owned_attrs: Vec<AttrsOwned> = runs
                .iter()
                .map(|(_, style)| style_to_attrs_owned(style.as_ref(), base_color))
                .collect();

            // Shape against the shared FontSystem; the closure holds the lock
            // only for the shaping calls and captures no `self` field.
            let buffer = self.font_system.with_mut(|font_system| {
                let mut buffer =
                    Buffer::new(font_system, Metrics::new(base_font_size, line_height));
                buffer.set_size(font_system, Some(buffer_width), None);
                buffer.set_rich_text(
                    font_system,
                    runs.iter()
                        .zip(owned_attrs.iter())
                        .map(|((text, _), attrs)| (text.as_str(), attrs.as_attrs())),
                    &Attrs::new(),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(font_system, false);
                buffer
            });

            self.rich_cache.insert(
                key.clone(),
                CachedBuffer {
                    buffer,
                    last_used_frame: self.current_frame,
                },
            );
            self.cache_misses += 1;
        }

        let default_color =
            GlyphonColor::rgba(base_color.r, base_color.g, base_color.b, base_color.a);
        self.batch.push(BatchEntry::Rich {
            key,
            position,
            default_color,
        });
    }

    // ------------------------------------------------------------------
    // Cache eviction
    // ------------------------------------------------------------------

    /// Evicts stale entries from both caches using LRU.
    ///
    /// Average O(n) over cache size; worst case same (no early exit).
    fn prune_cache(&mut self) {
        Self::evict_cache(
            &mut self.plain_cache,
            self.current_frame,
            self.max_cache_size,
        );
        Self::evict_cache(
            &mut self.rich_cache,
            self.current_frame,
            self.max_cache_size,
        );
    }

    fn evict_cache<K: Eq + std::hash::Hash + Clone>(
        cache: &mut HashMap<K, CachedBuffer>,
        current_frame: u64,
        max_size: usize,
    ) {
        if cache.len() <= max_size {
            return;
        }
        // First pass: drop entries not used in the last ~1 s (60 frames).
        let threshold = current_frame.saturating_sub(60);
        cache.retain(|_, v| v.last_used_frame >= threshold);

        // Second pass: if still over budget, remove the single oldest entry
        // one at a time until we fit.  Average O(n) per iteration; total
        // iterations bounded by initial overage.
        while cache.len() > max_size {
            let oldest = cache
                .iter()
                .min_by_key(|(_, v)| v.last_used_frame)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => {
                    cache.remove(&k);
                }
                None => break,
            }
        }
    }

    // ------------------------------------------------------------------
    // Frame render
    // ------------------------------------------------------------------

    /// Render exactly the batch entries in `range` at the CURRENT point of
    /// the encoder — the per-segment half of ordered text. Uses a pooled
    /// glyphon renderer per call (see [`Self::segment_renderers`]); never
    /// clears the batch or advances frame bookkeeping — that is
    /// [`Self::finish_pass`]'s job, once, at the end of the submit.
    #[must_use = "errors must be propagated or handled"]
    pub(crate) fn render_range(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        size: (u32, u32),
        range: std::ops::Range<usize>,
    ) -> crate::error::EngineResult<()> {
        let end = range.end.min(self.batch.len());
        if range.start >= end {
            return Ok(());
        }

        self.viewport.update(
            queue,
            Resolution {
                width: size.0,
                height: size.1,
            },
        );
        #[allow(clippy::cast_possible_wrap)]
        let full_bounds = TextBounds {
            left: 0,
            top: 0,
            right: size.0 as i32,
            bottom: size.1 as i32,
        };
        let text_areas: Vec<TextArea<'_>> = build_text_areas(
            &self.batch[range.start..end],
            &self.plain_cache,
            &self.rich_cache,
            full_bounds,
        );

        if self.next_segment_slot >= self.segment_renderers.len() {
            self.segment_renderers.push(GlyphonRenderer::new(
                &mut self.text_atlas,
                device,
                wgpu::MultisampleState::default(),
                None,
            ));
        }
        let slot = self.next_segment_slot;
        self.next_segment_slot += 1;

        let font_system = self.font_system.clone();
        font_system
            .with_mut(|font_system| {
                self.segment_renderers[slot].prepare(
                    device,
                    queue,
                    font_system,
                    &mut self.text_atlas,
                    &self.viewport,
                    text_areas,
                    &mut self.swash_cache,
                )
            })
            .map_err(crate::error::EngineError::text_prepare)?;

        let mut text_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Segment Text Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.segment_renderers[slot]
            .render(&self.text_atlas, &self.viewport, &mut text_pass)
            .map_err(crate::error::EngineError::text_render)?;
        Ok(())
    }

    /// End-of-submit bookkeeping for ordered text: clear the consumed
    /// batch, reset the pool cursor, and advance the frame counter that
    /// drives cache pruning. Must run exactly once per `WgpuPainter::render`
    /// cycle, after every `render_range`/`render` of that cycle.
    pub(crate) fn finish_pass(&mut self) {
        self.current_frame += 1;
        self.batch.clear();
        self.next_segment_slot = 0;
        if self.current_frame.is_multiple_of(60) {
            self.prune_cache();
        }
    }

    /// Reclaim glyph-atlas slots whose glyphs were not used this frame.
    ///
    /// glyphon tracks an in-use set per atlas that `prepare` adds to and only
    /// `TextAtlas::trim` clears.  Without a trim the set grows monotonically:
    /// every glyph ever rasterized (each distinct char × size × subpixel
    /// position) is treated as permanently live, so the atlas can only grow and
    /// never reuses a slot — unbounded GPU memory growth for UIs whose text
    /// changes over time.
    ///
    /// trim only clears the CPU-side in-use set; it does not touch the atlas
    /// texture the in-flight frame samples, so it is safe (and required) to call
    /// **once per frame, after the frame's text has been prepared, rendered, and
    /// submitted** — never between [`render_range`](Self::render_range) calls
    /// within a frame (`painter.render` runs multiple times per frame for
    /// backdrop-filter flushes, each rendering its segments' text).  The single
    /// correct caller is `WgpuPainter::end_frame_maintenance`, the same
    /// once-per-frame seam that drives texture-cache maintenance.
    pub(crate) fn atlas_trim(&mut self) {
        self.text_atlas.trim();
    }

    // ------------------------------------------------------------------
    // Diagnostics
    // ------------------------------------------------------------------

    /// Returns the number of text areas queued for the current frame.
    #[inline]
    pub fn text_count(&self) -> usize {
        self.batch.len()
    }

    /// Returns `(hits, misses, plain_cache_size, rich_cache_size)`.
    #[allow(dead_code)] // exposed for diagnostics / tests
    pub fn cache_stats(&self) -> (u64, u64, usize, usize) {
        (
            self.cache_hits,
            self.cache_misses,
            self.plain_cache.len(),
            self.rich_cache.len(),
        )
    }
}

// ---------------------------------------------------------------------------
// Free helper: build TextArea batch without borrowing the rest of TextRenderer
// ---------------------------------------------------------------------------

/// Collects [`TextArea`] values from the two caches for a single frame.
///
/// Extracted as a free function so that `render` can simultaneously hold
/// immutable borrows into `plain_cache` / `rich_cache` AND mutable borrows
/// into `font_system` / `text_atlas` / `swash_cache` — all disjoint
/// `TextRenderer` fields.  The borrow checker accepts this when the borrows
/// are named at the call site rather than going through `&mut self`.
///
/// Entries whose cache key is not found (should never happen: the key is
/// always inserted before the batch entry is pushed) are silently skipped so
/// that a logic error degrades gracefully rather than panicking.
fn build_text_areas<'cache>(
    batch: &[BatchEntry],
    plain_cache: &'cache HashMap<TextCacheKey, CachedBuffer>,
    rich_cache: &'cache HashMap<RichTextCacheKey, CachedBuffer>,
    bounds: TextBounds,
) -> Vec<TextArea<'cache>> {
    batch
        .iter()
        .filter_map(|entry| match entry {
            BatchEntry::Plain {
                key,
                position,
                color,
            } => plain_cache.get(key).map(|c| TextArea {
                buffer: &c.buffer,
                left: position.x.0,
                top: position.y.0,
                scale: 1.0,
                bounds,
                default_color: *color,
                custom_glyphs: &[],
            }),
            BatchEntry::Rich {
                key,
                position,
                default_color,
            } => rich_cache.get(key).map(|c| TextArea {
                buffer: &c.buffer,
                left: position.x.0,
                top: position.y.0,
                scale: 1.0,
                bounds,
                default_color: *default_color,
                custom_glyphs: &[],
            }),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use glyphon::fontdb;

    use super::{FontSystem, TextRenderer, collect_styled_spans};
    use flui_types::typography::{FontWeight, InlineSpan, TextSpan, TextStyle};

    /// A bold child of a sized parent must carry both bold weight and the
    /// inherited size after flattening.
    #[test]
    fn collect_styled_spans_inherits_size_and_bold() {
        let parent_style = TextStyle {
            font_size: Some(24.0),
            ..Default::default()
        };
        let child_style = TextStyle {
            font_weight: Some(FontWeight::W700),
            ..Default::default()
        };

        let span = InlineSpan::Text(std::sync::Arc::new(
            TextSpan::new("parent")
                .with_style(parent_style)
                .with_child(TextSpan::styled("child", child_style)),
        ));

        // scale = 2.0 baked in: parent font_size 24 → 48; child inherits 48.
        let runs = collect_styled_spans(&span, 2.0);

        assert_eq!(runs.len(), 2, "expected two runs: parent text + child text");

        let (parent_text, parent_style) = &runs[0];
        assert_eq!(parent_text, "parent");
        let ps = parent_style.as_ref().expect("parent run must have a style");
        assert!(
            (ps.font_size.unwrap() - 48.0).abs() < f64::EPSILON,
            "parent font_size should be 24 × 2 = 48, got {:?}",
            ps.font_size
        );
        assert!(
            ps.font_weight.is_none() || ps.font_weight == Some(FontWeight::W400),
            "parent run should carry no bold override"
        );

        let (child_text, child_style) = &runs[1];
        assert_eq!(child_text, "child");
        let cs = child_style.as_ref().expect("child run must have a style");
        // Child inherits parent's scaled size.
        assert!(
            (cs.font_size.unwrap() - 48.0).abs() < f64::EPSILON,
            "child should inherit parent font_size 48, got {:?}",
            cs.font_size
        );
        assert_eq!(
            cs.font_weight,
            Some(FontWeight::W700),
            "child must carry bold weight"
        );
    }

    /// A placeholder span at the root contributes one run with the
    /// Unicode Object Replacement Character (\u{FFFC}).
    #[test]
    fn collect_styled_spans_placeholder_yields_one_run() {
        use flui_types::typography::{PlaceholderAlignment, PlaceholderSpan};
        let span = InlineSpan::Placeholder(PlaceholderSpan::new(
            32.0,
            32.0,
            PlaceholderAlignment::Baseline,
        ));
        let runs = collect_styled_spans(&span, 1.0);
        assert_eq!(runs.len(), 1, "placeholder span must produce one run");
        assert_eq!(runs[0].0, "\u{FFFC}", "placeholder must be ORC character");
    }

    /// An empty text node is skipped; only non-empty nodes appear in output.
    #[test]
    fn collect_styled_spans_skips_empty_text() {
        let span = InlineSpan::Text(std::sync::Arc::new(
            TextSpan::new("").with_child(TextSpan::new("hello")),
        ));
        let runs = collect_styled_spans(&span, 1.0);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].0, "hello");
    }

    /// The cache key must change when any shaper-affecting field changes —
    /// otherwise two identically-worded but differently-styled spans collide
    /// and reuse the wrong shaped buffer.
    #[test]
    fn rich_cache_key_distinguishes_every_layout_affecting_field() {
        use flui_types::Color;

        use super::RichTextCacheKey;

        let base = TextStyle {
            font_size: Some(16.0),
            ..Default::default()
        };
        let runs_of = |s: &TextStyle| vec![("text".to_string(), Some(s.clone()))];
        let key = |s: &TextStyle| RichTextCacheKey::new(&runs_of(s), 14.0, Color::BLACK, None);
        let baseline = key(&base);

        let with_spacing = TextStyle {
            letter_spacing: Some(2.0),
            ..base.clone()
        };
        assert_ne!(baseline, key(&with_spacing), "letter_spacing must be keyed");

        let with_height = TextStyle {
            height: Some(1.5),
            ..base.clone()
        };
        assert_ne!(baseline, key(&with_height), "height must be keyed");

        let with_color = TextStyle {
            color: Some(Color::RED),
            ..base.clone()
        };
        assert_ne!(baseline, key(&with_color), "color must be keyed");

        // The buffer-level default size is part of the key too.
        assert_ne!(
            RichTextCacheKey::new(&runs_of(&base), 14.0, Color::BLACK, None),
            RichTextCacheKey::new(&runs_of(&base), 28.0, Color::BLACK, None),
            "base_font_size must be keyed",
        );
    }

    /// Run text containing the fingerprint's own separator bytes must not let
    /// two distinct run sequences collide (length-prefixed encoding).
    #[test]
    fn rich_cache_key_is_collision_free_with_separator_bytes_in_text() {
        use flui_types::Color;

        use super::RichTextCacheKey;

        let one_run = vec![("a\u{0}b\u{1}c\u{2}".to_string(), None)];
        let two_runs = vec![("a".to_string(), None), ("b\u{1}c\u{2}".to_string(), None)];
        assert_ne!(
            RichTextCacheKey::new(&one_run, 14.0, Color::BLACK, None),
            RichTextCacheKey::new(&two_runs, 14.0, Color::BLACK, None),
            "separator bytes in run text must not collide distinct run sequences",
        );
    }

    /// `collect_styled_spans` must scale `letter_spacing` by the same DPR
    /// factor as `font_size` so that `style_to_attrs_owned` computes the
    /// correct EM ratio at high device-pixel-ratio.
    ///
    /// Regression test for P1-3 (cross-crate): at DPR=2 a 16px/2px-spacing
    /// run was producing 2/32=0.0625 EM instead of 2/16=0.125 EM because
    /// `font_size` was scaled by DPR but `letter_spacing` was not.
    #[test]
    fn collect_styled_spans_scales_letter_spacing_with_dpr() {
        use flui_types::typography::{TextSpan, TextStyle};

        let style = TextStyle {
            font_size: Some(16.0),
            letter_spacing: Some(2.0),
            ..Default::default()
        };
        let span = InlineSpan::Text(std::sync::Arc::new(
            TextSpan::new("hello").with_style(style),
        ));

        // DPR = 1: no scaling, both values stay the same
        let runs_1x = collect_styled_spans(&span, 1.0);
        assert_eq!(runs_1x.len(), 1);
        let s1 = runs_1x[0].1.as_ref().expect("style must be present");
        #[allow(clippy::cast_possible_truncation)]
        // UI coordinate; f64 value ≤ 300 fits f32 exactly
        let size_1x = s1.font_size.expect("font_size must be present") as f32;
        #[allow(clippy::cast_possible_truncation)] // UI coordinate; spacing fits f32
        let spacing_1x = s1.letter_spacing.expect("letter_spacing must be present") as f32;
        let em_1x = spacing_1x / size_1x;
        assert!(
            (em_1x - 0.125_f32).abs() < 1e-5,
            "DPR=1 EM ratio must be 2/16=0.125, got {em_1x}"
        );

        // DPR = 2: both font_size and letter_spacing must be scaled by 2
        let runs_2x = collect_styled_spans(&span, 2.0);
        assert_eq!(runs_2x.len(), 1);
        let s2 = runs_2x[0].1.as_ref().expect("style must be present");
        #[allow(clippy::cast_possible_truncation)] // UI coordinate; fits f32
        let size_2x = s2.font_size.expect("font_size must be present") as f32;
        #[allow(clippy::cast_possible_truncation)] // UI coordinate; fits f32
        let spacing_2x = s2.letter_spacing.expect("letter_spacing must be present") as f32;
        let em_2x = spacing_2x / size_2x;
        assert!(
            (size_2x - 32.0_f32).abs() < 1e-5,
            "DPR=2 font_size must be 16×2=32, got {size_2x}"
        );
        assert!(
            (spacing_2x - 4.0_f32).abs() < 1e-5,
            "DPR=2 letter_spacing must be 2×2=4, got {spacing_2x}"
        );
        assert!(
            (em_2x - 0.125_f32).abs() < 1e-5,
            "DPR=2 EM ratio must equal DPR=1 ratio (4/32=0.125), got {em_2x}"
        );
    }

    /// `wrap_width` must be part of the cache key: the same styled runs at
    /// different wrap widths produce different line breaks, so they must not
    /// share a shaped buffer.
    ///
    /// Regression test for P0-3: `add_rich_text` was building the key with
    /// `RichTextCacheKey::new(runs, base_font_size, base_color)` — without
    /// `wrap_width` — so `Some(200.0)` and `Some(400.0)` and `None` all
    /// hashed to the same key and the first width's buffer was reused for
    /// the others, producing wrong line breaks at high DPR.
    #[test]
    fn rich_cache_key_distinguishes_wrap_width() {
        use flui_types::Color;

        use super::RichTextCacheKey;

        let runs = vec![("hello world".to_string(), None)];
        let key = |w: Option<f32>| RichTextCacheKey::new(&runs, 14.0, Color::BLACK, w);

        let narrow = key(Some(200.0));
        let wide = key(Some(400.0));
        let unbounded = key(None);

        assert_ne!(
            narrow, wide,
            "different wrap widths must produce distinct keys"
        );
        assert_ne!(
            narrow, unbounded,
            "Some(w) and None must produce distinct keys"
        );
        assert_ne!(
            wide, unbounded,
            "Some(w) and None must produce distinct keys"
        );

        // Identical wrap_width must produce equal keys (idempotence).
        assert_eq!(
            key(Some(200.0)),
            key(Some(200.0)),
            "identical wrap_width must be stable"
        );
        assert_eq!(key(None), key(None), "None must be stable");
    }

    /// Constructs a genuinely empty, hermetic `FontSystem`: an empty
    /// `fontdb::Database` and no system-font scan. `FontSystem::new()` would
    /// make icon-font assertions vacuous on any machine that happens to
    /// have a system-installed "Material Icons"/"CupertinoIcons" font (or
    /// on a machine where a previous test in the same process already
    /// registered one into a SHARED font system) -- this constructor
    /// guarantees the test starts from zero faces every time, on every
    /// machine.
    fn empty_font_system() -> FontSystem {
        FontSystem::new_with_locale_and_db("en-US".to_string(), fontdb::Database::new())
    }

    fn has_family(font_system: &FontSystem, family: &str) -> bool {
        font_system
            .db()
            .faces()
            .any(|face| face.families.iter().any(|(name, _)| name == family))
    }

    /// Regression test for the process-global `ICON_FONTS_LOADED` bug: icon
    /// fonts must load into EVERY independently constructed `FontSystem`, not
    /// just the first one `ensure_fonts_available` ever sees.
    ///
    /// Red before the per-instance fix: a single process-wide
    /// `AtomicBool::swap` flipped true on the FIRST call below and stayed
    /// true for the rest of the process, so the second, independent
    /// `FontSystem` never got its icon fonts loaded and both assertions on
    /// `fs_b` would fail.
    #[test]
    fn ensure_fonts_available_loads_both_icon_fonts_into_every_independent_font_system() {
        let mut fs_a = empty_font_system();
        TextRenderer::ensure_fonts_available(&mut fs_a);
        assert!(
            has_family(&fs_a, "Material Icons"),
            "the first FontSystem must resolve the Material Icons glyph after \
             ensure_fonts_available"
        );
        assert!(
            has_family(&fs_a, "CupertinoIcons"),
            "the first FontSystem must resolve the Cupertino Icons glyph after \
             ensure_fonts_available"
        );

        // A SECOND, independently constructed FontSystem must ALSO receive
        // BOTH icon fonts -- this is the case the process-global flag broke.
        let mut fs_b = empty_font_system();
        TextRenderer::ensure_fonts_available(&mut fs_b);
        assert!(
            has_family(&fs_b, "Material Icons"),
            "a second, independent FontSystem must also resolve the Material Icons \
             glyph -- regression test for the process-global ICON_FONTS_LOADED flag \
             that silently skipped icon-font loading for every FontSystem after the first"
        );
        assert!(
            has_family(&fs_b, "CupertinoIcons"),
            "a second, independent FontSystem must also resolve the Cupertino Icons glyph"
        );
    }

    /// Regression test for the single-probe bug: the guard used to check
    /// ONLY "Material Icons" presence but gated LOADING BOTH embedded fonts
    /// on that one check. A `FontSystem` that already carries a "Material
    /// Icons" face -- a real family name that can come from the OS or a
    /// prior `register_font` call, not exclusively from this crate's
    /// embedded TTF -- made the combined check see the family as present and
    /// skip loading Cupertino Icons entirely, even though nothing had ever
    /// provided it.
    ///
    /// Red under the single-probe bug: seeding a "Material Icons" face
    /// (without touching Cupertino Icons) makes the old combined check
    /// true, so `ensure_fonts_available` would return early and the
    /// Cupertino assertion below would fail.
    #[test]
    fn ensure_fonts_available_loads_cupertino_icons_even_when_material_icons_already_present() {
        let mut fs = empty_font_system();
        // Seed a "Material Icons" face WITHOUT going through
        // `ensure_fonts_available` -- simulates a system-installed or
        // previously registered face with that family name, independent of
        // the fix under test. Reusing the embedded Material Icons TTF here
        // is just a convenient real, loadable font with that exact family
        // name; nothing about this test depends on it being THE canonical
        // embedded copy.
        fs.db_mut().load_font_data(
            include_bytes!("../../assets/fonts/MaterialIcons-Regular.ttf").to_vec(),
        );
        assert!(
            has_family(&fs, "Material Icons"),
            "test setup must have seeded a Material Icons face"
        );
        assert!(
            !has_family(&fs, "CupertinoIcons"),
            "test setup must not have seeded a Cupertino Icons face"
        );

        TextRenderer::ensure_fonts_available(&mut fs);

        assert!(
            has_family(&fs, "CupertinoIcons"),
            "CupertinoIcons must load even when Material Icons is already present -- \
             regression test for the single combined-gate bug where checking ONLY \
             'Material Icons' presence silently skipped loading BOTH fonts"
        );
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    use std::sync::Arc;

    use flui_painting::PaintingBinding;
    use flui_types::{geometry::Pixels, geometry::Point, styling::Color};

    use super::TextRenderer;

    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
    const W: u32 = 128;
    const H: u32 = 32;

    fn device_queue() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("a GPU adapter must be available on a GPU-enabled test host");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("TextRenderer Test Device"),
            ..Default::default()
        }))
        .expect("GPU device creation succeeded when adapter was found");
        (Arc::new(device), Arc::new(queue))
    }

    fn make_target(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Text Test Target"),
            size: wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// Clear `view` to transparent — `TextRenderer::render` uses `LoadOp::Load`,
    /// so the target must be initialised before the text pass composites onto it.
    fn clear(view: &wgpu::TextureView, encoder: &mut wgpu::CommandEncoder) {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Text Test Clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    fn readback(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) -> Vec<[u8; 4]> {
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let bytes_per_row = (W * 4).div_ceil(align) * align;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Text Readback"),
            size: u64::from(bytes_per_row * H),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Text Readback Encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(H),
                },
            },
            wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));
        let slice = staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("device poll must complete the readback copy");
        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((W * H) as usize);
        for row in 0..H {
            let row_start = (row * bytes_per_row) as usize;
            for col in 0..W {
                let off = row_start + (col as usize) * 4;
                pixels.push([
                    mapped[off],
                    mapped[off + 1],
                    mapped[off + 2],
                    mapped[off + 3],
                ]);
            }
        }
        crate::wgpu::readback_dump::dump_frame(W, H, bytemuck::cast_slice(&pixels));
        pixels
    }

    fn render_one(
        tr: &mut TextRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Text Test Frame"),
        });
        clear(view, &mut encoder);
        tr.render_range(device, queue, view, &mut encoder, (W, H), 0..usize::MAX)
            .expect("text render must succeed");
        tr.finish_pass();
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Per-frame `atlas_trim` across frames whose glyph set keeps changing must
    /// not corrupt the atlas: text still rasterises and composites correctly on
    /// a later frame.
    ///
    /// This is a no-regression guard for the trim wiring, not a red→green
    /// behaviour test: `trim` changes no pixel output (it only clears the
    /// CPU-side in-use set so future frames may reuse slots), and glyphon
    /// exposes no public atlas-size accessor to assert the reclaim directly — so
    /// the leak fix is correct by glyphon's documented `trim` contract, while
    /// this test proves the per-frame trim does not break subsequent rendering.
    #[test]
    fn atlas_trim_each_frame_keeps_text_rendering() {
        let (device, queue) = device_queue();
        let (target, view) = make_target(&device);
        let font_system = PaintingBinding::new().font_system();
        let mut tr = TextRenderer::new(&device, &queue, FORMAT, font_system);
        let white = Color::rgba(255, 255, 255, 255);

        // Churn: every frame draws DISTINCT glyphs (varying text + size) and
        // trims, mimicking the once-per-frame seam. Without trim this set would
        // accumulate in the atlas; with trim slots become reusable.
        for frame in 0..24u32 {
            tr.add_text(
                &format!("churn {frame} #@%"),
                Point::new(Pixels(2.0), Pixels(2.0)),
                10.0 + (frame % 8) as f32,
                white,
            );
            render_one(&mut tr, &device, &queue, &view);
            tr.atlas_trim();
        }

        // Final known frame: opaque white text on a transparent target.
        tr.add_text("FLUI", Point::new(Pixels(4.0), Pixels(8.0)), 16.0, white);
        render_one(&mut tr, &device, &queue, &view);
        tr.atlas_trim();

        let pixels = readback(&device, &queue, &target);
        let lit = pixels.iter().filter(|p| p[3] > 0).count();
        assert!(
            lit > 0,
            "after 24 trimmed churning frames, text must still rasterise and \
             composite (found {lit} non-transparent pixels)"
        );
    }
}
