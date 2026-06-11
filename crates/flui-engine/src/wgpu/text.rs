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
/// final pixel sizes.  Placeholder spans contribute no text.
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
            if let Some(style) = &mut effective
                && let Some(size) = style.font_size
            {
                style.font_size = Some(size * f64::from(scale));
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
        InlineSpan::Placeholder(_) => {}
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
    /// buffer-level `base_font_size` default.
    fn new(runs: &[(String, Option<TextStyle>)], base_font_size: f32, base_color: Color) -> Self {
        let base_color_bits =
            u32::from_le_bytes([base_color.r, base_color.g, base_color.b, base_color.a]);
        let mut fingerprint = String::new();
        fingerprint.push_str(&base_font_size.to_bits().to_string());
        fingerprint.push('\x03');
        for (text, style) in runs {
            fingerprint.push_str(text);
            fingerprint.push('\x00');
            if let Some(s) = style {
                if let Some(ref fam) = s.font_family {
                    fingerprint.push_str(fam);
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
/// let mut text_renderer = TextRenderer::new(&device, &queue, surface_format)?;
///
/// // Add plain text during frame
/// text_renderer.add_text("Hello, World!", Point::new(10.0, 10.0), 16.0, Color::BLACK);
///
/// // Render all text at end of frame
/// text_renderer.render(&device, &queue, &view, &mut encoder, (800, 600))?;
/// ```
pub struct TextRenderer {
    /// Font system (manages font loading and shaping)
    font_system: FontSystem,

    /// Swash cache (rasterizes glyphs)
    swash_cache: SwashCache,

    /// Text atlas (texture atlas for glyphs)
    text_atlas: TextAtlas,

    /// Glyphon renderer
    renderer: GlyphonRenderer,

    /// Viewport (manages resolution and transforms)
    viewport: Viewport,

    /// Ordered batch of text buffers for the current frame.
    batch: Vec<BatchEntry>,

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
    /// Initializes the font system with a smart fallback strategy.
    ///
    /// Strategy (in order):
    /// 1. Try to load system fonts (works on desktop platforms).
    /// 2. If no system fonts are found, fall back to the embedded Roboto-Regular.
    fn initialize_font_system() -> FontSystem {
        let mut fs = FontSystem::new();

        let system_font_count = fs.db().faces().count();
        if system_font_count > 0 {
            tracing::trace!(count = system_font_count, "loaded system fonts");
        } else {
            tracing::warn!("no system fonts available; loading embedded fonts");
            Self::load_embedded_fonts(&mut fs);

            let embedded_count = fs.db().faces().count();
            if embedded_count == 0 {
                tracing::error!("failed to load any fonts; text rendering may be blank");
            } else {
                tracing::info!(count = embedded_count, "loaded embedded fonts");
            }
        }

        fs
    }

    /// Loads the embedded Roboto-Regular font into `fs`.
    ///
    /// Embedded fonts act as the final fallback when no system fonts are
    /// present (e.g. CI, headless, minimal containers).
    fn load_embedded_fonts(fs: &mut FontSystem) {
        const ROBOTO_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");
        fs.db_mut().load_font_data(ROBOTO_REGULAR.to_vec());
        tracing::trace!("loaded embedded Roboto-Regular font");

        // TODO(REMOVE_BY=2026-09-22, cycle-4 E-15): add more embedded
        // fonts if needed (Bold, Italic, etc.) OR delete this TODO if
        // the embedded-font set is intentionally minimal (Roboto-Regular
        // covers default Material text rendering). The cadence comment
        // forces a decision rather than letting the placeholder rot
        // (same discipline as cycle 3 PR #106 REMOVE_BY pattern).
    }

    /// Creates a new `TextRenderer` bound to the given wgpu `device`/`queue`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        tracing::trace!(format = ?format, "TextRenderer::new");

        let font_system = Self::initialize_font_system();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let mut text_atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer = GlyphonRenderer::new(
            &mut text_atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );
        let viewport = Viewport::new(device, &cache);

        Self {
            font_system,
            swash_cache,
            text_atlas,
            renderer,
            viewport,
            batch: Vec::new(),
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

    /// Ensures a plain-text buffer is present in the cache, creating it if
    /// needed, and returns an immutable reference to it.
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
                let mut buffer =
                    Buffer::new(&mut self.font_system, Metrics::new(font_size, line_height));
                // Unbounded width — wrap-width matching is a follow-up (paint seam).
                buffer.set_size(&mut self.font_system, Some(f32::MAX), None);
                let attrs = Attrs::new().family(Family::SansSerif);
                buffer.set_text(
                    &mut self.font_system,
                    &key.text,
                    &attrs,
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.font_system, false);
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
        tracing::trace!(text, ?position, font_size, ?color, "TextRenderer::add_text");

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
    ) {
        if runs.is_empty() {
            return;
        }

        tracing::trace!(
            run_count = runs.len(),
            ?position,
            base_font_size,
            ?base_color,
            "TextRenderer::add_rich_text"
        );

        let key = RichTextCacheKey::new(runs, base_font_size, base_color);
        match self.rich_cache.entry(key.clone()) {
            Entry::Occupied(mut e) => {
                e.get_mut().last_used_frame = self.current_frame;
                self.cache_hits += 1;
            }
            Entry::Vacant(e) => {
                let line_height = base_font_size * 1.2;
                let mut buffer = Buffer::new(
                    &mut self.font_system,
                    Metrics::new(base_font_size, line_height),
                );
                // Unbounded width — wrap-width matching is a follow-up (paint seam).
                buffer.set_size(&mut self.font_system, Some(f32::MAX), None);

                // Build per-run AttrsOwned; the iterator borrows from the vec
                // of owned values, satisfying set_rich_text's lifetime.
                let owned_attrs: Vec<AttrsOwned> = runs
                    .iter()
                    .map(|(_, style)| style_to_attrs_owned(style.as_ref(), base_color))
                    .collect();

                buffer.set_rich_text(
                    &mut self.font_system,
                    runs.iter()
                        .zip(owned_attrs.iter())
                        .map(|((text, _), attrs)| (text.as_str(), attrs.as_attrs())),
                    &Attrs::new(),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.font_system, false);

                e.insert(CachedBuffer {
                    buffer,
                    last_used_frame: self.current_frame,
                });
                self.cache_misses += 1;
            }
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

    /// Renders all batched text to the GPU.
    ///
    /// Call once per frame after all `add_text`/`add_rich_text` calls.
    #[must_use = "errors must be propagated or handled"]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        size: (u32, u32),
    ) -> crate::error::EngineResult<()> {
        self.current_frame += 1;

        if self.batch.is_empty() {
            return Ok(());
        }

        let total = self.batch.len();
        let hit_rate = if self.cache_hits + self.cache_misses > 0 {
            #[allow(clippy::cast_precision_loss)] // u64 → f64 for a display ratio
            let r = (self.cache_hits as f64 / (self.cache_hits + self.cache_misses) as f64) * 100.0;
            r
        } else {
            0.0
        };
        tracing::trace!(
            buffers = total,
            width = size.0,
            height = size.1,
            cache_hit_rate = format_args!("{hit_rate:.1}%"),
            "TextRenderer::render"
        );

        self.viewport.update(
            queue,
            Resolution {
                width: size.0,
                height: size.1,
            },
        );

        // i32 viewport bounds used by every TextArea.
        // Viewport width/height are u32; wgpu's maximum surface dimension is
        // 8192 (well under i32::MAX = 2 147 483 647), so wrapping is impossible.
        #[allow(clippy::cast_possible_wrap)]
        let right = size.0 as i32;
        #[allow(clippy::cast_possible_wrap)]
        let bottom = size.1 as i32;
        let full_bounds = TextBounds {
            left: 0,
            top: 0,
            right,
            bottom,
        };

        // Build TextArea values by field-splitting `self` so that the
        // immutable borrows into the two caches are disjoint from the
        // mutable borrows `prepare` needs.  All five fields accessed here
        // (`batch`, `plain_cache`, `rich_cache`, `renderer`, `font_system`,
        // `text_atlas`, `viewport`, `swash_cache`) are distinct struct
        // fields; Rust's borrow checker accepts simultaneous borrows of
        // disjoint fields when they are named directly (not through `self`).
        let text_areas: Vec<TextArea<'_>> = build_text_areas(
            &self.batch,
            &self.plain_cache,
            &self.rich_cache,
            full_bounds,
        );

        self.renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.text_atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .map_err(|e| crate::error::EngineError::text_render(format!("prepare: {e:?}")))?;

        let mut text_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Text Render Pass"),
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

        self.renderer
            .render(&self.text_atlas, &self.viewport, &mut text_pass)
            .map_err(|e| crate::error::EngineError::text_render(format!("render: {e:?}")))?;

        self.batch.clear();

        if self.current_frame.is_multiple_of(60) {
            self.prune_cache();
        }

        Ok(())
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
    use super::collect_styled_spans;
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

    /// A placeholder span at the root contributes zero runs.
    #[test]
    fn collect_styled_spans_placeholder_yields_no_runs() {
        use flui_types::typography::{PlaceholderAlignment, PlaceholderSpan};
        let span = InlineSpan::Placeholder(PlaceholderSpan::new(
            32.0,
            32.0,
            PlaceholderAlignment::Baseline,
        ));
        let runs = collect_styled_spans(&span, 1.0);
        assert!(runs.is_empty(), "placeholder span must produce no runs");
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
        let key = |s: &TextStyle| RichTextCacheKey::new(&runs_of(s), 14.0, Color::BLACK);
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
            RichTextCacheKey::new(&runs_of(&base), 14.0, Color::BLACK),
            RichTextCacheKey::new(&runs_of(&base), 28.0, Color::BLACK),
            "base_font_size must be keyed",
        );
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod gpu_tests {
    #[test]
    fn test_text_batching() {
        // GPU-backed batching tests require a wgpu device.
        // Run with: cargo test -p flui-engine --features enable-wgpu-tests,dx12
    }
}
